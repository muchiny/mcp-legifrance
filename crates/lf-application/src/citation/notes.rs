//! Notes rendues avec les citations (annexe B §7.10) : sandbox, budget épuisé, écart, ECLI natif,
//! extrait cité.
//!
//! Une [`Note`] est une valeur structurée : un gabarit fixe et des paramètres **typés**. Aucun
//! paramètre n'est une chaîne libre venue de l'API ou du texte soumis : le titre d'un code vient du
//! dictionnaire statique (`CodeKey::title`), les familles, juridictions et motifs sont des
//! vocabulaires fermés (`&'static str`), les dates sont rendues en ISO, les numéros par leur forme
//! canonique. [`Note::text`] la rend en anglais, bornée à 200 caractères (assainissement des sorties,
//! 05 §5.9) ; le gabarit et ses paramètres vivent côte à côte pour ne pas pouvoir diverger.

use lf_domain::{
    citation::Sigle,
    date::{CivilDate, VersionDate},
    ecli::Ecli,
    fond::Fond,
};

use crate::messages;

/// Longueur maximale, en caractères, du texte d'une note (annexe B §7.10).
pub const NOTE_MAX_CHARS: usize = 200;

/// Nombre maximal de dates rendues par [`Note::NoRecordAtDate`].
pub(crate) const FOUND_DATES_MAX: usize = 5;

/// Phrase de l'écart d'extrait, en tête de [`Note::QuoteMismatch`] et de [`Note::WithQuoteMismatch`].
const QUOTE_GAP: &str = "Quoted text not found in the official text.";

/// Texte de [`Note::NoCitationRecognised`].
const NO_CITATION: &str = "No citation recognised. Only citations introduced by 'article' or \
     'art.' are read; an empty result does not prove the text cites nothing.";

/// Texte de [`Note::NumberlessText`].
const NUMBERLESS_TEXT: &str = "A text cited without a number cannot be checked here; find it with \
     fr_search (query: its title and date).";

/// Texte de [`Note::NoDecisionReferences`].
const NO_DECISION_REFERENCES: &str = "The API gives no references for this decision; its text may \
     still cite some: read it with fr_get_decision and check it with fr_verify_citations.";

/// Champ dont l'écart donne `invalid` (annexe B §6.6, champs durs de §7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// Date de la décision ou du texte.
    Date,
    /// Numéro (affaire, décision, délibération, texte).
    Number,
    /// Chambre.
    Chamber,
    /// Formation.
    Formation,
    /// Siège de la juridiction.
    Seat,
    /// ECLI natif.
    Ecli,
    /// Nature du texte.
    Nature,
    /// Type de décision du Conseil constitutionnel.
    CcType,
    /// Code.
    Code,
    /// Numéro d'article.
    ArticleNum,
}

impl Field {
    /// Libellé anglais du champ, tel qu'il entre dans le gabarit `Mismatch`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Date => "date",
            Self::Number => "number",
            Self::Chamber => "chamber",
            Self::Formation => "formation",
            Self::Seat => "seat",
            Self::Ecli => "ECLI",
            Self::Nature => "nature",
            Self::CcType => "decision type",
            Self::Code => "code",
            Self::ArticleNum => "article number",
        }
    }
}

/// Pourquoi une citation n'a pas été contrôlée (gabarit `NotChecked`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotCheckedReason {
    /// Erreur du corpus amont sur cette citation.
    Upstream,
    /// Le budget de temps (80 % du délai de l'outil) est épuisé.
    TimeBudget,
}

impl NotCheckedReason {
    /// Motif en anglais.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Upstream => "upstream error",
            Self::TimeBudget => "time budget",
        }
    }
}

/// Note attachée à une citation contrôlée ou résolue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// Article absent à la date : `not_found` (production).
    NotInForce {
        /// Titre du code, tiré du dictionnaire statique ou de l'identifiant validé de l'entrée retenue.
        code: String,
        /// Numéro d'article, sous sa forme de clé.
        num: String,
        /// Date de la recherche.
        date: VersionDate,
    },
    /// Aucun enregistrement : `not_found` (production).
    NoRecord {
        /// Famille, vocabulaire fermé.
        family: &'static str,
        /// Valeur recherchée, sous sa forme canonique.
        value: String,
        /// Date de la recherche, s'il y en a une.
        date: Option<VersionDate>,
    },
    /// Des enregistrements portent ce numéro, aucun la date citée : `unresolvable`. Ce n'est pas
    /// une absence (`NoRecord`) : le numéro existe, à d'autres dates (L2-06, revue W4).
    NoRecordAtDate {
        /// Nombre d'enregistrements qui portent le numéro.
        n: usize,
        /// Numéro, sous sa forme canonique.
        number: String,
        /// Date citée.
        date: CivilDate,
        /// Dates trouvées, dédoublonnées, croissantes, au plus `FOUND_DATES_MAX`.
        found: Vec<CivilDate>,
        /// Nombre de dates distinctes de plus que celles de `found`.
        more: usize,
    },
    /// Champ dur différent de l'enregistrement officiel : `invalid`.
    Mismatch {
        /// Champ en écart.
        field: Field,
        /// Valeur citée, sous sa forme canonique.
        cited: String,
        /// Valeur officielle, sous sa forme canonique.
        actual: String,
    },
    /// Plusieurs champs durs différents de l'enregistrement officiel : `invalid`. Chaque écart
    /// est un champ, la valeur citée et la valeur officielle (formes canoniques), dans l'ordre
    /// où ils sont rendus.
    Mismatches(Vec<(Field, String, String)>),
    /// Chambre de la Cour de cassation citée, mais formation officielle qui n'est pas une chambre
    /// moderne (juridiction du fond, chambre historique, valeur inconnue) : le contrôle de la
    /// chambre n'a pas pu se faire, `verified` conservé.
    ChamberNotCompared,
    /// ECLI natif de la décision consultée.
    NativeEcli(Ecli),
    /// Impossibilité syntaxique relevée par le parseur : `invalid`.
    Impossible {
        /// Fait, phrase fixe du vocabulaire fermé des drapeaux.
        fact: &'static str,
    },
    /// Sigle ambigu sans contexte : `ambiguous`.
    AmbiguousSigle {
        /// Le sigle.
        sigle: Sigle,
    },
    /// Sigle lu par défaut : `verified`.
    ReadAs {
        /// Le sigle.
        sigle: Sigle,
        /// Titre du code retenu.
        code: &'static str,
    },
    /// Ni code ni texte pour un article : `ambiguous`.
    MissingTarget {
        /// Numéro d'article, sous sa forme de clé.
        num: String,
    },
    /// Décision sans numéro : `ambiguous`.
    NoNumber {
        /// Fond où chercher.
        fond: Fond,
        /// Date citée, s'il y en a une.
        date: Option<CivilDate>,
    },
    /// Décision sans juridiction : `ambiguous`.
    NoCourt,
    /// Convention sans identification : `ambiguous`.
    NoAgreement,
    /// Plusieurs enregistrements : `ambiguous`.
    Candidates {
        /// Nombre d'enregistrements.
        n: usize,
    },
    /// Famille absente de Légifrance : `unresolvable`.
    OutOfScope {
        /// Famille, vocabulaire fermé.
        family: &'static str,
    },
    /// Couverture partielle : `unresolvable`.
    OutOfCoverage {
        /// Juridiction, vocabulaire fermé.
        court: &'static str,
    },
    /// Hors des capacités de cette version : `unresolvable`.
    NotSupported {
        /// Ce qui n'est pas contrôlable, vocabulaire fermé.
        what: &'static str,
    },
    /// Texte cité sans numéro (« loi du 1er juillet 1901 », « la Constitution du 4 octobre
    /// 1958 ») : `unresolvable`, avec la piste de la recherche plein texte (L2-10).
    NumberlessText,
    /// Existence non confirmée : `unresolvable`.
    CouldNotConfirm {
        /// Index interrogé, vocabulaire fermé.
        index: &'static str,
    },
    /// Citation non contrôlée : `unresolvable`.
    NotChecked {
        /// Motif.
        reason: NotCheckedReason,
    },
    /// Code absent de la liste des codes : `unresolvable`.
    CodeNotListed,
    /// Début de la version en vigueur : `verified`.
    VersionSince(CivilDate),
    /// Version lue à une date passée qui n'est plus en vigueur (`etat` remplacé, abrogé…) :
    /// `verified`, mais le texte d'aujourd'hui peut différer (L2-12, resolve-5,
    /// journey_patrimoine-14). `etat` est le code de l'API, vocabulaire fermé ; `ended` la fin de
    /// vigueur de la version lue, si l'API la donne.
    NotCurrentVersion {
        /// Date de la lecture.
        on: VersionDate,
        /// Code d'état de la version lue.
        etat: &'static str,
        /// Fin de vigueur de la version lue.
        ended: Option<CivilDate>,
    },
    /// Citation d'article avec une subdivision (alinéa, point, phrase…) : seul l'article est
    /// contrôlé (L2-17, gap3-8). Statut inchangé.
    SubdivisionNotChecked,
    /// Texte de `fr_verify_citations` avec un passage entre guillemets que ne jouxte aucune
    /// citation : il n'a pas été contrôlé (L2-17, verify_citations-8). Note de la réponse, non
    /// d'une citation.
    QuoteNotAttached,
    /// Texte de `fr_verify_citations` avec une mention d'article (mot-clé puis numéro) que le
    /// parseur n'a pas lue, hors de toute citation lue : « CGI, art. 885-0 V bis A » est refusé en
    /// entier (filet N36) et disparaissait sans trace des citations rendues (revue finale I3). Note
    /// de la réponse, non d'une citation.
    ArticleNotRead,
    /// Plusieurs notes d'un même verdict, dans l'ordre où elles sont rendues (la plus utile en
    /// tête) : version lue, lecture du sigle, plage, subdivision. Bâtie par `Note::then`, donc
    /// toujours d'au moins deux notes et sans autre `Chain` à l'intérieur.
    Chain(Vec<Note>),
    /// Plage d'articles (« articles 1240 à 1244 ») dont les deux bornes existent : `verified` de
    /// confiance moyenne, les articles intermédiaires ne sont pas lus (L2-07).
    RangeEndpointsOnly,
    /// Plage d'articles dont les deux bornes existent, lue avec le code par défaut d'un sigle
    /// ambigu : la note enveloppée (`ReadAs`) puis la phrase de [`Note::RangeEndpointsOnly`], pour
    /// que la lecture du sigle ne masque pas que le milieu de la plage n'est pas lu.
    WithRange(Box<Note>),
    /// Plage d'articles dont la première borne existe mais pas la dernière : `ambiguous`, jamais
    /// `invalid` ni `not_found` (la plage peut être juste pour une autre date, L2-07).
    RangeEndNotFound {
        /// Dernier article de la plage, sous sa forme de clé.
        num: String,
    },
    /// Extrait cité introuvable dans le texte officiel (statut inchangé).
    QuoteMismatch,
    /// Extrait cité trop long à comparer au texte officiel (budget d'états épuisé) : il n'a pas
    /// été contrôlé, le champ `quote` reste absent (revue finale S1). Statut inchangé.
    QuoteTooLong,
    /// Note d'un statut `verified` précédée de l'écart d'extrait : l'écart est rendu en tête, avant
    /// la note enveloppée (version en vigueur, sigle lu par défaut, chambre non comparée, ECLI
    /// natif), pour ne jamais être masqué ni rognée par la borne de 200 caractères.
    WithQuoteMismatch(Box<Note>),
    /// Absence constatée en bac à sable : `unresolvable`, jamais `not_found` (00 §7).
    NotInSandbox {
        /// Famille, vocabulaire fermé.
        family: &'static str,
        /// Valeur recherchée, sous sa forme canonique.
        value: String,
        /// Date de la recherche, s'il y en a une.
        date: Option<VersionDate>,
    },
    /// Absence constatée dans l'index local DILA, un instantané peut-être périmé : `unresolvable`,
    /// jamais `not_found`.
    NotInSnapshot {
        /// Famille, vocabulaire fermé.
        family: &'static str,
        /// Valeur recherchée, sous sa forme canonique.
        value: String,
        /// Date de la recherche, s'il y en a une.
        date: Option<VersionDate>,
    },
    /// Aucune citation reconnue dans l'entrée de `fr_resolve` ou de `fr_verify_citations` : seules
    /// les citations introduites par « article » ou « art. » sont lues.
    NoCitationRecognised,
    /// Plusieurs citations dans l'entrée de `fr_resolve` : seule la première est résolue.
    OtherCitations {
        /// Nombre de citations reconnues en plus de la première.
        count: usize,
    },
    /// Appellation d'un texte : `fr_search` plutôt que `fr_resolve`.
    UseSearch,
    /// `fr_get_related` : l'API ne donne aucune référence (`citationJp`, `liens`) pour cette
    /// décision, alors que son texte en cite peut-être (V4-05) ; la liste vide ne prouve rien.
    NoDecisionReferences,
    /// `fr_get_related` : le code d'au moins un lien de décision n'était pas écrit à côté de son
    /// article et n'a pas été repris ; les titres de `liens` sont des fragments de prose (V-117),
    /// les codes des liens d'une décision peuvent donc être inexacts.
    DecisionLinkCodes,
}

/// Sens des sigles ambigus (annexe B §5.3), vocabulaire fermé.
const fn meanings(sigle: Sigle) -> &'static str {
    match sigle {
        Sigle::Ce => "Conseil d'État, Code électoral",
        Sigle::Cc => "Code civil, Conseil constitutionnel",
        Sigle::Cp => "Code pénal, Code des ports maritimes",
        Sigle::Ct => "Code du travail, Code du tourisme, Code des transports",
        Sigle::Cu => "Code de l'urbanisme",
        Sigle::Cpi => "Code de la propriété intellectuelle, Cour pénale internationale",
        Sigle::Ccp => "Code de la commande publique, Code de procédure pénale",
        Sigle::Cpce => {
            "Code des postes et des communications électroniques, Code des procédures civiles d'exécution"
        }
    }
}

/// Rend une note dont `label` (titre de code, ou « titre, art. clé ») est le seul morceau long :
/// quand le texte dépasse [`NOTE_MAX_CHARS`], le titre est raccourci par « … » plutôt que de
/// couper la fin de la note, qui porte la piste donnée à l'appelant.
fn fit(label: &str, build: impl Fn(&str) -> String) -> String {
    let full = build(label);
    let over = full.chars().count().saturating_sub(NOTE_MAX_CHARS);
    if over == 0 {
        return full;
    }
    let shrink = |title: &str| {
        let keep = title.chars().count().saturating_sub(over.saturating_add(1));
        title.chars().take(keep).chain(std::iter::once('…')).collect::<String>()
    };
    match label.rsplit_once(", art. ") {
        Some((title, key)) => build(&format!("{}, art. {key}", shrink(title))),
        None => build(&shrink(label)),
    }
}

/// Piste donnée avec l'absence d'un article : une version plus ancienne peut exister (resolve-5).
const EARLIER_VERSIONS: &str = "The article may exist in earlier versions; retry with a date.";

/// ` {EARLIER_VERSIONS}` pour la famille « article » (les seuls articles ont des versions
/// datées), rien pour les autres familles.
fn earlier_versions(family: &str) -> String {
    if family == "article" { format!(" {EARLIER_VERSIONS}") } else { String::new() }
}

/// ` on {date}` quand la date est connue.
fn on(date: Option<VersionDate>) -> String {
    date.map_or_else(String::new, |date| format!(" on {date}"))
}

/// Texte des notes de plage d'articles (L2-07).
fn range_text(note: &Note) -> String {
    match note {
        Note::RangeEndNotFound { num } => format!(
            "The first article of the range exists, but its last, article {num}, was not found; \
             check the number or the date."
        ),
        _ => "Only the first and last articles of the range were checked.".to_owned(),
    }
}

/// Texte de plusieurs écarts : le premier avec le gabarit `Mismatch`, les suivants abrégés,
/// séparés par « ; ».
fn mismatches_text(gaps: &[(Field, String, String)]) -> String {
    let parts: Vec<String> = gaps
        .iter()
        .enumerate()
        .map(|(rank, (field, cited, actual))| {
            if rank == 0 {
                format!(
                    "Cited {} {cited} differs from the official record ({actual})",
                    field.label()
                )
            } else {
                format!("{} {cited} differs ({actual})", field.label())
            }
        })
        .collect();
    format!("{}.", parts.join("; "))
}

/// Gabarit de [`Note::DecisionLinkCodes`].
const DECISION_LINK_CODES: &str = "The codes of decision links may be inexact: the API gives text \
     fragments, and a code not written next to its article is left out; check them in the \
     decision text with fr_get_decision.";

/// Textes de plusieurs notes mis bout à bout (une espace entre deux). Le premier est rendu en
/// entier (rogné par la borne de [`Note::text`] s'il la dépasse à lui seul) ; chacun des suivants
/// n'est ajouté que s'il tient en entier sous [`NOTE_MAX_CHARS`] : mieux vaut une note de moins
/// qu'une phrase coupée en son milieu.
fn join_fitting(parts: impl IntoIterator<Item = String>) -> String {
    let mut parts = parts.into_iter();
    let mut text = parts.next().unwrap_or_default();
    for part in parts {
        let joined = format!("{text} {part}");
        if joined.chars().count() <= NOTE_MAX_CHARS {
            text = joined;
        }
    }
    text
}

/// Texte de [`Note::NoRecordAtDate`] : le numéro raccourcit avant les dates.
fn no_record_at_date_text(
    n: usize,
    number: &str,
    date: CivilDate,
    found: &[CivilDate],
    more: usize,
) -> String {
    fit(number, |number| {
        let dates = found.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
        let rest = if more > 0 { format!(" and {more} more") } else { String::new() };
        format!("{n} records share number {number}, none dated {date}; dates found: {dates}{rest}.")
    })
}

/// Texte de [`Note::NotCurrentVersion`].
fn not_current_text(on: VersionDate, etat: &str, ended: Option<CivilDate>) -> String {
    let ended = ended.map_or_else(String::new, |date| format!(", ended {date}"));
    format!(
        "Version in force on {on} (etat={etat}{ended}); omit the date to read the current text."
    )
}

/// Texte de [`Note::WithQuoteMismatch`] : l'écart d'extrait en tête, puis la note enveloppée. Une
/// chaîne enveloppée est aplatie derrière l'écart, pour que ses notes se rendent entières ou pas
/// du tout.
fn quote_gap_text(inner: &Note) -> String {
    match inner {
        Note::Chain(parts) => {
            join_fitting(std::iter::once(QUOTE_GAP.to_owned()).chain(parts.iter().map(Note::text)))
        }
        other => format!("{QUOTE_GAP} {}", other.text()),
    }
}

/// Gabarit de [`Note::SubdivisionNotChecked`].
const SUBDIVISION_NOT_CHECKED: &str = "The sub-division was not checked, only the article.";

/// Gabarit de [`Note::QuoteTooLong`].
const QUOTE_TOO_LONG: &str = "The quoted passage was too long to compare and was not checked.";

/// Gabarit de [`Note::QuoteNotAttached`].
const QUOTE_NOT_ATTACHED: &str =
    "A quoted passage is not attached to a citation and was not checked.";

/// Gabarit de [`Note::ArticleNotRead`].
const ARTICLE_NOT_READ: &str = "An article reference could not be read and was not checked.";

impl Note {
    /// Cette note suivie de `next` : les deux sont rendues, celle-ci en tête (une seule note par
    /// verdict, la plus utile d'abord). Les chaînes s'aplatissent.
    #[must_use]
    pub(crate) fn then(self, next: Self) -> Self {
        let mut parts = match self {
            Self::Chain(parts) => parts,
            other => vec![other],
        };
        match next {
            Self::Chain(more) => parts.extend(more),
            other => parts.push(other),
        }
        Self::Chain(parts)
    }

    /// L'avertissement de version (`NotCurrentVersion`) que porte cette note en tête, seul ou en
    /// début de chaîne.
    pub(crate) fn version_warning(&self) -> Option<&Self> {
        match self {
            Self::NotCurrentVersion { .. } => Some(self),
            Self::Chain(parts) => parts.first().filter(|first| first.version_warning().is_some()),
            _ => None,
        }
    }

    /// Cette note finit-elle par la mention de plage ?
    pub(crate) fn ends_with_range(&self) -> bool {
        match self {
            Self::RangeEndpointsOnly => true,
            Self::Chain(parts) => parts.last() == Some(&Self::RangeEndpointsOnly),
            _ => false,
        }
    }

    /// Texte anglais de la note, gabarit de l'annexe B §7.10, borné à [`NOTE_MAX_CHARS`] caractères.
    #[must_use]
    pub fn text(&self) -> String {
        let text = match self {
            Self::NotInForce { code, num, date } => fit(code, |code| {
                format!("{code} has no article {num} in force on {date}. {EARLIER_VERSIONS}")
            }),
            Self::NoRecord { family, value, date } => {
                format!("No {family} record matches {value}{}.", on(*date))
            }
            Self::NoRecordAtDate { n, number, date, found, more } => {
                no_record_at_date_text(*n, number, *date, found, *more)
            }
            Self::Mismatch { field, cited, actual } => format!(
                "Cited {} {cited} differs from the official record ({actual}).",
                field.label()
            ),
            Self::Mismatches(gaps) => mismatches_text(gaps),
            Self::ChamberNotCompared => "The cited chamber could not be compared: the official \
                 record gives a formation that is not a modern Cour de cassation chamber."
                .to_owned(),
            Self::NativeEcli(ecli) => format!("Native ECLI is {ecli}."),
            Self::Impossible { fact } => format!("{fact}."),
            Self::AmbiguousSigle { sigle } => format!(
                "'{}' is ambiguous ({}); name the code in full.",
                sigle.as_str(),
                meanings(*sigle)
            ),
            Self::ReadAs { sigle, code } => format!("Read '{}' as {code}.", sigle.as_str()),
            Self::MissingTarget { num } => {
                format!(
                    "No recognised code or text for article {num} (an unknown or unattached \
                     abbreviation counts as missing); name the code next to the article, in full."
                )
            }
            Self::NoNumber { fond, date } => {
                let dates = date
                    .map_or_else(String::new, |date| format!(", date_from={date}, date_to={date}"));
                format!("No case number: use fr_search (fond={}{dates}).", fond.name())
            }
            Self::NoCourt => {
                "No court given: name it, or call fr_resolve to list the matching decisions."
                    .to_owned()
            }
            Self::NoAgreement => {
                "No collective agreement identified; give its IDCC (e.g. 'IDCC 1486').".to_owned()
            }
            Self::Candidates { n } => format!("{n} records match; call fr_resolve to list them."),
            Self::OutOfScope { family } => {
                format!("{family} is not on Légifrance (see EUR-Lex / HUDOC).")
            }
            Self::OutOfCoverage { court } => format!(
                "Légifrance only holds a selection of {court} decisions; absence is not proof \
                 (see fr_coverage)."
            ),
            Self::NotSupported { what } => format!("Not checkable in this version ({what})."),
            Self::NumberlessText => NUMBERLESS_TEXT.to_owned(),
            Self::CouldNotConfirm { index } => {
                format!(
                    "Could not confirm existence: nothing was found in {index}, which does not \
                     prove that it does not exist."
                )
            }
            Self::NotChecked { reason } => format!("Not checked: {}.", reason.label()),
            Self::CodeNotListed => "Code not found in the list of codes.".to_owned(),
            Self::VersionSince(date) => format!("Current version in force since {date}."),
            Self::NotCurrentVersion { on, etat, ended } => not_current_text(*on, etat, *ended),
            Self::SubdivisionNotChecked => SUBDIVISION_NOT_CHECKED.to_owned(),
            Self::QuoteNotAttached => QUOTE_NOT_ATTACHED.to_owned(),
            Self::ArticleNotRead => ARTICLE_NOT_READ.to_owned(),
            Self::Chain(parts) => join_fitting(parts.iter().map(Self::text)),
            Self::RangeEndpointsOnly | Self::RangeEndNotFound { .. } => range_text(self),
            Self::WithRange(inner) => {
                format!("{} {}", inner.text(), range_text(&Self::RangeEndpointsOnly))
            }
            Self::QuoteMismatch => QUOTE_GAP.to_owned(),
            Self::QuoteTooLong => QUOTE_TOO_LONG.to_owned(),
            Self::WithQuoteMismatch(inner) => quote_gap_text(inner),
            Self::NotInSandbox { family, value, date } => fit(value, |value| {
                format!(
                    "No {family} record matches {value}{} in the PISTE sandbox; sandbox data \
                     differ from production.{}",
                    on(*date),
                    earlier_versions(family)
                )
            }),
            Self::NotInSnapshot { family, value, date } => format!(
                "No {family} record matches {value}{} in the local DILA snapshot, which may be out \
                 of date.",
                on(*date)
            ),
            Self::NoCitationRecognised => NO_CITATION.to_owned(),
            Self::OtherCitations { count } => format!(
                "{count} more citation(s) found; only the first is resolved, use \
                 fr_verify_citations for the whole text."
            ),
            Self::UseSearch => {
                "A popular name is not a citation; find the text with fr_search.".to_owned()
            }
            Self::NoDecisionReferences => NO_DECISION_REFERENCES.to_owned(),
            Self::DecisionLinkCodes => DECISION_LINK_CODES.to_owned(),
        };
        messages::clip(&text, NOTE_MAX_CHARS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lf_domain::citation::Sigle;
    use pretty_assertions::assert_eq;

    fn date(text: &str) -> VersionDate {
        VersionDate::parse(text).unwrap()
    }

    fn civil(y: u16, m: u8, d: u8) -> CivilDate {
        CivilDate::new(y, m, d).unwrap()
    }

    /// Une note de chaque variante, avec le texte de l'annexe B §7.10.
    fn catalogue() -> Vec<(Note, &'static str)> {
        let ecli = Ecli::parse("ECLI:FR:CECHR:2026:506507.20260529").unwrap();
        vec![
            (
                Note::NotInForce {
                    code: "Code civil".to_owned(),
                    num: "9999".to_owned(),
                    date: date("2026-10-15"),
                },
                "Code civil has no article 9999 in force on 2026-10-15. The article may exist in earlier versions; retry with a date.",
            ),
            (
                Note::NoRecord {
                    family: "decision",
                    value: "506507".to_owned(),
                    date: Some(date("2026-10-15")),
                },
                "No decision record matches 506507 on 2026-10-15.",
            ),
            (
                Note::NoRecord { family: "text", value: "2016-1321".to_owned(), date: None },
                "No text record matches 2016-1321.",
            ),
            (
                Note::Mismatch {
                    field: Field::Date,
                    cited: "2015-10-07".to_owned(),
                    actual: "2016-10-07".to_owned(),
                },
                "Cited date 2015-10-07 differs from the official record (2016-10-07).",
            ),
            (Note::NativeEcli(ecli), "Native ECLI is ECLI:FR:CECHR:2026:506507.20260529."),
            (
                Note::Impossible { fact: "There is no such chamber at the Cour de cassation" },
                "There is no such chamber at the Cour de cassation.",
            ),
            (
                Note::AmbiguousSigle { sigle: Sigle::Ce },
                "'CE' is ambiguous (Conseil d'État, Code électoral); name the code in full.",
            ),
            (Note::ReadAs { sigle: Sigle::Cp, code: "Code pénal" }, "Read 'CP' as Code pénal."),
            (
                Note::NoNumber { fond: Fond::Juri, date: Some(civil(2018, 3, 28)) },
                "No case number: use fr_search (fond=JURI, date_from=2018-03-28, date_to=2018-03-28).",
            ),
            (
                Note::NoNumber { fond: Fond::Cetat, date: None },
                "No case number: use fr_search (fond=CETAT).",
            ),
            (
                Note::NoCourt,
                "No court given: name it, or call fr_resolve to list the matching decisions.",
            ),
            (
                Note::NoAgreement,
                "No collective agreement identified; give its IDCC (e.g. 'IDCC 1486').",
            ),
            (Note::Candidates { n: 3 }, "3 records match; call fr_resolve to list them."),
            (
                Note::OutOfScope { family: "EU law" },
                "EU law is not on Légifrance (see EUR-Lex / HUDOC).",
            ),
            (
                Note::OutOfCoverage { court: "Cour d'appel" },
                "Légifrance only holds a selection of Cour d'appel decisions; absence is not proof (see fr_coverage).",
            ),
            (Note::NotSupported { what: "JORF" }, "Not checkable in this version (JORF)."),
            (
                Note::NumberlessText,
                "A text cited without a number cannot be checked here; find it with fr_search (query: its title and date).",
            ),
            (
                Note::NotChecked { reason: NotCheckedReason::TimeBudget },
                "Not checked: time budget.",
            ),
            (
                Note::NotChecked { reason: NotCheckedReason::Upstream },
                "Not checked: upstream error.",
            ),
            (Note::CodeNotListed, "Code not found in the list of codes."),
            (Note::VersionSince(civil(2016, 10, 1)), "Current version in force since 2016-10-01."),
            (Note::QuoteMismatch, "Quoted text not found in the official text."),
            (Note::QuoteTooLong, "The quoted passage was too long to compare and was not checked."),
            (
                Note::NotInSandbox {
                    family: "code article",
                    value: "9999".to_owned(),
                    date: Some(date("2026-10-29")),
                },
                "No code article record matches 9999 on 2026-10-29 in the PISTE sandbox; sandbox data differ from production.",
            ),
            (
                Note::NoCitationRecognised,
                "No citation recognised. Only citations introduced by 'article' or 'art.' are read; an empty result does not prove the text cites nothing.",
            ),
            (
                Note::OtherCitations { count: 2 },
                "2 more citation(s) found; only the first is resolved, use fr_verify_citations for the whole text.",
            ),
            (Note::UseSearch, "A popular name is not a citation; find the text with fr_search."),
        ]
    }

    /// Les notes de la fin du projet : absence dans l'instantané local de l'index DILA (V4-02) et
    /// décision sans références pour `fr_get_related` (V4-05).
    fn later_catalogue() -> Vec<(Note, &'static str)> {
        vec![
            (
                Note::NoRecordAtDate {
                    n: 3,
                    number: "00-00.000".to_owned(),
                    date: civil(2021, 3, 17),
                    found: vec![civil(1989, 3, 17), civil(2003, 3, 17), civil(2004, 3, 17)],
                    more: 0,
                },
                "3 records share number 00-00.000, none dated 2021-03-17; dates found: 1989-03-17, 2003-03-17, 2004-03-17.",
            ),
            (
                Note::NoRecordAtDate {
                    n: 9,
                    number: "00-00.000".to_owned(),
                    date: civil(2021, 3, 17),
                    found: vec![civil(1989, 3, 17)],
                    more: 4,
                },
                "9 records share number 00-00.000, none dated 2021-03-17; dates found: 1989-03-17 and 4 more.",
            ),
            (
                Note::MissingTarget { num: "L1234-5".to_owned() },
                "No recognised code or text for article L1234-5 (an unknown or unattached abbreviation counts as missing); name the code next to the article, in full.",
            ),
            (
                Note::CouldNotConfirm { index: "LODA" },
                "Could not confirm existence: nothing was found in LODA, which does not prove that it does not exist.",
            ),
            (
                Note::AmbiguousSigle { sigle: Sigle::Cpce },
                "'CPCE' is ambiguous (Code des postes et des communications électroniques, Code des procédures civiles d'exécution); name the code in full.",
            ),
            (
                Note::NotInSandbox {
                    family: "article",
                    value: "Code général des impôts, art. 885 A".to_owned(),
                    date: Some(date("2026-11-01")),
                },
                "No article record matches Code général des impôts, art. 885 A on 2026-11-01 in the PISTE sandbox; sandbox data differ from production. The article may exist in earlier versions; retry with a date.",
            ),
            (
                Note::NoDecisionReferences,
                "The API gives no references for this decision; its text may still cite some: read it with fr_get_decision and check it with fr_verify_citations.",
            ),
            (
                Note::DecisionLinkCodes,
                "The codes of decision links may be inexact: the API gives text fragments, and a code not written next to its article is left out; check them in the decision text with fr_get_decision.",
            ),
            (
                Note::NotInSnapshot {
                    family: "code article",
                    value: "9999".to_owned(),
                    date: Some(date("2026-10-29")),
                },
                "No code article record matches 9999 on 2026-10-29 in the local DILA snapshot, which may be out of date.",
            ),
            (
                Note::NotInSnapshot { family: "text", value: "2016-1321".to_owned(), date: None },
                "No text record matches 2016-1321 in the local DILA snapshot, which may be out of date.",
            ),
            (
                Note::Mismatches(vec![(
                    Field::Date,
                    "2015-10-07".to_owned(),
                    "2016-10-07".to_owned(),
                )]),
                "Cited date 2015-10-07 differs from the official record (2016-10-07).",
            ),
            (
                Note::Mismatches(vec![
                    (Field::Date, "2015-10-07".to_owned(), "2016-10-07".to_owned()),
                    (Field::Chamber, "SOC".to_owned(), "CIV1".to_owned()),
                ]),
                "Cited date 2015-10-07 differs from the official record (2016-10-07); chamber SOC differs (CIV1).",
            ),
            (
                Note::Mismatches(vec![
                    (Field::Date, "2015-10-07".to_owned(), "2016-10-07".to_owned()),
                    (Field::Chamber, "PREMIER_PRESIDENT".to_owned(), "CIV1".to_owned()),
                    (Field::CcType, "QPC".to_owned(), "DC".to_owned()),
                ]),
                "Cited date 2015-10-07 differs from the official record (2016-10-07); chamber PREMIER_PRESIDENT differs (CIV1); decision type QPC differs (DC).",
            ),
            (
                Note::WithQuoteMismatch(Box::new(Note::VersionSince(civil(2016, 10, 1)))),
                "Quoted text not found in the official text. Current version in force since 2016-10-01.",
            ),
            (
                Note::WithQuoteMismatch(Box::new(Note::ReadAs {
                    sigle: Sigle::Cp,
                    code: "Code pénal",
                })),
                "Quoted text not found in the official text. Read 'CP' as Code pénal.",
            ),
            (
                Note::ChamberNotCompared,
                "The cited chamber could not be compared: the official record gives a formation that is not a modern Cour de cassation chamber.",
            ),
        ]
    }

    /// Les notes de plage d'articles (L2-07).
    fn range_catalogue() -> Vec<(Note, &'static str)> {
        vec![
            (
                Note::RangeEndpointsOnly,
                "Only the first and last articles of the range were checked.",
            ),
            (
                Note::WithRange(Box::new(Note::ReadAs { sigle: Sigle::Cp, code: "Code pénal" })),
                "Read 'CP' as Code pénal. Only the first and last articles of the range were checked.",
            ),
            (
                Note::RangeEndNotFound { num: "L9999-99".to_owned() },
                "The first article of the range exists, but its last, article L9999-99, was not found; check the number or the date.",
            ),
        ]
    }

    /// Les notes de L2-12 (version remplacée) et L2-17 (non contrôlé), et leurs compositions.
    fn version_catalogue() -> Vec<(Note, &'static str)> {
        let replaced = Note::NotCurrentVersion {
            on: date("2011-06-01"),
            etat: "MODIFIE",
            ended: Some(civil(2011, 6, 12)),
        };
        vec![
            (
                replaced.clone(),
                "Version in force on 2011-06-01 (etat=MODIFIE, ended 2011-06-12); omit the date to read the current text.",
            ),
            (
                Note::NotCurrentVersion { on: date("2011-06-01"), etat: "ABROGE", ended: None },
                "Version in force on 2011-06-01 (etat=ABROGE); omit the date to read the current text.",
            ),
            (Note::SubdivisionNotChecked, "The sub-division was not checked, only the article."),
            (
                Note::QuoteNotAttached,
                "A quoted passage is not attached to a citation and was not checked.",
            ),
            (Note::ArticleNotRead, "An article reference could not be read and was not checked."),
            (
                Note::ArticleNotRead.then(Note::QuoteNotAttached),
                "An article reference could not be read and was not checked. A quoted passage is not attached to a citation and was not checked.",
            ),
            (
                replaced.clone().then(Note::ReadAs { sigle: Sigle::Cp, code: "Code pénal" }),
                "Version in force on 2011-06-01 (etat=MODIFIE, ended 2011-06-12); omit the date to read the current text. Read 'CP' as Code pénal.",
            ),
            (
                Note::VersionSince(civil(2016, 10, 1)).then(Note::SubdivisionNotChecked),
                "Current version in force since 2016-10-01. The sub-division was not checked, only the article.",
            ),
            (
                Note::WithQuoteMismatch(Box::new(replaced.clone())),
                "Quoted text not found in the official text. Version in force on 2011-06-01 (etat=MODIFIE, ended 2011-06-12); omit the date to read the current text.",
            ),
            (
                Note::WithQuoteMismatch(Box::new(
                    Note::VersionSince(civil(2016, 10, 1)).then(Note::SubdivisionNotChecked),
                )),
                "Quoted text not found in the official text. Current version in force since 2016-10-01. The sub-division was not checked, only the article.",
            ),
        ]
    }

    #[test]
    fn notes_are_static_templates() {
        for (note, expected) in catalogue()
            .into_iter()
            .chain(later_catalogue())
            .chain(range_catalogue())
            .chain(version_catalogue())
        {
            let text = note.text();
            assert_eq!(text, expected, "{note:?}");
            assert!(text.chars().count() <= NOTE_MAX_CHARS, "{text}");
            assert!(!text.contains('\n'), "{text}");
        }
    }

    /// Revue finale I1 : la note de version et la mention de plage tiennent ensemble sous 200
    /// caractères, au pire état (`MODIFIE_MORT_NE`, avec une date de fin) ; sinon la mention de plage,
    /// qui justifie la confiance `medium`, disparaissait derrière la version.
    #[test]
    fn the_version_note_and_the_range_note_fit_together_at_the_worst_state() {
        let worst = Note::NotCurrentVersion {
            on: date("2011-06-01"),
            etat: "MODIFIE_MORT_NE",
            ended: Some(civil(2011, 6, 12)),
        };
        let version = "Version in force on 2011-06-01 (etat=MODIFIE_MORT_NE, ended 2011-06-12); \
                       omit the date to read the current text.";
        let range = "Only the first and last articles of the range were checked.";
        assert_eq!(worst.text(), version);
        assert_eq!(Note::RangeEndpointsOnly.text(), range);
        let text = worst.clone().then(Note::RangeEndpointsOnly).text();
        assert_eq!(text, format!("{version} {range}"));
        assert!(text.chars().count() <= NOTE_MAX_CHARS, "{text}");
        assert_eq!(text.chars().count(), 172);
    }

    /// Revue finale I1 : version, sigle lu par défaut et plage. Au pire état (`MODIFIE_MORT_NE`,
    /// date de fin), le code pénal et le code civil tiennent avec leurs trois phrases ; un titre plus
    /// long (ici le code des postes et des communications électroniques) ne tient pas : la lecture du
    /// sigle et la plage sautent ENSEMBLE (`WithRange` est une seule note), la version reste seule.
    /// Avec l'état courant `MODIFIE`, seuls les titres les plus longs sautent.
    #[test]
    fn the_version_sigle_and_range_notes_drop_together_when_the_title_is_long() {
        let version = |etat: &'static str| Note::NotCurrentVersion {
            on: date("2011-06-01"),
            etat,
            ended: Some(civil(2011, 6, 12)),
        };
        let chain = |etat: &'static str, sigle: Sigle, code: &'static str| {
            version(etat).then(Note::WithRange(Box::new(Note::ReadAs { sigle, code })))
        };
        let range = "Only the first and last articles of the range were checked.";
        for (sigle, code) in [(Sigle::Cp, "Code pénal"), (Sigle::Cc, "Code civil")] {
            let text = chain("MODIFIE_MORT_NE", sigle, code).text();
            assert!(text.chars().count() <= NOTE_MAX_CHARS, "{text}");
            assert!(text.contains("(etat=MODIFIE_MORT_NE, ended 2011-06-12)"), "{text}");
            assert!(text.contains(&format!("Read '{}' as {code}.", sigle.as_str())), "{text}");
            assert!(text.ends_with(range), "{text}");
        }
        let long = "Code des postes et des communications électroniques";
        for etat in ["MODIFIE_MORT_NE", "MODIFIE"] {
            let text = chain(etat, Sigle::Cpce, long).text();
            assert_eq!(text, version(etat).text(), "{etat}");
        }
        // Un titre moyen tient avec `MODIFIE`, pas au pire état.
        let text = chain("MODIFIE", Sigle::Ct, "Code du travail").text();
        assert!(text.ends_with(range), "{text}");
        let worst = chain("MODIFIE_MORT_NE", Sigle::Ct, "Code du travail").text();
        assert_eq!(worst, version("MODIFIE_MORT_NE").text());
    }

    #[test]
    fn then_flattens_and_keeps_the_order() {
        let a = Note::VersionSince(civil(2016, 10, 1));
        let b = Note::SubdivisionNotChecked;
        let c = Note::RangeEndpointsOnly;
        let ab = a.clone().then(b.clone());
        assert_eq!(ab, Note::Chain(vec![a.clone(), b.clone()]));
        // Une chaîne à gauche, à droite, ou des deux côtés : toujours une seule chaîne plate.
        assert_eq!(ab.clone().then(c.clone()), Note::Chain(vec![a.clone(), b.clone(), c.clone()]));
        assert_eq!(c.clone().then(ab.clone()), Note::Chain(vec![c.clone(), a.clone(), b.clone()]));
        assert_eq!(
            ab.clone().then(ab),
            Note::Chain(vec![a.clone(), b.clone(), a.clone(), b.clone()])
        );
    }

    #[test]
    fn the_version_warning_and_the_range_are_found_in_a_chain() {
        let warning =
            Note::NotCurrentVersion { on: date("2011-06-01"), etat: "MODIFIE", ended: None };
        // Seul, ou en tête de chaîne : l'avertissement est rendu. Ailleurs, non.
        assert_eq!(warning.version_warning(), Some(&warning));
        let chained = warning.clone().then(Note::RangeEndpointsOnly);
        assert_eq!(chained.version_warning(), Some(&warning));
        let behind = Note::SubdivisionNotChecked.then(warning.clone());
        assert_eq!(behind.version_warning(), None);
        assert_eq!(Note::VersionSince(civil(2016, 10, 1)).version_warning(), None);
        assert_eq!(Note::RangeEndpointsOnly.version_warning(), None);

        assert!(Note::RangeEndpointsOnly.ends_with_range());
        assert!(chained.ends_with_range());
        assert!(!behind.ends_with_range());
        assert!(!Note::SubdivisionNotChecked.ends_with_range());
        assert!(!warning.ends_with_range());
    }

    #[test]
    fn a_chain_leaves_out_a_note_that_does_not_fit_rather_than_cutting_it() {
        let replaced = Note::NotCurrentVersion {
            on: date("2011-06-01"),
            etat: "MODIFIE_MORT_NE",
            ended: Some(civil(2011, 6, 12)),
        };
        // 112 + 1 + 51 = 164 : tient. Avec l'écart d'extrait devant (43), 208 : la dernière saute.
        let both = replaced.clone().then(Note::SubdivisionNotChecked);
        assert_eq!(both.text().chars().count(), 164);
        assert!(both.text().ends_with("only the article."));
        let with_gap = Note::WithQuoteMismatch(Box::new(both)).text();
        assert!(with_gap.chars().count() <= NOTE_MAX_CHARS, "{with_gap}");
        assert!(
            with_gap.starts_with("Quoted text not found in the official text. Version in force on")
        );
        assert!(with_gap.ends_with("omit the date to read the current text."), "{with_gap}");
        // Une première note qui dépasse seule est rognée, les suivantes sautent.
        let long = Note::NoRecord { family: "text", value: "é".repeat(400), date: None };
        let text = long.then(Note::SubdivisionNotChecked).text();
        assert_eq!(text.chars().count(), NOTE_MAX_CHARS);
        assert!(!text.contains("sub-division"));
    }

    #[test]
    fn the_quote_gap_comes_first_and_survives_the_clip_of_a_long_inner_note() {
        let long = Note::NoRecord { family: "text", value: "é".repeat(400), date: None };
        let text = Note::WithQuoteMismatch(Box::new(long)).text();
        assert_eq!(text.chars().count(), NOTE_MAX_CHARS);
        assert!(text.starts_with("Quoted text not found in the official text. No text record"));
    }

    #[test]
    fn a_long_number_shrinks_before_the_dates_of_a_no_record_at_date_note() {
        let note = Note::NoRecordAtDate {
            n: 9,
            number: "é".repeat(300),
            date: civil(2021, 3, 17),
            found: (1..=FOUND_DATES_MAX)
                .map(|d| civil(2000, 1, u8::try_from(d).unwrap()))
                .collect(),
            more: 12,
        };
        let text = note.text();
        assert!(text.chars().count() <= NOTE_MAX_CHARS, "{text}");
        assert!(text.contains('…'), "{text}");
        assert!(text.ends_with("2000-01-05 and 12 more."), "{text}");
    }

    #[test]
    fn a_note_is_clipped_to_200_characters_without_splitting_a_character() {
        let long = Note::NoRecord { family: "text", value: "é".repeat(400), date: None };
        let text = long.text();
        assert_eq!(text.chars().count(), NOTE_MAX_CHARS);
        assert!(text.starts_with("No text record matches éé"));
    }

    #[test]
    fn labels_are_closed_vocabularies() {
        let fields = [
            (Field::Date, "date"),
            (Field::Number, "number"),
            (Field::Chamber, "chamber"),
            (Field::Formation, "formation"),
            (Field::Seat, "seat"),
            (Field::Ecli, "ECLI"),
            (Field::Nature, "nature"),
            (Field::CcType, "decision type"),
            (Field::Code, "code"),
            (Field::ArticleNum, "article number"),
        ];
        for (field, label) in fields {
            assert_eq!(field.label(), label);
        }
        assert_eq!(NotCheckedReason::Upstream.label(), "upstream error");
        assert_eq!(NotCheckedReason::TimeBudget.label(), "time budget");
    }

    #[test]
    fn every_ambiguous_sigle_has_its_meanings() {
        for sigle in Sigle::ALL {
            assert!(!meanings(sigle).is_empty(), "{sigle:?}");
        }
        assert_eq!(
            meanings(Sigle::Cpce),
            "Code des postes et des communications électroniques, Code des procédures civiles d'exécution"
        );
    }

    #[test]
    fn the_earlier_versions_hint_survives_the_clip_for_every_code() {
        use lf_domain::citation::CODES;
        const HINT: &str = "The article may exist in earlier versions; retry with a date.";
        for entry in CODES {
            let num = "L1234-5-1".to_owned();
            let in_force = Note::NotInForce {
                code: entry.title.to_owned(),
                num: num.clone(),
                date: date("2026-11-01"),
            }
            .text();
            assert!(in_force.ends_with(HINT), "{in_force}");
            assert!(in_force.chars().count() <= NOTE_MAX_CHARS, "{in_force}");
            assert!(in_force.contains("article L1234-5-1 in force on 2026-11-01"), "{in_force}");
            let sandbox = Note::NotInSandbox {
                family: "article",
                value: format!("{}, art. {num}", entry.title),
                date: Some(date("2026-11-01")),
            }
            .text();
            assert!(sandbox.ends_with(HINT), "{sandbox}");
            assert!(sandbox.chars().count() <= NOTE_MAX_CHARS, "{sandbox}");
            assert!(sandbox.contains(", art. L1234-5-1 on 2026-11-01 in the PISTE"), "{sandbox}");
            assert!(sandbox.contains("sandbox data differ from production."), "{sandbox}");
        }
    }

    #[test]
    fn a_long_title_is_shortened_but_a_short_one_is_left_alone() {
        let short = Note::NotInForce {
            code: "Code civil".to_owned(),
            num: "9".to_owned(),
            date: date("2026-11-01"),
        };
        assert!(!short.text().contains('…'));
        let long_title = "Code du domaine de l'Etat et des collectivités publiques applicable à la collectivité territoriale de Mayotte";
        let long = Note::NotInForce {
            code: long_title.to_owned(),
            num: "L1234-5-1".to_owned(),
            date: date("2026-11-01"),
        }
        .text();
        assert_eq!(long.chars().count(), NOTE_MAX_CHARS, "{long}");
        assert!(long.starts_with("Code du domaine de l'Etat"), "{long}");
        assert!(long.contains("… has no article L1234-5-1 in force"), "{long}");
        // Sans « , art. » dans la valeur, c'est la valeur entière qui est raccourcie.
        let bare =
            Note::NotInSandbox { family: "article", value: "é".repeat(300), date: None }.text();
        assert_eq!(bare.chars().count(), NOTE_MAX_CHARS, "{bare}");
        assert!(bare.contains("é… in the PISTE sandbox"), "{bare}");
        assert!(bare.ends_with("retry with a date."), "{bare}");
    }

    #[test]
    fn only_the_article_absences_suggest_earlier_versions() {
        let decision = Note::NotInSandbox {
            family: "decision",
            value: "506507".to_owned(),
            date: Some(date("2026-11-01")),
        };
        assert!(!decision.text().contains("earlier versions"));
        let snapshot = Note::NotInSnapshot {
            family: "article",
            value: "Code civil, art. 9999".to_owned(),
            date: Some(date("2026-11-01")),
        };
        assert!(!snapshot.text().contains("earlier versions"));
    }

    #[test]
    fn the_notes_of_this_task_do_not_send_a_tool_to_itself_or_claim_a_nonexistence() {
        let missing = Note::MissingTarget { num: "1".to_owned() }.text();
        assert!(!missing.contains("fr_resolve"), "{missing}");
        let unconfirmed = Note::CouldNotConfirm { index: "LODA" }.text();
        assert!(unconfirmed.contains("does not prove"), "{unconfirmed}");
        // `Candidates` garde son renvoi : la liste est celle de `fr_resolve`.
        assert!(Note::Candidates { n: 2 }.text().contains("fr_resolve"));
    }
}
