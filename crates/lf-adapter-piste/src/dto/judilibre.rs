//! DTO des réponses de Judilibre (`GET /search`, `GET /decision`, ADR-0035).
//!
//! Minimaux et tolérants : les champs inconnus sont ignorés, et un champ de texte d'un type
//! inattendu donne `None` au lieu de faire échouer la page (`opt_string_or_number`), car la source
//! est en bêta et ses données sont parfois mal formées. Les dates restent du texte, telles que
//! publiées (une décision de 2017 porte `0201-02-24`). Seules l'enveloppe d'une recherche et la
//! forme des listes sont exigées. Chaque élément de liste est payé sur le budget du document
//! (`de::bounded_list`).
//!
//! Un même DTO sert au résultat de recherche et à la décision : le résultat n'a ni texte ni zones,
//! la décision n'a pas d'extraits surlignés.
//!
//! `Debug` est dérivé pour les tests : un DTO porte du texte du corps, il ne se journalise pas.

use serde::{Deserialize, Deserializer};

use super::de::{self, capped_list, null_default, opt_string_or_number, opt_u64_lenient};

/// Plus grand nombre de résultats admis dans une page : `page_size` vaut au plus 50.
pub(crate) const HITS_MAX: usize = 50;

/// Réponse de `GET /search`.
#[derive(Debug, Deserialize)]
pub(crate) struct SearchDto {
    /// Nombre total de décisions trouvées.
    pub(crate) total: u64,
    /// Résultats de la page, [`HITS_MAX`] au plus.
    #[serde(default, deserialize_with = "bounded_hits")]
    pub(crate) results: Vec<DecisionDto>,
}

/// Lit les résultats d'une page : au-delà de [`HITS_MAX`], `too many elements`.
fn bounded_hits<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<DecisionDto>, D::Error> {
    capped_list(deserializer, HITS_MAX)
}

/// Une décision : résultat de recherche, ou réponse de `GET /decision`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct DecisionDto {
    /// Identifiant (24 caractères hexadécimaux).
    #[serde(deserialize_with = "opt_string_or_number")]
    pub(crate) id: Option<String>,
    /// Code de la juridiction : `cc`, `ca`, `tj`, `tcom`, `cph`.
    #[serde(deserialize_with = "opt_string_or_number")]
    pub(crate) jurisdiction: Option<String>,
    /// Chambre : codée pour la Cour de cassation (`civ1`), en clair ailleurs.
    #[serde(deserialize_with = "opt_string_or_number")]
    pub(crate) chamber: Option<String>,
    /// Formation de la Cour de cassation (`fs`).
    #[serde(deserialize_with = "opt_string_or_number")]
    pub(crate) formation: Option<String>,
    /// Siège (`ca_paris`, `tj75056`).
    #[serde(deserialize_with = "opt_string_or_number")]
    pub(crate) location: Option<String>,
    /// Date de la décision, telle que publiée.
    #[serde(deserialize_with = "opt_string_or_number")]
    pub(crate) decision_date: Option<String>,
    /// Numéro de pourvoi ou de rôle.
    #[serde(deserialize_with = "opt_string_or_number")]
    pub(crate) number: Option<String>,
    /// ECLI (Cour de cassation).
    #[serde(deserialize_with = "opt_string_or_number")]
    pub(crate) ecli: Option<String>,
    /// Solution codée (`cassation`, `other`), ou en texte libre (tribunaux judiciaires).
    #[serde(deserialize_with = "opt_string_or_number")]
    pub(crate) solution: Option<String>,
    /// Solution en clair, quand `solution` vaut `other`.
    #[serde(deserialize_with = "opt_string_or_number")]
    pub(crate) solution_alt: Option<String>,
    /// Niveaux de publication codés (`b`, `r`, `l`, `c`, `n`).
    #[serde(deserialize_with = "de::bounded_list")]
    pub(crate) publication: Vec<Word>,
    /// Sommaire de la Cour de cassation.
    #[serde(deserialize_with = "opt_string_or_number")]
    pub(crate) summary: Option<String>,
    /// Thèmes.
    #[serde(deserialize_with = "de::bounded_list")]
    pub(crate) themes: Vec<Word>,
    /// Passages surlignés d'un résultat de recherche.
    #[serde(deserialize_with = "null_default")]
    pub(crate) highlights: HighlightsDto,
    /// Texte entier de la décision.
    #[serde(deserialize_with = "opt_string_or_number")]
    pub(crate) text: Option<String>,
    /// Zones du texte ; absentes pour une décision ancienne.
    #[serde(deserialize_with = "null_default")]
    pub(crate) zones: ZonesDto,
    /// Textes appliqués : intitulés avec des liens HTML.
    #[serde(deserialize_with = "de::bounded_list")]
    pub(crate) visa: Vec<TitleDto>,
    /// Rapprochements : intitulés avec des liens HTML.
    #[serde(deserialize_with = "de::bounded_list")]
    pub(crate) rapprochements: Vec<TitleDto>,
    /// Décision attaquée.
    pub(crate) contested: Option<ContestedDto>,
    /// Date de dernière mise à jour, telle que publiée.
    #[serde(deserialize_with = "opt_string_or_number")]
    pub(crate) update_date: Option<String>,
    /// Décision publiée en partie seulement.
    #[serde(deserialize_with = "null_default")]
    pub(crate) partial: bool,
    /// Décision retirée de la base.
    #[serde(deserialize_with = "null_default")]
    pub(crate) to_be_deleted: bool,
}

/// Un mot d'une liste (thème, niveau de publication) : `None` s'il n'est pas du texte.
#[derive(Debug, Default, Deserialize)]
#[serde(transparent)]
pub(crate) struct Word(
    #[serde(deserialize_with = "opt_string_or_number")] pub(crate) Option<String>,
);

/// Passages surlignés : `text` seul est lu.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct HighlightsDto {
    /// Passages du texte, avec des balises `<em>`.
    #[serde(deserialize_with = "de::bounded_list")]
    pub(crate) text: Vec<Word>,
}

/// Zones d'une décision : une liste de fragments par partie. Struct et non table : aucune clé du
/// corps ne devient un nom (`decode::safe_key`).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ZonesDto {
    /// Entête.
    #[serde(deserialize_with = "de::bounded_list")]
    pub(crate) introduction: Vec<SpanDto>,
    /// Exposé du litige.
    #[serde(deserialize_with = "de::bounded_list")]
    pub(crate) expose: Vec<SpanDto>,
    /// Moyens.
    #[serde(deserialize_with = "de::bounded_list")]
    pub(crate) moyens: Vec<SpanDto>,
    /// Motivation.
    #[serde(deserialize_with = "de::bounded_list")]
    pub(crate) motivations: Vec<SpanDto>,
    /// Dispositif.
    #[serde(deserialize_with = "de::bounded_list")]
    pub(crate) dispositif: Vec<SpanDto>,
    /// Moyens annexés.
    #[serde(deserialize_with = "de::bounded_list")]
    pub(crate) annexes: Vec<SpanDto>,
}

/// Un fragment de zone, en unités UTF-16 ; une borne illisible donne `None`.
#[derive(Debug, Default, Clone, Copy, Deserialize)]
#[serde(default)]
pub(crate) struct SpanDto {
    /// Début, inclus.
    #[serde(deserialize_with = "opt_u64_lenient")]
    pub(crate) start: Option<u64>,
    /// Fin, exclue.
    #[serde(deserialize_with = "opt_u64_lenient")]
    pub(crate) end: Option<u64>,
}

/// Un intitulé de visa ou de rapprochement.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct TitleDto {
    /// Intitulé, avec des liens HTML.
    #[serde(deserialize_with = "opt_string_or_number")]
    pub(crate) title: Option<String>,
}

/// La décision attaquée.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ContestedDto {
    /// Juridiction et siège, en clair.
    #[serde(deserialize_with = "opt_string_or_number")]
    pub(crate) jurisdiction: Option<String>,
    /// Chambre.
    #[serde(deserialize_with = "opt_string_or_number")]
    pub(crate) chamber: Option<String>,
    /// Date.
    #[serde(deserialize_with = "opt_string_or_number")]
    pub(crate) date: Option<String>,
    /// Solution.
    #[serde(deserialize_with = "opt_string_or_number")]
    pub(crate) solution: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use lf_application::ports::CorpusError;
    use lf_domain::judilibre::CASE_LAW_PAGE_SIZE_MAX;

    use super::*;
    use crate::dto::decode::decode;
    use crate::request::judilibre::{DECISION, SEARCH};

    /// Texte d'une fixture de `tests/fixtures/judilibre/`.
    fn fixture(name: &str) -> String {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/judilibre").join(name);
        std::fs::read_to_string(path).unwrap()
    }

    fn words(list: &[Word]) -> Vec<Option<&str>> {
        list.iter().map(|word| word.0.as_deref()).collect()
    }

    #[test]
    fn the_page_cap_is_the_largest_page_size() {
        assert_eq!(u32::try_from(HITS_MAX).unwrap(), CASE_LAW_PAGE_SIZE_MAX);
    }

    #[test]
    fn a_cassation_search_page_is_read() {
        let page: SearchDto = decode(&fixture("search-cc.json"), SEARCH).unwrap();
        assert_eq!((page.total, page.results.len()), (240, 3));
        let first = &page.results[0];
        assert_eq!(first.id.as_deref(), Some("5fca624b7eeb384a40cafb85"));
        assert_eq!(first.jurisdiction.as_deref(), Some("cc"));
        assert_eq!(first.chamber.as_deref(), Some("civ1"));
        assert_eq!(first.formation.as_deref(), Some("fs"));
        assert_eq!(first.decision_date.as_deref(), Some("2019-11-20"));
        assert_eq!(first.number.as_deref(), Some("16-15.867"));
        assert_eq!(first.ecli.as_deref(), Some("ECLI:FR:CCASS:2019:C100963"));
        assert_eq!(first.solution.as_deref(), Some("cassation"));
        assert_eq!(words(&first.publication), [Some("b")]);
        assert!(first.summary.as_deref().unwrap().starts_with("Un époux commun en bien"));
        assert_eq!(first.themes.len(), 11);
        assert_eq!(first.highlights.text.len(), 2);
        assert_eq!((first.text.as_deref(), first.zones.dispositif.len()), (None, 0));
    }

    #[test]
    fn an_appeal_search_page_is_read() {
        let page: SearchDto = decode(&fixture("search-ca.json"), SEARCH).unwrap();
        assert_eq!((page.total, page.results.len()), (58, 2));
        let first = &page.results[0];
        assert_eq!(first.chamber.as_deref(), Some("Pôle 3 - Chambre 1"));
        assert_eq!(first.location.as_deref(), Some("ca_paris"));
        assert_eq!(first.solution.as_deref(), Some("other"));
        assert!(first.solution_alt.as_deref().unwrap().starts_with("Confirme la décision"));
        assert_eq!((first.ecli.as_deref(), first.summary.as_deref()), (None, None));
        assert!(first.publication.is_empty());
        let empty: SearchDto = decode(&fixture("search-empty-page.json"), SEARCH).unwrap();
        assert_eq!((empty.total, empty.results.len()), (336_114, 0));
    }

    #[test]
    fn a_zoned_decision_is_read() {
        let doc: DecisionDto = decode(&fixture("decision-cc.json"), DECISION).unwrap();
        assert_eq!(doc.update_date.as_deref(), Some("2024-02-28"));
        assert_eq!((doc.moyens_and_motivations(), doc.zones.annexes.len()), ((2, 2), 1));
        assert_eq!((doc.visa.len(), doc.rapprochements.len()), (3, 2));
        let contested = doc.contested.as_ref().unwrap();
        assert_eq!(contested.jurisdiction.as_deref(), Some("Cour d'appel de Paris"));
        assert_eq!(contested.chamber.as_deref(), Some("E1"));
        assert_eq!(contested.date.as_deref(), Some("2016-01-27"));
        assert!(!doc.partial && !doc.to_be_deleted);
        let old: DecisionDto = decode(&fixture("decision-ca-sans-zones.json"), DECISION).unwrap();
        assert_eq!(old.decision_date.as_deref(), Some("0201-02-24"));
        assert_eq!((old.chamber.as_deref(), old.zones.dispositif.len()), (None, 0));
        assert!(old.contested.is_none());
    }

    impl DecisionDto {
        fn moyens_and_motivations(&self) -> (usize, usize) {
            (self.zones.moyens.len(), self.zones.motivations.len())
        }
    }

    #[test]
    fn odd_values_are_tolerated_but_not_odd_shapes() {
        let raw = r#"{"id":123,"jurisdiction":["cc"],"themes":["a",2,null,{"x":1}],
            "zones":{"motivations":[{"start":-1,"end":"12"},{"start":3.5}],"unknown":[1]},
            "highlights":null,"partial":null,"contested":{"jurisdiction":7},"to_be_deleted":true}"#;
        let doc: DecisionDto = decode(raw, DECISION).unwrap();
        assert_eq!((doc.id.as_deref(), doc.jurisdiction.as_deref()), (Some("123"), None));
        assert_eq!(words(&doc.themes), [Some("a"), Some("2"), None, None]);
        let spans: Vec<_> =
            doc.zones.motivations.iter().map(|span| (span.start, span.end)).collect();
        assert_eq!(spans, [(None, Some(12)), (None, None)]);
        assert_eq!(doc.contested.unwrap().jurisdiction.as_deref(), Some("7"));
        assert!(doc.to_be_deleted && !doc.partial && doc.highlights.text.is_empty());
        // L'enveloppe et la forme des listes restent exigées.
        let missing = decode::<SearchDto>(r#"{"results":[]}"#, SEARCH).unwrap_err();
        assert_eq!(
            missing,
            CorpusError::Decode { detail: format!("{SEARCH}: .: missing field `total`") }
        );
        let shape = decode::<DecisionDto>(r#"{"visa":"Article 894"}"#, DECISION).unwrap_err();
        let detail = format!("{DECISION}: visa: invalid type: string, expected a sequence");
        assert_eq!(shape, CorpusError::Decode { detail });
    }

    #[test]
    fn a_page_longer_than_fifty_is_refused() {
        let results = vec!["{}"; HITS_MAX + 1].join(",");
        let raw = format!(r#"{{"total":51,"results":[{results}]}}"#);
        let error = decode::<SearchDto>(&raw, SEARCH).unwrap_err();
        assert_eq!(
            error,
            CorpusError::Decode { detail: format!("{SEARCH}: results[50]: too many elements") }
        );
        let raw = format!(r#"{{"total":50,"results":[{}]}}"#, vec!["{}"; HITS_MAX].join(","));
        assert_eq!(decode::<SearchDto>(&raw, SEARCH).unwrap().results.len(), HITS_MAX);
    }
}
