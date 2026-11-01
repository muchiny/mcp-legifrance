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
    /// Champ dur différent de l'enregistrement officiel : `invalid`.
    Mismatch {
        /// Champ en écart.
        field: Field,
        /// Valeur citée, sous sa forme canonique.
        cited: String,
        /// Valeur officielle, sous sa forme canonique.
        actual: String,
    },
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
    /// Extrait cité introuvable dans le texte officiel (statut inchangé).
    QuoteMismatch,
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

impl Note {
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
            Self::Mismatch { field, cited, actual } => format!(
                "Cited {} {cited} differs from the official record ({actual}).",
                field.label()
            ),
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
                    "No recognised code or text for article {num} (an unknown abbreviation counts \
                     as missing); name the code in full, for example \"article {num} du code civil\"."
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
            Self::CouldNotConfirm { index } => {
                format!(
                    "Could not confirm existence: nothing was found in {index}, which does not \
                     prove that it does not exist."
                )
            }
            Self::NotChecked { reason } => format!("Not checked: {}.", reason.label()),
            Self::CodeNotListed => "Code not found in the list of codes.".to_owned(),
            Self::VersionSince(date) => format!("Current version in force since {date}."),
            Self::QuoteMismatch => "Quoted text not found in the official text.".to_owned(),
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
            Self::NoCitationRecognised => {
                "No citation recognised. Only citations introduced by 'article' \
                 or 'art.' are read; an empty result does not prove the text cites nothing."
                    .to_owned()
            }
            Self::OtherCitations { count } => format!(
                "{count} more citation(s) found; only the first is resolved, use \
                 fr_verify_citations for the whole text."
            ),
            Self::UseSearch => {
                "A popular name is not a citation; find the text with fr_search.".to_owned()
            }
            Self::NoDecisionReferences => "The API gives no references for this decision; its \
                 text may still cite some: read it with fr_get_decision and check it with \
                 fr_verify_citations."
                .to_owned(),
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
                Note::MissingTarget { num: "L1234-5".to_owned() },
                "No recognised code or text for article L1234-5 (an unknown abbreviation counts as missing); name the code in full, for example \"article L1234-5 du code civil\".",
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
        ]
    }

    #[test]
    fn notes_are_static_templates() {
        for (note, expected) in catalogue().into_iter().chain(later_catalogue()) {
            let text = note.text();
            assert_eq!(text, expected, "{note:?}");
            assert!(text.chars().count() <= NOTE_MAX_CHARS, "{text}");
            assert!(!text.contains('\n'), "{text}");
        }
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
        assert!(!short.text().contains('…'), "{}", short.text());
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
        assert!(!decision.text().contains("earlier versions"), "{}", decision.text());
        let snapshot = Note::NotInSnapshot {
            family: "article",
            value: "Code civil, art. 9999".to_owned(),
            date: Some(date("2026-11-01")),
        };
        assert!(!snapshot.text().contains("earlier versions"), "{}", snapshot.text());
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
