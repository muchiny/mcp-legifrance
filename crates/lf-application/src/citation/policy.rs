//! Politique de confiance, de budget et de concurrence des résolutions (annexe B §7.1, §7.2, §7.9).
//!
//! Quatre décisions vivent ici, chacune en un seul endroit pour être relue et testée :
//!
//! - la **concurrence** (quatre résolutions à la fois) et le **budget de temps** (80 % du délai de
//!   l'outil) ;
//! - la **preuve d'absence** : la table `POLICY` dit, famille par famille, si l'absence d'un
//!   enregistrement permet de conclure `not_found` (§7.2) ;
//! - la **règle d'extrait stricte** (L2-03) : `check_quote` exige que chaque mot de l'extrait
//!   figure dans le texte officiel lu, à la suite et dans l'ordre (seuls la casse, les accents, la
//!   ponctuation, les espaces, les repères d'énumération et la forme `L. 1234-9` sont tolérés) ;
//! - les bornes de consultation (candidats rendus, documents consultés par numéro).

use std::{collections::HashSet, iter, mem, time::Duration};

use lf_domain::text::fold;

use super::QuoteCheck;

/// Résolutions simultanément en vol (`buffer_unordered(4)`, 02 §6.2, 05 §5.9). Le limiteur de
/// l'adapter régule le reste du débit.
pub(crate) const CONCURRENCY: usize = 4;

/// Part du délai de l'outil que la résolution se réserve, en pourcent (§7.7 étape 5) : au-delà, les
/// citations restantes sont `unresolvable` (« time budget »). Les 20 % restants laissent le temps
/// de composer la réponse et d'écrire la ligne d'audit.
pub(crate) const BUDGET_PERCENT: u128 = 80;

/// Candidats rendus au plus par `fr_resolve` (05 §5.10) et par une ambiguïté.
pub(crate) const MAX_CANDIDATES: usize = 5;

/// Documents consultés au plus pour un même numéro (§7.3, « sélection parmi plusieurs hits »)
/// : au-delà, la citation est `ambiguous` et la résolution ne consulte pas chaque document.
pub(crate) const CONSULT_LIMIT: usize = 5;

/// Budget de temps accordé à la résolution : [`BUDGET_PERCENT`] du délai restant.
pub(crate) fn budget_of(remaining: Duration) -> Duration {
    let millis = remaining.as_millis().saturating_mul(BUDGET_PERCENT) / 100;
    Duration::from_millis(u64::try_from(millis).unwrap_or(u64::MAX))
}

/// Famille de citation, pour la politique de preuve d'absence (§7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Family {
    /// Article d'un code.
    CodeArticle,
    /// Article d'un texte numéroté résolu.
    TextArticle,
    /// Identifiant Légifrance consulté directement.
    LegifranceId,
    /// Texte numéroté (loi, décret, ordonnance).
    Text,
    /// Cour de cassation, Conseil d'État, Conseil constitutionnel, ECLI de ces juridictions.
    Supreme,
    /// Cours d'appel, tribunaux judiciaires, juridictions administratives du fond.
    Lower,
    /// Délibération de la CNIL.
    Cnil,
    /// Convention collective (IDCC, avenants, brochures).
    Convention,
    /// NOR, ELI, JORF.
    Index,
}

impl Family {
    /// Toutes les familles, pour le test de couverture de la table.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 9] = [
        Self::CodeArticle,
        Self::TextArticle,
        Self::LegifranceId,
        Self::Text,
        Self::Supreme,
        Self::Lower,
        Self::Cnil,
        Self::Convention,
        Self::Index,
    ];
}

/// Ce que l'absence d'un enregistrement permet de conclure (§7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Absence {
    /// L'index est complet pour cette famille : `not_found` en production, sur preuve.
    Allowed,
    /// À autoriser après vérification de la couverture du fonds (V-134) ; d'ici là, `unresolvable`.
    AfterVerification,
    /// Jamais : la couverture est partielle, l'index incomplet ou la procédure non publique.
    Never,
}

/// Table de la politique de preuve d'absence (annexe B §7.2), une ligne par famille.
pub(crate) const POLICY: [(Family, Absence); 9] = [
    (Family::CodeArticle, Absence::Allowed),
    (Family::TextArticle, Absence::Allowed),
    (Family::LegifranceId, Absence::Allowed),
    (Family::Text, Absence::Never),
    (Family::Supreme, Absence::AfterVerification),
    (Family::Lower, Absence::Never),
    (Family::Cnil, Absence::Never),
    (Family::Convention, Absence::Never),
    (Family::Index, Absence::Never),
];

/// Politique d'absence de `family` ; une famille absente de la table n'autorise rien.
pub(crate) fn absence(family: Family) -> Absence {
    POLICY
        .iter()
        .find_map(|(listed, absence)| (*listed == family).then_some(*absence))
        .unwrap_or(Absence::Never)
}

/// Mots d'un texte, repliés : minuscules, accents français retirés (`œ` devient `oe`, par
/// [`lf_domain::text::fold`]), tout ce qui n'est ni lettre ni chiffre sépare deux mots
/// (apostrophes, ponctuation, espaces insécables).
pub(crate) fn fold_words(text: &str) -> Vec<String> {
    fold(text)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Texte replié sur une ligne, mots séparés par une espace : la forme de comparaison des titres.
pub(crate) fn fold_line(text: &str) -> String {
    fold_words(text).join(" ")
}

/// Nombre minimal de mots d'un extrait à ellipses (« début […] fin ») : en deçà, des tronçons d'un
/// ou deux mots sont vrais presque partout dans un texte et ne prouvent rien.
const MIN_ELIDED_WORDS: usize = 3;

/// Mot replié d'un extrait ou du texte officiel.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Word {
    text: String,
    /// Repère d'énumération (« 1° », « a) », « I.- ») : peut manquer d'un côté ou de l'autre sans
    /// que l'extrait cesse d'être fidèle.
    marker: bool,
}

/// Un symbole qui porte un sens (unité, taux) reste un mot : « 2 % » n'est pas « 2 € ».
fn is_symbol(c: char) -> bool {
    matches!(c, '€' | '%' | '‰' | '$' | '£')
}

/// Lettre ou chiffre d'un mot (les indicateurs ordinaux `º` et `ª` n'en font pas partie).
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() && !matches!(c, 'º' | 'ª')
}

/// Mots repliés d'un morceau sans espace : minuscules, accents retirés, tout ce qui n'est ni
/// lettre, ni chiffre, ni symbole de [`is_symbol`] sépare deux mots.
fn split_words(chunk: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    // La sentinelle vide le dernier mot.
    for c in fold(chunk).chars().chain(iter::once(' ')) {
        if is_word_char(c) {
            current.push(c);
            continue;
        }
        if !current.is_empty() {
            words.push(mem::take(&mut current));
        }
        if is_symbol(c) {
            words.push(c.to_string());
        }
    }
    words
}

/// Colle la lettre d'un numéro d'article à ses chiffres : « L. 1234-9 » devient « L1234-9 ».
/// Seules `L`, `R`, `D` et `A` isolées sont concernées ; la même transformation s'applique à
/// l'extrait et au texte officiel, elle ne peut donc créer d'écart que si les deux formes diffèrent.
fn join_article_letters(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        out.push(c);
        at = at.saturating_add(1);
        let preceded = at.checked_sub(2).and_then(|before| chars.get(before));
        if !matches!(c, 'L' | 'R' | 'D' | 'A') || preceded.is_some_and(|p| p.is_alphanumeric()) {
            continue;
        }
        let mut next = at;
        if chars.get(next) == Some(&'.') {
            next = next.saturating_add(1);
        }
        while chars.get(next).is_some_and(|b| b.is_whitespace()) {
            next = next.saturating_add(1);
        }
        if chars.get(next).is_some_and(char::is_ascii_digit) {
            at = next;
        }
    }
    out
}

/// Repère d'énumération : `Some(true)` s'il se reconnaît partout (« 1° », « a) », « (2) »),
/// `Some(false)` s'il ne vaut qu'en tête d'alinéa (« I.- », « II. », « 1. »), `None` sinon.
fn marker_kind(chunk: &str) -> Option<bool> {
    let core = chunk.trim_end_matches(['-', '–', '—']);
    let shaped = |body: &str, valid: fn(char) -> bool| {
        (1..=4).contains(&body.len()) && body.chars().all(valid)
    };
    if let Some(body) = core.strip_suffix('.') {
        let numeral = shaped(body, |c| matches!(c, 'I' | 'V' | 'X'));
        return (numeral || shaped(body, |c| c.is_ascii_digit())).then_some(false);
    }
    if let Some(body) = core.strip_suffix(['°', 'º']) {
        return shaped(body, |c| c.is_ascii_digit()).then_some(true);
    }
    let body = core.strip_suffix(')')?;
    let body = body.strip_prefix('(').unwrap_or(body);
    ((1..=2).contains(&body.chars().count()) && body.chars().all(char::is_alphanumeric))
        .then_some(true)
}

/// Mots d'un texte. Un repère d'énumération en tête d'alinéa, après `;`, `:` ou `.`, ou à la suite
/// d'un autre repère est marqué ; avec `lenient` (l'extrait), un repère de forme sûre (« 1° »,
/// « a) ») l'est aussi au milieu d'une phrase, car l'auteur a pu mettre bout à bout des alinéas.
fn words(text: &str, lenient: bool) -> Vec<Word> {
    let mut out = Vec::new();
    for line in join_article_letters(text).split('\n') {
        let mut at_start = true;
        for chunk in line.split_whitespace() {
            let parts = split_words(chunk);
            let ends_item = chunk.ends_with([';', ':', '.']);
            if parts.is_empty() {
                at_start = at_start || ends_item;
                continue;
            }
            let marker = marker_kind(chunk).is_some_and(|sure| at_start || (lenient && sure));
            at_start = marker || ends_item;
            out.extend(parts.into_iter().map(|text| Word { text, marker }));
        }
    }
    out
}

/// Tronçons de l'extrait, coupés aux ellipses (`…`, `...`, `[…]`, `(...)`). Un tronçon sans mot
/// (ou fait de seuls repères d'énumération) disparaît.
fn quote_segments(quoted: &str) -> Vec<Vec<Word>> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut dots = String::new();
    for c in quoted.replace('…', "...").chars().chain(iter::once(' ')) {
        if c == '.' {
            dots.push(c);
            continue;
        }
        if dots.len() >= 3 {
            parts.push(mem::take(&mut current));
        } else {
            current.push_str(&dots);
        }
        dots.clear();
        current.push(c);
    }
    parts.push(current);
    parts
        .iter()
        .map(|part| words(part, true))
        .filter(|segment| segment.iter().any(|word| !word.marker))
        .collect()
}

/// Fin de la première suite de `official` à partir de `start` qui égale `wanted` mot pour mot, en
/// laissant passer les repères d'énumération de l'un ou de l'autre.
fn match_from(
    official: &[Word],
    start: usize,
    wanted: &[Word],
    seen: &mut HashSet<(usize, usize)>,
) -> Option<usize> {
    let mut stack = vec![(start, 0_usize)];
    while let Some((at, next)) = stack.pop() {
        let Some(want) = wanted.get(next) else {
            return Some(at);
        };
        if !seen.insert((at, next)) {
            continue;
        }
        // Empilés du moins au plus souhaitable : la lecture littérale est essayée la première.
        if want.marker {
            stack.push((at, next.saturating_add(1)));
        }
        if let Some(have) = official.get(at) {
            if have.marker {
                stack.push((at.saturating_add(1), next));
            }
            if have.text == want.text {
                stack.push((at.saturating_add(1), next.saturating_add(1)));
            }
        }
    }
    None
}

/// Compare l'extrait cité au texte officiel lu (§7.9, règle d'extrait stricte de L2-03).
///
/// `Match` si chaque tronçon de l'extrait (coupé aux ellipses) est une suite de mots du texte
/// officiel, mot pour mot, et si les tronçons se suivent dans l'ordre. Sont ignorés : la casse, les
/// accents, la ponctuation, les espaces (insécables compris), les apostrophes, les repères
/// d'énumération (« 1° », « a) », « I.- ») et la forme du numéro d'article (« L. 1234-9 » vaut
/// « L1234-9 »). Un chiffre, un mot ou un ajout entre crochets de plus ou de moins est un écart.
/// Un extrait à ellipses compte au moins [`MIN_ELIDED_WORDS`] mots ; un extrait sans mot ne peut
/// pas être tenu pour trouvé. Faiblesse acceptée : un tronçon d'un seul mot est vrai presque
/// partout (« le […] la fin »).
pub(crate) fn check_quote(quoted: &str, official: &str) -> QuoteCheck {
    let segments = quote_segments(quoted);
    let count: usize =
        segments.iter().map(|segment| segment.iter().filter(|word| !word.marker).count()).sum();
    if segments.is_empty() || (segments.len() > 1 && count < MIN_ELIDED_WORDS) {
        return QuoteCheck::QuoteMismatch;
    }
    let text = words(official, false);
    let found = segments.iter().try_fold(0, |from, segment| {
        // Un état déjà exploré sans succès échoue pour tout départ : la mémoire est commune.
        let mut seen = HashSet::new();
        (from..text.len()).find_map(|start| match_from(&text, start, segment, &mut seen))
    });
    if found.is_some() { QuoteCheck::Match } else { QuoteCheck::QuoteMismatch }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn every_family_has_exactly_one_line_in_the_policy_table() {
        for family in Family::ALL {
            let lines = POLICY.iter().filter(|(listed, _)| *listed == family).count();
            assert_eq!(lines, 1, "{family:?}");
        }
        assert_eq!(POLICY.len(), Family::ALL.len());
    }

    #[test]
    fn absence_is_allowed_only_where_the_index_is_complete() {
        assert_eq!(absence(Family::CodeArticle), Absence::Allowed);
        assert_eq!(absence(Family::TextArticle), Absence::Allowed);
        assert_eq!(absence(Family::LegifranceId), Absence::Allowed);
        assert_eq!(absence(Family::Supreme), Absence::AfterVerification);
        for never in [Family::Text, Family::Lower, Family::Cnil, Family::Convention, Family::Index]
        {
            assert_eq!(absence(never), Absence::Never, "{never:?}");
        }
    }

    #[test]
    fn the_budget_is_80_percent_of_the_remaining_time() {
        assert_eq!(budget_of(Duration::from_secs(60)), Duration::from_secs(48));
        assert_eq!(budget_of(Duration::from_millis(1_001)), Duration::from_millis(800));
        assert_eq!(budget_of(Duration::ZERO), Duration::ZERO);
        // Un délai démesuré ne déborde pas.
        assert!(budget_of(Duration::MAX) > Duration::from_secs(1_000_000));
    }

    #[test]
    fn the_constants_match_the_annex() {
        assert_eq!(CONCURRENCY, 4);
        assert_eq!(BUDGET_PERCENT, 80);
        assert_eq!(MAX_CANDIDATES, 5);
        assert_eq!(CONSULT_LIMIT, 5);
    }

    #[test]
    fn folding_drops_case_accents_and_punctuation() {
        assert_eq!(
            fold_words("Chacun a droit au respect de sa vie privée, à l’œuvre !"),
            [
                "chacun", "a", "droit", "au", "respect", "de", "sa", "vie", "privee", "a", "l",
                "oeuvre"
            ]
        );
        assert_eq!(fold_line("Code  pénal\u{a0}(ancien)"), "code penal ancien");
        assert_eq!(
            fold_words("ÀÂÄ ÇÉÈÊË ÎÏÔÖ ÙÛÜ ÿ æ"),
            ["aaa", "ceeee", "iioo", "uuu", "y", "ae"]
        );
        assert_eq!(fold_words(" ,; "), Vec::<String>::new());
    }

    const OFFICIAL: &str = "Chacun a le droit au respect de sa vie privée. Les juges peuvent \
        prescrire toutes mesures propres à empêcher ou faire cesser une atteinte à l'intimité de \
        la vie privée.";

    // Textes officiels réels (Légifrance, `fr_get_text`, relevés le 2026-11-01) et décision
    // enregistrée dans les fixtures de `lf-adapter-piste`.
    const L1234_9: &str = "Le salarié titulaire d'un contrat de travail à durée indéterminée, \
        licencié alors qu'il compte 8 mois d'ancienneté ininterrompus au service du même \
        employeur, a droit, sauf en cas de faute grave, à une indemnité de licenciement.\n\nLes \
        modalités de calcul de cette indemnité sont fonction de la rémunération brute dont le \
        salarié bénéficiait antérieurement à la rupture du contrat de travail. Ce taux et ces \
        modalités sont déterminés par voie réglementaire.";
    const C1240: &str = "Tout fait quelconque de l'homme, qui cause à autrui un dommage, oblige \
        celui par la faute duquel il est arrivé à le réparer.";
    const C1103: &str = "Les contrats légalement formés tiennent lieu de loi à ceux qui les ont \
        faits.";
    const C544: &str = "La propriété est le droit de jouir et disposer des choses de la manière \
        la plus absolue, pourvu qu'on n'en fasse pas un usage prohibé par les lois ou par les \
        règlements.";
    const C1231_1: &str = "Le débiteur est condamné, s'il y a lieu, au paiement de dommages et \
        intérêts soit à raison de l'inexécution de l'obligation, soit à raison du retard dans \
        l'exécution, s'il ne justifie pas que l'exécution a été empêchée par la force majeure.";
    const C9: &str = "Chacun a droit au respect de sa vie privée. Les juges peuvent, sans \
        préjudice de la réparation du dommage subi, prescrire toutes mesures, telles que \
        séquestre, saisie et autres, propres à empêcher ou faire cesser une atteinte à \
        l'intimité de la vie privée : ces mesures peuvent, s'il y a urgence, être ordonnées en \
        référé.";
    const L3121_27: &str = "La durée légale de travail effectif des salariés à temps complet est \
        fixée à trente-cinq heures par semaine.";
    const L221_18: &str = "Le consommateur dispose d'un délai de quatorze jours pour exercer son \
        droit de rétractation d'un contrat conclu à distance, à la suite d'un démarchage \
        téléphonique ou hors établissement, sans avoir à motiver sa décision ni à supporter \
        d'autres coûts que ceux prévus aux articles L. 221-23 à L. 221-25.\n\nLe délai mentionné \
        au premier alinéa court à compter du jour :\n\n1° De la conclusion du contrat, pour les \
        contrats de prestation de services et ceux mentionnés à l'article L. 221-4 ;\n\n2° De la \
        réception du bien par le consommateur ou un tiers, autre que le transporteur, désigné \
        par lui, pour les contrats de vente de biens.";
    const L1233_3: &str = "Constitue un licenciement pour motif économique le licenciement \
        effectué par un employeur pour un ou plusieurs motifs non inhérents à la personne du \
        salarié résultant d'une suppression ou transformation d'emploi ou d'une modification, \
        refusée par le salarié, d'un élément essentiel du contrat de travail, consécutives \
        notamment :\n\n1° A des difficultés économiques caractérisées soit par l'évolution \
        significative d'au moins un indicateur économique tel qu'une baisse des commandes ou du \
        chiffre d'affaires, des pertes d'exploitation ou une dégradation de la trésorerie ou de \
        l'excédent brut d'exploitation, soit par tout autre élément de nature à justifier de ces \
        difficultés.\n\nUne baisse significative des commandes ou du chiffre d'affaires est \
        constituée dès lors que la durée de cette baisse est, en comparaison avec la même \
        période de l'année précédente, au moins égale à :\n\na) Un trimestre pour une entreprise \
        de moins de onze salariés ;\n\nb) Deux trimestres consécutifs pour une entreprise \
        d'au moins onze salariés et de moins de cinquante salariés ;\n\nc) Trois trimestres \
        consécutifs pour une entreprise d'au moins cinquante salariés et de moins de trois \
        cents salariés ;\n\nd) Quatre trimestres consécutifs pour une entreprise de trois cents \
        salariés et plus ;\n\n2° A des mutations technologiques ;\n\n3° A une réorganisation de \
        l'entreprise nécessaire à la sauvegarde de sa compétitivité ;\n\nPour l'application du \
        présent article, la notion de groupe désigne le groupe formé par une entreprise appelée \
        entreprise dominante et les entreprises qu'elle contrôle dans les conditions définies à \
        l'article L. 233-1, aux I et II de l'article L. 233-3 et à l'article L. 233-16 du code \
        de commerce.";
    const L1237_19_1: &str = "L'accord portant rupture conventionnelle collective détermine :\n\n\
        4° Les modalités de présentation et d'examen des candidatures au départ des salariés ;\
        \n\n4° bis Les modalités de conclusion d'une convention individuelle de rupture entre \
        l'employeur et le salarié et d'exercice du droit de rétractation des parties ;\n\n7° bis \
        Le cas échéant, les mesures mentionnées au II de l'article L. 6324-9 ;\n\n8° Les \
        modalités de suivi de la mise en œuvre effective de l'accord portant rupture \
        conventionnelle collective.";
    const CGI_150U: &str = "I. – Sous réserve des dispositions propres aux bénéfices \
        industriels et commerciaux, aux bénéfices agricoles et aux bénéfices non commerciaux, \
        les plus-values réalisées par les personnes physiques sont passibles de l'impôt sur le \
        revenu dans les conditions prévues aux articles 150 V à 150 VH.\n\nCes dispositions \
        s'appliquent, sous réserve de celles prévues au 3° du I de l'article 35, aux \
        plus-values réalisées lors de la cession d'un terrain divisé en lots destinés à être \
        construits.\n\nII. – Les dispositions du I ne s'appliquent pas aux immeubles :\n\n1° Qui \
        constituent la résidence principale du cédant au jour de la cession ;\n\nL'exonération \
        mentionnée au premier alinéa du présent 2° s'applique, dans la limite d'une résidence \
        par contribuable et de 150 000 € de plus-value nette imposable, aux cessions \
        réalisées :\n\na) Au plus tard le 31 décembre de la dixième année suivant celle du \
        transfert par le cédant de son domicile fiscal hors de France ;\n\n6° Dont le prix de \
        cession est inférieur ou égal à 15 000 €. Le seuil de 15 000 € s'apprécie en tenant \
        compte de la valeur en pleine propriété de l'immeuble ou de la partie d'immeuble ;";
    const CONSTEXT: &str = "LE CONSEIL CONSTITUTIONNEL A ÉTÉ SAISI le 8 août 2025 par la Cour de \
        cassation (chambre criminelle, arrêt n°\u{a0}1133 du 6 août\u{a0}2025), dans les \
        conditions prévues à l’article 61-1 de la Constitution, d’une question prioritaire de \
        constitutionnalité. Cette question a été posée pour M. Amine\u{a0}G. par Me Pierre de \
        Combles de Nayves, avocat au barreau de Paris. Au vu des textes suivants\u{a0}: - la \
        Constitution\u{a0}; - l’ordonnance n°\u{a0}58-1067 du 7 novembre 1958 portant loi \
        organique sur le Conseil constitutionnel\u{a0}; - le code de procédure pénale\u{a0};";

    /// Extraits fidèles réels, première moitié du jeu : codes civil, du travail, de la consommation.
    fn faithful_codes() -> &'static [(&'static str, &'static str)] {
        &[
            // L. 1234-9 : alinéa entier, apostrophes typographiques, casse, NBSP, chevauchement.
            (
                L1234_9,
                "Le salarié titulaire d'un contrat de travail à durée indéterminée, licencié \
                       alors qu'il compte 8 mois d'ancienneté ininterrompus au service du même \
                       employeur, a droit, sauf en cas de faute grave, à une indemnité de \
                       licenciement.",
            ),
            (L1234_9, "licencié alors qu’il compte 8 mois d’ancienneté ininterrompus"),
            (L1234_9, "A DROIT, SAUF EN CAS DE FAUTE GRAVE, A UNE INDEMNITE DE LICENCIEMENT"),
            (L1234_9, "a\u{a0}droit,\u{a0}sauf en cas de faute grave"),
            (L1234_9, "Le salarié titulaire […] a droit, sauf en cas de faute grave"),
            (L1234_9, "[…] sauf en cas de faute grave, à une indemnité […]"),
            (
                L1234_9,
                "à une indemnité de licenciement. Les modalités de calcul de cette indemnité",
            ),
            (
                L1234_9,
                "bénéficiait antérieurement à la rupture du contrat de travail. Ce taux et \
                       ces modalités sont déterminés par voie réglementaire",
            ),
            (L1234_9, "compte 8 mois d'ancienneté (...) a droit (…) à une indemnité"),
            // Code civil.
            (
                C1240,
                "Tout fait quelconque de l'homme, qui cause à autrui un dommage, oblige celui \
                     par la faute duquel il est arrivé à le réparer",
            ),
            (C1240, "qui cause à autrui UN DOMMAGE, oblige celui par la faute duquel"),
            (C1103, "Les contrats légalement formés tiennent lieu de loi à ceux qui les ont faits"),
            (
                C544,
                "pourvu qu’on n’en fasse pas un usage prohibé par les lois ou par les règlements",
            ),
            (
                C1231_1,
                "au paiement de dommages et intérêts soit à raison de l’inexécution de \
                       l’obligation, soit à raison du retard",
            ),
            (C9, "Chacun a droit au respect de sa vie privée"),
            (
                C9,
                "Les juges peuvent, sans préjudice de la réparation du dommage subi, prescrire \
                  toutes mesures, telles que séquestre, saisie et autres",
            ),
            // Code du travail et de la consommation : nombres en lettres, listes, renvois.
            (L3121_27, "fixée à trente-cinq heures par semaine"),
            (
                L221_18,
                "dispose d’un délai de quatorze jours pour exercer son droit de rétractation",
            ),
            (L221_18, "prévus aux articles L221-23 à L221-25"),
            (L221_18, "prévus aux articles L. 221-23 à L. 221-25."),
            (
                L221_18,
                "court à compter du jour : 1° De la conclusion du contrat, pour les contrats \
                       de prestation de services",
            ),
            (L221_18, "2° De la réception du bien par le consommateur ou un tiers"),
            (
                L221_18,
                "du jour : De la conclusion du contrat, pour les contrats de prestation de \
                       services et ceux mentionnés à l'article L. 221-4 ; De la réception du bien",
            ),
            (
                L1233_3,
                "a) Un trimestre pour une entreprise de moins de onze salariés ; b) Deux \
                       trimestres consécutifs pour une entreprise d'au moins onze salariés",
            ),
            (
                L1233_3,
                "Quatre trimestres consécutifs pour une entreprise de trois cents salariés et plus",
            ),
            (L1233_3, "aux I et II de l’article L233-3 et à l’article L233-16 du code de commerce"),
            (
                L1233_3,
                "Constitue un licenciement pour motif économique [...] consécutives notamment",
            ),
            (
                L1237_19_1,
                "4° bis Les modalités de conclusion d’une convention individuelle de rupture",
            ),
            (
                L1237_19_1,
                "7° bis Le cas échéant, les mesures mentionnées au II de l’article L. 6324-9",
            ),
            (L1237_19_1, "Les modalités de suivi de la mise en oeuvre effective de l'accord"),
        ]
    }

    /// Seconde moitié du jeu : code général des impôts et décision enregistrée.
    fn faithful_tax_and_decision() -> &'static [(&'static str, &'static str)] {
        &[
            // CGI 150 U : « I. – », « II.- », « 1° », « a) », €, NBSP.
            (
                CGI_150U,
                "I.- Sous réserve des dispositions propres aux bénéfices industriels et \
                        commerciaux",
            ),
            (CGI_150U, "Sous réserve des dispositions propres aux bénéfices industriels"),
            (CGI_150U, "prévues au 3° du I de l'article 35, aux plus-values réalisées"),
            (CGI_150U, "de 150\u{a0}000\u{a0}€ de plus-value nette imposable"),
            (CGI_150U, "6° Dont le prix de cession est inférieur ou égal à 15 000 €"),
            (
                CGI_150U,
                "II. Les dispositions du I ne s'appliquent pas aux immeubles : 1° Qui \
                        constituent la résidence principale du cédant",
            ),
            (
                CGI_150U,
                "aux cessions réalisées : a) Au plus tard le 31 décembre de la dixième année",
            ),
            // Décision du Conseil constitutionnel : NBSP, apostrophes, tirets de liste.
            (
                CONSTEXT,
                "arrêt n° 1133 du 6 août 2025), dans les conditions prévues à l'article \
                        61-1 de la Constitution",
            ),
        ]
    }

    /// Jeu de non-régression (Q5) : des extraits fidèles réels, jamais déclarés en écart.
    #[test]
    fn faithful_real_quotes_never_raise_a_false_mismatch() {
        let all: Vec<(&str, &str)> =
            faithful_codes().iter().chain(faithful_tax_and_decision()).copied().collect();
        assert!(all.len() >= 20, "le jeu doit compter au moins 20 extraits");
        for (official, quote) in all {
            assert_eq!(check_quote(quote, official), QuoteCheck::Match, "{quote}");
        }
    }

    /// Jumeau du jeu fidèle : la même source, un mot ou un chiffre changé, est toujours un écart.
    #[test]
    fn altered_real_quotes_are_mismatches() {
        for (official, quote) in [
            // Le cas de l'E2E : « 12 mois » pour « 8 mois ».
            (L1234_9, "licencié alors qu'il compte 12 mois d'ancienneté ininterrompus"),
            (
                L1234_9,
                "Le salarié titulaire d'un contrat de travail à durée indéterminée, licencié \
                 alors qu'il compte 12 mois d'ancienneté ininterrompus au service du même \
                 employeur, a droit, sauf en cas de faute grave, à une indemnité de licenciement.",
            ),
            (L1234_9, "a droit, y compris en cas de faute grave, à une indemnité de licenciement"),
            (
                L1234_9,
                "a droit, sauf en cas de faute grave, à une indemnité de licenciement et de préavis",
            ),
            (L1234_9, "n'a pas droit, sauf en cas de faute grave, à une indemnité"),
            (L1234_9, "[le salarié] a droit, sauf en cas de faute grave"),
            (L1234_9, "a droit, sauf en cas de faute grave, à une [sic] indemnité de licenciement"),
            (C1240, "qui cause à autrui un préjudice, oblige celui par la faute duquel"),
            (C1240, "qui cause à autrui un dommage, n'oblige pas celui par la faute duquel"),
            (
                C1103,
                "Les contrats légalement formés tiennent lieu de loi à ceux qui les ont voulus",
            ),
            (L3121_27, "fixée à quarante heures par semaine"),
            (L221_18, "dispose d'un délai de quinze jours pour exercer son droit de rétractation"),
            (L221_18, "prévus aux articles L. 221-24 à L. 221-25"),
            (
                L1233_3,
                "aux I et II de l'article L. 233-4 et à l'article L. 233-16 du code de commerce",
            ),
            (
                L1233_3,
                "b) Deux trimestres consécutifs pour une entreprise d'au moins vingt salariés",
            ),
            (CGI_150U, "prévues au 2° du I de l'article 35, aux plus-values réalisées"),
            (CGI_150U, "prévues au I de l'article 35, aux plus-values réalisées"),
            (CGI_150U, "de 150 000 % de plus-value nette imposable"),
            (CGI_150U, "de 15 000 € de plus-value nette imposable"),
            (CGI_150U, "6° Dont le prix de cession est inférieur ou égal à 50 000 €"),
            (CONSTEXT, "arrêt n° 1134 du 6 août 2025"),
        ] {
            assert_eq!(check_quote(quote, official), QuoteCheck::QuoteMismatch, "{quote}");
        }
    }

    #[test]
    fn an_unrelated_or_empty_quote_is_a_mismatch() {
        // Un texte sans rapport, une ponctuation seule, un texte officiel vide.
        assert_eq!(
            check_quote("Toute personne dispose du droit de vote", OFFICIAL),
            QuoteCheck::QuoteMismatch
        );
        assert_eq!(check_quote(" « … » ", OFFICIAL), QuoteCheck::QuoteMismatch);
        assert_eq!(check_quote("", OFFICIAL), QuoteCheck::QuoteMismatch);
        assert_eq!(check_quote("Chacun a le droit", ""), QuoteCheck::QuoteMismatch);
        // Des repères d'énumération seuls ne forment pas un extrait.
        assert_eq!(check_quote("1° a) I.-", OFFICIAL), QuoteCheck::QuoteMismatch);
        assert_eq!(check_quote("[…] 1° […]", OFFICIAL), QuoteCheck::QuoteMismatch);
    }

    /// Un extrait d'un seul tronçon, même d'un seul mot, est une suite contiguë ou un écart.
    #[test]
    fn a_single_segment_must_be_contiguous() {
        assert_eq!(
            check_quote("chacun a le DROIT au respect de sa vie privee", OFFICIAL),
            QuoteCheck::Match
        );
        assert_eq!(check_quote("intimité", OFFICIAL), QuoteCheck::Match);
        assert_eq!(check_quote("inexistant", OFFICIAL), QuoteCheck::QuoteMismatch);
        // Les bons mots, pas à la suite.
        assert_eq!(check_quote("Chacun a droit au respect", OFFICIAL), QuoteCheck::QuoteMismatch);
        // Un extrait qui dépasse la fin du texte officiel.
        assert_eq!(check_quote("vie privée et plus", OFFICIAL), QuoteCheck::QuoteMismatch);
        // Un mot altéré sur vingt (l'ancien seuil le laissait passer) est un écart.
        let twenty = "Les juges peuvent prescrire toutes mesures propres à empêcher ou faire \
                      cesser une atteinte à l'intimité de la vie privée";
        assert_eq!(check_quote(twenty, OFFICIAL), QuoteCheck::Match);
        assert_eq!(
            check_quote(&twenty.replace("propres", "utiles"), OFFICIAL),
            QuoteCheck::QuoteMismatch
        );
    }

    #[test]
    fn ellipses_cut_the_quote_into_ordered_segments() {
        let official = L1234_9;
        // Toutes les formes d'ellipse.
        for mark in ["…", "...", "[…]", "[...]", "(…)", "(...)", "[ … ]", "(….)", "…...."]
        {
            let quote = format!("Le salarié titulaire {mark} a droit, sauf en cas de faute grave");
            assert_eq!(check_quote(&quote, official), QuoteCheck::Match, "{mark}");
        }
        // En tête et en queue.
        assert_eq!(check_quote("[…] sauf en cas de faute grave […]", official), QuoteCheck::Match);
        assert_eq!(check_quote("…sauf en cas de faute grave…", official), QuoteCheck::Match);
        // Dans l'ordre seulement : l'inverse est un écart.
        assert_eq!(
            check_quote("a droit, sauf en cas de faute grave […] Le salarié titulaire", official),
            QuoteCheck::QuoteMismatch
        );
        // Un tronçon introuvable suffit à l'écart, de même qu'un tronçon réutilisé.
        assert_eq!(
            check_quote(
                "Le salarié titulaire […] a droit, y compris en cas de faute grave",
                official
            ),
            QuoteCheck::QuoteMismatch
        );
        assert_eq!(
            check_quote("Le salarié titulaire […] Le salarié titulaire […] a droit", official),
            QuoteCheck::QuoteMismatch
        );
        // Deux points ne font pas une ellipse : ce n'est que de la ponctuation.
        assert_eq!(check_quote("faute grave.. à une indemnité", official), QuoteCheck::Match);
        assert_eq!(check_quote("faute grave, à une indemnité..", official), QuoteCheck::Match);
    }

    /// Au moins trois mots sur les tronçons d'un extrait à ellipses ; un tronçon d'un mot reste
    /// accepté (faiblesse connue, notée dans la fiche).
    #[test]
    fn an_elided_quote_needs_three_words() {
        assert_eq!(check_quote("le […] la", OFFICIAL), QuoteCheck::QuoteMismatch);
        assert_eq!(check_quote("Chacun […] privée", OFFICIAL), QuoteCheck::QuoteMismatch);
        assert_eq!(check_quote("Chacun […] sa vie", OFFICIAL), QuoteCheck::Match);
        assert_eq!(check_quote("Chacun a […] vie", OFFICIAL), QuoteCheck::Match);
        // Les repères d'énumération ne comptent pas comme des mots.
        assert_eq!(check_quote("Chacun […] 1° privée", OFFICIAL), QuoteCheck::QuoteMismatch);
    }

    #[test]
    fn enumeration_markers_are_tolerated_on_either_side() {
        // Repères présents dans l'extrait seulement, ou dans le texte seulement.
        assert_eq!(
            check_quote("a droit, 1° sauf en cas de faute grave, b) à une indemnité", L1234_9),
            QuoteCheck::Match
        );
        assert_eq!(
            check_quote("Un trimestre pour une entreprise de moins de onze salariés", L1233_3),
            QuoteCheck::Match
        );
        // Un repère en milieu de phrase, sans « : » ni « ; » avant lui.
        assert_eq!(
            check_quote(
                "consécutives notamment 1° A des difficultés économiques caractérisées",
                L1233_3
            ),
            QuoteCheck::Match
        );
        // Plusieurs repères de suite, tirets, numéro de paragraphe, parenthèses.
        let official = "II. – 1° Les parties conviennent ;\n\n(2) Les parties s'engagent ;\n\n\
                        3. Les parties signent.";
        for quote in [
            "Les parties conviennent",
            "II.- 1° Les parties conviennent",
            "II. – 1° Les parties conviennent ; 2) Les parties s'engagent",
            "(2) Les parties s'engagent ; 3. Les parties signent",
            "Les parties conviennent ; Les parties s'engagent ; Les parties signent",
        ] {
            assert_eq!(check_quote(quote, official), QuoteCheck::Match, "{quote}");
        }
        // Un repère de forme sûre en milieu de phrase du texte officiel est un mot : il ne
        // s'omet pas, et il ne se change pas.
        assert_eq!(check_quote("au 3° du I de l'article 35", CGI_150U), QuoteCheck::Match);
        assert_eq!(check_quote("au 3 º du I de l'article 35", CGI_150U), QuoteCheck::Match);
        assert_eq!(check_quote("au du I de l'article 35", CGI_150U), QuoteCheck::QuoteMismatch);
        assert_eq!(check_quote("au 4° du I de l'article 35", CGI_150U), QuoteCheck::QuoteMismatch);
        // Un chiffre qui n'est pas un repère reste un mot.
        assert_eq!(check_quote("Les parties 2 conviennent", official), QuoteCheck::QuoteMismatch);
    }

    #[test]
    fn the_article_letter_may_be_stuck_to_its_number() {
        let official = "Voir l'article L. 1234-9 et l'article R1234-2, puis D. 12-1 et A 7.";
        for quote in [
            "article L. 1234-9 et l'article R. 1234-2",
            "article L1234-9 et l'article R 1234-2",
            "article L.1234-9",
            "puis D12-1 et A. 7",
        ] {
            assert_eq!(check_quote(quote, official), QuoteCheck::Match, "{quote}");
        }
        for quote in
            ["article L. 1234-8", "article L. 1234-9 et l'article R. 1234-3", "article 1234-9"]
        {
            assert_eq!(check_quote(quote, official), QuoteCheck::QuoteMismatch, "{quote}");
        }
        // Une capitale qui termine un mot, ou qui n'est pas suivie d'un chiffre, ne colle rien.
        assert_eq!(
            join_article_letters("ALL. 12 et L'article 3 ; L. A"),
            "ALL. 12 et L'article 3 ; L. A"
        );
        assert_eq!(join_article_letters("L. 1234 L 5 L.\n7 R.9"), "L1234 L5 L7 R9");
    }

    #[test]
    fn units_and_figures_are_words() {
        let official =
            "Une retenue de 2 % sur 12\u{a0}109 € ou 12 109 euros, soit 2 ‰ ou 3 $ ou 4 £.";
        for quote in [
            "retenue de 2% sur 12 109€",
            "sur 12\u{a0}109\u{a0}€ ou 12 109 euros, soit 2‰ ou 3 $ ou 4 £",
        ] {
            assert_eq!(check_quote(quote, official), QuoteCheck::Match, "{quote}");
        }
        for quote in [
            "retenue de 2 € sur 12 109 €",
            "retenue de 2 % sur 12 119 €",
            "retenue de 2 % sur 12109 €",
            "retenue de 2 % sur 12 109 $",
        ] {
            assert_eq!(check_quote(quote, official), QuoteCheck::QuoteMismatch, "{quote}");
        }
        assert_eq!(split_words("2%º ª€"), ["2", "%", "€"]);
    }

    #[test]
    fn marker_shapes() {
        for (chunk, expected) in [
            ("1°", Some(true)),
            ("12º", Some(true)),
            ("1°-", Some(true)),
            ("a)", Some(true)),
            ("(a)", Some(true)),
            ("(1)", Some(true)),
            ("iv)", Some(true)),
            ("I.", Some(false)),
            ("I.-", Some(false)),
            ("II.–", Some(false)),
            ("IV.—", Some(false)),
            ("1.", Some(false)),
            ("1234.", Some(false)),
            ("n°", None),
            ("mois", None),
            ("5", None),
            ("a.", None),
            ("12345.", None),
            ("IIIII.", None),
            ("abc)", None),
            (")", None),
            ("()", None),
            ("(a", None),
            ("", None),
            ("-", None),
        ] {
            assert_eq!(marker_kind(chunk), expected, "{chunk:?}");
        }
    }

    #[test]
    fn markers_are_flagged_only_where_a_list_item_starts() {
        let flags = |text: &str, lenient: bool| -> Vec<(String, bool)> {
            words(text, lenient).into_iter().map(|w| (w.text, w.marker)).collect()
        };
        let marked = |text: &str, lenient: bool| -> Vec<String> {
            flags(text, lenient).into_iter().filter(|(_, m)| *m).map(|(w, _)| w).collect()
        };
        // En tête d'alinéa, après « ; », « : » ou « . » (même détachés), après un autre repère.
        assert_eq!(
            marked("1° a ;\n\n2° b : c) d . 3° e. I. – 1° f", false),
            ["1", "2", "c", "3", "i", "1"]
        );
        assert_eq!(marked("a ; b ;\u{a0}c) d", false), ["c"]);
        // Au milieu d'une phrase : un repère sûr ne l'est que dans l'extrait.
        assert_eq!(marked("le 3° du I", false), Vec::<String>::new());
        assert_eq!(marked("le 3° du I.", true), ["3"]);
        assert_eq!(marked("le 5. du I.", true), Vec::<String>::new());
        // Un blanc seul ou un tiret n'effacent pas l'état de tête d'alinéa.
        assert_eq!(marked("  – 1° a", false), ["1"]);
        assert_eq!(flags("L. 12 mois", false)[0], ("l12".to_owned(), false));
    }

    /// Les chemins en concurrence (sauter un repère ou le lire) convergent : le parcours mémorise
    /// ses états et reste borné face à une suite hostile de repères.
    #[test]
    fn a_hostile_run_of_markers_stays_cheap() {
        let official = format!("{}fin", "1° ".repeat(300));
        let quote = format!("{}fin", "1° ".repeat(300));
        assert_eq!(check_quote(&quote, &official), QuoteCheck::Match);
        let altered = format!("{}fins", "1° ".repeat(300));
        assert_eq!(check_quote(&altered, &official), QuoteCheck::QuoteMismatch);
        let scattered = format!("{}fin", "1° a) ".repeat(150));
        assert_eq!(check_quote(&scattered, &official), QuoteCheck::Match);
    }
}
