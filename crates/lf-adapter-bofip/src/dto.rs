//! Formes des réponses du portail, telles qu'il les écrit (API « Explore v2.1 »).
//!
//! Tolérantes : un champ inconnu est ignoré (le portail peut en ajouter), un champ absent ou
//! `null` est `None`. Le contrôle des valeurs est l'affaire de `mapping`, qui ne laisse passer
//! que des types du domaine. Seuls `total_count` et `results` sont exigés.
//!
//! Aucun `Debug` sur ces types : ils portent le texte des publications et, pour un refus, un
//! message qui recopie la requête.
//!
//! `results` est lu élément par élément et s'arrête au premier de trop ([`RESULTS_MAX`]) : un
//! enregistrement vide (`{}`, trois octets) occupe près de deux cents octets une fois décodé, et
//! un corps de quelques mégaoctets fait de `{}` ne doit pas devenir des centaines de mégaoctets
//! avant d'être refusé.

use std::fmt;

use serde::{
    Deserialize, Deserializer,
    de::{Error, SeqAccess, Visitor},
};

/// Enregistrements d'une page, au plus : la plus grande page qu'une recherche peut demander
/// (`PageSize::MAX`) ; une lecture en demande dix. `mapping` garde le contrôle exact contre la
/// limite de chaque appel.
pub(crate) const RESULTS_MAX: usize = 100;

/// Réponse d'une recherche ou d'une lecture : une page d'enregistrements.
#[derive(Deserialize)]
pub(crate) struct Page {
    /// Nombre d'enregistrements qui répondent au filtre, toutes pages confondues.
    pub(crate) total_count: u64,
    /// Enregistrements de la page, [`RESULTS_MAX`] au plus.
    #[serde(deserialize_with = "bounded_results")]
    pub(crate) results: Vec<Record>,
}

/// Lit `results` : un tableau de [`RESULTS_MAX`] enregistrements au plus.
///
/// # Errors
///
/// L'erreur du désérialiseur si la valeur n'est pas un tableau, si un enregistrement est mal
/// formé, ou au premier enregistrement de trop.
fn bounded_results<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Record>, D::Error> {
    deserializer.deserialize_seq(Results)
}

/// Lecteur de `results`, qui s'arrête au premier enregistrement de trop.
struct Results;

impl<'de> Visitor<'de> for Results {
    type Value = Vec<Record>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an array of at most 100 records")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut records = Vec::new();
        while let Some(record) = seq.next_element()? {
            if records.len() >= RESULTS_MAX {
                return Err(A::Error::custom("more records than a page can hold"));
            }
            records.push(record);
        }
        Ok(records)
    }
}

/// Une publication, avec les seuls champs demandés par `select`.
#[derive(Deserialize)]
pub(crate) struct Record {
    /// Identifiant juridique (`BOI-…`, `ACTU-…`).
    pub(crate) identifiant_juridique: Option<String>,
    /// Titre.
    pub(crate) titre: Option<String>,
    /// Nature : `Contenu` ou `Actualité`.
    #[serde(rename = "type")]
    pub(crate) kind: Option<String>,
    /// Code de série ; absent pour quelques publications.
    pub(crate) serie: Option<String>,
    /// Division ; demandée par la lecture seulement.
    pub(crate) division: Option<String>,
    /// Début de validité, `AAAA-MM-JJ`.
    pub(crate) debut_de_validite: Option<String>,
    /// Adresse de la publication sur le site du BOFiP ; elle porte son numéro.
    pub(crate) permalien: Option<String>,
    /// Texte entier, brut ; demandé par la lecture seulement, `null` pour une publication sans
    /// texte.
    pub(crate) contenu: Option<String>,
    /// Texte entier, en HTML ; demandé par la lecture seulement. Le portail y double chaque
    /// guillemet double (ADR-0030).
    pub(crate) contenu_html: Option<String>,
}

/// Corps d'un refus (400). Son `message` recopie la requête : il n'est jamais lu.
#[derive(Deserialize)]
pub(crate) struct Refusal {
    /// Code du refus, ex. `ODSQLSyntaxError`.
    pub(crate) error_code: Option<String>,
}

#[cfg(test)]
mod tests {
    use lf_domain::search::PageSize;

    use super::*;

    /// Corps d'une page de `records` enregistrements vides.
    fn page_of(records: usize) -> String {
        format!(r#"{{"total_count":0,"results":[{}]}}"#, vec!["{}"; records].join(","))
    }

    /// Texte du refus de `serde_json`, vide si le corps est accepté.
    fn refusal(json: &str) -> String {
        serde_json::from_str::<Page>(json).err().map(|error| error.to_string()).unwrap_or_default()
    }

    #[test]
    fn a_page_holds_one_hundred_records_at_most() {
        for records in [0, 1, 100] {
            let page = serde_json::from_str::<Page>(&page_of(records)).ok().unwrap();
            assert_eq!(page.results.len(), records);
        }
        for records in [101, 102, 5_000] {
            let refusal = refusal(&page_of(records));
            assert!(refusal.starts_with("more records than a page can hold"), "{refusal}");
        }
    }

    #[test]
    fn the_cap_is_the_largest_page_a_search_can_ask_for() {
        assert_eq!(u32::try_from(RESULTS_MAX), Ok(PageSize::MAX));
    }

    #[test]
    fn results_must_be_an_array_and_cannot_be_missing() {
        for body in [r#"{"total_count":0,"results":7}"#, r#"{"total_count":0,"results":null}"#] {
            let refusal = refusal(body);
            assert!(refusal.contains("expected an array of at most 100 records"), "{refusal}");
        }
        let missing = refusal(r#"{"total_count":0}"#);
        assert!(missing.starts_with("missing field `results`"), "{missing}");
    }

    #[test]
    fn a_malformed_record_is_refused() {
        let refusal = refusal(r#"{"total_count":1,"results":[{"titre":7}]}"#);
        assert!(refusal.starts_with("invalid type: integer `7`, expected a string"), "{refusal}");
    }
}
