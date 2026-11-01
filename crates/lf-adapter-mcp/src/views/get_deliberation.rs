//! Vue de sortie de `fr_get_deliberation` (05 §5.5) : `DeliberationView`.
//!
//! Construite depuis le [`GetDeliberationOutput`] de l'application : les métadonnées de la
//! délibération, sa citation et son URL source, puis les champs de lecture communs
//! ([`ReadingView`], aplatis, 05 §5.0.3). Les champs facultatifs sont omis quand ils sont absents
//! (05 §1.4 règle 1), les dates au-delà du seuil des sentinelles aussi (règle 9). `title` suit la
//! règle de la CNIL (05 §5.5) : `titreLong` s'il n'est pas déjà dans la citation, sinon `titre`
//! selon la même règle (05 §5.0.4). Les commentaires des champs sont des `//` : un doc comment
//! partirait dans l'`outputSchema`, dont la taille compte (05 §3.4).

use lf_application::output::get_deliberation::GetDeliberationOutput;
use lf_domain::contract::Citable;
use schemars::JsonSchema;
use serde::Serialize;

use super::{
    common::{date_unless_sentinel, etat_code, title_unless_in_citation},
    fit::fit_under_cap,
    reading::ReadingView,
};

// Ce que `fr_get_deliberation` rend : les métadonnées, la citation et la lecture du texte.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct DeliberationView {
    pub(crate) deliberation_id: String,
    // Numéro de la délibération (SAN-2026-002).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub(crate) num: Option<String>,
    // Nature (Délibération ou Décision).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub(crate) nature: Option<String>,
    // Nature de la délibération (Sanction...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub(crate) nature_delib: Option<String>,
    // Code d'API de l'état (VIGUEUR, ABROGE...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub(crate) etat: Option<String>,
    // Date de la délibération, AAAA-MM-JJ.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub(crate) date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub(crate) date_publication: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String", description = "Omitted when citation already contains it.")]
    pub(crate) title: Option<String>,
    pub(crate) citation: String,
    pub(crate) source_url: String,
    #[serde(flatten)]
    pub(crate) reading: ReadingView,
}

/// `title` d'une délibération (05 §5.5) : `titreLong` tant que la citation ne le contient pas,
/// sinon `titre` à la même condition. Rend `None` quand les deux sont absents, vides ou déjà dans
/// la citation.
fn rendered_title(title_long: Option<&str>, title: Option<&str>, citation: &str) -> Option<String> {
    title_unless_in_citation(title_long, citation)
        .or_else(|| title_unless_in_citation(title, citation))
}

impl From<GetDeliberationOutput> for DeliberationView {
    fn from(output: GetDeliberationOutput) -> Self {
        let GetDeliberationOutput { deliberation, reading } = output;
        let contract = deliberation.contract();
        let citation = contract.citation.as_str();
        let mut view = Self {
            deliberation_id: deliberation.deliberation_id.to_string(),
            title: rendered_title(
                deliberation.title_long.as_deref(),
                deliberation.title.as_deref(),
                citation,
            ),
            num: deliberation.num.clone(),
            nature: deliberation.nature.clone(),
            nature_delib: deliberation.nature_delib.clone(),
            etat: etat_code(deliberation.etat.as_ref()),
            date: date_unless_sentinel(deliberation.date_texte),
            date_publication: date_unless_sentinel(deliberation.date_publication),
            citation: citation.to_owned(),
            source_url: contract.source_url.to_string(),
            reading: ReadingView::from(reading),
        };
        // `max_chars` (40 000), `title` et `citation` (1 000 chacun) et les échappements JSON
        // peuvent ensemble dépasser le plafond dur : la mesure exacte raccourcit la fenêtre ou les
        // passages, `next_offset` restant exact (05 §6.4, point 4).
        fit_under_cap(&mut view, |view| view.reading.degrade());
        view
    }
}

#[cfg(test)]
mod tests {
    use lf_application::{
        budget::{PassageOut, Reading, ReadingRequest, ResponseBudget, read as read_text},
        input::ReadingInput,
    };
    use lf_domain::{
        date::CivilDate,
        entity::Deliberation,
        id::CnilTextId,
        status::LegalStatus,
        text::{DispositifRule, LongText, Summary, SummarySource},
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::schema::output_schema;

    const ID: &str = "CNILTEXT000053352643";

    /// Les trois paragraphes (593 caractères) de la fixture `consult_cnil.json`, texte tronqué.
    const PARAGRAPHS: [&str; 3] = [
        "- Les développements de la délibération comportant des données à caractère personnel ou des secrets protégés par la loi sont remplacés par le signe […] -",
        "La Commission nationale de l’informatique et des libertés, réunie en sa formation restreinte composée de M. Philippe-Pierre CABOURDIN, président, M. Vincent LESCLOUS, vice-président, Mmes Laurence FRANCESCHINI et Isabelle LATOURNARIE-WILLEMS, MM. KLING, membres,",
        "Vu le règlement (UE) 2016/679 du Parlement européen et du Conseil du 27 avril 2016 relatif à la protection des personnes physiques à l’égard du traitement des données à [...]",
    ];

    const TITLE: &str = "Délibération SAN-2026-002 du 8 janvier 2026";
    const TITLE_LONG: &str = "Délibération de la formation restreinte n° SAN-2026-002 du 8 janvier 2026 prononçant une sanction pécuniaire à l'encontre de la société FREE";
    const CITATION: &str = "CNIL, délibération n° SAN-2026-002 du 8 janvier 2026";

    /// La délibération de la fixture `consult_cnil.json` (05 §5.5), sans son texte.
    fn free() -> Deliberation {
        Deliberation {
            deliberation_id: CnilTextId::parse(ID).unwrap(),
            num: Some("SAN-2026-002".to_owned()),
            nature: Some("Délibération".to_owned()),
            nature_delib: Some("Sanction".to_owned()),
            etat: LegalStatus::parse("VIGUEUR"),
            date_texte: CivilDate::new(2026, 1, 8),
            date_publication: CivilDate::new(2026, 1, 14),
            title: Some(TITLE.to_owned()),
            title_long: Some(TITLE_LONG.to_owned()),
            text: LongText::default(),
        }
    }

    /// Une délibération qui ne porte que son identifiant.
    fn bare() -> Deliberation {
        Deliberation {
            num: None,
            nature: None,
            nature_delib: None,
            etat: None,
            date_texte: None,
            date_publication: None,
            title: None,
            title_long: None,
            ..free()
        }
    }

    fn whole(text: &LongText) -> Reading {
        let request = ReadingRequest::parse(&ReadingInput::default());
        read_text(text, None, DispositifRule::None, &request.unwrap()).unwrap()
    }

    fn output(deliberation: Deliberation, reading: Reading) -> GetDeliberationOutput {
        GetDeliberationOutput { deliberation, reading }
    }

    fn bare_reading(text_length: u32) -> Reading {
        Reading {
            summary: None,
            text: None,
            text_length,
            next_offset: None,
            passages: None,
            note: None,
            summary_truncated: false,
        }
    }

    fn view(deliberation: Deliberation) -> Value {
        serde_json::to_value(DeliberationView::from(output(deliberation, bare_reading(0)))).unwrap()
    }

    #[test]
    fn the_view_is_the_documented_example() {
        let text = LongText::from_plain(&PARAGRAPHS.join("\n")).unwrap();
        let actual =
            serde_json::to_value(DeliberationView::from(output(free(), whole(&text)))).unwrap();
        let expected = json!({
            "deliberation_id": "CNILTEXT000053352643",
            "num": "SAN-2026-002",
            "nature": "Délibération",
            "nature_delib": "Sanction",
            "etat": "VIGUEUR",
            "date": "2026-01-08",
            "date_publication": "2026-01-14",
            "title": TITLE_LONG,
            "citation": CITATION,
            "source_url": "https://www.legifrance.gouv.fr/cnil/id/CNILTEXT000053352643",
            "text": PARAGRAPHS.join("\n\n"),
            "text_length": 593
        });
        assert_eq!(actual, expected);
        // 1 099 caractères de JSON compact (05 §5.5, §6.5) ; l'ordre des clés est celui du tableau.
        let written = serde_json::to_string(&DeliberationView::from(output(free(), whole(&text))));
        assert_eq!(written.unwrap().chars().count(), 1_099);
    }

    #[test]
    fn keys_are_written_in_the_order_of_the_reference() {
        let reading =
            Reading { text: Some("t".to_owned()), next_offset: Some(1), ..bare_reading(9) };
        let written =
            serde_json::to_string(&DeliberationView::from(output(free(), reading))).unwrap();
        let keys = [
            "deliberation_id",
            "num",
            "nature",
            "nature_delib",
            "etat",
            "date",
            "date_publication",
            "title",
            "citation",
            "source_url",
            "text",
            "text_length",
            "next_offset",
        ];
        let positions: Vec<usize> =
            keys.iter().map(|key| written.find(&format!("\"{key}\":")).unwrap()).collect();
        assert!(positions.is_sorted(), "{written}");
    }

    #[test]
    fn title_long_rule() {
        let rendered = |title_long: Option<&str>, title: Option<&str>, citation: &str| {
            rendered_title(title_long, title, citation)
        };
        // Le titre long qui n'est pas dans la citation l'emporte sur le titre court.
        assert_eq!(rendered(Some(TITLE_LONG), Some(TITLE), CITATION), Some(TITLE_LONG.to_owned()));
        assert_eq!(view(free())["title"], TITLE_LONG);
        // Titre long contenu dans la citation : le titre court est essayé, à la même règle.
        assert_eq!(
            rendered(Some("n° SAN-2026-002"), Some(TITLE), CITATION),
            Some(TITLE.to_owned())
        );
        assert_eq!(rendered(Some("n° SAN-2026-002"), Some("du 8 janvier 2026"), CITATION), None);
        // Titre long absent, vide ou blanc : le titre court, sauf s'il est dans la citation.
        for absent in [None, Some(""), Some("  \n ")] {
            assert_eq!(
                rendered(absent, Some(TITLE), CITATION),
                Some(TITLE.to_owned()),
                "{absent:?}"
            );
            assert_eq!(rendered(absent, Some("SAN-2026-002"), CITATION), None, "{absent:?}");
        }
        // Rien à rendre.
        assert_eq!(rendered(None, None, CITATION), None);
        assert_eq!(rendered(Some(" "), Some(""), CITATION), None);
        // Les blancs de bord du titre rendu sont retirés.
        assert_eq!(rendered(Some("  Titre long  "), None, CITATION), Some("Titre long".to_owned()));
    }

    #[test]
    fn title_long_rule_through_the_view() {
        // Sans numéro, la citation est le titre court : celui-ci est omis, le titre long non.
        let no_num = Deliberation { num: None, ..free() };
        let actual = view(no_num);
        assert_eq!(actual["citation"], TITLE);
        assert_eq!(actual["title"], TITLE_LONG);
        // Sans numéro ni titre long, le titre court égale la citation : rien à rendre.
        let only_title = Deliberation { num: None, title_long: None, ..free() };
        assert!(view(only_title).get("title").is_none());
        // Avec numéro mais sans titre long, le titre court n'est pas dans la citation : rendu.
        let short = Deliberation { title_long: None, ..free() };
        assert_eq!(view(short)["title"], TITLE);
        // Un titre long qui est la citation elle-même est omis, comme le titre court contenu.
        let echo = Deliberation { title_long: Some(CITATION.to_owned()), title: None, ..free() };
        assert!(view(echo).get("title").is_none());
    }

    #[test]
    fn absent_fields_are_omitted_not_null() {
        let actual = view(bare());
        let keys: std::collections::BTreeSet<&str> =
            actual.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            ["citation", "deliberation_id", "source_url", "text_length"].into(),
            "{actual}"
        );
        // Sans numéro, titre ni date, la citation est le repli sur l'identifiant.
        assert_eq!(actual["citation"], "Légifrance CNILTEXT000053352643");
        assert_eq!(actual["text_length"], 0);
        assert!(!actual.to_string().contains("null"));
    }

    #[test]
    fn sentinel_dates_are_omitted() {
        // 2222-02-22 et 2999-01-01 valent « pas de date » (03 §6.2).
        let deliberation = Deliberation {
            date_texte: CivilDate::new(2222, 2, 22),
            date_publication: CivilDate::new(2999, 1, 1),
            ..free()
        };
        let actual = view(deliberation);
        assert!(actual.get("date").is_none());
        assert!(actual.get("date_publication").is_none());
        assert_eq!(view(free())["date"], "2026-01-08");
    }

    #[test]
    fn the_status_is_the_api_code_even_when_unknown() {
        assert_eq!(view(free())["etat"], "VIGUEUR");
        let abrogated = Deliberation { etat: LegalStatus::parse("ABROGE"), ..free() };
        assert_eq!(view(abrogated)["etat"], "ABROGE");
        let unknown = Deliberation { etat: LegalStatus::parse(" NOUVEAU_CODE "), ..free() };
        assert_eq!(view(unknown)["etat"], "NOUVEAU_CODE");
    }

    #[test]
    fn a_summary_comes_with_its_source_and_the_text_length() {
        let reading = Reading {
            summary: Some(Summary {
                source: SummarySource::Lead,
                text: "La formation restreinte...".to_owned(),
                truncated: true,
            }),
            ..bare_reading(15_000)
        };
        let actual = serde_json::to_value(DeliberationView::from(output(free(), reading))).unwrap();
        assert_eq!(actual["summary"], "La formation restreinte...");
        assert_eq!(actual["summary_source"], "lead");
        assert_eq!(actual["text_length"], 15_000);
        assert!(actual.get("text").is_none());
    }

    #[test]
    fn passages_and_windows_pass_through() {
        let passages = Reading {
            passages: Some(vec![PassageOut { offset: 120, text: "Le grief.".to_owned() }]),
            ..bare_reading(900)
        };
        let actual =
            serde_json::to_value(DeliberationView::from(output(free(), passages))).unwrap();
        assert_eq!(actual["passages"], json!([{"offset": 120, "text": "Le grief."}]));
        let window =
            Reading { text: Some("Page.".to_owned()), next_offset: Some(5), ..bare_reading(900) };
        let actual = serde_json::to_value(DeliberationView::from(output(free(), window))).unwrap();
        assert_eq!(
            (actual["text"].as_str(), actual["next_offset"].as_u64()),
            (Some("Page."), Some(5))
        );
    }

    #[test]
    fn an_escape_rich_window_is_shortened_under_the_hard_cap() {
        // Guillemets, antislashs et emoji hors BMP : 6 caractères de JSON pour 4 de texte, donc une
        // fenêtre de 40 000 caractères en pèse 60 000, et le titre long s'y ajoute.
        let text = LongText::from_plain(&"\"\\😀 ".repeat(10_000)).unwrap();
        let request = ReadingRequest::parse(&ReadingInput {
            detail: Some("full".to_owned()),
            max_chars: Some(40_000),
            ..ReadingInput::default()
        })
        .unwrap();
        let reading = read_text(&text, None, DispositifRule::None, &request).unwrap();
        let window = reading.text.clone().unwrap();
        // La fenêtre vaut entre la moitié et la totalité de `max_chars` (05 §6.1).
        assert!((20_000..=40_000).contains(&window.chars().count()));
        assert!(window.chars().count() > 39_000);
        let long = Deliberation { title_long: Some("é".repeat(1_000)), ..free() };
        let view = DeliberationView::from(output(long, reading));
        let written = serde_json::to_string(&view).unwrap();
        let size = written.chars().count();
        assert!(size <= ResponseBudget::HARD_CAP_CHARS, "{size} caractères");
        // La fenêtre est un début de l'original, et `next_offset` est exact.
        let kept = view.reading.text.as_deref().unwrap();
        assert!(kept.chars().count() < window.chars().count() && window.starts_with(kept));
        assert_eq!(view.reading.next_offset, Some(u32::try_from(kept.chars().count()).unwrap()));
        assert_eq!(view.reading.text_length, text.char_len());
    }

    #[test]
    fn a_response_already_under_the_cap_is_left_alone() {
        let text = LongText::from_plain(&PARAGRAPHS.join("\n")).unwrap();
        let reading = whole(&text);
        let view = DeliberationView::from(output(free(), reading.clone()));
        assert_eq!(view.reading.text, reading.text);
        assert_eq!(view.reading.next_offset, None);
    }

    #[test]
    fn the_output_schema_lists_the_always_present_fields_as_required() {
        let schema = Value::Object(output_schema::<DeliberationView>().as_ref().clone());
        assert_eq!(schema["type"], "object");
        let mut required: Vec<_> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap().to_owned())
            .collect();
        required.sort();
        assert_eq!(required, ["citation", "deliberation_id", "source_url", "text_length"]);
        assert!(schema.get("$schema").is_none());
        let properties = schema["properties"].as_object().unwrap();
        for name in [
            "num",
            "nature",
            "nature_delib",
            "etat",
            "date",
            "date_publication",
            "title",
            "summary",
            "summary_source",
            "text",
            "next_offset",
            "passages",
            "note",
        ] {
            assert!(properties.contains_key(name), "{name}");
        }
        // Les champs de sortie sont décrits par leur nom : pas de `null` dans le schéma.
        let written = serde_json::to_string(&schema).unwrap();
        assert!(written.is_ascii());
        assert!(!written.contains("null"));
    }

    #[test]
    fn output_schema_snapshot() {
        let schema = Value::Object(output_schema::<DeliberationView>().as_ref().clone());
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("output_schema", schema);
        });
    }

    #[test]
    fn the_view_matches_its_snapshot() {
        let text = LongText::from_plain(&PARAGRAPHS.join("\n")).unwrap();
        let actual =
            serde_json::to_value(DeliberationView::from(output(free(), whole(&text)))).unwrap();
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("the_view_matches_its_snapshot", actual);
        });
    }

    /// La vue de `free()` lue en `mode` sur la fixture répétée quatre fois (2 400 caractères).
    fn read_view(mode: &ReadingInput) -> Value {
        let text = LongText::from_plain(&PARAGRAPHS.repeat(4).join("\n")).unwrap();
        let request = ReadingRequest::parse(mode).unwrap();
        let reading = read_text(&text, None, DispositifRule::None, &request).unwrap();
        serde_json::to_value(DeliberationView::from(output(free(), reading))).unwrap()
    }

    #[test]
    fn the_summary_mode_matches_its_snapshot() {
        let actual = read_view(&ReadingInput::default());
        assert_eq!(actual["summary_source"], "lead");
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("the_summary_mode_matches_its_snapshot", actual);
        });
    }

    #[test]
    fn the_full_mode_matches_its_snapshot() {
        let actual = read_view(&ReadingInput {
            detail: Some("full".to_owned()),
            max_chars: Some(500),
            ..ReadingInput::default()
        });
        assert!(actual["next_offset"].as_u64().unwrap() > 0);
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("the_full_mode_matches_its_snapshot", actual);
        });
    }

    #[test]
    fn the_focus_mode_matches_its_snapshot() {
        let actual = read_view(&ReadingInput {
            focus: Some("Commission nationale informatique libertés".to_owned()),
            passages: Some(2),
            ..ReadingInput::default()
        });
        assert_eq!(actual["passages"].as_array().unwrap().len(), 2);
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("the_focus_mode_matches_its_snapshot", actual);
        });
    }
}
