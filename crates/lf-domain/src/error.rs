//! Erreurs du domaine.
//!
//! # Variantes fermées, enum ouverte au filtrage
//!
//! Chaque variante qui recopie une entrée (`input`) porte `#[non_exhaustive]` : hors du crate
//! elle ne se construit pas, seuls les analyseurs du crate la produisent, via `echo`, d'où la
//! garantie qu'`input` ne dépasse jamais [`ECHO_MAX_CHARS`] caractères (plus `…`). Toute nouvelle
//! variante porteuse d'`input` reçoit le même attribut. L'enum elle-même n'est pas
//! `#[non_exhaustive]` : l'attribut d'enum n'empêcherait pas la construction et imposerait un
//! bras `_` aux `match` en aval, alors qu'un `match` exhaustif en aval (rendu des messages,
//! conversion vers l'erreur applicative) est une protection voulue. En aval, le motif d'une
//! variante fermée s'écrit avec `..`. Choix réexaminé à la publication sur crates.io (M5).
//! `echo` est `pub(crate)` ; [`ECHO_MAX_CHARS`] reste publique (le rendu M4 tronquera à la même
//! longueur).
//!
//! ```compile_fail
//! let _ = lf_domain::error::echo("LEGITEXT000006070721");
//! ```

use thiserror::Error;

use crate::{date::VersionDate, fond::Fond};

/// Longueur maximale, en caractères, d'une entrée recopiée dans un message d'erreur.
pub const ECHO_MAX_CHARS: usize = 64;

/// Erreur de validation d'une valeur du domaine.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DomainError {
    /// Identifiant Légifrance absent ou malformé.
    ///
    /// Ne se construit pas hors du crate :
    ///
    /// ```compile_fail
    /// let _ = lf_domain::DomainError::InvalidId { input: "x".repeat(99), expected: "LEGITEXT" };
    /// ```
    #[error("invalid Legifrance identifier {input:?}: expected {expected} followed by 12 digits")]
    #[non_exhaustive]
    InvalidId {
        /// Entrée reçue, tronquée à [`ECHO_MAX_CHARS`] caractères.
        input: String,
        /// Préfixe(s) accepté(s), ex. `LEGITEXT`.
        expected: &'static str,
    },
    /// Date absente ou malformée ; pour une [`VersionDate`], aussi une date hors de sa plage.
    ///
    /// Ne se construit pas hors du crate :
    ///
    /// ```compile_fail
    /// let _ = lf_domain::DomainError::InvalidDate { input: "x".repeat(99) };
    /// ```
    #[error("invalid date {input:?}: expected YYYY-MM-DD")]
    #[non_exhaustive]
    InvalidDate {
        /// Entrée reçue, tronquée.
        input: String,
    },
    /// ECLI malformé.
    ///
    /// Ne se construit pas hors du crate :
    ///
    /// ```compile_fail
    /// let _ = lf_domain::DomainError::InvalidEcli { input: "x".repeat(99) };
    /// ```
    #[error("invalid ECLI {input:?}: expected ECLI:FR:<court>:<year>:<ordinal>")]
    #[non_exhaustive]
    InvalidEcli {
        /// Entrée reçue, tronquée.
        input: String,
    },
    /// Fond inconnu.
    ///
    /// Ne se construit pas hors du crate :
    ///
    /// ```compile_fail
    /// let _ = lf_domain::DomainError::UnknownFond { input: "x".repeat(99) };
    /// ```
    #[error(
        "unknown fond {input:?}: expected one of LODA, CODE, JURI, CONSTIT, CETAT, CNIL, KALI, ACCO"
    )]
    #[non_exhaustive]
    UnknownFond {
        /// Entrée reçue, tronquée.
        input: String,
    },
    /// Suffixe de version mal formé (`_foo`, `_` seul, `_31-02-2023`) : il doit
    /// s'écrire `_JJ-MM-AAAA` et désigner une vraie date (décision C4).
    ///
    /// Ne se construit pas hors du crate :
    ///
    /// ```compile_fail
    /// let _ = lf_domain::DomainError::BadSuffix { input: "x".repeat(99) };
    /// ```
    #[error(
        "invalid version suffix in {input:?}: expected _DD-MM-YYYY with a real date, e.g. _08-09-2023"
    )]
    #[non_exhaustive]
    BadSuffix {
        /// Identifiant reçu en entier, suffixe compris, tronqué.
        input: String,
    },
    /// Déclaration de couverture sans lacune connue.
    #[error("coverage must declare at least one known gap")]
    EmptyCoverageGaps,
    /// Table des matières amont plus profonde que [`TOC_MAX_DEPTH`](crate::entity::TOC_MAX_DEPTH) niveaux (données incohérentes, pas une erreur de l'utilisateur).
    #[error("table of contents deeper than {max} levels")]
    TocTooDeep {
        /// Profondeur maximale acceptée.
        max: u16,
    },
    /// `toc_offset` au-delà de la dernière entrée de la table des matières.
    #[error("toc_offset={offset} is past the end (toc_total={total})")]
    TocOffsetPastEnd {
        /// Décalage demandé.
        offset: u32,
        /// Nombre total d'entrées.
        total: u32,
    },
    /// Période inversée : `date_from` postérieure à `date_to` ([`DateRange::new`](crate::date::DateRange::new)).
    #[error("date_from={from} is after date_to={to}")]
    InvertedDateRange {
        /// Début demandé.
        from: VersionDate,
        /// Fin demandée.
        to: VersionDate,
    },
    /// Requête de recherche vide : absente après normalisation (blancs et caractères de contrôle retirés).
    #[error("query is required")]
    EmptyQuery,
    /// Requête de recherche plus longue que [`QUERY_MAX_CHARS`](crate::search::QUERY_MAX_CHARS) caractères.
    #[error("query is too long ({len} characters, max {max})")]
    QueryTooLong {
        /// Longueur de la requête normalisée, en caractères.
        len: usize,
        /// Longueur maximale acceptée.
        max: usize,
    },
    /// Valeur numérique hors de ses bornes (bornes incluses), ex. `page_size`.
    #[error("{field}={value} out of range ({min}..{max})")]
    OutOfRange {
        /// Nom du paramètre côté utilisateur.
        field: &'static str,
        /// Valeur reçue.
        value: i64,
        /// Borne basse acceptée.
        min: i64,
        /// Borne haute acceptée.
        max: i64,
    },
    /// `version_date` fournie pour un fond qui n'est ni `LODA` ni `CODE`.
    #[error("version_date only applies to fond LODA or CODE, not {fond}")]
    VersionDateNotApplicable {
        /// Fond demandé.
        fond: Fond,
    },
    /// `date_from` ou `date_to` fournie pour un fond sans filtre de période (`CODE`).
    #[error("date_from/date_to are not supported for fond={fond}")]
    DateFilterNotApplicable {
        /// Fond demandé.
        fond: Fond,
    },
    /// Page hors de la fenêtre de résultats atteignables
    /// ([`REACHABLE_RESULTS`](crate::search::REACHABLE_RESULTS)).
    #[error("page={page} with page_size={page_size} goes beyond the {max} reachable results")]
    PageBeyondReach {
        /// Page demandée.
        page: u32,
        /// Taille de page demandée.
        page_size: u32,
        /// Nombre maximal de résultats atteignables.
        max: u64,
    },
    /// Identifiant de résultat de recherche d'une autre famille que celle du fond interrogé
    /// (par exemple un `JURITEXT` dans un résultat `LODA`).
    ///
    /// Ne se construit pas hors du crate :
    ///
    /// ```compile_fail
    /// let _ = lf_domain::DomainError::HitKindMismatch {
    ///     id: "x".repeat(99),
    ///     fond: lf_domain::fond::Fond::Loda,
    /// };
    /// ```
    #[error("hit identifier {id:?} does not belong to fond {fond}")]
    #[non_exhaustive]
    HitKindMismatch {
        /// Identifiant reçu, tronqué à [`ECHO_MAX_CHARS`] caractères.
        id: String,
        /// Fond interrogé.
        fond: Fond,
    },
    /// Texte de plus de 16 000 000 de caractères (garde du texte long).
    #[error("text of {chars} characters exceeds the 16000000 limit")]
    TextTooLarge {
        /// Longueur du texte reçu, en caractères.
        chars: u64,
    },
}

/// Recopie `input` pour un message d'erreur, tronquée à [`ECHO_MAX_CHARS`] caractères.
pub(crate) fn echo(input: &str) -> String {
    let mut chars = input.chars();
    let head: String = chars.by_ref().take(ECHO_MAX_CHARS).collect();
    if chars.next().is_some() { head + "…" } else { head }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        date::{CivilDate, VersionDate},
        ecli::Ecli,
        id::{AnyId, ArticleId, DecisionId, DocumentId, LegiTextId, SectionId, parse_versioned},
    };
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    /// Entrée recopiée dans l'erreur, quelle que soit la variante.
    fn echoed(error: &DomainError) -> &str {
        match error {
            DomainError::InvalidId { input, .. }
            | DomainError::InvalidDate { input }
            | DomainError::InvalidEcli { input }
            | DomainError::UnknownFond { input }
            | DomainError::BadSuffix { input }
            | DomainError::HitKindMismatch { id: input, .. } => input,
            DomainError::EmptyCoverageGaps
            | DomainError::TocTooDeep { .. }
            | DomainError::TocOffsetPastEnd { .. }
            | DomainError::InvertedDateRange { .. }
            | DomainError::EmptyQuery
            | DomainError::QueryTooLong { .. }
            | DomainError::OutOfRange { .. }
            | DomainError::VersionDateNotApplicable { .. }
            | DomainError::DateFilterNotApplicable { .. }
            | DomainError::PageBeyondReach { .. }
            | DomainError::TextTooLarge { .. } => "",
        }
    }

    #[test]
    fn echo_keeps_short_input() {
        assert_eq!(echo("LEGITEXT"), "LEGITEXT");
    }

    #[test]
    fn echo_keeps_input_of_exactly_max_chars() {
        let input = "é".repeat(ECHO_MAX_CHARS);
        assert_eq!(echo(&input), input);
    }

    #[test]
    fn echo_truncates_long_input_with_ellipsis() {
        let input = "x".repeat(ECHO_MAX_CHARS + 1);
        assert_eq!(echo(&input), format!("{}…", "x".repeat(ECHO_MAX_CHARS)));
    }

    #[test]
    fn messages_are_actionable() {
        let err = DomainError::InvalidId { input: "ABC".into(), expected: "LEGITEXT" };
        assert_eq!(
            err.to_string(),
            "invalid Legifrance identifier \"ABC\": expected LEGITEXT followed by 12 digits"
        );
        assert_eq!(
            DomainError::InvalidDate { input: "x".into() }.to_string(),
            "invalid date \"x\": expected YYYY-MM-DD"
        );
        assert_eq!(
            DomainError::InvalidEcli { input: "x".into() }.to_string(),
            "invalid ECLI \"x\": expected ECLI:FR:<court>:<year>:<ordinal>"
        );
        assert_eq!(
            DomainError::UnknownFond { input: "x".into() }.to_string(),
            "unknown fond \"x\": expected one of LODA, CODE, JURI, CONSTIT, CETAT, CNIL, KALI, ACCO"
        );
        assert_eq!(
            DomainError::BadSuffix { input: "LEGITEXT000033205014_31-02-2023".into() }.to_string(),
            "invalid version suffix in \"LEGITEXT000033205014_31-02-2023\": \
             expected _DD-MM-YYYY with a real date, e.g. _08-09-2023"
        );
        assert_eq!(
            DomainError::EmptyCoverageGaps.to_string(),
            "coverage must declare at least one known gap"
        );
        assert_eq!(
            DomainError::TocTooDeep { max: 64 }.to_string(),
            "table of contents deeper than 64 levels"
        );
        assert_eq!(
            DomainError::TocOffsetPastEnd { offset: 300, total: 88 }.to_string(),
            "toc_offset=300 is past the end (toc_total=88)"
        );
        let from = VersionDate::parse("2026-01-01").unwrap();
        let to = VersionDate::parse("2025-01-01").unwrap();
        assert_eq!(
            DomainError::InvertedDateRange { from, to }.to_string(),
            "date_from=2026-01-01 is after date_to=2025-01-01"
        );
        assert_eq!(DomainError::EmptyQuery.to_string(), "query is required");
        assert_eq!(
            DomainError::QueryTooLong { len: 612, max: 500 }.to_string(),
            "query is too long (612 characters, max 500)"
        );
        assert_eq!(
            DomainError::OutOfRange { field: "page_size", value: 0, min: 1, max: 100 }.to_string(),
            "page_size=0 out of range (1..100)"
        );
        assert_eq!(
            DomainError::VersionDateNotApplicable { fond: Fond::Juri }.to_string(),
            "version_date only applies to fond LODA or CODE, not JURI"
        );
        assert_eq!(
            DomainError::DateFilterNotApplicable { fond: Fond::Code }.to_string(),
            "date_from/date_to are not supported for fond=CODE"
        );
        assert_eq!(
            DomainError::PageBeyondReach { page: 200, page_size: 100, max: 10_000 }.to_string(),
            "page=200 with page_size=100 goes beyond the 10000 reachable results"
        );
    }

    #[test]
    fn hit_kind_mismatch_echoes_a_truncated_id() {
        let long = "J".repeat(10_000);
        let error = DomainError::HitKindMismatch { id: echo(&long), fond: Fond::Juri };
        assert_eq!(echoed(&error), echo(&long));
        assert_eq!(
            error.to_string(),
            format!("hit identifier \"{}…\" does not belong to fond JURI", "J".repeat(64))
        );
        let short =
            DomainError::HitKindMismatch { id: "LEGITEXT000006070721".into(), fond: Fond::Kali };
        assert_eq!(
            short.to_string(),
            "hit identifier \"LEGITEXT000006070721\" does not belong to fond KALI"
        );
    }

    #[test]
    fn text_too_large_message_names_the_limit() {
        assert_eq!(
            DomainError::TextTooLarge { chars: 16_000_001 }.to_string(),
            "text of 16000001 characters exceeds the 16000000 limit"
        );
    }

    #[test]
    fn every_parser_echoes_a_truncated_input() {
        let long = "A".repeat(10_000);
        let errors = [
            LegiTextId::parse(&long).unwrap_err(),
            AnyId::parse(&long).unwrap_err(),
            ArticleId::parse(&long).unwrap_err(),
            DecisionId::parse(&long).unwrap_err(),
            DocumentId::parse(&long).unwrap_err(),
            SectionId::parse(&long).unwrap_err(),
            parse_versioned(&long).unwrap_err(),
            CivilDate::parse(&long).unwrap_err(),
            Ecli::parse(&long).unwrap_err(),
            Fond::parse(&long).unwrap_err(),
        ];
        for error in &errors {
            assert_eq!(echoed(error), echo(&long));
            assert!(error.to_string().chars().count() < 250, "message too long: {error}");
        }
    }

    #[test]
    fn bad_suffix_echoes_the_whole_input_truncated() {
        // L'écho porte l'identifiant entier (03 §2.7), tronqué comme ailleurs.
        let long = format!("LEGITEXT000006070721_{}", "x".repeat(100));
        let errors = [
            LegiTextId::parse(&long).unwrap_err(),
            AnyId::parse(&long).unwrap_err(),
            parse_versioned(&long).unwrap_err(),
        ];
        for error in &errors {
            assert_eq!(*error, DomainError::BadSuffix { input: echo(&long) });
            assert_eq!(echoed(error), format!("LEGITEXT000006070721_{}…", "x".repeat(43)));
        }
    }

    #[test]
    fn variants_without_input_echo_nothing() {
        for error in [
            DomainError::EmptyCoverageGaps,
            DomainError::TocTooDeep { max: 64 },
            DomainError::TocOffsetPastEnd { offset: 300, total: 88 },
            DomainError::InvertedDateRange { from: VersionDate::MAX, to: VersionDate::MIN },
            DomainError::EmptyQuery,
            DomainError::QueryTooLong { len: 501, max: 500 },
            DomainError::OutOfRange { field: "page_size", value: 0, min: 1, max: 100 },
            DomainError::VersionDateNotApplicable { fond: Fond::Juri },
            DomainError::DateFilterNotApplicable { fond: Fond::Code },
            DomainError::PageBeyondReach { page: 200, page_size: 100, max: 10_000 },
            DomainError::TextTooLarge { chars: 16_000_001 },
        ] {
            assert_eq!(echoed(&error), "", "{error}");
        }
    }

    #[test]
    fn messages_escape_control_characters() {
        let raw = "a\nb\0\u{1b}";
        let messages = [
            DomainError::InvalidId { input: raw.into(), expected: "LEGITEXT" }.to_string(),
            DomainError::InvalidDate { input: raw.into() }.to_string(),
            DomainError::BadSuffix { input: raw.into() }.to_string(),
        ];
        for message in &messages {
            assert!(message.contains(r"a\nb\0\u{1b}"), "not escaped: {message}");
            assert!(!message.contains('\n') && !message.contains('\0'), "raw control: {message}");
        }
    }

    proptest! {
        #[test]
        fn echo_truncates_at_max_chars(s in "\\PC{0,200}") {
            let out = echo(&s);
            let truncated = s.chars().count() > ECHO_MAX_CHARS;
            prop_assert!(out.chars().count() <= ECHO_MAX_CHARS + 1);
            if truncated {
                let head: String = s.chars().take(ECHO_MAX_CHARS).collect();
                prop_assert_eq!(out, format!("{head}…"));
            } else {
                prop_assert_eq!(out, s);
            }
        }
    }
}
