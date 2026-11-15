//! Formes des réponses du portail, telles qu'il les écrit (API « Explore v2.1 »).
//!
//! Tolérantes : un champ inconnu est ignoré (le portail peut en ajouter), un champ absent ou
//! `null` est `None`. Le contrôle des valeurs est l'affaire de `mapping`, qui ne laisse passer
//! que des types du domaine. Seuls `total_count` et `results` sont exigés.
//!
//! Aucun `Debug` sur ces types : ils portent le texte des publications et, pour un refus, un
//! message qui recopie la requête.

use serde::Deserialize;

/// Réponse d'une recherche ou d'une lecture : une page d'enregistrements.
#[derive(Deserialize)]
pub(crate) struct Page {
    /// Nombre d'enregistrements qui répondent au filtre, toutes pages confondues.
    pub(crate) total_count: u64,
    /// Enregistrements de la page.
    pub(crate) results: Vec<Record>,
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
    /// Texte entier ; demandé par la lecture seulement, `null` pour une publication sans texte.
    pub(crate) contenu: Option<String>,
}

/// Corps d'un refus (400). Son `message` recopie la requête : il n'est jamais lu.
#[derive(Deserialize)]
pub(crate) struct Refusal {
    /// Code du refus, ex. `ODSQLSyntaxError`.
    pub(crate) error_code: Option<String>,
}
