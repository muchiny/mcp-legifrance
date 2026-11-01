//! Vue des lectures longues : `summary`, `text`, `passages`, `next_offset` (05 §5.0.3).
//!
//! [`ReadingView`] est l'objet commun aux sorties de `fr_get_text`, `fr_get_decision` et
//! `fr_get_deliberation` : chaque outil l'aplatit (`#[serde(flatten)]`) dans sa propre vue, après
//! ses métadonnées. Elle est construite depuis la [`Reading`] de l'application ; les champs
//! absents ne sont pas sérialisés.

use lf_application::budget::{PassageOut, Reading};
use lf_domain::text::{SummarySource, window};
use schemars::JsonSchema;
use serde::Serialize;

// Les commentaires des champs de sortie sont des `//` : un commentaire de documentation serait
// repris par schemars comme `description` du schéma de sortie, qui doit rester court et en
// anglais (05 §3.4).

// Origine du résumé (`summary_source`, 00 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum SummarySourceView {
    Official,
    Dispositif,
    Lead,
}

// Un passage du mode `focus` : sa position et son texte.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(inline)]
pub(crate) struct PassageView {
    pub(crate) offset: u32,
    pub(crate) text: String,
}

// Champs de lecture communs en sortie (05 §5.0.3), dans l'ordre du tableau.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub(crate) struct ReadingView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub(crate) summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SummarySourceView")]
    pub(crate) summary_source: Option<SummarySourceView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        with = "bool",
        description = "True when the summary stops before the end of the text."
    )]
    pub(crate) summary_truncated: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub(crate) text: Option<String>,
    pub(crate) text_length: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u32", description = "Pass as offset to read the next window.")]
    pub(crate) next_offset: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<PassageView>")]
    pub(crate) passages: Option<Vec<PassageView>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub(crate) note: Option<String>,
}

impl ReadingView {
    /// Retire un cran de contenu, pour [`fit_under_cap`](super::fit::fit_under_cap) (05 §6.4,
    /// point 4) ; rend `false` quand il n'y a plus rien à retirer.
    ///
    /// - **Passages** : le dernier est retiré (les passages sont classés du meilleur au moins
    ///   bon), puis, quand il n'en reste qu'un, son texte est raccourci de moitié.
    /// - **Fenêtre de texte** : la moitié finale est retirée à une coupure de phrase ou de mot
    ///   (`window`) et `next_offset` reste exact : les pages se partitionnent, donc la fenêtre
    ///   commençait à `next_offset` (ou `text_length` en fin de texte) moins ses caractères, et
    ///   finit à ce début plus les caractères gardés. La fenêtre raccourcie d'une dernière page
    ///   gagne ainsi un `next_offset`.
    /// - **Résumé** (au plus 1 200 caractères) : rien à retirer.
    pub(crate) fn degrade(&mut self) -> bool {
        if let Some(passages) = &mut self.passages {
            return degrade_passages(passages);
        }
        let end = self.next_offset.unwrap_or(self.text_length);
        let Some(text) = &mut self.text else { return false };
        let removed = halve(text);
        if removed == 0 {
            return false;
        }
        self.next_offset = Some(end.saturating_sub(removed));
        true
    }
}

/// Retire le dernier passage ; le seul qui reste est raccourci de moitié.
fn degrade_passages(passages: &mut Vec<PassageView>) -> bool {
    if passages.len() > 1 {
        passages.pop();
        return true;
    }
    passages.first_mut().is_some_and(|only| halve(&mut only.text) > 0)
}

/// Ne garde que le début de `text`, d'au plus la moitié de ses caractères, coupé comme une
/// fenêtre de lecture. Rend le nombre de caractères retirés : 0 d'un texte d'un caractère ou vide.
fn halve(text: &mut String) -> u32 {
    let kept = window(text, 0, text.chars().count() / 2).text.len();
    let removed = text[kept..].chars().count();
    text.truncate(kept);
    u32::try_from(removed).unwrap_or(u32::MAX)
}

impl From<SummarySource> for SummarySourceView {
    fn from(source: SummarySource) -> Self {
        match source {
            SummarySource::Official => Self::Official,
            SummarySource::Dispositif => Self::Dispositif,
            SummarySource::Lead => Self::Lead,
        }
    }
}

impl From<PassageOut> for PassageView {
    fn from(passage: PassageOut) -> Self {
        Self { offset: passage.offset, text: passage.text }
    }
}

impl From<Reading> for ReadingView {
    fn from(reading: Reading) -> Self {
        let (summary, summary_source) = match reading.summary {
            Some(summary) => (Some(summary.text), Some(summary.source.into())),
            None => (None, None),
        };
        Self {
            summary,
            summary_source,
            // Absent sauf si vrai : un résumé complet n'a rien à signaler.
            summary_truncated: reading.summary_truncated.then_some(true),
            text: reading.text,
            text_length: reading.text_length,
            next_offset: reading.next_offset,
            passages: reading
                .passages
                .map(|passages| passages.into_iter().map(PassageView::from).collect()),
            note: reading.note,
        }
    }
}

#[cfg(test)]
mod tests {
    use lf_application::{
        budget::{ReadingRequest, read},
        input::ReadingInput,
    };
    use lf_domain::text::{DispositifRule, LongText, Summary};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::{schema::output_schema, views::fit::fit_under_cap};

    fn view(reading: Reading) -> Value {
        serde_json::to_value(ReadingView::from(reading)).unwrap()
    }

    fn bare(text_length: u32) -> Reading {
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

    #[test]
    fn absent_fields_are_not_serialized() {
        assert_eq!(view(bare(0)), json!({"text_length": 0}));
        assert_eq!(
            view(Reading { text: Some("Chacun a droit au respect.".to_owned()), ..bare(26) }),
            json!({"text": "Chacun a droit au respect.", "text_length": 26})
        );
    }

    #[test]
    fn summary_comes_with_its_source() {
        let reading = Reading {
            summary: Some(Summary {
                source: SummarySource::Dispositif,
                text: "Article 1er.".to_owned(),
                truncated: true,
            }),
            ..bare(14_332)
        };
        assert_eq!(
            view(reading),
            json!({"summary": "Article 1er.", "summary_source": "dispositif", "text_length": 14_332})
        );
    }

    #[test]
    fn summary_truncated_is_written_only_when_true() {
        let lead = |truncated, summary_truncated| Reading {
            summary: Some(Summary {
                source: SummarySource::Lead,
                text: "Début.".to_owned(),
                truncated,
            }),
            summary_truncated,
            ..bare(1_307)
        };
        // Vrai : le drapeau est écrit, après `summary_source`.
        assert_eq!(
            view(lead(false, true)),
            json!({
                "summary": "Début.",
                "summary_source": "lead",
                "summary_truncated": true,
                "text_length": 1_307
            })
        );
        // Faux : absent, jamais `false`. La coupe du domaine seule ne l'écrit pas : c'est
        // `Reading::summary_truncated` (calculé par l'application) qui décide.
        for reading in [lead(false, false), lead(true, false)] {
            assert_eq!(
                view(reading),
                json!({"summary": "Début.", "summary_source": "lead", "text_length": 1_307})
            );
        }
        assert_eq!(view(bare(0)), json!({"text_length": 0}));
    }

    #[test]
    fn the_three_long_readings_validate_summary_truncated_against_their_output_schema() {
        use crate::views::{
            get_decision::DecisionView, get_deliberation::DeliberationView, get_text::ArticleView,
        };
        let base = json!({
            "citation": "Code civil, art. 9",
            "source_url": "https://www.legifrance.gouv.fr/",
            "text_length": 1_307,
            "summary": "Début.",
            "summary_source": "lead",
            "summary_truncated": true,
        });
        let cases = [
            (output_schema::<ArticleView>(), "article_id"),
            (output_schema::<DecisionView>(), "decision_id"),
            (output_schema::<DeliberationView>(), "deliberation_id"),
        ];
        for (schema, id_field) in cases {
            let schema = Value::Object(schema.as_ref().clone());
            let property = &schema["properties"]["summary_truncated"];
            assert_eq!(property["type"], "boolean", "{schema}");
            assert!(property["description"].as_str().unwrap().is_ascii());
            // Optionnel : jamais requis.
            let required = schema["required"].as_array().unwrap();
            assert!(!required.contains(&json!("summary_truncated")));
            let validator = jsonschema::validator_for(&schema).unwrap();
            let mut valid = base.clone();
            valid[id_field] = json!("ID000000000001");
            assert!(validator.is_valid(&valid), "{id_field}");
            valid.as_object_mut().unwrap().remove("summary_truncated");
            assert!(validator.is_valid(&valid), "{id_field} sans le drapeau");
            valid["summary_truncated"] = json!("true");
            assert!(!validator.is_valid(&valid), "{id_field} : un texte n'est pas un booléen");
        }
    }

    #[test]
    fn source_names_are_the_wire_values_of_the_domain() {
        for source in [SummarySource::Official, SummarySource::Dispositif, SummarySource::Lead] {
            let reading = Reading {
                summary: Some(Summary { source, text: "x".to_owned(), truncated: false }),
                ..bare(9_999)
            };
            assert_eq!(view(reading)["summary_source"], json!(source.as_str()));
        }
    }

    #[test]
    fn a_window_carries_next_offset() {
        let reading =
            Reading {
                text: Some("Première page. ".to_owned()), next_offset: Some(15), ..bare(40)
            };
        assert_eq!(
            view(reading),
            json!({"text": "Première page. ", "text_length": 40, "next_offset": 15})
        );
    }

    #[test]
    fn passages_keep_their_offsets_and_an_empty_list_stays() {
        let reading = Reading {
            passages: Some(vec![
                PassageOut { offset: 11_008, text: "14. Dès lors.".to_owned() },
                PassageOut { offset: 8_853, text: "8. M. A.".to_owned() },
            ]),
            ..bare(14_332)
        };
        assert_eq!(
            view(reading),
            json!({
                "passages": [
                    {"offset": 11_008, "text": "14. Dès lors."},
                    {"offset": 8_853, "text": "8. M. A."},
                ],
                "text_length": 14_332
            })
        );
        let none = Reading {
            passages: Some(Vec::new()),
            note: Some("No passage matches focus; read the text with detail=\"full\".".to_owned()),
            ..bare(10)
        };
        assert_eq!(
            view(none),
            json!({
                "text_length": 10,
                "passages": [],
                "note": "No passage matches focus; read the text with detail=\"full\"."
            })
        );
    }

    #[test]
    fn fields_are_written_in_the_order_of_the_reference() {
        let reading = Reading {
            summary: Some(Summary {
                source: SummarySource::Lead,
                text: "début".to_owned(),
                truncated: false,
            }),
            text: Some("t".to_owned()),
            next_offset: Some(1),
            passages: Some(vec![PassageOut { offset: 0, text: "p".to_owned() }]),
            note: Some("n".to_owned()),
            summary_truncated: true,
            ..bare(9)
        };
        assert_eq!(
            serde_json::to_string(&ReadingView::from(reading)).unwrap(),
            concat!(
                r#"{"summary":"début","summary_source":"lead","summary_truncated":true,"#,
                r#""text":"t","text_length":9,"#,
                r#""next_offset":1,"passages":[{"offset":0,"text":"p"}],"note":"n"}"#
            )
        );
    }

    fn chars(text: &str) -> usize {
        text.chars().count()
    }

    /// Lit `text` en texte plein à partir de `offset`.
    fn page(text: &LongText, offset: i64, max_chars: i64) -> Reading {
        let input = ReadingInput {
            detail: Some("full".to_owned()),
            offset: Some(offset),
            max_chars: Some(max_chars),
            ..ReadingInput::default()
        };
        let req = ReadingRequest::parse(&input).unwrap();
        read(text, None, DispositifRule::None, &req).unwrap()
    }

    fn prose(sentences: usize) -> LongText {
        let sentence = "Considérant que l'article L. 1-1 du code civil prévoit une règle précise. ";
        LongText::from_plain(&sentence.repeat(sentences)).unwrap()
    }

    #[test]
    fn degrade_shortens_the_window_and_keeps_next_offset_exact() {
        let text = prose(60);
        let reading = page(&text, 500, 1_000);
        let original = reading.text.clone().unwrap();
        assert_eq!(reading.next_offset, Some(500 + u32::try_from(chars(&original)).unwrap()));
        let mut view = ReadingView::from(reading);
        let mut previous = chars(&original);
        let mut steps = 0;
        while view.degrade() {
            let kept = view.text.as_deref().unwrap();
            assert!(chars(kept) < previous, "chaque cran raccourcit la fenêtre");
            assert!(original.starts_with(kept));
            // La fenêtre restante est toujours exactement [500, next_offset[.
            assert_eq!(view.next_offset, Some(500 + u32::try_from(chars(kept)).unwrap()));
            assert_eq!(view.text_length, text.char_len());
            previous = chars(kept);
            steps += 1;
            assert!(steps < 64);
        }
        assert_eq!(previous, 1, "il ne reste qu'un caractère quand plus rien ne se retire");
        assert!(steps >= 5, "{steps} crans");
        assert!(!view.degrade());
    }

    #[test]
    fn degrading_the_last_window_gives_it_a_next_offset() {
        let text = prose(10);
        let reading = page(&text, 100, 4_000);
        assert_eq!(reading.next_offset, None);
        let whole = reading.text.clone().unwrap();
        assert_eq!(100 + chars(&whole), usize::try_from(text.char_len()).unwrap());
        let mut view = ReadingView::from(reading);
        assert!(view.degrade());
        let kept = view.text.as_deref().unwrap();
        assert!(chars(kept) <= chars(&whole) / 2 && whole.starts_with(kept));
        assert_eq!(view.next_offset, Some(100 + u32::try_from(chars(kept)).unwrap()));
    }

    #[test]
    fn degrade_drops_passages_from_the_end_then_shortens_the_last_one() {
        let passage = |offset: u32, text: &str| PassageOut { offset, text: text.to_owned() };
        let reading = Reading {
            passages: Some(vec![
                passage(40, "Premier passage, le plus pertinent."),
                passage(10, "Deuxième."),
                passage(90, "Troisième."),
            ]),
            ..bare(120)
        };
        let mut view = ReadingView::from(reading);
        assert!(view.degrade());
        assert!(view.degrade());
        let offsets: Vec<u32> = view.passages.as_ref().unwrap().iter().map(|p| p.offset).collect();
        assert_eq!(offsets, [40]);
        let mut previous = chars("Premier passage, le plus pertinent.");
        while view.degrade() {
            let kept = &view.passages.as_ref().unwrap()[0];
            assert_eq!(kept.offset, 40);
            assert!(chars(&kept.text) < previous);
            assert!("Premier passage, le plus pertinent.".starts_with(&kept.text));
            previous = chars(&kept.text);
        }
        assert_eq!(previous, 1);
        assert_eq!(view.next_offset, None, "les passages n'ont pas de fenêtre suivante");
    }

    #[test]
    fn degrade_has_nothing_to_remove_without_text_or_passages() {
        assert!(!ReadingView::from(bare(0)).degrade());
        let summary = Summary {
            source: SummarySource::Lead,
            text: "Début du texte.".to_owned(),
            truncated: true,
        };
        let mut view = ReadingView::from(Reading { summary: Some(summary), ..bare(5_000) });
        assert!(!view.degrade());
        assert_eq!(view.summary.as_deref(), Some("Début du texte."));
        let mut empty = ReadingView::from(Reading { passages: Some(Vec::new()), ..bare(5_000) });
        assert!(!empty.degrade());
        let mut one_char = ReadingView::from(Reading { text: Some("a".to_owned()), ..bare(1) });
        assert!(!one_char.degrade());
        assert_eq!(one_char.next_offset, None);
    }

    #[test]
    fn an_escape_rich_window_is_degraded_under_the_hard_cap() {
        // Guillemets, antislashs et emoji hors BMP : 6 caractères de JSON par groupe de 4.
        let text = LongText::from_plain(&"\"\\😀 ".repeat(10_000)).unwrap();
        let reading = page(&text, 0, 40_000);
        let original = reading.text.clone().unwrap();
        assert_eq!(reading.next_offset, None);
        let mut view = ReadingView::from(reading);
        let cap = lf_application::budget::ResponseBudget::HARD_CAP_CHARS;
        assert!(serde_json::to_string(&view).unwrap().chars().count() > cap);
        let size = fit_under_cap(&mut view, ReadingView::degrade);
        assert!(size <= cap, "{size} caractères");
        assert_eq!(size, serde_json::to_string(&view).unwrap().chars().count());
        let kept = view.text.as_deref().unwrap();
        assert!(original.starts_with(kept));
        assert_eq!(view.next_offset, Some(u32::try_from(chars(kept)).unwrap()));
    }

    #[test]
    fn output_schema_is_an_open_object_with_one_required_field() {
        let schema = output_schema::<ReadingView>();
        // Snapshots de la crate dans `src/snapshots`, comme ceux des modules de premier niveau.
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!(schema);
        });
        // Seul `text_length` est toujours présent ; la sortie reste ouverte (05 §2.5).
        assert_eq!(schema["required"], json!(["text_length"]));
        assert!(!schema.contains_key("additionalProperties"));
        // Descriptions de sortie en anglais, courtes, là où le nom ne suffit pas (05 §3.4).
        assert!(schema.get("description").is_none());
        let written = serde_json::to_string(&*schema).unwrap();
        assert!(written.is_ascii());
        // Les champs absents sont omis, jamais `null` : les types du schéma sont simples.
        assert!(!written.contains("null"));
    }
}
