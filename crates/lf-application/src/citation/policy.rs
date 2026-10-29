//! Politique de confiance, de budget et de concurrence des résolutions (annexe B §7.1, §7.2, §7.9).
//!
//! Quatre décisions vivent ici, chacune en un seul endroit pour être relue et testée :
//!
//! - la **concurrence** (quatre résolutions à la fois) et le **budget de temps** (80 % du délai de
//!   l'outil) ;
//! - la **preuve d'absence** : la table [`POLICY`] dit, famille par famille, si l'absence d'un
//!   enregistrement permet de conclure `not_found` (§7.2) ;
//! - le **seuil de l'extrait cité** (V-162) : [`check_quote`] compare l'extrait au texte officiel
//!   lu, après repli des accents et de la casse ;
//! - les bornes de consultation (candidats rendus, documents consultés par numéro).

use std::{collections::HashSet, time::Duration};

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

/// Seuil de similarité de l'extrait cité (V-162, **NON VÉRIFIÉ**) : au moins 9 bigrammes de mots
/// de l'extrait sur 10 doivent figurer dans le texte officiel pour que l'extrait soit tenu pour
/// le même. Un mot altéré dans un extrait de vingt mots en fait perdre deux sur dix-neuf : il
/// passe ; deux mots altérés ne passent plus.
pub(crate) const QUOTE_SIMILARITY: (usize, usize) = (9, 10);

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

/// Compare l'extrait cité au texte officiel lu (§7.9, V-162).
///
/// `Match` si l'extrait replié figure tel quel dans le texte replié (suite de mots contiguë), ou si
/// au moins [`QUOTE_SIMILARITY`] de ses bigrammes de mots figurent dans le texte officiel ; sinon
/// `QuoteMismatch`. Un extrait d'un seul mot ne passe que par la première voie. Un extrait sans
/// mot (ponctuation seule) ne peut pas être tenu pour trouvé.
pub(crate) fn check_quote(quoted: &str, official: &str) -> QuoteCheck {
    let wanted = fold_words(quoted);
    if wanted.is_empty() {
        return QuoteCheck::QuoteMismatch;
    }
    let text = fold_words(official);
    if text.windows(wanted.len()).any(|window| window == wanted.as_slice()) {
        return QuoteCheck::Match;
    }
    let known: HashSet<(&str, &str)> =
        text.windows(2).map(|pair| (pair[0].as_str(), pair[1].as_str())).collect();
    let pairs = wanted.len().saturating_sub(1);
    let found = wanted
        .windows(2)
        .filter(|pair| known.contains(&(pair[0].as_str(), pair[1].as_str())))
        .count();
    let (numerator, denominator) = QUOTE_SIMILARITY;
    if pairs > 0 && found * denominator >= pairs * numerator {
        QuoteCheck::Match
    } else {
        QuoteCheck::QuoteMismatch
    }
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
        assert_eq!(QUOTE_SIMILARITY, (9, 10));
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

    #[test]
    fn quote_auto_threshold() {
        // Extrait exact, à la casse, aux accents et à la ponctuation près : correspondance.
        assert_eq!(
            check_quote("chacun a le DROIT au respect de sa vie privee", OFFICIAL),
            QuoteCheck::Match
        );
        // Un mot altéré dans un extrait de vingt mots : deux bigrammes perdus sur dix-neuf, en
        // dessous de 10 %, donc encore une correspondance (seuil de V-162).
        let twenty = "Les juges peuvent prescrire toutes mesures propres à empêcher ou faire cesser \
                      une atteinte à l'intimité de la vie privée";
        let one_altered = twenty.replace("propres", "utiles");
        assert_eq!(check_quote(&one_altered, OFFICIAL), QuoteCheck::Match);
        // Deux mots altérés : quatre bigrammes perdus sur dix-neuf, au-delà du seuil.
        let two_altered = one_altered.replace("faire", "tenter");
        assert_eq!(check_quote(&two_altered, OFFICIAL), QuoteCheck::QuoteMismatch);
        // Un texte sans rapport : écart.
        assert_eq!(
            check_quote("Toute personne dispose du droit de vote", OFFICIAL),
            QuoteCheck::QuoteMismatch
        );
        // Un seul mot ne passe que s'il est contenu tel quel.
        assert_eq!(check_quote("intimité", OFFICIAL), QuoteCheck::Match);
        assert_eq!(check_quote("inexistant", OFFICIAL), QuoteCheck::QuoteMismatch);
        // Ni mot ni correspondance : écart.
        assert_eq!(check_quote(" « … » ", OFFICIAL), QuoteCheck::QuoteMismatch);
        assert_eq!(check_quote("Chacun a le droit", ""), QuoteCheck::QuoteMismatch);
    }
}
