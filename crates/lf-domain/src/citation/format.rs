//! Forme canonique et résolvabilité d'une citation (annexe B §6.5, 03 §8.7).
//!
//! # Forme canonique
//!
//! [`canonical`] écrit ce que le parseur a **reconnu**, à la française : « Code civil, art. 9 »,
//! « Cass. 1re civ., 4 juin 2025, n° 24-13.470 », « CE, 29 mai 2026, n° 506507 ». C'est la valeur
//! `interpreted_as` de `fr_resolve`, qui laisse le modèle voir une mauvaise lecture. Les formes
//! de l'annexe B §6.5 passent par [`crate::contract`] (`article_citation`, `constit_citation`,
//! `cnil_citation`) et par `CivilDate::to_french`. Elle ne porte que ce qui identifie le
//! document cité : ni les subdivisions, ni le qualificatif de version, ni l'origine du code, ni la
//! confiance. L'identifiant d'un document résolu vient toujours du contrat du document lu
//! (jamais construit ici).
//!
//! Une valeur sans forme sûre rend `None` : article sans code (`Missing`, anaphore), article d'un
//! texte inconnu, décision sans juridiction ou sans numéro ni date, citation hors périmètre.
//! [`canonical`] ne lit que la valeur : l'ambiguïté `emetteur_inconnu` d'une délibération sans
//! « CNIL » ni série, ou `appellation` d'un texte, reste portée par le [`Spanned`] et c'est au cas
//! d'usage de la signaler à côté de la forme.
//!
//! # Résolvabilité
//!
//! [`CitationRef::resolvability`] ne regarde que la valeur (famille et champs) ;
//! [`Spanned::effective_resolvability`] y ajoute ce que le parseur a signalé :
//!
//! | Source | Résolvabilité |
//! |---|---|
//! | drapeau de gravité `Impossible` (`date_invalide`, `annee_future`, `chambre_inexistante`, `chambre_cass_inconnue`, `ordinal_*_non_conforme`, `ecli_annee_anterieure_numero`, `annee_pourvoi_posterieure`) | `Impossible(drapeau)`, le premier dans l'ordre du tableau `flags` |
//! | drapeaux `Suspect` et `Info` | aucun effet : l'application les signale dans une note |
//! | `sigle_ambigu:X` sur un article sans code ou une décision sans juridiction | `SigleSansContexte(X)` |
//! | `sigle_ambigu:X` avec un code déjà lu (« 121-3 CP » donne le Code pénal) | aucun effet : lecture par défaut, confiance basse |
//! | `anaphore_non_resolue` | `AnaphoreNonResolue` |
//! | `reference_de_publication_seule` | `PublicationSeule` |
//! | `code_manquant`, `texte_manquant` | `SansCode` |
//! | les autres ambiguïtés | aucun effet : l'application cherche des candidats |

use crate::citation::dict::courts::{CassChamber, CeFormation};
use crate::citation::types::{
    Ambiguity, ArticleRef, CaseNumber, CcNumber, CitationRef, CnilCite, CnilNumber, CodeRef,
    ConventionCite, ConventionKind, Court, DecisionCite, JorfCite, LegifranceIdRef, Resolvability,
    Severity, Spanned, TextCite, TextNature, UnresolvableWhy, UnsupportedCite, UnsupportedFamily,
};
use crate::contract::{article_citation, cnil_citation, constit_citation};
use crate::date::CivilDate;

/// Forme canonique de ce que le parseur a reconnu (« Code civil, art. 9 »), valeur
/// `interpreted_as` de `fr_resolve`. `None` pour une citation sans forme sûre.
pub fn canonical(c: &CitationRef) -> Option<String> {
    match c {
        CitationRef::CodeArticle { code, article, .. } => {
            let title = match code {
                CodeRef::Known(key) => key.title().to_owned(),
                CodeRef::Unknown(title) => capitalize(title.trim()),
                CodeRef::Anaphora(_) | CodeRef::Missing => return None,
            };
            (!title.is_empty()).then(|| with_article(&title, article))
        }
        CitationRef::TextArticle { text, article, .. } => {
            Some(with_article(&text_form(text.as_ref()?)?, article))
        }
        CitationRef::Text(text) => text_form(text),
        CitationRef::Decision(decision) => decision_form(decision),
        CitationRef::Ecli(ecli) => Some(ecli.to_string()),
        CitationRef::CnilDeliberation(cnil) => Some(cnil_form(cnil)),
        CitationRef::Convention(convention) => convention_form(convention),
        CitationRef::Jorf(jorf) => jorf_form(jorf),
        CitationRef::Nor(nor) => Some(format!("NOR {nor}")),
        CitationRef::Eli(eli) => Some(eli.to_string()),
        CitationRef::LegifranceId(id) => Some(legifrance_id_form(id)),
        CitationRef::Unsupported(_) => None,
    }
}

impl CitationRef {
    /// La citation peut-elle être cherchée dans le corpus ?
    ///
    /// Le verdict ne dépend que de la valeur : hors périmètre (UE, CJUE, CEDH, doctrine, ECLI
    /// étranger), recueil seul, article sans code ou sans texte, anaphore sans antécédent,
    /// décision réduite à sa seule mention de publication. Les ambiguïtés et les drapeaux
    /// portés par le [`Spanned`] s'ajoutent dans [`Spanned::effective_resolvability`].
    pub fn resolvability(&self) -> Resolvability {
        match self.unresolvable_why() {
            Some(why) => Resolvability::Unresolvable(why),
            None => Resolvability::Resolvable,
        }
    }

    /// Pourquoi la valeur seule ne peut pas être cherchée.
    fn unresolvable_why(&self) -> Option<UnresolvableWhy> {
        match self {
            Self::CodeArticle { code: CodeRef::Anaphora(_), .. } => {
                Some(UnresolvableWhy::AnaphoreNonResolue)
            }
            Self::CodeArticle { code: CodeRef::Missing, .. }
            | Self::TextArticle { text: None, .. } => Some(UnresolvableWhy::SansCode),
            Self::TextArticle { text: Some(text), .. } | Self::Text(text)
                if is_dangling_anaphora(text) =>
            {
                Some(UnresolvableWhy::AnaphoreNonResolue)
            }
            Self::Decision(decision) if is_publication_only(decision) => {
                Some(UnresolvableWhy::PublicationSeule)
            }
            Self::Unsupported(UnsupportedCite { family: UnsupportedFamily::Recueil, .. }) => {
                Some(UnresolvableWhy::PublicationSeule)
            }
            Self::Unsupported(unsupported) => {
                Some(UnresolvableWhy::HorsPerimetre(unsupported.family))
            }
            _ => None,
        }
    }

    /// La lecture de la famille reste ouverte : article sans code, ou décision sans juridiction.
    /// Un sigle ambigu y bloque (« art. L. 52-8 CE »), alors qu'avec un code déjà lu il laisse la
    /// lecture par défaut (« article 121-3 CP »).
    fn reading_is_open(&self) -> bool {
        matches!(
            self,
            Self::CodeArticle { code: CodeRef::Missing, .. }
                | Self::Decision(DecisionCite { court: None, .. })
        )
    }
}

impl Spanned<CitationRef> {
    /// Résolvabilité compte tenu des drapeaux `Impossible` et des ambiguïtés bloquantes (voir le
    /// tableau du module). Un drapeau `Impossible` l'emporte sur tout ; une ambiguïté plus
    /// précise que le verdict de la valeur (`sigle_ambigu:CE` sur un article sans code) le
    /// remplace.
    pub fn effective_resolvability(&self) -> Resolvability {
        if let Some(flag) = self.flags.iter().find(|flag| flag.severity() == Severity::Impossible) {
            return Resolvability::Unresolvable(UnresolvableWhy::Impossible(flag.clone()));
        }
        let by_ambiguity = match &self.ambiguity {
            Some(Ambiguity::SigleAmbigu(sigle)) if self.value.reading_is_open() => {
                Some(UnresolvableWhy::SigleSansContexte(*sigle))
            }
            Some(Ambiguity::AnaphoreNonResolue) => Some(UnresolvableWhy::AnaphoreNonResolue),
            Some(Ambiguity::ReferenceDePublicationSeule) => Some(UnresolvableWhy::PublicationSeule),
            _ => None,
        };
        let missing =
            matches!(self.ambiguity, Some(Ambiguity::CodeManquant | Ambiguity::TexteManquant));
        match (by_ambiguity, self.value.resolvability()) {
            (Some(why), _) | (None, Resolvability::Unresolvable(why)) => {
                Resolvability::Unresolvable(why)
            }
            (None, Resolvability::Resolvable) if missing => {
                Resolvability::Unresolvable(UnresolvableWhy::SansCode)
            }
            (None, Resolvability::Resolvable) => Resolvability::Resolvable,
        }
    }
}

/// Texte désigné par une anaphore seule (« la loi susvisée ») : ni numéro ni date à chercher.
fn is_dangling_anaphora(text: &TextCite) -> bool {
    text.anaphora.is_some()
        && text.number.is_none()
        && text.date.is_none()
        && text.date_raw.is_none()
}

/// Décision réduite à sa mention de publication (« Bull. civ. I, n° 270 ») : ni numéro d'affaire
/// ni date, donc rien à chercher.
fn is_publication_only(decision: &DecisionCite) -> bool {
    decision.numbers.is_empty() && decision.date.is_none() && decision.publication.is_some()
}

/// Première lettre en majuscule, le reste inchangé (« code de la voirie » donne « Code de la
/// voirie »).
fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

/// Chaque segment séparé par `-` en minuscules sauf sa première lettre (`BASSE-TERRE` donne
/// `Basse-Terre`).
fn title_case(text: &str) -> String {
    text.split('-').map(|part| capitalize(&part.to_lowercase())).collect::<Vec<_>>().join("-")
}

/// « {titre}, art. {numéro} », avec la plage « 1100 à 1100-2 » quand l'article en porte une.
fn with_article(title: &str, article: &ArticleRef) -> String {
    let mut num = article.num.num_key();
    if let Some(end) = &article.range_to {
        num = format!("{num} à {}", end.num_key());
    }
    article_citation(Some(title), Some(&num)).unwrap_or_default()
}

/// Libellé d'une nature de texte. Les quatre textes du bloc de constitutionnalité sont cités
/// par leur titre entier, sans numéro.
const fn nature_label(nature: TextNature) -> &'static str {
    match nature {
        TextNature::Loi => "Loi",
        TextNature::LoiOrganique => "Loi organique",
        TextNature::LoiConstitutionnelle => "Loi constitutionnelle",
        TextNature::LoiDeFinances => "Loi de finances",
        TextNature::LoiDeFinancementSecu => "Loi de financement de la sécurité sociale",
        TextNature::LoiDuPays => "Loi du pays",
        TextNature::Ordonnance => "Ordonnance",
        TextNature::Decret => "Décret",
        TextNature::DecretLoi => "Décret-loi",
        TextNature::Arrete => "Arrêté",
        TextNature::Reglement => "Règlement",
        TextNature::Circulaire => "Circulaire",
        TextNature::Instruction => "Instruction",
        TextNature::Deliberation => "Délibération",
        TextNature::Decision => "Décision",
        TextNature::Constitution => "Constitution du 4 octobre 1958",
        TextNature::Ddhc => "Déclaration des droits de l'homme et du citoyen de 1789",
        TextNature::Preambule1946 => "Préambule de la Constitution du 27 octobre 1946",
        TextNature::CharteEnvironnement => "Charte de l'environnement de 2004",
    }
}

/// Forme d'un texte : « Loi n° 2016-1321 du 7 octobre 2016 ». Sans numéro ni date, seule une
/// appellation d'usage (« Loi Macron ») donne une forme.
fn text_form(text: &TextCite) -> Option<String> {
    if matches!(
        text.nature,
        TextNature::Constitution
            | TextNature::Ddhc
            | TextNature::Preambule1946
            | TextNature::CharteEnvironnement
    ) {
        return Some(nature_label(text.nature).to_owned());
    }
    let mut parts = vec![nature_label(text.nature).to_owned()];
    parts.extend(text.emitter.clone());
    let number = text.number.as_ref().map(|number| format!("n° {}", number.raw));
    let date = text
        .date
        .map(CivilDate::to_french)
        .or_else(|| text.date_raw.clone())
        .map(|date| format!("du {date}"));
    if number.is_none() && date.is_none() {
        parts.push(text.appellation.clone()?);
    } else {
        parts.extend(number);
        parts.extend(date);
    }
    Some(parts.join(" "))
}

/// Libellé d'une chambre de la Cour de cassation : « 1re civ. », « soc. », « ch. mixte ».
const fn chamber_label(chamber: CassChamber) -> &'static str {
    match chamber {
        CassChamber::Civ1 => "1re civ.",
        CassChamber::Civ2 => "2e civ.",
        CassChamber::Civ3 => "3e civ.",
        CassChamber::Com => "com.",
        CassChamber::Soc => "soc.",
        CassChamber::Crim => "crim.",
        CassChamber::AssPlen => "ass. plén.",
        CassChamber::Mixte => "ch. mixte",
        CassChamber::Reunies => "ch. réun.",
        CassChamber::CivUnique => "civ.",
        CassChamber::Requetes => "req.",
        CassChamber::PremierPresident => "ord. prés.",
    }
}

/// Formations du Conseil d'État que l'usage cite dans la référence (« CE, Ass., … »). Les
/// chambres, qui ne changent pas la référence usuelle, ne sont pas rendues.
const fn formation_label(formation: CeFormation) -> Option<&'static str> {
    match formation {
        CeFormation::Assemblee => Some("Ass."),
        CeFormation::Section => Some("Sect."),
        CeFormation::ChambresReunies
        | CeFormation::ChambreSeule
        | CeFormation::SousSectionsReunies
        | CeFormation::SousSectionSeule
        | CeFormation::Ordonnance
        | CeFormation::Avis => None,
    }
}

/// Sièges dont la graphie courante n'est pas celle, sans accents et en majuscules, de la facette
/// ou du siège relevé : `(valeur relevée, graphie courante)`.
const SEAT_LABELS: [(&str, &str); 8] = [
    ("AIX-PROVENCE", "Aix-en-Provence"),
    ("ST-DENIS-REUNION", "Saint-Denis de la Réunion"),
    ("FORT-DE-FRANCE", "Fort-de-France"),
    ("NIMES", "Nîmes"),
    ("BESANCON", "Besançon"),
    ("CHAMBERY", "Chambéry"),
    ("NOUMEA", "Nouméa"),
    ("ORLEANS", "Orléans"),
];

/// Siège d'une cour d'appel ou d'un tribunal judiciaire, relevé en majuscules sans accents
/// (`PARIS`, `AIX-PROVENCE`), dans sa graphie courante.
fn seat_label(seat: &str) -> String {
    SEAT_LABELS
        .iter()
        .find(|(raw, _)| *raw == seat)
        .map_or_else(|| title_case(seat), |(_, label)| (*label).to_owned())
}

/// `{sigle} {siège}`, ou le sigle seul quand le siège n'est pas cité.
fn with_seat(initials: &str, seat: Option<String>) -> String {
    seat.map_or_else(|| initials.to_owned(), |seat| format!("{initials} {seat}"))
}

/// Tête d'une référence de décision : « Cass. 1re civ. », « CE », « CAA Marseille ».
fn court_label(court: &Court) -> String {
    match court {
        Court::CourDeCassation { chamber } => {
            with_seat("Cass.", chamber.map(|chamber| chamber_label(chamber).to_owned()))
        }
        Court::CourAppel { seat, .. } => {
            with_seat("CA", seat.map(|seat| seat_label(seat.as_str())))
        }
        Court::TribunalJudiciaire { seat } => with_seat("TJ", seat.as_deref().map(seat_label)),
        Court::AutreJudiciaire { label } => label.clone(),
        Court::ConseilEtat { formation, .. } => match formation.and_then(formation_label) {
            Some(formation) => format!("CE, {formation}"),
            None => "CE".to_owned(),
        },
        Court::Caa { seat, .. } => with_seat("CAA", seat.map(|seat| seat.as_str().to_owned())),
        Court::Ta { seat, .. } => with_seat("TA", seat.map(|seat| seat.as_str().to_owned())),
        Court::ConseilConstitutionnel => "Cons. const.".to_owned(),
    }
}

/// Numéro d'affaire tel qu'il s'écrit dans une référence.
fn case_number_form(number: &CaseNumber) -> String {
    match number {
        CaseNumber::Pourvoi(pourvoi) => pourvoi.to_string(),
        CaseNumber::Rg(rg) => rg.to_string(),
        CaseNumber::Portalis(text) | CaseNumber::Unknown(text) => text.clone(),
        CaseNumber::CeRequete(request) => request.to_string(),
        CaseNumber::Caa(caa) => caa.to_string(),
        CaseNumber::Ta(ta) => ta.to_string(),
        CaseNumber::Cc(cc) => cc.to_string(),
    }
}

/// « Cons. const., décision n° 2025-1173 QPC du 7 novembre 2025 » ; sans date, la même forme
/// s'arrête au type.
fn constit_form(number: &CcNumber, date: Option<CivilDate>) -> String {
    let num = number.to_string();
    let kind = number.kind.as_str();
    constit_citation(Some(&num), Some(kind), date)
        .unwrap_or_else(|| format!("Cons. const., décision n° {num} {kind}"))
}

/// Forme d'une décision : « Cass. 1re civ., 4 juin 2025, n° 24-13.470 ». Sans juridiction, ou
/// sans numéro ni date, il n'y a pas de forme sûre.
fn decision_form(decision: &DecisionCite) -> Option<String> {
    let court = decision.court.as_ref()?;
    if let (Court::ConseilConstitutionnel, Some(number)) = (
        court,
        decision.numbers.iter().find_map(|number| match number {
            CaseNumber::Cc(cc) => Some(cc),
            _ => None,
        }),
    ) {
        return Some(constit_form(number, decision.date));
    }
    if decision.numbers.is_empty() && decision.date.is_none() {
        return None;
    }
    let mut parts = vec![court_label(court)];
    parts.extend(decision.date.map(CivilDate::to_french));
    if !decision.numbers.is_empty() {
        let numbers: Vec<String> = decision.numbers.iter().map(case_number_form).collect();
        parts.push(format!("n° {}", numbers.join(", ")));
    }
    Some(parts.join(", "))
}

/// « CNIL, délibération n° SAN-2026-002 du 8 janvier 2026 » ; sans date, la forme s'arrête au
/// numéro.
fn cnil_form(cnil: &CnilCite) -> String {
    let number = CnilNumber { series: cnil.series.clone(), year: cnil.year, seq: cnil.seq };
    let number = number.to_string();
    cnil_citation(Some(&number), cnil.date)
        .unwrap_or_else(|| format!("CNIL, délibération n° {number}"))
}

/// Libellé d'une nature d'accord collectif.
const fn convention_label(kind: ConventionKind) -> &'static str {
    match kind {
        ConventionKind::Ccn => "Convention collective nationale",
        ConventionKind::Convention => "Convention collective",
        ConventionKind::Accord => "Accord",
        ConventionKind::Ani => "Accord national interprofessionnel",
        ConventionKind::Avenant => "Avenant",
        ConventionKind::Annexe => "Annexe",
    }
}

/// Forme d'un accord collectif : « Convention collective nationale IDCC 1486 ». Sans IDCC,
/// brochure, avenant ni titre, la citation n'identifie rien.
fn convention_form(convention: &ConventionCite) -> Option<String> {
    if matches!(
        (&convention.idcc, convention.brochure, convention.avenant, &convention.title_hint),
        (None, None, None, None)
    ) {
        return None;
    }
    let kind = convention.kind.map_or("Convention collective", convention_label);
    let mut parts = vec![kind.to_owned()];
    parts.extend(convention.title_hint.clone());
    parts.extend(convention.avenant.map(|avenant| format!("n° {avenant}")));
    parts.extend(convention.idcc.map(|idcc| format!("IDCC {idcc}")));
    parts.extend(convention.brochure.map(|brochure| format!("brochure n° {brochure:04}")));
    parts.extend(convention.date.map(|date| format!("du {}", date.to_french())));
    Some(parts.join(" "))
}

/// « JORF n°0263 du 8 novembre 2025, texte n° 152 ». Une référence sans numéro, date, texte ni
/// page n'identifie rien.
fn jorf_form(jorf: &JorfCite) -> Option<String> {
    if matches!((jorf.number, jorf.date, jorf.texte, jorf.page), (None, None, None, None)) {
        return None;
    }
    let mut head = vec!["JORF".to_owned()];
    head.extend(jorf.number.map(|number| format!("n°{number:04}")));
    head.extend(jorf.date.map(|date| format!("du {}", date.to_french())));
    let mut parts = vec![head.join(" ")];
    parts.extend(jorf.texte.map(|texte| format!("texte n° {texte}")));
    parts.extend(jorf.page.map(|page| format!("p. {page}")));
    Some(parts.join(", "))
}

/// Identifiant Légifrance cité, avec son suffixe de version `_JJ-MM-AAAA` éventuel.
fn legifrance_id_form(id: &LegifranceIdRef) -> String {
    let base = format!("{}{:012}", id.prefix.as_str(), id.digits);
    match id.version_start {
        Some(date) => format!("{base}_{:02}-{:02}-{:04}", date.day(), date.month(), date.year()),
        None => base,
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::citation::dict::codes::CodeKey;
    use crate::citation::dict::courts::{CA_SEATS, CaSeat, CaaSeat, TaSeat};
    use crate::citation::types::{
        Anaphora, BulletinRef, CodeVia, Confidence, EcliFamily, Flag, IdPrefix, Publication, Sigle,
        Span, SpecialNum, Subdivision, TextAnaphora, TextNumber, VersionQualifier,
    };
    use crate::contract::{Eli, Idcc, Nor};
    use crate::ecli::Ecli;

    fn date(year: u16, month: u8, day: u8) -> CivilDate {
        CivilDate::new(year, month, day).expect("date valide")
    }

    fn article(num: &str) -> ArticleRef {
        ArticleRef { num: num.parse().expect("numéro valide"), sub: Vec::new(), range_to: None }
    }

    fn code_article(code: CodeRef, num: &str) -> CitationRef {
        CitationRef::CodeArticle { code, article: article(num), version: None, code_via: None }
    }

    fn known(key: CodeKey, num: &str) -> CitationRef {
        code_article(CodeRef::Known(key), num)
    }

    /// `2016-1321` donne l'année 2016 et le numéro d'ordre 1321.
    fn text_number(raw: &str) -> TextNumber {
        let (year, seq) = raw.split_once('-').expect("AAAA-N");
        TextNumber {
            year: year.parse().ok(),
            seq: seq.parse().expect("numéro d'ordre"),
            extra: None,
            raw: raw.to_owned(),
        }
    }

    fn text(nature: TextNature, number: Option<&str>, on: Option<CivilDate>) -> TextCite {
        TextCite {
            nature,
            number: number.map(text_number),
            date: on,
            date_raw: None,
            title_hint: None,
            anaphora: None,
            appellation: None,
            emitter: None,
        }
    }

    fn law_2016() -> TextCite {
        text(TextNature::Loi, Some("2016-1321"), Some(date(2016, 10, 7)))
    }

    fn decision(
        court: Option<Court>,
        on: Option<CivilDate>,
        numbers: Vec<CaseNumber>,
    ) -> DecisionCite {
        DecisionCite {
            court,
            court_via: None,
            date: on,
            numbers,
            kind: None,
            publication: None,
            solution: None,
            parties: None,
            subject: None,
            arret_num: None,
            formation_code: None,
            publication_codes: Vec::new(),
        }
    }

    fn pourvoi(raw: &str) -> CaseNumber {
        CaseNumber::Pourvoi(raw.parse().expect("pourvoi valide"))
    }

    fn cass(chamber: Option<CassChamber>) -> Court {
        Court::CourDeCassation { chamber }
    }

    fn decision_ref(
        court: Option<Court>,
        on: Option<CivilDate>,
        numbers: Vec<CaseNumber>,
    ) -> CitationRef {
        CitationRef::Decision(decision(court, on, numbers))
    }

    fn cnil(series: Option<&str>, year: u16, seq: u16, on: Option<CivilDate>) -> CnilCite {
        CnilCite {
            series: series.map(str::to_owned),
            year,
            seq,
            date: on,
            formation_restreinte: false,
        }
    }

    fn convention() -> ConventionCite {
        ConventionCite {
            kind: None,
            idcc: None,
            brochure: None,
            avenant: None,
            title_hint: None,
            date: None,
        }
    }

    fn spanned(
        value: CitationRef,
        ambiguity: Option<Ambiguity>,
        flags: Vec<Flag>,
    ) -> Spanned<CitationRef> {
        Spanned {
            value,
            span: Span { start: 0, end: 1 },
            confidence: Confidence::High,
            ambiguity,
            flags,
            enum_group: None,
        }
    }

    #[test]
    fn a_citation_without_flag_has_the_resolvability_of_its_value() {
        let nor = Nor::parse("ECFI1524250L").expect("NOR valide");
        let value = CitationRef::Nor(nor);
        let spanned = Spanned::new(value.clone(), Span { start: 0, end: 12 });
        assert_eq!(spanned.effective_resolvability(), value.resolvability());
        // La forme canonique, quelle qu'elle soit, ne dépend que de la valeur.
        assert_eq!(canonical(&value), canonical(&spanned.value));
    }

    #[test]
    fn canonical_forms_of_annex_b_6_5() {
        // Les six formes de l'annexe B §6.5.
        let cons_const = decision_ref(
            Some(Court::ConseilConstitutionnel),
            Some(date(2025, 11, 7)),
            vec![CaseNumber::Cc("2025-1173 QPC".parse().expect("numéro CC"))],
        );
        let cnil_san =
            CitationRef::CnilDeliberation(cnil(Some("SAN"), 2026, 2, Some(date(2026, 1, 8))));
        let cass_civ1 = decision_ref(
            Some(cass(Some(CassChamber::Civ1))),
            Some(date(2025, 6, 4)),
            vec![pourvoi("N 24-13.470")],
        );
        let ce = decision_ref(
            Some(Court::ConseilEtat { formation: None, chambers: Vec::new() }),
            Some(date(2026, 5, 29)),
            vec![CaseNumber::CeRequete(506_507)],
        );
        let table = [
            (known(CodeKey::CodeCivil, "9"), "Code civil, art. 9"),
            (cons_const, "Cons. const., décision n° 2025-1173 QPC du 7 novembre 2025"),
            (cnil_san, "CNIL, délibération n° SAN-2026-002 du 8 janvier 2026"),
            (CitationRef::Text(law_2016()), "Loi n° 2016-1321 du 7 octobre 2016"),
            (cass_civ1, "Cass. 1re civ., 4 juin 2025, n° 24-13.470"),
            (ce, "CE, 29 mai 2026, n° 506507"),
        ];
        for (citation, expected) in table {
            assert_eq!(canonical(&citation).as_deref(), Some(expected), "{citation:?}");
        }
    }

    #[test]
    fn code_article_forms() {
        let table = [
            (
                known(CodeKey::CodeDeJusticeAdministrative, "L. 761-1"),
                "Code de justice administrative, art. L761-1",
            ),
            (
                known(CodeKey::CodeGeneralDesImpots, "199 undecies B"),
                "Code général des impôts, art. 199 undecies B",
            ),
            (known(CodeKey::CodeCivil, "1er"), "Code civil, art. 1er"),
            (known(CodeKey::CodeCivil, "R*111-1"), "Code civil, art. R*111-1"),
            // Code absent du dictionnaire : le titre cité, avec sa majuscule.
            (
                code_article(CodeRef::Unknown("code de la voirie".to_owned()), "R. 12-1"),
                "Code de la voirie, art. R12-1",
            ),
            (
                code_article(CodeRef::Unknown("  Code rural  ".to_owned()), "L. 1-1"),
                "Code rural, art. L1-1",
            ),
        ];
        for (citation, expected) in table {
            assert_eq!(canonical(&citation).as_deref(), Some(expected), "{citation:?}");
        }

        // Sans code lu, il n'y a pas de forme sûre.
        for code in [
            CodeRef::Missing,
            CodeRef::Anaphora(Anaphora::Dudit),
            CodeRef::Unknown(String::new()),
            CodeRef::Unknown("   ".to_owned()),
        ] {
            assert_eq!(canonical(&code_article(code.clone(), "9")), None, "{code:?}");
        }
    }

    #[test]
    fn code_article_form_renders_ranges_and_ignores_subdivisions_and_versions() {
        let mut range = article("1100");
        range.range_to = Some("1100-2".parse().expect("numéro valide"));
        let citation = CitationRef::CodeArticle {
            code: CodeRef::Known(CodeKey::CodeCivil),
            article: range,
            version: None,
            code_via: None,
        };
        assert_eq!(canonical(&citation).as_deref(), Some("Code civil, art. 1100 à 1100-2"));

        // Subdivision, qualificatif de version et origine du code ne changent pas le document cité.
        let mut with_sub = article("9");
        with_sub.sub = vec![Subdivision::Alinea(2)];
        let citation = CitationRef::CodeArticle {
            code: CodeRef::Known(CodeKey::CodeCivil),
            article: with_sub,
            version: Some(VersionQualifier::Ancien),
            code_via: Some(CodeVia::ContexteDocument),
        };
        assert_eq!(canonical(&citation).as_deref(), Some("Code civil, art. 9"));

        // Alinéa du Préambule, sans numéro d'article : la forme s'arrête au titre du texte.
        let mut preamble = article("1er");
        preamble.num.special = Some(SpecialNum::SansNumero);
        let citation = CitationRef::TextArticle {
            text: Some(text(TextNature::Preambule1946, None, None)),
            article: preamble,
            at_date: None,
        };
        assert_eq!(
            canonical(&citation).as_deref(),
            Some("Préambule de la Constitution du 27 octobre 1946")
        );
    }

    #[test]
    fn text_forms() {
        let on = Some(date(2016, 10, 7));
        let table = [
            (TextNature::Loi, "Loi"),
            (TextNature::LoiOrganique, "Loi organique"),
            (TextNature::LoiConstitutionnelle, "Loi constitutionnelle"),
            (TextNature::LoiDeFinances, "Loi de finances"),
            (TextNature::LoiDeFinancementSecu, "Loi de financement de la sécurité sociale"),
            (TextNature::LoiDuPays, "Loi du pays"),
            (TextNature::Ordonnance, "Ordonnance"),
            (TextNature::Decret, "Décret"),
            (TextNature::DecretLoi, "Décret-loi"),
            (TextNature::Arrete, "Arrêté"),
            (TextNature::Reglement, "Règlement"),
            (TextNature::Circulaire, "Circulaire"),
            (TextNature::Instruction, "Instruction"),
            (TextNature::Deliberation, "Délibération"),
            (TextNature::Decision, "Décision"),
        ];
        for (nature, label) in table {
            let citation = CitationRef::Text(text(nature, Some("2016-1321"), on));
            assert_eq!(
                canonical(&citation),
                Some(format!("{label} n° 2016-1321 du 7 octobre 2016")),
                "{nature:?}"
            );
        }

        // Les textes du bloc de constitutionnalité sont cités par leur titre, numéro et date ignorés.
        let table = [
            (TextNature::Constitution, "Constitution du 4 octobre 1958"),
            (TextNature::Ddhc, "Déclaration des droits de l'homme et du citoyen de 1789"),
            (TextNature::Preambule1946, "Préambule de la Constitution du 27 octobre 1946"),
            (TextNature::CharteEnvironnement, "Charte de l'environnement de 2004"),
        ];
        for (nature, title) in table {
            let citation = CitationRef::Text(text(nature, Some("2016-1321"), on));
            assert_eq!(canonical(&citation).as_deref(), Some(title), "{nature:?}");
        }
    }

    #[test]
    fn text_forms_with_partial_information() {
        let render = |cite: TextCite| canonical(&CitationRef::Text(cite));
        // Date seule, numéro seul.
        let date_only = text(TextNature::Loi, None, Some(date(1978, 7, 17)));
        assert_eq!(render(date_only).as_deref(), Some("Loi du 17 juillet 1978"));
        let number_only = text(TextNature::Decret, Some("93-1419"), None);
        assert_eq!(render(number_only).as_deref(), Some("Décret n° 93-1419"));

        // Émetteur, et date républicaine gardée brute.
        let mut ministerial = text(TextNature::Arrete, None, None);
        ministerial.emitter = Some("ministériel".to_owned());
        ministerial.date_raw = Some("3 brumaire an IV".to_owned());
        assert_eq!(render(ministerial).as_deref(), Some("Arrêté ministériel du 3 brumaire an IV"));

        // Anaphore avec date : la date suffit ; la date lue l'emporte sur la date brute.
        let mut anaphora = text(TextNature::Loi, None, Some(date(2023, 11, 20)));
        anaphora.anaphora = Some(TextAnaphora::MentionneeCiDessus);
        anaphora.date_raw = Some("ignorée".to_owned());
        assert_eq!(render(anaphora).as_deref(), Some("Loi du 20 novembre 2023"));

        // Appellation d'usage seule, puis ignorée dès qu'un numéro est lu.
        let mut appellation = text(TextNature::Loi, None, None);
        appellation.appellation = Some("Macron".to_owned());
        assert_eq!(render(appellation.clone()).as_deref(), Some("Loi Macron"));
        appellation.number = Some(text_number("2015-990"));
        assert_eq!(render(appellation).as_deref(), Some("Loi n° 2015-990"));

        // Ni numéro, ni date, ni appellation : rien de sûr.
        assert_eq!(render(text(TextNature::Loi, None, None)), None);
        let mut dangling = text(TextNature::Loi, None, None);
        dangling.anaphora = Some(TextAnaphora::Susvisee);
        assert_eq!(render(dangling), None);
    }

    #[test]
    fn text_article_forms() {
        let in_text = |cite: Option<TextCite>, num: &str| CitationRef::TextArticle {
            text: cite,
            article: article(num),
            at_date: None,
        };
        let table = [
            (in_text(Some(law_2016()), "3"), Some("Loi n° 2016-1321 du 7 octobre 2016, art. 3")),
            (
                in_text(Some(text(TextNature::Constitution, None, None)), "61-1"),
                Some("Constitution du 4 octobre 1958, art. 61-1"),
            ),
            (
                in_text(Some(text(TextNature::Ddhc, None, None)), "16"),
                Some("Déclaration des droits de l'homme et du citoyen de 1789, art. 16"),
            ),
            // Texte inconnu (« la Convention »), ou texte sans forme sûre.
            (in_text(None, "6"), None),
            (in_text(Some(text(TextNature::Loi, None, None)), "6"), None),
        ];
        for (citation, expected) in table {
            assert_eq!(canonical(&citation).as_deref(), expected, "{citation:?}");
        }
    }

    #[test]
    fn court_forms() {
        let on = Some(date(2025, 6, 4));
        let render = |court: Court| {
            canonical(&decision_ref(Some(court), on, vec![CaseNumber::Unknown("42".to_owned())]))
        };
        let chambers = [
            (CassChamber::Civ1, "1re civ."),
            (CassChamber::Civ2, "2e civ."),
            (CassChamber::Civ3, "3e civ."),
            (CassChamber::Com, "com."),
            (CassChamber::Soc, "soc."),
            (CassChamber::Crim, "crim."),
            (CassChamber::AssPlen, "ass. plén."),
            (CassChamber::Mixte, "ch. mixte"),
            (CassChamber::Reunies, "ch. réun."),
            (CassChamber::CivUnique, "civ."),
            (CassChamber::Requetes, "req."),
            (CassChamber::PremierPresident, "ord. prés."),
        ];
        for (chamber, label) in chambers {
            assert_eq!(
                render(cass(Some(chamber))),
                Some(format!("Cass. {label}, 4 juin 2025, n° 42")),
                "{chamber:?}"
            );
        }
        assert_eq!(render(cass(None)).as_deref(), Some("Cass., 4 juin 2025, n° 42"));

        // Conseil d'État : seules l'assemblée et la section figurent dans la référence usuelle.
        let ce = |formation| Court::ConseilEtat { formation, chambers: vec![2, 7] };
        assert_eq!(render(ce(None)).as_deref(), Some("CE, 4 juin 2025, n° 42"));
        assert_eq!(
            render(ce(Some(CeFormation::Assemblee))).as_deref(),
            Some("CE, Ass., 4 juin 2025, n° 42")
        );
        assert_eq!(
            render(ce(Some(CeFormation::Section))).as_deref(),
            Some("CE, Sect., 4 juin 2025, n° 42")
        );
        for formation in CeFormation::ALL.into_iter().skip(2) {
            assert_eq!(
                render(ce(Some(formation))).as_deref(),
                Some("CE, 4 juin 2025, n° 42"),
                "{formation:?}"
            );
        }

        // Juridictions du fond, avec ou sans siège.
        let seat = |facet| CaSeat::from_facet(facet).expect("siège connu");
        let marseille = CaaSeat::parse_folded("marseille");
        let nimes = TaSeat::parse_folded("nimes");
        assert!(marseille.is_some() && nimes.is_some());
        let table = [
            (Court::CourAppel { seat: Some(seat("PARIS")), chamber: None }, "CA Paris"),
            (Court::CourAppel { seat: None, chamber: Some("Pôle 1".to_owned()) }, "CA"),
            (Court::TribunalJudiciaire { seat: Some("LYON".to_owned()) }, "TJ Lyon"),
            (Court::TribunalJudiciaire { seat: Some("NIMES".to_owned()) }, "TJ Nîmes"),
            (Court::TribunalJudiciaire { seat: None }, "TJ"),
            (Court::AutreJudiciaire { label: "T. com. Lille".to_owned() }, "T. com. Lille"),
            (Court::Caa { seat: marseille, chamber: None }, "CAA Marseille"),
            (Court::Caa { seat: None, chamber: None }, "CAA"),
            (Court::Ta { seat: nimes, chamber: Some("3e chambre".to_owned()) }, "TA Nîmes"),
            (Court::Ta { seat: None, chamber: None }, "TA"),
            (Court::ConseilConstitutionnel, "Cons. const."),
        ];
        for (court, head) in table {
            assert_eq!(
                render(court.clone()),
                Some(format!("{head}, 4 juin 2025, n° 42")),
                "{court:?}"
            );
        }
    }

    #[test]
    fn seats_are_written_the_usual_way() {
        let table = [
            ("PARIS", "Paris"),
            ("BASSE-TERRE", "Basse-Terre"),
            ("AIX-PROVENCE", "Aix-en-Provence"),
            ("ST-DENIS-REUNION", "Saint-Denis de la Réunion"),
            ("FORT-DE-FRANCE", "Fort-de-France"),
            ("NIMES", "Nîmes"),
            ("BESANCON", "Besançon"),
            ("CHAMBERY", "Chambéry"),
            ("NOUMEA", "Nouméa"),
            ("ORLEANS", "Orléans"),
            ("", ""),
        ];
        for (raw, expected) in table {
            assert_eq!(seat_label(raw), expected, "{raw}");
        }
        // Les 35 sièges de la facette sont lisibles : majuscule initiale, jamais en capitales.
        for facet in CA_SEATS {
            let label = seat_label(facet);
            assert!(label.starts_with(|c: char| c.is_uppercase()), "{label}");
            assert!(label.chars().any(char::is_lowercase), "{label}");
        }
        assert_eq!(capitalize("éléphant"), "Éléphant");
        assert_eq!(capitalize(""), "");
    }

    #[test]
    fn decision_forms_need_a_court_and_a_number_or_a_date() {
        let ce = || Some(Court::ConseilEtat { formation: None, chambers: Vec::new() });
        let numbers = || vec![CaseNumber::CeRequete(506_507)];
        let table = [
            // Numéro seul, date seule, les deux.
            (decision_ref(ce(), None, numbers()), Some("CE, n° 506507")),
            (decision_ref(ce(), Some(date(2026, 5, 29)), Vec::new()), Some("CE, 29 mai 2026")),
            (
                decision_ref(ce(), Some(date(2026, 5, 29)), numbers()),
                Some("CE, 29 mai 2026, n° 506507"),
            ),
            // Plusieurs numéros joints.
            (
                decision_ref(
                    Some(cass(Some(CassChamber::Soc))),
                    Some(date(2025, 6, 4)),
                    vec![pourvoi("24-13.470"), pourvoi("24-13.471")],
                ),
                Some("Cass. soc., 4 juin 2025, n° 24-13.470, 24-13.471"),
            ),
            // Sans juridiction (N21, N28), ou sans numéro ni date (N31, juridiction seule).
            (decision_ref(None, Some(date(2026, 5, 29)), numbers()), None),
            (decision_ref(ce(), None, Vec::new()), None),
            (decision_ref(Some(cass(None)), None, Vec::new()), None),
        ];
        for (citation, expected) in table {
            assert_eq!(canonical(&citation).as_deref(), expected, "{citation:?}");
        }
    }

    #[test]
    fn case_numbers_are_written_as_cited() {
        let on = Some(date(2025, 6, 4));
        let cases = [
            (pourvoi("21-23174"), "21-23.174"),
            (CaseNumber::Rg("24/15215".parse().expect("RG")), "24/15215"),
            (CaseNumber::Portalis("35L7-V-B7H-CIDHB".to_owned()), "35L7-V-B7H-CIDHB"),
            (CaseNumber::CeRequete(216_642), "216642"),
            (CaseNumber::Caa("24MA02429".parse().expect("CAA")), "24MA02429"),
            (CaseNumber::Ta("2005064".parse().expect("TA")), "2005064"),
            (CaseNumber::Cc("2010-14/22 QPC".parse().expect("CC")), "2010-14/22"),
            (CaseNumber::Unknown("A-1".to_owned()), "A-1"),
        ];
        for (number, expected) in cases {
            let citation = decision_ref(Some(cass(None)), on, vec![number.clone()]);
            assert_eq!(
                canonical(&citation),
                Some(format!("Cass., 4 juin 2025, n° {expected}")),
                "{number:?}"
            );
        }
    }

    #[test]
    fn constitutional_council_decisions() {
        let number = |text: &str| CaseNumber::Cc(text.parse().expect("numéro CC"));
        let render = |on, numbers| {
            canonical(&decision_ref(Some(Court::ConseilConstitutionnel), on, numbers))
        };
        // Avec date : la forme de `contract::constit_citation`.
        assert_eq!(
            render(Some(date(2021, 5, 20)), vec![number("2021-817 DC")]).as_deref(),
            Some("Cons. const., décision n° 2021-817 DC du 20 mai 2021")
        );
        // Sans date : elle s'arrête au type.
        assert_eq!(
            render(None, vec![number("2025-1173 QPC")]).as_deref(),
            Some("Cons. const., décision n° 2025-1173 QPC")
        );
        // Numéros joints et rectification gardés.
        assert_eq!(
            render(None, vec![number("2015-491R QPC")]).as_deref(),
            Some("Cons. const., décision n° 2015-491R QPC")
        );
        // Aucun numéro du Conseil : la forme générique.
        assert_eq!(
            render(Some(date(2021, 5, 20)), vec![CaseNumber::Unknown("x".to_owned())]).as_deref(),
            Some("Cons. const., 20 mai 2021, n° x")
        );
        assert_eq!(render(None, Vec::new()), None);
    }

    #[test]
    fn cnil_forms() {
        let render = |series, year, seq, on| {
            canonical(&CitationRef::CnilDeliberation(cnil(series, year, seq, on)))
        };
        assert_eq!(
            render(Some("SAN"), 2026, 2, Some(date(2026, 1, 8))).as_deref(),
            Some("CNIL, délibération n° SAN-2026-002 du 8 janvier 2026")
        );
        // Sans série ni date : le numéro seul.
        assert_eq!(render(None, 2023, 62, None).as_deref(), Some("CNIL, délibération n° 2023-062"));
    }

    #[test]
    fn convention_forms() {
        let render = |cite: ConventionCite| canonical(&CitationRef::Convention(cite));
        let mut syntec = convention();
        syntec.kind = Some(ConventionKind::Ccn);
        syntec.idcc = Idcc::parse("1486");
        assert_eq!(
            render(syntec.clone()).as_deref(),
            Some("Convention collective nationale IDCC 1486")
        );

        // Sans nature : « Convention collective » ; brochure sur quatre chiffres.
        let mut brochure = convention();
        brochure.brochure = Some(3018);
        brochure.idcc = Idcc::parse("1486");
        assert_eq!(
            render(brochure).as_deref(),
            Some("Convention collective IDCC 1486 brochure n° 3018")
        );

        // Titre, avenant et date.
        let mut avenant = convention();
        avenant.kind = Some(ConventionKind::Avenant);
        avenant.avenant = Some(18);
        avenant.date = Some(date(2020, 3, 3));
        avenant.title_hint = Some("à la convention de l'édition".to_owned());
        assert_eq!(
            render(avenant).as_deref(),
            Some("Avenant à la convention de l'édition n° 18 du 3 mars 2020")
        );

        let kinds = [
            (ConventionKind::Convention, "Convention collective"),
            (ConventionKind::Accord, "Accord"),
            (ConventionKind::Ani, "Accord national interprofessionnel"),
            (ConventionKind::Annexe, "Annexe"),
        ];
        for (kind, label) in kinds {
            let mut cite = syntec.clone();
            cite.kind = Some(kind);
            assert_eq!(render(cite), Some(format!("{label} IDCC 1486")), "{kind:?}");
        }

        // Ni IDCC, ni brochure, ni avenant, ni titre : rien n'est identifié.
        let mut dated = convention();
        dated.date = Some(date(2020, 3, 3));
        assert_eq!(render(dated), None);
        assert_eq!(render(convention()), None);
    }

    #[test]
    fn jorf_forms() {
        let render = |jorf: JorfCite| canonical(&CitationRef::Jorf(jorf));
        let full = JorfCite {
            number: Some(263),
            date: Some(date(2025, 11, 8)),
            texte: Some(152),
            page: Some(12),
        };
        assert_eq!(
            render(full).as_deref(),
            Some("JORF n°0263 du 8 novembre 2025, texte n° 152, p. 12")
        );
        let date_only =
            JorfCite { number: None, date: Some(date(2025, 11, 8)), texte: None, page: None };
        assert_eq!(render(date_only).as_deref(), Some("JORF du 8 novembre 2025"));
        let texte_only = JorfCite { number: None, date: None, texte: Some(7), page: None };
        assert_eq!(render(texte_only).as_deref(), Some("JORF, texte n° 7"));
        let page_only = JorfCite { number: Some(5), date: None, texte: None, page: Some(3) };
        assert_eq!(render(page_only).as_deref(), Some("JORF n°0005, p. 3"));
        assert_eq!(render(JorfCite { number: None, date: None, texte: None, page: None }), None);
    }

    #[test]
    fn identifier_forms_are_the_cited_identifiers() {
        let judicial = Ecli::parse("ECLI:FR:CCASS:2025:C100399").expect("ECLI valide");
        assert_eq!(
            canonical(&CitationRef::Ecli(judicial)).as_deref(),
            Some("ECLI:FR:CCASS:2025:C100399")
        );

        let nor = Nor::parse("ecfi1524250l").expect("NOR valide");
        assert_eq!(canonical(&CitationRef::Nor(nor)).as_deref(), Some("NOR ECFI1524250L"));

        let eli = Eli::parse("/eli/decret/2021/7/13/PRMD2117108D/jo/article_1").expect("ELI");
        assert_eq!(
            canonical(&CitationRef::Eli(eli)).as_deref(),
            Some("https://www.legifrance.gouv.fr/eli/decret/2021/7/13/PRMD2117108D/jo/article_1")
        );

        let id = |version_start| {
            CitationRef::LegifranceId(LegifranceIdRef {
                prefix: IdPrefix::LegiArti,
                digits: 6_419_288,
                version_start,
                url_kind: None,
            })
        };
        assert_eq!(canonical(&id(None)).as_deref(), Some("LEGIARTI000006419288"));
        assert_eq!(
            canonical(&id(Some(date(2018, 1, 1)))).as_deref(),
            Some("LEGIARTI000006419288_01-01-2018")
        );
    }

    #[test]
    fn unsupported_citations_have_no_canonical_form() {
        for family in [
            UnsupportedFamily::Ue,
            UnsupportedFamily::Cjue,
            UnsupportedFamily::Cedh,
            UnsupportedFamily::Doctrine,
            UnsupportedFamily::Recueil,
            UnsupportedFamily::ForeignEcli,
        ] {
            let citation = CitationRef::Unsupported(UnsupportedCite {
                family,
                key: Some("85/374/CEE".to_owned()),
            });
            assert_eq!(canonical(&citation), None, "{family:?}");
        }
    }

    #[test]
    fn resolvability_by_kind() {
        use Resolvability::{Resolvable, Unresolvable};

        let judicial = Ecli::parse("ECLI:FR:CCASS:2025:C100399").expect("ECLI valide");
        let nor = Nor::parse("ECFI1524250L").expect("NOR valide");
        let eli = Eli::parse("/eli/jo/2019/1/25/0021").expect("ELI valide");
        let id = LegifranceIdRef {
            prefix: IdPrefix::JorfText,
            digits: 1,
            version_start: None,
            url_kind: None,
        };
        let jorf = JorfCite { number: None, date: None, texte: None, page: None };
        let unsupported = |family| CitationRef::Unsupported(UnsupportedCite { family, key: None });
        let table = [
            // Les douze familles, dans le cas nominal.
            (known(CodeKey::CodeCivil, "9"), Resolvable),
            (
                CitationRef::TextArticle {
                    text: Some(law_2016()),
                    article: article("3"),
                    at_date: None,
                },
                Resolvable,
            ),
            (CitationRef::Text(law_2016()), Resolvable),
            (decision_ref(Some(cass(None)), None, vec![pourvoi("24-13.470")]), Resolvable),
            (CitationRef::Ecli(judicial), Resolvable),
            (CitationRef::CnilDeliberation(cnil(Some("SAN"), 2026, 2, None)), Resolvable),
            (CitationRef::Convention(convention()), Resolvable),
            (CitationRef::Jorf(jorf), Resolvable),
            (CitationRef::Nor(nor), Resolvable),
            (CitationRef::Eli(eli), Resolvable),
            (CitationRef::LegifranceId(id), Resolvable),
            // Hors périmètre : une raison par famille, le recueil étant une publication seule.
            (
                unsupported(UnsupportedFamily::Ue),
                Unresolvable(UnresolvableWhy::HorsPerimetre(UnsupportedFamily::Ue)),
            ),
            (
                unsupported(UnsupportedFamily::Cjue),
                Unresolvable(UnresolvableWhy::HorsPerimetre(UnsupportedFamily::Cjue)),
            ),
            (
                unsupported(UnsupportedFamily::Cedh),
                Unresolvable(UnresolvableWhy::HorsPerimetre(UnsupportedFamily::Cedh)),
            ),
            (
                unsupported(UnsupportedFamily::Doctrine),
                Unresolvable(UnresolvableWhy::HorsPerimetre(UnsupportedFamily::Doctrine)),
            ),
            (
                unsupported(UnsupportedFamily::ForeignEcli),
                Unresolvable(UnresolvableWhy::HorsPerimetre(UnsupportedFamily::ForeignEcli)),
            ),
            (
                unsupported(UnsupportedFamily::Recueil),
                Unresolvable(UnresolvableWhy::PublicationSeule),
            ),
            // Article sans code, anaphore de code sans antécédent.
            (code_article(CodeRef::Missing, "9"), Unresolvable(UnresolvableWhy::SansCode)),
            (
                code_article(CodeRef::Anaphora(Anaphora::MemeCode), "9"),
                Unresolvable(UnresolvableWhy::AnaphoreNonResolue),
            ),
            (code_article(CodeRef::Unknown("code de la voirie".to_owned()), "9"), Resolvable),
            // Article d'un texte inconnu (« la Convention »).
            (
                CitationRef::TextArticle { text: None, article: article("6"), at_date: None },
                Unresolvable(UnresolvableWhy::SansCode),
            ),
        ];
        for (citation, expected) in table {
            assert_eq!(citation.resolvability(), expected, "{citation:?}");
        }
    }

    #[test]
    fn resolvability_of_texts_and_decisions_depends_on_their_fields() {
        use Resolvability::{Resolvable, Unresolvable};

        // « La loi susvisée » : une anaphore sans rien à chercher ; avec une date, c'est cherchable.
        let mut dangling = text(TextNature::Loi, None, None);
        dangling.anaphora = Some(TextAnaphora::Susvisee);
        let in_text = CitationRef::TextArticle {
            text: Some(dangling.clone()),
            article: article("16"),
            at_date: None,
        };
        let why = Unresolvable(UnresolvableWhy::AnaphoreNonResolue);
        assert_eq!(CitationRef::Text(dangling.clone()).resolvability(), why);
        assert_eq!(in_text.resolvability(), why);
        let mut dated = dangling.clone();
        dated.date = Some(date(2023, 11, 20));
        assert_eq!(CitationRef::Text(dated).resolvability(), Resolvable);
        let mut raw = dangling.clone();
        raw.date_raw = Some("3 brumaire an IV".to_owned());
        assert_eq!(CitationRef::Text(raw).resolvability(), Resolvable);
        let mut numbered = dangling;
        numbered.number = Some(text_number("2016-1321"));
        assert_eq!(CitationRef::Text(numbered).resolvability(), Resolvable);

        // Une appellation d'usage reste cherchable par le cas d'usage.
        let mut appellation = text(TextNature::Loi, None, None);
        appellation.appellation = Some("Macron".to_owned());
        assert_eq!(CitationRef::Text(appellation).resolvability(), Resolvable);

        // « Bull. civ. I, n° 270 » : une publication, rien d'autre.
        let publication = Publication {
            bulletin: Some(BulletinRef::Reference("Bull. civ. I, n° 270".to_owned())),
            lebon: None,
        };
        let mut bulletin = decision(Some(cass(None)), None, Vec::new());
        bulletin.publication = Some(publication.clone());
        assert_eq!(
            CitationRef::Decision(bulletin.clone()).resolvability(),
            Unresolvable(UnresolvableWhy::PublicationSeule)
        );
        // Avec un numéro ou une date, la publication n'est qu'un détail.
        bulletin.numbers = vec![pourvoi("24-13.470")];
        assert_eq!(CitationRef::Decision(bulletin.clone()).resolvability(), Resolvable);
        bulletin.numbers = Vec::new();
        bulletin.date = Some(date(2018, 3, 28));
        assert_eq!(CitationRef::Decision(bulletin).resolvability(), Resolvable);
        // Sans juridiction ni publication, le cas d'usage cherche par numéro (N21, N28).
        let numbered = decision_ref(None, None, vec![CaseNumber::CeRequete(506_507)]);
        assert_eq!(numbered.resolvability(), Resolvable);
        assert_eq!(decision_ref(None, None, Vec::new()).resolvability(), Resolvable);
    }

    #[test]
    fn impossible_flags_make_a_citation_unresolvable() {
        let citation = known(CodeKey::CodeCivil, "9");
        for flag in [
            Flag::DateInvalide,
            Flag::AnneeFuture,
            Flag::ChambreInexistante("civ4".to_owned()),
            Flag::ChambreCassInconnue("XX".to_owned()),
            Flag::EcliOrdinalNonConforme(EcliFamily::Cc),
            Flag::EcliAnneeAnterieureNumero,
            Flag::AnneePourvoiPosterieure,
        ] {
            assert_eq!(flag.severity(), Severity::Impossible, "{flag:?}");
            let flagged = spanned(citation.clone(), None, vec![flag.clone()]);
            assert_eq!(
                flagged.effective_resolvability(),
                Resolvability::Unresolvable(UnresolvableWhy::Impossible(flag.clone())),
                "{flag:?}"
            );
        }

        // Le premier drapeau impossible, après les drapeaux sans gravité ; il l'emporte sur la
        // valeur (hors périmètre) et sur l'ambiguïté.
        let mixed = spanned(
            CitationRef::Unsupported(UnsupportedCite {
                family: UnsupportedFamily::Cjue,
                key: None,
            }),
            Some(Ambiguity::AnaphoreNonResolue),
            vec![
                Flag::MojibakeRepare,
                Flag::AnneeImprobable,
                Flag::DateInvalide,
                Flag::AnneeFuture,
            ],
        );
        assert_eq!(
            mixed.effective_resolvability(),
            Resolvability::Unresolvable(UnresolvableWhy::Impossible(Flag::DateInvalide))
        );
    }

    #[test]
    fn suspect_and_info_flags_do_not_block() {
        let flags = vec![
            Flag::EcliCodeCeSeul,
            Flag::AnneeNumeroDifferenteAnneeDate,
            Flag::AnneeImprobable,
            Flag::TypeCcInconnu("X".to_owned()),
            Flag::CaaCodeSiegeIncoherent,
            Flag::DateRepublicaine,
            Flag::MojibakeRepare,
        ];
        for flag in &flags {
            assert_ne!(flag.severity(), Severity::Impossible, "{flag:?}");
        }
        let flagged = spanned(known(CodeKey::CodeCivil, "9"), None, flags);
        assert_eq!(flagged.effective_resolvability(), Resolvability::Resolvable);
    }

    #[test]
    fn blocking_ambiguities_decide_the_effective_resolvability() {
        use Resolvability::{Resolvable, Unresolvable};

        let article_without_code = code_article(CodeRef::Missing, "L. 52-8");
        let article_of_penal_code = known(CodeKey::CodePenal, "121-3");
        let without_court = decision_ref(None, None, vec![CaseNumber::CeRequete(506_507)]);
        let with_court = decision_ref(
            Some(Court::ConseilEtat { formation: None, chambers: Vec::new() }),
            None,
            vec![CaseNumber::CeRequete(506_507)],
        );
        let bulletin = {
            let mut cite = decision(Some(cass(None)), None, Vec::new());
            cite.publication =
                Some(Publication { bulletin: Some(BulletinRef::Publie), lebon: None });
            CitationRef::Decision(cite)
        };
        let table = [
            // N04 : « art. L. 52-8 CE » n'est jamais lu comme un code.
            (
                article_without_code.clone(),
                Some(Ambiguity::SigleAmbigu(Sigle::Ce)),
                Unresolvable(UnresolvableWhy::SigleSansContexte(Sigle::Ce)),
            ),
            // N05 : « article 121-3 CP » est lu comme le Code pénal, en confiance basse.
            (article_of_penal_code.clone(), Some(Ambiguity::SigleAmbigu(Sigle::Cp)), Resolvable),
            // Sigle sur une décision : bloquant sans juridiction, sans effet avec.
            (
                without_court.clone(),
                Some(Ambiguity::SigleAmbigu(Sigle::Cc)),
                Unresolvable(UnresolvableWhy::SigleSansContexte(Sigle::Cc)),
            ),
            (with_court.clone(), Some(Ambiguity::SigleAmbigu(Sigle::Ce)), Resolvable),
            // Anaphore, publication seule, code et texte manquants.
            (
                article_of_penal_code.clone(),
                Some(Ambiguity::AnaphoreNonResolue),
                Unresolvable(UnresolvableWhy::AnaphoreNonResolue),
            ),
            (
                article_without_code.clone(),
                Some(Ambiguity::AnaphoreNonResolue),
                Unresolvable(UnresolvableWhy::AnaphoreNonResolue),
            ),
            (
                bulletin,
                Some(Ambiguity::ReferenceDePublicationSeule),
                Unresolvable(UnresolvableWhy::PublicationSeule),
            ),
            (
                article_of_penal_code.clone(),
                Some(Ambiguity::ReferenceDePublicationSeule),
                Unresolvable(UnresolvableWhy::PublicationSeule),
            ),
            (
                article_of_penal_code.clone(),
                Some(Ambiguity::CodeManquant),
                Unresolvable(UnresolvableWhy::SansCode),
            ),
            (
                article_of_penal_code.clone(),
                Some(Ambiguity::TexteManquant),
                Unresolvable(UnresolvableWhy::SansCode),
            ),
            (
                article_without_code.clone(),
                Some(Ambiguity::CodeManquant),
                Unresolvable(UnresolvableWhy::SansCode),
            ),
            // Sans ambiguïté, c'est le verdict de la valeur.
            (article_without_code, None, Unresolvable(UnresolvableWhy::SansCode)),
            (article_of_penal_code, None, Resolvable),
        ];
        for (value, ambiguity, expected) in table {
            let citation = spanned(value.clone(), ambiguity.clone(), Vec::new());
            assert_eq!(citation.effective_resolvability(), expected, "{value:?} / {ambiguity:?}");
        }
    }

    #[test]
    fn other_ambiguities_leave_the_search_to_the_use_case() {
        let value = decision_ref(None, Some(date(2025, 11, 7)), vec![CaseNumber::CeRequete(1)]);
        let ambiguities = [
            Ambiguity::CodeImplicite,
            Ambiguity::NumeroManquant,
            Ambiguity::JuridictionManquante,
            Ambiguity::JuridictionEtTypeManquants,
            Ambiguity::EmetteurInconnu,
            Ambiguity::ConventionInconnue,
            Ambiguity::ConventionNonIdentifiee,
            Ambiguity::Appellation,
            Ambiguity::CodeAVariantes,
        ];
        for ambiguity in ambiguities {
            let citation = spanned(value.clone(), Some(ambiguity.clone()), Vec::new());
            assert_eq!(
                citation.effective_resolvability(),
                Resolvability::Resolvable,
                "{ambiguity:?}"
            );
        }
    }
}
