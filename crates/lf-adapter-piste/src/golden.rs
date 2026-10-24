//! Harnais du golden de parité (annexe A §4, §5.1 à §5.4 ; 06 §2.3 `parity_golden` ; D12, D13) :
//! les divergences volontaires déclarées en données, les valeurs explicites (surcharges) et leur
//! cohérence avec l'annexe A de parité. Ce module n'existe que sous `cfg(test)`.
//!
//! Le golden (`tests/fixtures/reference/parite-golden.json`) décrit la **référence** Python sur ses
//! seize fixtures ; il est copié sans modification depuis `docs/plan/annexes/donnees/`, et la copie
//! est comparée à l'octet au fichier du plan (`golden_copy_matches_plan_data`, jamais ignoré).

use serde_json::Value;

/// Une variante par clé du §4 de l'annexe A dont la colonne « Golden » vaut « oui », dans l'ordre
/// de l'annexe. Le test `golden_divergences_match_annex_a` garde l'égalité des deux ensembles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum GoldenDivergence {
    /// `human_readable_citation` devient `citation`.
    CitationRenamed,
    /// `eli_uri` supprimé.
    EliUriDropped,
    /// `eli_note` et `dataset_note` absentes.
    NotesDropped,
    /// Champs `null` et listes vides par défaut omis.
    NullsOmitted,
    /// `fond` et `query` non renvoyés par les recherches.
    SearchEchoDropped,
    /// Champs renommés (`date_texte`, `text_id`, `date_parution`, `title_long`, `known_gaps`).
    FieldsRenamed,
    /// Champs que la sortie ne rend plus.
    FieldsNotRendered,
    /// `title` omis quand la citation le contient.
    TitleOmittedWhenInCitation,
    /// Champs ajoutés par la sortie (liste blanche).
    FieldsAdded,
    /// Typographie des citations (accents, suffixe d'objet).
    CitationTypography,
    /// Dates sentinelles (à partir de 2222-01-01) omises.
    SentinelDatesNone,
    /// Mojibake réparé avant tout traitement.
    MojibakeRepaired,
    /// Textes courts nettoyés (`text::clean`).
    ShortTextCleaned,
    /// Textes longs structurés en paragraphes (comparaison aux blancs près).
    LongTextStructured,
    /// `byte_size` remplacé par `text_length`.
    ByteSizeToTextLength,
    /// Table des matières aplatie en préordre (projection).
    TocTree,
    /// `etat` de repli sur `jurisState`, en code API.
    EtatJurisStateFallback,
    /// `numero_affaire` d'une décision du Conseil d'État renseigné.
    NumeroAffaireCetat,
    /// Identifiant renvoyé par l'API conservé, non l'identifiant demandé.
    ReturnedIdKept,
}

impl GoldenDivergence {
    /// Les dix-neuf divergences, dans l'ordre de l'annexe A §4.
    pub(crate) const ALL: [Self; 19] = [
        Self::CitationRenamed,
        Self::EliUriDropped,
        Self::NotesDropped,
        Self::NullsOmitted,
        Self::SearchEchoDropped,
        Self::FieldsRenamed,
        Self::FieldsNotRendered,
        Self::TitleOmittedWhenInCitation,
        Self::FieldsAdded,
        Self::CitationTypography,
        Self::SentinelDatesNone,
        Self::MojibakeRepaired,
        Self::ShortTextCleaned,
        Self::LongTextStructured,
        Self::ByteSizeToTextLength,
        Self::TocTree,
        Self::EtatJurisStateFallback,
        Self::NumeroAffaireCetat,
        Self::ReturnedIdKept,
    ];

    /// Les huit divergences qui ne changent que la sortie d'outil : le niveau 1 (entités) ne
    /// peut pas les exercer, le niveau 2 les couvre (M5, D12).
    pub(crate) const TOOL_OUTPUT_ONLY: [Self; 8] = [
        Self::NotesDropped,
        Self::NullsOmitted,
        Self::SearchEchoDropped,
        Self::FieldsRenamed,
        Self::FieldsNotRendered,
        Self::TitleOmittedWhenInCitation,
        Self::FieldsAdded,
        Self::ByteSizeToTextLength,
    ];

    /// Clé de l'annexe A (`snake_case`).
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::CitationRenamed => "citation_renamed",
            Self::EliUriDropped => "eli_uri_dropped",
            Self::NotesDropped => "notes_dropped",
            Self::NullsOmitted => "nulls_omitted",
            Self::SearchEchoDropped => "search_echo_dropped",
            Self::FieldsRenamed => "fields_renamed",
            Self::FieldsNotRendered => "fields_not_rendered",
            Self::TitleOmittedWhenInCitation => "title_omitted_when_in_citation",
            Self::FieldsAdded => "fields_added",
            Self::CitationTypography => "citation_typography",
            Self::SentinelDatesNone => "sentinel_dates_none",
            Self::MojibakeRepaired => "mojibake_repaired",
            Self::ShortTextCleaned => "short_text_cleaned",
            Self::LongTextStructured => "long_text_structured",
            Self::ByteSizeToTextLength => "byte_size_to_text_length",
            Self::TocTree => "toc_tree",
            Self::EtatJurisStateFallback => "etat_juris_state_fallback",
            Self::NumeroAffaireCetat => "numero_affaire_cetat",
            Self::ReturnedIdKept => "returned_id_kept",
        }
    }
}

/// Valeur explicite d'une entrée du golden, quand l'attendu n'est pas une transformation
/// mécanique de la valeur de la référence (annexe A §5.4, comparateur `Override`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Override {
    /// Clé de l'entrée dans `normalizer` (par exemple `consult_loda.json`).
    pub(crate) entry: &'static str,
    /// Pointeur JSON du champ dans l'entrée.
    pub(crate) pointer: &'static str,
    /// Valeur attendue côté Rust, en JSON.
    pub(crate) expected: &'static str,
    /// Divergence qui justifie la valeur.
    pub(crate) because: GoldenDivergence,
}

/// Entrée de la variante de `test_parse.py:226` : l'identifiant demandé est une `KALIARTI`.
const RETURNED_ID_ENTRY: &str = "consult_article.json[id=KALIARTI000049886059]";

/// Les six lignes de l'annexe A §5.4 (« Valeurs explicites »).
pub(crate) const OVERRIDES: [Override; 6] = [
    Override {
        entry: "consult_juri_constit.json",
        pointer: "/human_readable_citation",
        expected: r#""Cons. const., décision n° 2025-1173 QPC du 7 novembre 2025""#,
        because: GoldenDivergence::CitationTypography,
    },
    Override {
        entry: "consult_cnil.json",
        pointer: "/human_readable_citation",
        expected: r#""CNIL, délibération n° SAN-2026-002 du 8 janvier 2026""#,
        because: GoldenDivergence::CitationTypography,
    },
    Override {
        entry: "consult_loda.json",
        pointer: "/etat",
        expected: r#""VIGUEUR""#,
        because: GoldenDivergence::EtatJurisStateFallback,
    },
    Override {
        entry: "consult_juri_cetat.json",
        pointer: "/numero_affaire",
        expected: r#"["506507"]"#,
        because: GoldenDivergence::NumeroAffaireCetat,
    },
    Override {
        entry: RETURNED_ID_ENTRY,
        pointer: "/article_id",
        expected: r#""LEGIARTI000006419288""#,
        because: GoldenDivergence::ReturnedIdKept,
    },
    Override {
        entry: RETURNED_ID_ENTRY,
        pointer: "/source_url",
        expected: r#""https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288""#,
        because: GoldenDivergence::ReturnedIdKept,
    },
];

/// Valeur explicite et divergence qui la justifie pour le champ `pointer` de l'entrée `entry`,
/// `None` si le champ n'a pas de surcharge.
///
/// # Panics
///
/// Si une valeur de [`OVERRIDES`] n'est pas du JSON (le test des surcharges l'exclut).
pub(crate) fn overridden(entry: &str, pointer: &str) -> Option<(Value, GoldenDivergence)> {
    OVERRIDES
        .iter()
        .find(|over| over.entry == entry && over.pointer == pointer)
        .map(|over| (serde_json::from_str(over.expected).expect("override is JSON"), over.because))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::{Value, json};

    use super::{GoldenDivergence, OVERRIDES, overridden};
    use crate::integrity::{hex, sha256, sidecar, text_at};
    use crate::map::fixtures::golden;

    /// Copie du golden dans le crate, à l'octet.
    const COPY: &[u8] = include_bytes!("../tests/fixtures/reference/parite-golden.json");
    /// Fichier du plan dont la copie est tirée, à l'octet.
    const PLAN: &[u8] = include_bytes!("../../../docs/plan/annexes/donnees/parite-golden.json");
    /// Annexe A de parité.
    const ANNEX_A: &str = include_str!("../../../docs/plan/annexes/A-parite.md");

    /// Clés du §4 de l'annexe dont la colonne « Golden » vaut « oui », lues ligne à ligne.
    fn annex_keys(annex: &str) -> BTreeSet<String> {
        let start = annex.find("## 4. Divergences volontaires consolidées").expect("§4 starts");
        let end = annex.find("## 5. Données golden").expect("§5 starts");
        annex[start..end]
            .lines()
            .filter(|line| line.starts_with("| `"))
            .filter(|line| {
                line.trim_end()
                    .trim_end_matches('|')
                    .rsplit('|')
                    .next()
                    .is_some_and(|last| last.trim().starts_with("oui"))
            })
            .map(|line| line.split('|').nth(1).expect("first cell").trim().trim_matches('`').into())
            .collect()
    }

    /// Clés de `ALL`, sans doublon.
    fn code_keys() -> BTreeSet<String> {
        GoldenDivergence::ALL.iter().map(|d| d.key().to_owned()).collect()
    }

    #[test]
    fn golden_divergences_match_annex_a() {
        let from_code = code_keys();
        assert_eq!(from_code.len(), 19);
        assert_eq!(annex_keys(ANNEX_A), from_code);
        assert_eq!(annex_keys(&ANNEX_A.replace('\n', "\r\n")), from_code);
    }

    #[test]
    fn tool_output_only_divergences_are_known_variants() {
        assert_eq!(GoldenDivergence::TOOL_OUTPUT_ONLY.len(), 8);
        let all: BTreeSet<_> = GoldenDivergence::ALL.into_iter().collect();
        let only: BTreeSet<_> = GoldenDivergence::TOOL_OUTPUT_ONLY.into_iter().collect();
        assert_eq!(only.len(), 8);
        assert!(only.is_subset(&all));
    }

    #[test]
    fn overrides_point_to_existing_golden_values() {
        let golden = golden();
        for over in OVERRIDES {
            let entry = &golden["normalizer"][over.entry];
            let current = entry.pointer(over.pointer).expect("pointer exists in the golden");
            let expected: Value = serde_json::from_str(over.expected).expect("expected is JSON");
            assert_ne!(current, &expected, "{} {}", over.entry, over.pointer);
            assert_eq!(
                overridden(over.entry, over.pointer),
                Some((expected, over.because)),
                "{} {}",
                over.entry,
                over.pointer
            );
            assert!(GoldenDivergence::ALL.contains(&over.because));
            assert!(!GoldenDivergence::TOOL_OUTPUT_ONLY.contains(&over.because));
        }
        assert_eq!(overridden("consult_loda.json", "/title"), None);
    }

    #[test]
    fn golden_meta_is_expected() {
        let golden = golden();
        assert_eq!(
            golden["_meta"],
            json!({
                "generated_by": "scratchpad/scripts/golden.py (re-implementation independante, n'importe pas la reference)",
                "reference": "matematicsolutions/fr-eli-mcp@d3b5b13 tests/fixtures/*.json",
                "normalizer": "sortie des fonctions citations.normalize_* (dict brut, avant modele pydantic)",
                "tool_output": "structuredContent attendu de l'outil (ordre des champs des modeles models.py, defauts appliques, byte_size, eli_note/dataset_note)",
            })
        );
        assert_eq!(golden["normalizer"].as_object().expect("object").len(), 17);
        assert_eq!(golden["tool_output"].as_object().expect("object").len(), 16);
        let meta = sidecar("parite-golden");
        let keys: BTreeSet<&str> = meta.keys().map(String::as_str).collect();
        assert_eq!(keys, BTreeSet::from(["license", "reference", "sha256", "source"]));
        assert_eq!(text_at(&meta, "source"), Some("docs/plan/annexes/donnees/parite-golden.json"));
        assert_eq!(text_at(&meta, "reference"), Some("matematicsolutions/fr-eli-mcp@d3b5b13"));
        assert_eq!(text_at(&meta, "sha256"), Some(hex(&sha256(COPY)).as_str()));
    }

    #[test]
    fn golden_copy_matches_plan_data() {
        assert_eq!(COPY.len(), 118_107);
        assert!(COPY == PLAN, "the crate copy of the golden differs from the plan data");
    }
}
