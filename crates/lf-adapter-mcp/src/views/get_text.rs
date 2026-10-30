//! Vue de sortie de `fr_get_text` (05 §5.3) : `ArticleView`.
//!
//! Construite depuis le [`GetTextOutput`] de l'application : les métadonnées de l'article, sa
//! citation et son URL, puis les champs de lecture communs ([`ReadingView`], aplatis). Les champs
//! facultatifs sont omis quand ils sont vides (05 §1.4 règle 1), les dates au-delà du seuil des
//! sentinelles aussi (règle 9). Les commentaires des champs sont des `//` : un doc comment
//! partirait dans l'`outputSchema`, dont la taille compte (05 §3.4).
//!
//! L'estimation du cas d'usage ignore les échappements JSON : la mesure exacte tranche. Si la vue
//! dépasse le plafond dur, la fenêtre (ou les passages) se raccourcit ([`ReadingView::degrade`]) et
//! `next_offset` reste exact (05 §6.4, étape 4).

use lf_application::output::get_text::GetTextOutput;
use lf_domain::contract::{Citable, Eli};
use schemars::JsonSchema;
use serde::Serialize;

use super::{
    common::{date_unless_sentinel, etat_code},
    fit::fit_under_cap,
    reading::ReadingView,
};

/// Ce que `fr_get_text` rend : les métadonnées de l'article, sa citation et la lecture.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct ArticleView {
    article_id: String,
    // Numéro affiché.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    num: Option<String>,
    // Code d'API de l'état (VIGUEUR, ABROGE, VIGUEUR_DIFF...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    etat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    code_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String", description = "LEGITEXT or KALITEXT of the parent text.")]
    parent_id: Option<String>,
    // Titres hiérarchiques, du plus haut au plus bas.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    path: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        with = "String",
        description = "Omitted when the entry into force is not set yet (see etat)."
    )]
    date_debut: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        with = "String",
        description = "Omitted when the article is in force without an end date."
    )]
    date_fin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    nota: Option<String>,
    // ELI natif, jamais fabriqué.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    eli: Option<String>,
    citation: String,
    source_url: String,
    #[serde(flatten)]
    reading: ReadingView,
}

impl From<GetTextOutput> for ArticleView {
    fn from(output: GetTextOutput) -> Self {
        let GetTextOutput { article, nota, reading } = output;
        let contract = article.contract();
        let mut view = Self {
            article_id: article.article_id.as_str().to_owned(),
            num: article.num,
            etat: etat_code(article.etat.as_ref()),
            code_title: article.code_title,
            parent_id: article.parent.map(|parent| parent.id.as_str().to_owned()),
            path: article.path,
            date_debut: date_unless_sentinel(article.date_debut),
            date_fin: date_unless_sentinel(article.date_fin),
            nota,
            eli: contract.eli.as_ref().map(Eli::as_str).map(str::to_owned),
            citation: contract.citation.as_str().to_owned(),
            source_url: contract.source_url.to_string(),
            reading: ReadingView::from(reading),
        };
        fit_under_cap(&mut view, |view| view.reading.degrade());
        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::output_schema;
    use lf_application::{
        budget::{PassageOut, Reading, ResponseBudget},
        output::get_text::GetTextOutput,
    };
    use lf_domain::{
        date::CivilDate,
        entity::{ArticleText, ParentText},
        id::{ArticleId, DocumentId},
        status::LegalStatus,
        text::{LongText, Summary, SummarySource},
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    const ARTICLE: &str = "LEGIARTI000006419288";
    const TEXT: &str = "Chacun a droit au respect de sa vie privée. Les juges peuvent, sans préjudice de la réparation du dommage subi, prescrire toutes mesures, telles que séquestre, saisie et autres, propres à empêcher ou faire cesser une atteinte à l'intimité de la vie privée : ces mesures peuvent, s'il y a urgence, être ordonnées en référé.";

    fn day(year: u16, month: u8, day: u8) -> CivilDate {
        CivilDate::new(year, month, day).unwrap()
    }

    /// L'article 9 du Code civil de l'exemple de 05 §5.3, sans son texte (dans la lecture).
    fn article() -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(ARTICLE).unwrap(),
            num: Some("9".to_owned()),
            etat: LegalStatus::parse("VIGUEUR"),
            code_title: Some("Code civil".to_owned()),
            parent: Some(ParentText::new(
                DocumentId::parse("LEGITEXT000006070721").unwrap(),
                Some("CODE"),
            )),
            path: vec![
                "Livre Ier : Des personnes".to_owned(),
                "Titre Ier : Des droits civils".to_owned(),
            ],
            full_sections_titre: None,
            date_debut: Some(day(1970, 7, 19)),
            // Sentinelle de l'API : « en vigueur sans date de fin », donc absente de la vue.
            date_fin: Some(day(2999, 1, 1)),
            text: LongText::default(),
            nota: None,
        }
    }

    fn whole(text: &str) -> Reading {
        Reading {
            summary: None,
            text: Some(text.to_owned()),
            text_length: u32::try_from(text.chars().count()).unwrap(),
            next_offset: None,
            passages: None,
            note: None,
        }
    }

    fn output(article: ArticleText, reading: Reading) -> GetTextOutput {
        GetTextOutput { article, nota: None, reading }
    }

    fn view_json(output: GetTextOutput) -> Value {
        serde_json::to_value(ArticleView::from(output)).unwrap()
    }

    #[test]
    fn article_view_matches_the_example_of_the_spec() {
        assert_eq!(
            view_json(output(article(), whole(TEXT))),
            json!({
                "article_id": ARTICLE,
                "num": "9",
                "etat": "VIGUEUR",
                "code_title": "Code civil",
                "parent_id": "LEGITEXT000006070721",
                "path": ["Livre Ier : Des personnes", "Titre Ier : Des droits civils"],
                "date_debut": "1970-07-19",
                "citation": "Code civil, art. 9",
                "source_url": "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288",
                "text": TEXT,
                "text_length": 322,
            })
        );
    }

    #[test]
    fn article_view_without_optional_data_keeps_only_the_contract_and_the_length() {
        let bare = ArticleText {
            num: None,
            etat: None,
            code_title: None,
            parent: None,
            path: Vec::new(),
            date_debut: None,
            date_fin: None,
            ..article()
        };

        assert_eq!(
            view_json(output(bare, whole(""))),
            json!({
                "article_id": ARTICLE,
                "citation": "Légifrance LEGIARTI000006419288",
                "source_url": "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288",
                "text": "",
                "text_length": 0,
            })
        );
    }

    #[test]
    fn a_kali_article_points_to_its_convention_and_the_conv_coll_page() {
        let kali = ArticleText {
            article_id: ArticleId::parse("KALIARTI000005805295").unwrap(),
            parent: Some(ParentText::new(DocumentId::parse("KALITEXT000005635534").unwrap(), None)),
            code_title: Some("Convention collective nationale de la métallurgie".to_owned()),
            ..article()
        };

        let view = view_json(output(kali, whole("Texte.")));

        assert_eq!(view["article_id"], "KALIARTI000005805295");
        assert_eq!(view["parent_id"], "KALITEXT000005635534");
        assert_eq!(
            view["source_url"],
            "https://www.legifrance.gouv.fr/conv_coll/id/KALIARTI000005805295"
        );
    }

    #[test]
    fn the_nota_is_rendered_after_the_dates_and_before_the_citation() {
        let mut with_nota = output(article(), whole("Texte."));
        with_nota.nota = Some("Version issue de la loi n° 2016-1321.".to_owned());

        let view = serde_json::to_string(&ArticleView::from(with_nota)).unwrap();

        let position = |key: &str| view.find(&format!("\"{key}\":")).expect(key);
        assert!(position("date_debut") < position("nota"));
        assert!(position("nota") < position("citation"));
        assert!(view.contains("\"nota\":\"Version issue de la loi n° 2016-1321.\""));
    }

    #[test]
    fn dates_in_the_sentinel_zone_are_omitted() {
        let mut dated = article();
        dated.date_debut = Some(day(2222, 2, 22));
        dated.date_fin = Some(day(2999, 1, 1));

        let view = view_json(output(dated, whole("Texte.")));
        assert!(view.get("date_debut").is_none() && view.get("date_fin").is_none());

        // Une vraie date de fin reste rendue : l'article est abrogé à cette date.
        let mut ended = article();
        ended.date_fin = Some(day(2023, 9, 7));
        assert_eq!(view_json(output(ended, whole("Texte.")))["date_fin"], "2023-09-07");
    }

    #[test]
    fn a_summary_comes_with_its_source_and_the_length_of_the_whole_text() {
        let reading = Reading {
            summary: Some(Summary {
                source: SummarySource::Lead,
                text: "Début.".to_owned(),
                truncated: true,
            }),
            text: None,
            text_length: 5_000,
            ..whole("")
        };

        let view = view_json(output(article(), reading));

        assert_eq!(view["summary"], "Début.");
        assert_eq!(view["summary_source"], "lead");
        assert_eq!(view["text_length"], 5_000);
        assert!(view.get("text").is_none());
    }

    #[test]
    fn serialization_writes_the_metadata_then_the_reading() {
        let reading = Reading {
            passages: Some(vec![PassageOut { offset: 0, text: "p".to_owned() }]),
            text: None,
            ..whole("")
        };
        let written =
            serde_json::to_string(&ArticleView::from(output(article(), reading))).unwrap();

        assert_eq!(
            written,
            concat!(
                r#"{"article_id":"LEGIARTI000006419288","num":"9","etat":"VIGUEUR","#,
                r#""code_title":"Code civil","parent_id":"LEGITEXT000006070721","#,
                r#""path":["Livre Ier : Des personnes","Titre Ier : Des droits civils"],"#,
                r#""date_debut":"1970-07-19","citation":"Code civil, art. 9","#,
                r#""source_url":"https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288","#,
                r#""text_length":0,"passages":[{"offset":0,"text":"p"}]}"#
            )
        );
    }

    #[test]
    fn an_escape_rich_window_is_shortened_under_the_hard_cap_with_an_exact_next_offset() {
        // Guillemets, antislashs et emoji hors BMP : 6 caractères de JSON par groupe de 4.
        let heavy = "\"\\😀 ".repeat(10_000);
        let reading = whole(&heavy);

        let view = view_json(output(article(), reading));

        let size = view.to_string().chars().count();
        assert!(size <= ResponseBudget::HARD_CAP_CHARS, "{size}");
        let kept = view["text"].as_str().unwrap();
        assert!(heavy.starts_with(kept) && kept.chars().count() < heavy.chars().count());
        // La fenêtre raccourcie finit où la page suivante commence.
        assert_eq!(view["next_offset"], kept.chars().count());
        assert_eq!(view["text_length"], 40_000);
    }

    #[test]
    fn the_view_validates_against_its_output_schema() {
        let schema = Value::Object(output_schema::<ArticleView>().as_ref().clone());
        let validator = jsonschema::validator_for(&schema).unwrap();

        assert!(validator.is_valid(&view_json(output(article(), whole(TEXT)))));
        let summary = Reading {
            summary: Some(Summary {
                source: SummarySource::Lead,
                text: "Début.".to_owned(),
                truncated: true,
            }),
            text: None,
            ..whole("")
        };
        assert!(validator.is_valid(&view_json(output(article(), summary))));
        let mut broken = view_json(output(article(), whole(TEXT)));
        broken.as_object_mut().unwrap().remove("citation");
        assert!(!validator.is_valid(&broken));
    }

    #[test]
    fn the_output_schema_merges_the_reading_fields_and_has_no_defs_or_dollar_schema() {
        let schema = Value::Object(output_schema::<ArticleView>().as_ref().clone());

        assert!(schema.get("$schema").is_none());
        assert!(schema.get("$defs").is_none(), "{schema}");
        let properties = schema["properties"].as_object().unwrap();
        let mut names: Vec<&str> = properties.keys().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "article_id",
                "citation",
                "code_title",
                "date_debut",
                "date_fin",
                "eli",
                "etat",
                "next_offset",
                "nota",
                "note",
                "num",
                "parent_id",
                "passages",
                "path",
                "source_url",
                "summary",
                "summary_source",
                "text",
                "text_length",
            ]
        );
        let mut required: Vec<&str> =
            schema["required"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        required.sort_unstable();
        assert_eq!(required, ["article_id", "citation", "source_url", "text_length"]);
        // Les champs absents sont omis, jamais `null` : les types du schéma sont simples.
        assert!(!schema.to_string().contains("null"));
    }

    #[test]
    fn output_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        let schema = Value::Object(output_schema::<ArticleView>().as_ref().clone());
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("output_schema", schema);
        });
    }
}
