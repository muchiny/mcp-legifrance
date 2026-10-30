//! Vue de sortie de `fr_get_convention` (05 §5.6) : `ConventionView`.
//!
//! Construite depuis le [`GetConventionOutput`] de l'application, avec les règles de rendu de
//! `fr_get_act` : champs vides omis, `title` omis quand `citation` le contient, table des
//! matières de [`TocView`] aplatie dans la vue. Pas de `date` : la convention n'est pas versionnée
//! par l'API (00 §7). Les commentaires des champs sont des `//` : un doc comment partirait dans
//! l'`outputSchema`, dont la taille compte (05 §3.4).

use lf_application::output::get_convention::GetConventionOutput;
use lf_domain::contract::{Citable, Nor};
use schemars::JsonSchema;
use serde::Serialize;

use super::{
    common::{date_unless_sentinel, etat_code, title_unless_in_citation},
    fit::fit_under_cap,
    toc::TocView,
};

/// Ce que `fr_get_convention` rend : les métadonnées, la citation et une page de table des matières.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct ConventionView {
    convention_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Omitted when citation already contains it.")]
    title: Option<String>,
    // Numéro IDCC de la convention de rattachement.
    #[serde(skip_serializing_if = "Option::is_none")]
    idcc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nor: Option<String>,
    // Titre de la convention de rattachement.
    #[serde(skip_serializing_if = "Option::is_none")]
    container_title: Option<String>,
    // Code d'API de l'état (VIGUEUR, VIGUEUR_ETEN, ABROGE...).
    #[serde(skip_serializing_if = "Option::is_none")]
    etat: Option<String>,
    // Bulletin officiel de publication.
    #[serde(skip_serializing_if = "Option::is_none")]
    bulletin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    date_publication: Option<String>,
    citation: String,
    source_url: String,
    #[serde(flatten)]
    toc: TocView,
}

impl From<GetConventionOutput> for ConventionView {
    fn from(output: GetConventionOutput) -> Self {
        let GetConventionOutput { convention, toc, toc_total, next_toc_offset } = output;
        let contract = convention.contract();
        let citation = contract.citation.as_str();
        let body = &convention.body;
        let mut view = Self {
            convention_id: convention.text_id.as_str().to_owned(),
            title: title_unless_in_citation(body.title.as_deref(), citation),
            idcc: convention.idcc.as_ref().map(ToString::to_string),
            nor: body.nor.as_ref().map(Nor::as_str).map(str::to_owned),
            container_title: convention.container_title.clone(),
            etat: etat_code(body.etat.as_ref()),
            bulletin: convention.bulletin.clone(),
            date_publication: date_unless_sentinel(body.date_parution),
            citation: citation.to_owned(),
            source_url: contract.source_url.to_string(),
            toc: TocView::new(&toc, toc_total, next_toc_offset, body.etat.as_ref()),
        };
        // L'estimation du cas d'usage ignore les échappements JSON : la mesure exacte tranche, et
        // la page raccourcie repousse `next_toc_offset` d'autant.
        fit_under_cap(&mut view, |view| view.toc.drop_last());
        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::output_schema;
    use lf_application::{budget::ResponseBudget, output::get_act::TocLine};
    use lf_domain::{
        contract::Idcc,
        date::CivilDate,
        entity::{Convention, TextBody},
        id::{ArticleId, KaliTextId},
        status::LegalStatus,
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    const ID: &str = "KALITEXT000049878950";
    const TITLE: &str = "Annexe IX - Stipulations spécifiques applicables aux techniciens du spectacle et aux artistes interprètes, engagés pour la production de phonogrammes et vidéogrammes musicaux";

    /// La convention de l'exemple de 05 §5.6, sans sa table.
    fn convention() -> Convention {
        Convention {
            text_id: KaliTextId::parse(ID).unwrap(),
            body: TextBody {
                nor: Nor::parse("ASET2450455M"),
                title: Some(TITLE.to_owned()),
                etat: LegalStatus::parse("VIGUEUR_ETEN"),
                ..TextBody::default()
            },
            idcc: Idcc::parse("2121"),
            container_title: Some(
                "Convention collective nationale de l'édition du 14 janvier 2000".to_owned(),
            ),
            container_id: Some("KALICONT000005635096".to_owned()),
            bulletin: Some("BO n°2024-20".to_owned()),
        }
    }

    fn example_output() -> GetConventionOutput {
        GetConventionOutput {
            convention: convention(),
            toc: vec![
                TocLine::Section {
                    depth: 1,
                    title: "Chapitre 1er : Recours au CDD d'usage".to_owned(),
                },
                TocLine::Article {
                    depth: 2,
                    article_id: ArticleId::parse("KALIARTI000049886059").unwrap(),
                    num: Some("19 (des dispositions communes)".to_owned()),
                    // Égal à l'état du texte : non rendu.
                    etat: LegalStatus::parse("VIGUEUR_ETEN"),
                },
            ],
            toc_total: 2,
            next_toc_offset: None,
        }
    }

    fn view_json(output: GetConventionOutput) -> Value {
        serde_json::to_value(ConventionView::from(output)).unwrap()
    }

    #[test]
    fn convention_view_matches_the_example_of_the_spec() {
        assert_eq!(
            view_json(example_output()),
            json!({
                "convention_id": ID,
                "idcc": "2121",
                "nor": "ASET2450455M",
                "container_title": "Convention collective nationale de l'édition du 14 janvier 2000",
                "etat": "VIGUEUR_ETEN",
                "bulletin": "BO n°2024-20",
                "citation": TITLE,
                "source_url": format!("https://www.legifrance.gouv.fr/conv_coll/id/{ID}"),
                "toc": [
                    {"depth": 1, "title": "Chapitre 1er : Recours au CDD d'usage"},
                    {"depth": 2, "article_id": "KALIARTI000049886059",
                     "num": "19 (des dispositions communes)"},
                ],
                "toc_total": 2,
            })
        );
    }

    #[test]
    fn a_bare_convention_keeps_only_the_contract_and_the_toc() {
        let bare = GetConventionOutput {
            convention: Convention {
                text_id: KaliTextId::parse(ID).unwrap(),
                body: TextBody::default(),
                idcc: None,
                container_title: None,
                container_id: None,
                bulletin: None,
            },
            toc: Vec::new(),
            toc_total: 0,
            next_toc_offset: None,
        };

        assert_eq!(
            view_json(bare),
            json!({
                "convention_id": ID,
                "citation": format!("Légifrance {ID}"),
                "source_url": format!("https://www.legifrance.gouv.fr/conv_coll/id/{ID}"),
                "toc": [],
                "toc_total": 0,
            })
        );
    }

    #[test]
    fn the_publication_date_is_rendered_unless_it_is_a_sentinel() {
        let mut output = example_output();
        output.convention.body.date_parution = CivilDate::new(2024, 5, 14);
        assert_eq!(view_json(output.clone())["date_publication"], "2024-05-14");

        output.convention.body.date_parution = CivilDate::new(2222, 1, 1);
        assert!(view_json(output).get("date_publication").is_none());
    }

    #[test]
    fn the_title_is_rendered_when_the_citation_is_cut() {
        let long = "mot ".repeat(300);
        let mut output = example_output();
        output.convention.body.title = Some(long.clone());

        let view = view_json(output);

        assert_eq!(view["title"], long.trim());
        assert!(view["citation"].as_str().unwrap().ends_with('…'));
    }

    #[test]
    fn a_next_offset_is_rendered_when_the_page_is_not_the_last() {
        let mut output = example_output();
        output.toc_total = 40;
        output.next_toc_offset = Some(2);

        assert_eq!(view_json(output)["next_toc_offset"], 2);
    }

    #[test]
    fn fit_degrades_the_toc_and_moves_next_toc_offset_when_escapes_blow_the_estimate() {
        // Même cas que `fr_get_act` : 150 guillemets estimés 150, mesurés 300 en JSON.
        let title = "\"".repeat(150);
        let mut output = example_output();
        output.toc =
            (0..200).map(|_| TocLine::Section { depth: 1, title: title.clone() }).collect();
        output.toc_total = 200;
        output.next_toc_offset = None;

        let view = view_json(output);

        let kept = view["toc"].as_array().unwrap().len();
        assert!(0 < kept && kept < 200, "{kept}");
        assert!(view.to_string().chars().count() <= ResponseBudget::HARD_CAP_CHARS);
        assert_eq!(view["toc_total"], 200);
        assert_eq!(view["next_toc_offset"], kept);
    }

    #[test]
    fn the_view_validates_against_its_output_schema() {
        let schema = Value::Object(output_schema::<ConventionView>().as_ref().clone());
        let validator = jsonschema::validator_for(&schema).unwrap();

        assert!(validator.is_valid(&view_json(example_output())));
        let mut bare = example_output();
        bare.convention.body = TextBody::default();
        bare.convention.idcc = None;
        bare.convention.container_title = None;
        bare.convention.bulletin = None;
        bare.toc = Vec::new();
        bare.toc_total = 0;
        let bare = view_json(bare);
        assert!(validator.is_valid(&bare), "{bare}");
        let mut broken = view_json(example_output());
        broken.as_object_mut().unwrap().remove("convention_id");
        assert!(!validator.is_valid(&broken));
    }

    #[test]
    fn the_output_schema_requires_the_contract_and_the_toc_and_has_no_date() {
        let schema = Value::Object(output_schema::<ConventionView>().as_ref().clone());

        assert!(schema.get("$schema").is_none());
        assert!(schema.get("$defs").is_none(), "{schema}");
        let required = schema["required"].as_array().unwrap();
        for name in ["convention_id", "citation", "source_url", "toc", "toc_total"] {
            assert!(required.contains(&json!(name)), "{name}");
        }
        for name in ["title", "idcc", "nor", "etat", "bulletin", "next_toc_offset"] {
            assert!(!required.contains(&json!(name)), "{name}");
        }
        assert!(schema["properties"].get("date").is_none());
    }

    #[test]
    fn output_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        let schema = Value::Object(output_schema::<ConventionView>().as_ref().clone());
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("output_schema", schema);
        });
    }
}
