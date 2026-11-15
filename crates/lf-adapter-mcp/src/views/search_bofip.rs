//! Vue de sortie de `fr_search_bofip` (05 §5.14) : `BofipSearchView`.
//!
//! Construite depuis le [`SearchBofipOutput`] de l'application : une page de publications du
//! BOFiP, jamais leur texte. La vue y ajoute deux constantes, présentes dans chaque réponse :
//! [`BOFIP_ATTRIBUTION`] (mention de la Licence Ouverte) et [`DOCTRINE_NOTE`] (ce qu'est la
//! doctrine, et sa limite). L'application leur réserve
//! [`BOFIP_CONSTANTS_RESERVE_CHARS`](lf_application::output::search_bofip::BOFIP_CONSTANTS_RESERVE_CHARS)
//! caractères dans son calcul de taille : un test vérifie qu'elles y tiennent.
//!
//! L'application compte exactement le JSON de chaque résultat (échappements compris) et raccourcit
//! les titres avant de construire la vue : la mesure de `views::fit` n'est ici qu'un filet, qui ne
//! retire rien (jamais un résultat, 05 §6.4). Les commentaires des champs sont des `//` : un doc
//! comment partirait dans l'`outputSchema`, dont la taille compte (05 §3.4).

use lf_application::output::search_bofip::SearchBofipOutput;
use lf_domain::doctrine::{DoctrineHit, DoctrineKind};
use schemars::JsonSchema;
use serde::Serialize;

use super::fit::fit_under_cap;

/// `attribution` des réponses du BOFiP : mention exacte de la Licence Ouverte (ADR-0028,
/// décision 9).
pub(crate) const BOFIP_ATTRIBUTION: &str = "Source : BOFiP-Impôts / DGFiP — Licence Ouverte 2.0";

/// `doctrine_note` des réponses du BOFiP : nature du texte, garantie de l'article L. 80 A du LPF
/// et limite de la source (ADR-0029, V-225).
///
/// Phrase confrontée le 2026-11-15 à l'article lu dans Légifrance (`LEGIARTI000037312533`, version
/// en vigueur depuis le 2018-08-12). Son troisième alinéa protège le redevable qui a appliqué un
/// texte fiscal selon l'interprétation publiée par l'administration et non rapportée « à la date
/// des opérations en cause » : d'où « under the conditions of », et le rappel de la date de
/// validité, puisque seule la doctrine en vigueur aujourd'hui est servie.
pub(crate) const DOCTRINE_NOTE: &str = "BOFiP is the tax administration's published doctrine: its reading of tax law, not the law itself. A taxpayer may rely on it against the administration under the conditions of article L. 80 A of the Livre des procédures fiscales. Only the publications in force today are available here: check the validity date before relying on a passage for an earlier period.";

// Nature d'une publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum BofipKindView {
    Doctrine,
    News,
}

impl From<DoctrineKind> for BofipKindView {
    fn from(kind: DoctrineKind) -> Self {
        match kind {
            DoctrineKind::Doctrine => Self::Doctrine,
            DoctrineKind::News => Self::News,
        }
    }
}

// Un résultat : de quoi choisir la publication et la lire avec `fr_get_bofip`.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(inline)]
struct BofipHitView {
    // `BOI-…` ou `ACTU-…`.
    id: String,
    #[schemars(description = "Number of the publication on the BOFiP site.")]
    publication_id: u32,
    title: String,
    kind: BofipKindView,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    serie: Option<String>,
    // Début de validité, `AAAA-MM-JJ`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    valid_from: Option<String>,
    // Omis quand le permalien du jeu ne suit pas la grammaire stricte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    source_url: Option<String>,
}

impl From<DoctrineHit> for BofipHitView {
    fn from(hit: DoctrineHit) -> Self {
        Self {
            id: hit.id.as_str().to_owned(),
            publication_id: hit.publication_id.get(),
            title: hit.title,
            kind: hit.kind.into(),
            serie: hit.serie.map(|serie| serie.as_str().to_owned()),
            valid_from: hit.valid_from.map(|date| date.to_string()),
            source_url: hit.source_url.map(|url| url.as_str().to_owned()),
        }
    }
}

/// Ce que `fr_search_bofip` rend : une page de résultats et les deux constantes du BOFiP.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct BofipSearchView {
    total: u64,
    page: u32,
    #[schemars(description = "True if the next page exists and can be reached.")]
    has_more: bool,
    hits: Vec<BofipHitView>,
    attribution: String,
    #[schemars(description = "What BOFiP is, and the limit of this source.")]
    doctrine_note: String,
    // Indice facultatif : aucun résultat, fin de la fenêtre atteignable, titres raccourcis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    note: Option<String>,
}

impl From<SearchBofipOutput> for BofipSearchView {
    fn from(output: SearchBofipOutput) -> Self {
        let mut view = Self {
            total: output.total,
            page: output.page,
            has_more: output.has_more,
            hits: output.hits.into_iter().map(BofipHitView::from).collect(),
            attribution: BOFIP_ATTRIBUTION.to_owned(),
            doctrine_note: DOCTRINE_NOTE.to_owned(),
            note: output.note,
        };
        // Filet : l'application a déjà ramené la page sous le plafond. Rien n'est retiré ici, un
        // dépassement serait seulement signalé au journal.
        fit_under_cap(&mut view, |_| false);
        view
    }
}

#[cfg(test)]
mod tests {
    use lf_application::{
        budget::ResponseBudget, output::search_bofip::BOFIP_CONSTANTS_RESERVE_CHARS,
    };
    use lf_domain::{
        date::CivilDate,
        doctrine::{BoiId, Permalink, Serie},
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::schema::output_schema;

    const ID: &str = "BOI-ENR-DMTG-10-50-20";

    /// Résultat complet : la publication `number` de [`ID`], de titre `title`.
    fn hit(number: u32, title: &str) -> DoctrineHit {
        let raw = format!(
            "https://bofip.impots.gouv.fr/bofip/{number}-PGP.html/identifiant={ID}-20230524"
        );
        let link = Permalink::parse(&raw).unwrap();
        DoctrineHit {
            id: BoiId::parse(ID).unwrap(),
            publication_id: link.publication_id,
            title: title.to_owned(),
            kind: DoctrineKind::Doctrine,
            serie: Some(Serie::parse("ENR").unwrap()),
            valid_from: CivilDate::new(2023, 5, 24),
            source_url: link.url,
        }
    }

    /// Résultat réduit à ses champs obligatoires : une actualité sans série, date ni adresse.
    fn bare(number: u32, title: &str) -> DoctrineHit {
        DoctrineHit {
            kind: DoctrineKind::News,
            serie: None,
            valid_from: None,
            source_url: None,
            ..hit(number, title)
        }
    }

    fn output(hits: Vec<DoctrineHit>) -> SearchBofipOutput {
        SearchBofipOutput { total: 357, page: 12, has_more: true, hits, note: None }
    }

    fn json_of(output: SearchBofipOutput) -> Value {
        serde_json::to_value(BofipSearchView::from(output)).unwrap()
    }

    #[test]
    fn the_view_carries_the_page_its_hits_and_the_two_constants() {
        let value = json_of(output(vec![hit(3350, "Donations entre époux")]));
        assert_eq!(
            value,
            json!({
                "total": 357,
                "page": 12,
                "has_more": true,
                "hits": [{
                    "id": ID,
                    "publication_id": 3350,
                    "title": "Donations entre époux",
                    "kind": "doctrine",
                    "serie": "ENR",
                    "valid_from": "2023-05-24",
                    "source_url": "https://bofip.impots.gouv.fr/bofip/3350-PGP.html/identifiant=BOI-ENR-DMTG-10-50-20-20230524"
                }],
                "attribution": "Source : BOFiP-Impôts / DGFiP — Licence Ouverte 2.0",
                "doctrine_note": DOCTRINE_NOTE,
            })
        );
    }

    #[test]
    fn absent_fields_are_omitted_and_the_note_is_passed_on() {
        let mut empty = output(vec![bare(12, "a")]);
        empty.has_more = false;
        empty.note = Some("No BOFiP publication matches.".to_owned());
        let value = json_of(empty);
        assert_eq!(
            value["hits"],
            json!([{"id": ID, "publication_id": 12, "title": "a", "kind": "news"}])
        );
        assert_eq!(value["has_more"], false);
        assert_eq!(value["note"], "No BOFiP publication matches.");
        // Sans remarque, la clé `note` n'existe pas.
        assert!(json_of(output(Vec::new())).get("note").is_none());
        assert_eq!(json_of(output(Vec::new()))["hits"], json!([]));
    }

    /// Parité avec le calcul de taille de l'application (`hit_cost`, `envelope_cost` de
    /// `usecase/search_bofip.rs`) : les mêmes littéraux JSON que ses tests, à l'octet. Un champ
    /// renommé, déplacé ou ajouté dans la vue casse ce test, et dit de reprendre le calcul.
    #[test]
    fn the_compact_json_is_the_one_the_application_counts() {
        let full = BofipHitView::from(hit(3350, "Donations \"entre\" époux\\"));
        assert_eq!(
            serde_json::to_string(&full).unwrap(),
            r#"{"id":"BOI-ENR-DMTG-10-50-20","publication_id":3350,"title":"Donations \"entre\" époux\\","kind":"doctrine","serie":"ENR","valid_from":"2023-05-24","source_url":"https://bofip.impots.gouv.fr/bofip/3350-PGP.html/identifiant=BOI-ENR-DMTG-10-50-20-20230524"}"#
        );
        let reduced = BofipHitView::from(bare(12, "a"));
        assert_eq!(
            serde_json::to_string(&reduced).unwrap(),
            r#"{"id":"BOI-ENR-DMTG-10-50-20","publication_id":12,"title":"a","kind":"news"}"#
        );

        // L'enveloppe : celle que l'application compte, plus les deux constantes de la vue.
        let mut noted = output(Vec::new());
        noted.has_more = false;
        noted.note = Some("fin \"de\" fenêtre".to_owned());
        let constants =
            format!(r#","attribution":"{BOFIP_ATTRIBUTION}","doctrine_note":"{DOCTRINE_NOTE}""#);
        assert_eq!(
            serde_json::to_string(&BofipSearchView::from(noted)).unwrap(),
            format!(
                r#"{{"total":357,"page":12,"has_more":false,"hits":[]{constants},"note":"fin \"de\" fenêtre"}}"#
            )
        );
        assert_eq!(
            serde_json::to_string(&BofipSearchView::from(output(Vec::new()))).unwrap(),
            format!(r#"{{"total":357,"page":12,"has_more":true,"hits":[]{constants}}}"#)
        );
    }

    /// Les deux constantes, clés, guillemets et virgules compris, tiennent dans la réserve que
    /// l'application leur garde ; aucune n'a besoin d'échappement JSON.
    #[test]
    fn the_two_constants_fit_in_the_reserve_of_the_application() {
        let constants =
            format!(r#","attribution":"{BOFIP_ATTRIBUTION}","doctrine_note":"{DOCTRINE_NOTE}""#);
        assert_eq!(constants.chars().count(), 447);
        assert!(constants.chars().count() <= BOFIP_CONSTANTS_RESERVE_CHARS);
        for constant in [BOFIP_ATTRIBUTION, DOCTRINE_NOTE] {
            assert_eq!(serde_json::to_string(constant).unwrap(), format!("\"{constant}\""));
        }
        assert_eq!(BOFIP_CONSTANTS_RESERVE_CHARS, 1_000);
    }

    #[test]
    fn the_constants_say_the_licence_and_what_doctrine_is() {
        assert_eq!(BOFIP_ATTRIBUTION, "Source : BOFiP-Impôts / DGFiP — Licence Ouverte 2.0");
        assert!(DOCTRINE_NOTE.starts_with("BOFiP is the tax administration's published doctrine"));
        assert!(DOCTRINE_NOTE.contains("not the law itself"));
        assert!(DOCTRINE_NOTE.contains("article L. 80 A of the Livre des procédures fiscales"));
        assert!(DOCTRINE_NOTE.contains("Only the publications in force today"));
        // Aucune adresse : seuls les liens vers Légifrance sont admis dans les textes fixes.
        assert!(!DOCTRINE_NOTE.contains("http"));
    }

    /// Une page pleine de titres déjà raccourcis par l'application passe sous le plafond dur, et
    /// la vue ne retire jamais un résultat.
    #[test]
    fn a_full_page_stays_whole_under_the_hard_cap() {
        let title = "é".repeat(120);
        let hits: Vec<DoctrineHit> = (1..=100).map(|number| hit(number, &title)).collect();
        let view = BofipSearchView::from(output(hits));
        assert_eq!(view.hits.len(), 100);
        let size = serde_json::to_string(&view).unwrap().chars().count();
        assert!(size <= ResponseBudget::HARD_CAP_CHARS);
    }

    #[test]
    fn the_output_schema_names_the_fields_in_order() {
        let schema = Value::Object(output_schema::<BofipSearchView>().as_ref().clone());
        assert_eq!(schema["type"], "object");
        assert_eq!(
            schema["required"],
            json!(["total", "page", "has_more", "hits", "attribution", "doctrine_note"])
        );
        let hit = &schema["properties"]["hits"]["items"];
        assert_eq!(hit["required"], json!(["id", "publication_id", "title", "kind"]));
        assert_eq!(hit["properties"]["kind"]["enum"], json!(["doctrine", "news"]));
        assert!(schema.get("$schema").is_none());
    }

    #[test]
    fn output_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        let schema = Value::Object(output_schema::<BofipSearchView>().as_ref().clone());
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("output_schema", schema);
        });
    }
}
