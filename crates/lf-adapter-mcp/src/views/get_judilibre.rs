//! Vue de sortie de `fr_get_judilibre` (05 §5.19) : `CaseLawView`.
//!
//! Construite depuis le [`GetJudilibreOutput`] de l'application : les mentions de la décision
//! (juridiction et siège, chambre ou formation, date), ses métadonnées et sa citation, ses liens
//! (textes appliqués, décisions rapprochées, décision attaquée), les mentions des CGU de chaque
//! réponse ([`ATTRIBUTION`], [`CASE_LAW_NOTE`]), puis les champs de lecture communs
//! ([`ReadingView`], aplatis). Les champs absents sont omis (jamais `null`). Aucun nom de personne
//! n'est rendu dans un champ à part : ceux des magistrats et des greffiers restent dans le texte,
//! tel que publié. `source_url` attend V-239 (ADR-0035, décision 11).
//!
//! Si le JSON exact dépasse le plafond dur, la vue retire d'abord ce qui pèse le plus : la lecture
//! (fenêtre ou passages) est raccourcie tant qu'elle pèse plus que les liens ; sinon les derniers
//! rapprochements, puis les derniers visas, sont retirés, ce que dit `note` (05 §6.4). Les
//! commentaires des champs sont des `//` : un commentaire de documentation partirait dans
//! l'`outputSchema`, dont la taille compte (05 §3.4).

use lf_application::output::get_judilibre::GetJudilibreOutput;
use lf_domain::judilibre::{ContestedDecision, RelatedDecision};
use schemars::JsonSchema;
use serde::Serialize;

use super::{
    fit::fit_under_cap,
    reading::ReadingView,
    search_judilibre::{ATTRIBUTION, CASE_LAW_NOTE},
};

/// Remarque ajoutée quand des liens ont dû être retirés pour tenir sous le plafond dur.
pub(crate) const LINKS_CUT_NOTE: &str =
    "Some related decisions or applied texts were left out to fit the response size limit.";

// Les noms de champs et leur ordre sont ceux de 05 §5.19.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct CaseLawView {
    // 24 caractères hexadécimaux, en minuscules.
    id: String,
    // Juridiction et siège, en clair.
    court: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    chamber: Option<String>,
    // Cour de cassation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    formation: Option<String>,
    // Telle que publiée.
    date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    ecli: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    solution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    publication: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    themes: Vec<String>,
    citation: String,
    // Dernière mise à jour dans Judilibre (`update_date`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    updated: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(description = "Texts applied by the decision; check them with fr_resolve.")]
    visa: Vec<VisaView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(
        description = "Related decisions; id is read with fr_get_judilibre, legifrance_id with fr_get_decision."
    )]
    related: Vec<RelatedView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ContestedView", description = "The decision under appeal.")]
    contested: Option<ContestedView>,
    attribution: String,
    #[schemars(description = "Terms of reuse of these decisions.")]
    case_law_note: String,
    #[serde(flatten)]
    reading: ReadingView,
    // Vrai une fois des liens retirés : la remarque n'est ajoutée qu'une fois.
    #[serde(skip)]
    links_cut: bool,
}

// Un texte appliqué (visa), balises retirées.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(inline)]
struct VisaView {
    title: String,
}

// Une décision rapprochée.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(inline)]
struct RelatedView {
    title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    id: Option<String>,
    // `JURITEXT…`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    legifrance_id: Option<String>,
}

impl From<RelatedDecision> for RelatedView {
    fn from(related: RelatedDecision) -> Self {
        Self {
            title: related.title,
            id: related.id.map(|id| id.as_str().to_owned()),
            legifrance_id: related.legifrance_id,
        }
    }
}

// La décision attaquée, telle que publiée.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(inline)]
struct ContestedView {
    court: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    chamber: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    solution: Option<String>,
}

impl From<ContestedDecision> for ContestedView {
    fn from(contested: ContestedDecision) -> Self {
        Self {
            court: contested.court,
            chamber: contested.chamber,
            date: contested.date,
            solution: contested.solution,
        }
    }
}

impl From<GetJudilibreOutput> for CaseLawView {
    fn from(output: GetJudilibreOutput) -> Self {
        let decision = output.decision;
        let citation = decision.mention.citation(decision.number.as_deref());
        let mut view = Self {
            id: decision.id.as_str().to_owned(),
            court: decision.mention.court,
            chamber: decision.mention.chamber,
            formation: decision.mention.formation,
            date: decision.mention.date,
            number: decision.number,
            ecli: decision.ecli,
            solution: decision.solution,
            publication: decision.publication,
            themes: decision.themes,
            citation,
            updated: decision.updated,
            visa: decision.visa.into_iter().map(|title| VisaView { title }).collect(),
            related: decision.related.into_iter().map(RelatedView::from).collect(),
            contested: decision.contested.map(ContestedView::from),
            attribution: ATTRIBUTION.to_owned(),
            case_law_note: CASE_LAW_NOTE.to_owned(),
            reading: ReadingView::from(output.reading),
            links_cut: false,
        };
        // L'application ne voit pas les échappements JSON : la mesure exacte tranche, et la
        // fenêtre raccourcie garde un `next_offset` exact.
        fit_under_cap(&mut view, Self::degrade);
        view
    }
}

impl CaseLawView {
    /// Retire un cran de contenu, en commençant par ce qui pèse le plus : la lecture tant qu'elle
    /// pèse plus que les liens, sinon le dernier rapprochement, puis le dernier visa, en le
    /// disant une fois dans `note` ; `false` quand il ne reste rien à retirer.
    fn degrade(&mut self) -> bool {
        if self.reading_weight() >= self.links_weight() && self.reading.degrade() {
            return true;
        }
        self.drop_link() || self.reading.degrade()
    }

    /// Octets des textes de la lecture que la dégradation peut raccourcir : la fenêtre ou les
    /// passages (le résumé est déjà borné).
    fn reading_weight(&self) -> usize {
        let window = self.reading.text.as_ref().map_or(0, String::len);
        let passages = self
            .reading
            .passages
            .iter()
            .flatten()
            .fold(0_usize, |sum, passage| sum.saturating_add(passage.text.len()));
        window.saturating_add(passages)
    }

    /// Octets des intitulés des visas et des rapprochements.
    fn links_weight(&self) -> usize {
        let visa = self.visa.iter().map(|visa| visa.title.len());
        let related = self.related.iter().map(|related| related.title.len());
        visa.chain(related).fold(0_usize, usize::saturating_add)
    }

    /// Retire le dernier rapprochement, sinon le dernier visa, et le dit une fois dans `note` ;
    /// `false` s'il n'y a plus de lien.
    fn drop_link(&mut self) -> bool {
        if self.related.pop().is_none() && self.visa.pop().is_none() {
            return false;
        }
        if !self.links_cut {
            self.links_cut = true;
            self.reading.note = Some(match self.reading.note.take() {
                Some(note) => format!("{note} {LINKS_CUT_NOTE}"),
                None => LINKS_CUT_NOTE.to_owned(),
            });
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use lf_application::budget::{Reading, ResponseBudget};
    use lf_domain::{
        judilibre::{CaseLawDoc, CaseLawMention, Court, JudilibreId, Zones},
        text::{Summary, SummarySource},
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::{schema::output_schema, views::reading::PassageView};

    /// La décision de la Cour de cassation des fixtures, texte et zones déjà retirés par
    /// l'application, avec un lien de chaque sorte.
    fn decision() -> CaseLawDoc {
        CaseLawDoc {
            id: JudilibreId::parse("5fca624b7eeb384a40cafb85").unwrap(),
            jurisdiction: Court::Cassation,
            mention: CaseLawMention {
                court: "Cour de cassation".to_owned(),
                chamber: Some("Première chambre civile".to_owned()),
                formation: Some("Formation de section".to_owned()),
                date: "2020-12-02".to_owned(),
            },
            number: Some("19-20.000".to_owned()),
            ecli: Some("ECLI:FR:CCASS:2020:C100001".to_owned()),
            solution: Some("Cassation".to_owned()),
            publication: Some("Publié au Bulletin".to_owned()),
            themes: vec!["Donation".to_owned()],
            summary: Some("Une donation entre vifs est irrévocable.".to_owned()),
            text: String::new(),
            zones: Zones::default(),
            visa: vec!["article 894 du code civil".to_owned()],
            related: vec![RelatedDecision::from_html(
                r#"<a href="5fca624b7eeb384a40cafb86">1re Civ., 22 octobre 1980</a>"#,
            )],
            contested: Some(ContestedDecision {
                court: "Cour d'appel de Paris".to_owned(),
                chamber: None,
                date: Some("2019-05-14".to_owned()),
                solution: None,
            }),
            updated: Some("2024-03-01".to_owned()),
            partial: false,
            to_be_deleted: false,
        }
    }

    fn reading(text: &str) -> Reading {
        Reading {
            summary: Some(Summary {
                source: SummarySource::Official,
                text: "Une donation entre vifs est irrévocable.".to_owned(),
                truncated: false,
            }),
            text: Some(text.to_owned()),
            text_length: u32::try_from(text.chars().count()).unwrap(),
            next_offset: None,
            passages: None,
            note: None,
            summary_truncated: false,
        }
    }

    fn view(decision: CaseLawDoc, reading: Reading) -> CaseLawView {
        CaseLawView::from(GetJudilibreOutput { decision, part: None, reading })
    }

    fn json_of(view: &CaseLawView) -> Value {
        serde_json::to_value(view).unwrap()
    }

    #[test]
    fn the_view_carries_the_mentions_the_links_and_the_reading() {
        let value = json_of(&view(decision(), reading("Attendu que la donation…")));
        assert_eq!(
            value,
            json!({
                "id": "5fca624b7eeb384a40cafb85",
                "court": "Cour de cassation",
                "chamber": "Première chambre civile",
                "formation": "Formation de section",
                "date": "2020-12-02",
                "number": "19-20.000",
                "ecli": "ECLI:FR:CCASS:2020:C100001",
                "solution": "Cassation",
                "publication": "Publié au Bulletin",
                "themes": ["Donation"],
                "citation": "Cour de cassation, Première chambre civile, 2 décembre 2020, n° 19-20.000",
                "updated": "2024-03-01",
                "visa": [{"title": "article 894 du code civil"}],
                "related": [{
                    "title": "1re Civ., 22 octobre 1980",
                    "id": "5fca624b7eeb384a40cafb86"
                }],
                "contested": {"court": "Cour d'appel de Paris", "date": "2019-05-14"},
                "attribution": ATTRIBUTION,
                "case_law_note": CASE_LAW_NOTE,
                "summary": "Une donation entre vifs est irrévocable.",
                "summary_source": "official",
                "text": "Attendu que la donation…",
                "text_length": 24,
            })
        );
    }

    #[test]
    fn absent_fields_and_empty_links_are_omitted() {
        let bare = CaseLawDoc {
            mention: CaseLawMention {
                court: "Conseil de prud'hommes".to_owned(),
                chamber: None,
                formation: None,
                date: "2023-01-18".to_owned(),
            },
            number: None,
            ecli: None,
            solution: None,
            publication: None,
            themes: Vec::new(),
            visa: Vec::new(),
            related: Vec::new(),
            contested: None,
            updated: None,
            ..decision()
        };
        // L'ordre des clés est celui de 05 §5.19 (`serde_json::Value` les trierait).
        let json = serde_json::to_string(&view(bare, reading("Texte."))).unwrap();
        assert_eq!(
            json,
            format!(
                r#"{{"id":"5fca624b7eeb384a40cafb85","court":"Conseil de prud'hommes","date":"2023-01-18","citation":"Conseil de prud'hommes, 18 janvier 2023","attribution":{},"case_law_note":{},"summary":"Une donation entre vifs est irrévocable.","summary_source":"official","text":"Texte.","text_length":6}}"#,
                serde_json::to_string(ATTRIBUTION).unwrap(),
                serde_json::to_string(CASE_LAW_NOTE).unwrap()
            )
        );
    }

    #[test]
    fn a_related_decision_keeps_its_legifrance_id() {
        let related = RelatedView::from(RelatedDecision {
            title: "Cass. 1re civ., 1980".to_owned(),
            id: None,
            legifrance_id: Some("JURITEXT000007006523".to_owned()),
        });
        assert_eq!(
            serde_json::to_value(related).unwrap(),
            json!({"title": "Cass. 1re civ., 1980", "legifrance_id": "JURITEXT000007006523"})
        );
    }

    /// Une fenêtre pleine d'échappements est raccourcie sous le plafond, liens intacts.
    #[test]
    fn an_escape_rich_window_is_shortened_first() {
        let text = "\"".repeat(ResponseBudget::MAX_CHARS_MAX);
        let shown = view(decision(), reading(&text));
        let size = serde_json::to_string(&shown).unwrap().chars().count();
        assert!(size <= ResponseBudget::HARD_CAP_CHARS, "{size}");
        assert_eq!((shown.related.len(), shown.visa.len()), (1, 1));
        assert!(shown.reading.next_offset.is_some());
        assert_eq!(shown.reading.note, None);
    }

    /// Des liens démesurés (réponse amont anormale) sont retirés par la fin, et la remarque le dit
    /// une seule fois, après celle de la lecture.
    #[test]
    fn oversized_links_are_dropped_from_the_end_with_one_note() {
        let title = "é".repeat(400);
        let many = CaseLawDoc {
            visa: vec![title.clone(); 150],
            related: (0..100).map(|_| RelatedDecision::from_html(&title)).collect(),
            ..decision()
        };
        let mut noted = reading("");
        noted.text = None;
        noted.note = Some("Note.".to_owned());
        let shown = view(many, noted);
        let size = serde_json::to_string(&shown).unwrap().chars().count();
        assert!(size <= ResponseBudget::HARD_CAP_CHARS, "{size}");
        assert_eq!(shown.related.len(), 0);
        let kept = shown.visa.len();
        assert!(kept < 150 && kept > 0, "{kept}");
        assert_eq!(shown.reading.note, Some(format!("Note. {LINKS_CUT_NOTE}")));
    }

    /// Sans lecture, chaque cran retire un lien, la remarque des liens est seule, puis il ne reste
    /// rien à retirer. Ce test court, qui ne passe pas par `fit_under_cap`, échoue tout de suite si
    /// `degrade` ou `drop_link` promettent un cran sans rien retirer : l'arrêt au premier échec de
    /// nextest coupe alors les tests qui bouclent (mutants `-> true`, CONTRIBUTING).
    #[test]
    fn a_view_without_reading_drops_one_link_per_step_then_stops() {
        let mut quiet = view(decision(), reading(""));
        quiet.reading.text = None;
        assert!(quiet.degrade());
        assert_eq!((quiet.related.len(), quiet.visa.len()), (0, 1));
        assert!(quiet.degrade());
        assert_eq!(quiet.reading.note.as_deref(), Some(LINKS_CUT_NOTE));
        assert!(!quiet.drop_link());
        assert!(!quiet.degrade());
    }

    /// Quand les liens pèsent plus que la lecture, ils partent les premiers : un texte court reste
    /// entier. Une lecture plus lourde que les liens est raccourcie avant eux.
    #[test]
    fn the_heavier_of_reading_and_links_is_cut_first() {
        let title = "é".repeat(400);
        let many = CaseLawDoc {
            visa: vec![title.clone(); 150],
            related: (0..100).map(|_| RelatedDecision::from_html(&title)).collect(),
            ..decision()
        };
        let short = "Un texte court, rendu entier.";
        let shown = view(many, reading(short));
        assert_eq!(shown.reading.text.as_deref(), Some(short));
        assert_eq!(shown.reading.next_offset, None);
        assert!(shown.related.is_empty() && !shown.visa.is_empty());
        // Lecture plus lourde : elle est raccourcie, les liens restent.
        let mut heavy = view(decision(), reading(&"Un mot. ".repeat(100)));
        assert!(heavy.reading_weight() > heavy.links_weight());
        assert!(heavy.degrade());
        assert_eq!((heavy.related.len(), heavy.visa.len(), heavy.links_cut), (1, 1, false));
        assert!(heavy.reading.next_offset.is_some());
        // Des passages pèsent leurs textes, comme la fenêtre.
        let mut focused = view(decision(), reading(""));
        focused.reading.text = None;
        focused.reading.passages = Some(vec![
            PassageView { offset: 0, text: "é".repeat(10), score: 1.0 },
            PassageView { offset: 40, text: "a".repeat(5), score: 0.5 },
        ]);
        assert_eq!(focused.reading_weight(), 25);
    }

    #[test]
    fn the_output_schema_names_the_fields_in_order() {
        let schema = Value::Object(output_schema::<CaseLawView>().as_ref().clone());
        assert_eq!(
            schema["required"],
            json!([
                "id",
                "court",
                "date",
                "citation",
                "attribution",
                "case_law_note",
                "text_length"
            ])
        );
        let related = &schema["properties"]["related"]["items"];
        assert_eq!(related["required"], json!(["title"]));
        assert_eq!(schema["properties"]["contested"]["required"], json!(["court"]));
        assert!(schema["properties"].get("links_cut").is_none());
        assert!(schema["properties"].get("source_url").is_none(), "V-239");
    }

    #[test]
    fn output_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        let schema = Value::Object(output_schema::<CaseLawView>().as_ref().clone());
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("output_schema", schema);
        });
    }
}
