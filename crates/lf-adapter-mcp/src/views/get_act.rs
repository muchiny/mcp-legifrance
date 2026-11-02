//! Vue de sortie de `fr_get_act` (05 §5.2) : `ActView`.
//!
//! Construite depuis le [`GetActOutput`] de l'application. Les champs facultatifs sont omis quand
//! ils sont vides (05 §1.4 règle 1), les dates au-delà du seuil des sentinelles aussi (règle 9),
//! `title` quand `citation` le contient déjà (05 §5.0.4). La table des matières est celle de
//! `fr_get_convention` ([`TocView`]), aplatie dans la vue. Les commentaires des champs sont des
//! `//` : un doc comment partirait dans l'`outputSchema`, dont la taille compte (05 §3.4).

use lf_application::output::get_act::GetActOutput;
use lf_domain::contract::{Citable, Eli, Nor};
use schemars::JsonSchema;
use serde::Serialize;

use super::{
    common::{date_unless_sentinel, etat_code, title_unless_in_citation},
    fit::fit_under_cap,
    toc::TocView,
};

/// Ce que `fr_get_act` rend : les métadonnées, la citation et une page de table des matières.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct ActView {
    // Identifiant du texte, sans suffixe de version.
    text_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Omitted when citation already contains it.")]
    title: Option<String>,
    // Nature (LOI, DECRET, CODE...).
    #[serde(skip_serializing_if = "Option::is_none")]
    nature: Option<String>,
    // Code d'API de l'état (VIGUEUR, ABROGE, VIGUEUR_ETEN...).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "In force: VIGUEUR*, ABROGE_DIFF; other codes not.")]
    etat: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    date_parution: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_parution: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    jo_reference: Option<String>,
    // Appellations usuelles (« loi Lemaire »).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    aliases: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    date_debut_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    date_fin_version: Option<String>,
    // Version servie, AAAA-MM-JJ.
    date: String,
    // ELI natif, jamais fabriqué.
    #[serde(skip_serializing_if = "Option::is_none")]
    eli: Option<String>,
    citation: String,
    source_url: String,
    #[serde(flatten)]
    toc: TocView,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<&'static str>,
}

impl From<GetActOutput> for ActView {
    fn from(output: GetActOutput) -> Self {
        let GetActOutput { act, date, toc, toc_total, next_toc_offset, note } = output;
        let contract = act.contract();
        let citation = contract.citation.as_str();
        let body = &act.body;
        let mut view = Self {
            text_id: act.text_id.as_str().to_owned(),
            title: title_unless_in_citation(body.title.as_deref(), citation),
            nature: body.nature.clone(),
            etat: etat_code(body.etat.as_ref()),
            nor: body.nor.as_ref().map(Nor::as_str).map(str::to_owned),
            date_parution: date_unless_sentinel(body.date_parution),
            num_parution: body.num_parution.clone(),
            jo_reference: body.jo_reference.clone(),
            aliases: body.aliases.clone(),
            date_debut_version: date_unless_sentinel(body.date_debut_version),
            date_fin_version: date_unless_sentinel(body.date_fin_version),
            date: date.to_string(),
            eli: contract.eli.as_ref().map(Eli::as_str).map(str::to_owned),
            citation: citation.to_owned(),
            source_url: contract.source_url.to_string(),
            toc: TocView::new(&toc, toc_total, next_toc_offset, body.etat.as_ref()),
            note,
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

    /// D-12 : la règle générale de `etat` est dans la description du champ (`VIGUEUR_ETEN`, `PERIME`,
    /// ANNULE... ne sont pas dans le glossaire des instructions).
    #[test]
    fn the_etat_field_carries_the_general_rule() {
        let schema = serde_json::Value::Object(output_schema::<ActView>().as_ref().clone());
        let text = schema.to_string();
        assert!(text.contains("In force: VIGUEUR*, ABROGE_DIFF; other codes not."), "{text}");
    }
    use lf_application::{budget::ResponseBudget, output::get_act::TocLine};
    use lf_domain::{
        date::{CivilDate, VersionDate},
        entity::{Act, TextBody, Toc},
        id::{ArticleId, LegiTextId},
        status::LegalStatus,
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    const CITATION: &str = "LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique (1)";

    fn day(year: u16, month: u8, day: u8) -> CivilDate {
        CivilDate::new(year, month, day).unwrap()
    }

    /// La loi du 7 octobre 2016 de l'exemple de 05 §5.2, sans sa table.
    fn act() -> Act {
        Act {
            text_id: LegiTextId::parse("LEGITEXT000033205014").unwrap(),
            body: TextBody {
                nor: Nor::parse("ECFI1524250L"),
                title: Some(CITATION.to_owned()),
                nature: Some("LOI".to_owned()),
                etat: LegalStatus::parse("VIGUEUR"),
                date_parution: Some(day(2016, 10, 8)),
                num_parution: Some("0235".to_owned()),
                date_debut_version: Some(day(2023, 9, 8)),
                // Sentinelle de l'API : « en vigueur sans date de fin », donc absente de la vue.
                date_fin_version: Some(day(2999, 1, 1)),
                jo_reference: Some("JORF n°0235 du 8 octobre 2016".to_owned()),
                aliases: vec![
                    "loi Lemaire".to_owned(),
                    "loi numérique".to_owned(),
                    "loi Lemaire [2016]".to_owned(),
                ],
                toc: Toc::default(),
                ..TextBody::default()
            },
        }
    }

    fn section(depth: u16, title: &str) -> TocLine {
        TocLine::Section { depth, title: title.to_owned() }
    }

    fn article(depth: u16, id: &str, num: &str) -> TocLine {
        TocLine::Article {
            depth,
            article_id: ArticleId::parse(id).unwrap(),
            num: Some(num.to_owned()),
            etat: LegalStatus::parse("VIGUEUR"),
        }
    }

    /// Les huit premières entrées de l'exemple de 05 §5.2 (la table en compte 88).
    fn example_output() -> GetActOutput {
        GetActOutput {
            act: act(),
            date: VersionDate::parse("2026-10-15").unwrap(),
            toc: vec![
                section(1, "Titre Ier : LA CIRCULATION DES DONNÉES ET DU SAVOIR"),
                section(2, "Chapitre Ier : Economie de la donnée"),
                section(3, "Section 1 : Ouverture de l'accès aux données publiques"),
                article(4, "LEGIARTI000033205126", "1"),
                article(4, "LEGIARTI000033205128", "2"),
                article(4, "LEGIARTI000033205130", "3"),
                article(4, "LEGIARTI000033205132", "4"),
                article(4, "LEGIARTI000033205062", "5"),
            ],
            toc_total: 88,
            next_toc_offset: Some(8),
            note: None,
        }
    }

    fn view_json(output: GetActOutput) -> Value {
        serde_json::to_value(ActView::from(output)).unwrap()
    }

    #[test]
    fn act_view_matches_the_example_of_the_spec() {
        // L'`etat` des articles (en vigueur) égale celui du texte : il n'est pas rendu.
        assert_eq!(
            view_json(example_output()),
            json!({
                "text_id": "LEGITEXT000033205014",
                "nature": "LOI",
                "etat": "VIGUEUR",
                "nor": "ECFI1524250L",
                "date_parution": "2016-10-08",
                "num_parution": "0235",
                "jo_reference": "JORF n°0235 du 8 octobre 2016",
                "aliases": ["loi Lemaire", "loi numérique", "loi Lemaire [2016]"],
                "date_debut_version": "2023-09-08",
                "date": "2026-10-15",
                "citation": CITATION,
                "source_url": "https://www.legifrance.gouv.fr/loda/id/LEGITEXT000033205014",
                "toc": [
                    {"depth": 1, "title": "Titre Ier : LA CIRCULATION DES DONNÉES ET DU SAVOIR"},
                    {"depth": 2, "title": "Chapitre Ier : Economie de la donnée"},
                    {"depth": 3, "title": "Section 1 : Ouverture de l'accès aux données publiques"},
                    {"depth": 4, "article_id": "LEGIARTI000033205126", "num": "1"},
                    {"depth": 4, "article_id": "LEGIARTI000033205128", "num": "2"},
                    {"depth": 4, "article_id": "LEGIARTI000033205130", "num": "3"},
                    {"depth": 4, "article_id": "LEGIARTI000033205132", "num": "4"},
                    {"depth": 4, "article_id": "LEGIARTI000033205062", "num": "5"},
                ],
                "toc_total": 88,
                "next_toc_offset": 8,
            })
        );
    }

    #[test]
    fn act_view_without_optional_data_keeps_only_the_contract_and_the_toc() {
        let bare = GetActOutput {
            act: Act {
                text_id: LegiTextId::parse("LEGITEXT000006070721").unwrap(),
                body: TextBody::default(),
            },
            date: VersionDate::parse("2026-10-29").unwrap(),
            toc: Vec::new(),
            toc_total: 0,
            next_toc_offset: None,
            note: None,
        };

        assert_eq!(
            view_json(bare),
            json!({
                "text_id": "LEGITEXT000006070721",
                "date": "2026-10-29",
                "citation": "Légifrance LEGITEXT000006070721",
                "source_url": "https://www.legifrance.gouv.fr/loda/id/LEGITEXT000006070721",
                "toc": [],
                "toc_total": 0,
            })
        );
    }

    #[test]
    fn dates_in_the_sentinel_zone_are_omitted() {
        let mut output = example_output();
        output.act.body.date_debut_version = Some(day(2222, 2, 22));
        output.act.body.date_parution = Some(day(2222, 1, 1));

        let view = view_json(output);

        for field in ["date_debut_version", "date_fin_version", "date_parution"] {
            assert!(view.get(field).is_none(), "{field}");
        }
    }

    #[test]
    fn the_title_is_rendered_only_when_the_citation_does_not_contain_it() {
        // Cas courant : la citation est le titre, qui n'est pas répété.
        assert!(view_json(example_output()).get("title").is_none());

        // Un titre de 1 199 caractères : la citation est coupée (`…`), le titre complet est rendu.
        let long = "mot ".repeat(300);
        let mut output = example_output();
        output.act.body.title = Some(long.clone());
        output.toc = Vec::new();
        output.toc_total = 0;
        output.next_toc_offset = None;

        let view = view_json(output);

        assert_eq!(view["title"], long.trim());
        assert!(view["citation"].as_str().unwrap().ends_with('…'));
    }

    #[test]
    fn the_note_of_a_code_is_rendered() {
        let mut output = example_output();
        output.note = Some("note de test");

        assert_eq!(view_json(output)["note"], "note de test");
    }

    #[test]
    fn a_code_is_cited_with_its_code_url() {
        let mut output = example_output();
        output.act.body.nature = Some("CODE".to_owned());

        assert_eq!(
            view_json(output)["source_url"],
            "https://www.legifrance.gouv.fr/codes/texte_lc/LEGITEXT000033205014"
        );
    }

    #[test]
    fn fit_degrades_the_toc_and_moves_next_toc_offset_when_escapes_blow_the_estimate() {
        // 200 titres de 150 guillemets : 150 caractères estimés chacun, mais 300 en JSON (`\"`).
        // La page passe l'estimation du cas d'usage et dépasse le plafond dur à la mesure exacte.
        let title = "\"".repeat(150);
        let mut output = example_output();
        output.toc = (0..200).map(|_| section(1, &title)).collect();
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
        let schema = Value::Object(output_schema::<ActView>().as_ref().clone());
        let validator = jsonschema::validator_for(&schema).unwrap();

        // Exemple complet, puis vue réduite au contrat (toutes les listes omissibles vides).
        assert!(validator.is_valid(&view_json(example_output())));
        let mut bare = example_output();
        bare.act.body = TextBody::default();
        bare.toc = Vec::new();
        bare.toc_total = 0;
        bare.next_toc_offset = None;
        let bare = view_json(bare);
        assert!(validator.is_valid(&bare), "{bare}");
        // Un champ requis absent n'est pas conforme.
        let mut broken = view_json(example_output());
        broken.as_object_mut().unwrap().remove("toc");
        assert!(!validator.is_valid(&broken));
    }

    #[test]
    fn the_output_schema_inlines_toc_entries_and_has_no_defs_or_dollar_schema() {
        let schema = Value::Object(output_schema::<ActView>().as_ref().clone());

        assert!(schema.get("$schema").is_none());
        assert!(schema.get("$defs").is_none(), "{schema}");
        let entry = &schema["properties"]["toc"]["items"]["properties"];
        let mut names: Vec<&str> = entry.as_object().unwrap().keys().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(names, ["article_id", "depth", "etat", "num", "title"]);
        for required in ["text_id", "date", "citation", "source_url", "toc", "toc_total"] {
            assert!(
                schema["required"].as_array().unwrap().contains(&json!(required)),
                "{required}"
            );
        }
        for optional in ["title", "etat", "next_toc_offset", "note", "aliases", "eli"] {
            assert!(
                !schema["required"].as_array().unwrap().contains(&json!(optional)),
                "{optional}"
            );
        }
    }

    #[test]
    fn output_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        let schema = Value::Object(output_schema::<ActView>().as_ref().clone());
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("output_schema", schema);
        });
    }
}
