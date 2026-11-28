//! Vue de sortie de `fr_search_judilibre` (05 §5.18) : `CaseLawSearchView`.
//!
//! Construite depuis le [`SearchJudilibreOutput`] de l'application : une page de décisions,
//! jamais leur texte. Chaque résultat porte les mentions des CGU (juridiction et siège, chambre ou
//! formation, date) et sa citation ; chaque réponse porte la source ([`ATTRIBUTION`]), le jour de
//! la consultation et [`CASE_LAW_NOTE`] (ADR-0035, décision 6). L'application réserve
//! [`CASE_LAW_CONSTANTS_RESERVE_CHARS`](lf_application::output::search_judilibre::CASE_LAW_CONSTANTS_RESERVE_CHARS)
//! caractères aux deux constantes : un test vérifie qu'elles y tiennent.
//!
//! L'application compte exactement le JSON de chaque résultat (échappements compris) et raccourcit
//! les sommaires avant de construire la vue : la mesure de `views::fit` n'est ici qu'un filet, qui
//! ne retire rien (jamais un résultat, 05 §6.4). `source_url` attend V-239. Les commentaires des
//! champs sont des `//` : un doc comment partirait dans l'`outputSchema`, dont la taille compte
//! (05 §3.4).

use lf_application::output::search_judilibre::SearchJudilibreOutput;
use lf_domain::judilibre::CaseLawHit;
use schemars::JsonSchema;
use serde::Serialize;

use super::fit::fit_under_cap;

/// Source de chaque réponse de Judilibre, exigée par les CGU (ADR-0035, décision 6).
pub(crate) const ATTRIBUTION: &str = "Source : Cour de cassation, base Open Data des décisions judiciaires (Judilibre) — Licence Ouverte 2.0";

/// Note fixe de chaque réponse de Judilibre : décisions pseudonymisées et telles que publiées,
/// réidentification et profilage des magistrats et des greffiers interdits (ADR-0035, décisions 6
/// et 7 ; article L. 111-13 du code de l'organisation judiciaire).
pub(crate) const CASE_LAW_NOTE: &str = "Decisions are pseudonymised and given as published by the Cour de cassation. Never try to re-identify a party. French law (article L. 111-13 of the Code de l'organisation judiciaire) forbids using the identity of judges and court clerks to evaluate, analyse, compare or predict their professional practices.";

// Un résultat : de quoi citer la décision et la lire avec `fr_get_judilibre`. Ordre et champs
// sont ceux que compte `hit_cost` de l'application.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(inline)]
struct CaseLawHitView {
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
    // Sommaire de la Cour de cassation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    summary: Option<String>,
    // Premier extrait surligné, sans balises.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    excerpt: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    themes: Vec<String>,
    citation: String,
}

impl From<CaseLawHit> for CaseLawHitView {
    fn from(hit: CaseLawHit) -> Self {
        let citation = hit.mention.citation(hit.number.as_deref());
        Self {
            id: hit.id.as_str().to_owned(),
            court: hit.mention.court,
            chamber: hit.mention.chamber,
            formation: hit.mention.formation,
            date: hit.mention.date,
            number: hit.number,
            ecli: hit.ecli,
            solution: hit.solution,
            publication: hit.publication,
            summary: hit.summary,
            excerpt: hit.excerpt,
            themes: hit.themes,
            citation,
        }
    }
}

/// Ce que `fr_search_judilibre` rend : une page de résultats et les mentions des CGU.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct CaseLawSearchView {
    total: u64,
    page: u32,
    #[schemars(description = "True if the next page exists and is reachable.")]
    has_more: bool,
    hits: Vec<CaseLawHitView>,
    attribution: String,
    // Jour de la consultation, Europe/Paris.
    consulted_on: String,
    #[schemars(description = "Terms of reuse of these decisions.")]
    case_law_note: String,
    // Indice facultatif : aucun résultat, page vide, fin de la fenêtre, sommaires raccourcis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    note: Option<String>,
}

impl From<SearchJudilibreOutput> for CaseLawSearchView {
    fn from(output: SearchJudilibreOutput) -> Self {
        let mut view = Self {
            total: output.total,
            page: output.page,
            has_more: output.has_more,
            hits: output.hits.into_iter().map(CaseLawHitView::from).collect(),
            attribution: ATTRIBUTION.to_owned(),
            consulted_on: output.consulted_on.to_string(),
            case_law_note: CASE_LAW_NOTE.to_owned(),
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
        budget::ResponseBudget, output::search_judilibre::CASE_LAW_CONSTANTS_RESERVE_CHARS,
    };
    use lf_domain::{
        date::CivilDate,
        judilibre::{CaseLawMention, Court, EXCERPT_MAX_CHARS, JudilibreId},
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::schema::output_schema;

    /// Le résultat de la Cour de cassation des fixtures de l'application, champ pour champ.
    fn cassation() -> CaseLawHit {
        CaseLawHit {
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
            summary: Some("Une donation entre vifs est irrévocable.".to_owned()),
            excerpt: None,
            themes: vec!["Donation".to_owned()],
        }
    }

    /// Le résultat de cour d'appel des fixtures de l'application.
    fn appeal() -> CaseLawHit {
        CaseLawHit {
            id: JudilibreId::parse("63c8eefddc5b777c90992fd8").unwrap(),
            jurisdiction: Court::Appeal,
            mention: CaseLawMention {
                court: "Cour d'appel de Paris".to_owned(),
                chamber: Some("Pôle 3 - Chambre 1".to_owned()),
                formation: None,
                date: "2023-01-18".to_owned(),
            },
            number: Some("20/17369".to_owned()),
            ecli: None,
            solution: Some("Infirmation".to_owned()),
            publication: None,
            summary: None,
            excerpt: Some("la donation consentie entre époux".to_owned()),
            themes: vec!["Donation".to_owned()],
        }
    }

    fn output(hits: Vec<CaseLawHit>) -> SearchJudilibreOutput {
        SearchJudilibreOutput {
            total: 1_234,
            page: 12,
            has_more: true,
            hits,
            consulted_on: CivilDate::new(2026, 11, 27).unwrap(),
            note: None,
        }
    }

    fn json_of(output: SearchJudilibreOutput) -> Value {
        serde_json::to_value(CaseLawSearchView::from(output)).unwrap()
    }

    #[test]
    fn the_view_carries_the_page_the_mentions_and_the_terms_of_reuse() {
        let value = json_of(output(vec![cassation()]));
        assert_eq!(
            value,
            json!({
                "total": 1234,
                "page": 12,
                "has_more": true,
                "hits": [{
                    "id": "5fca624b7eeb384a40cafb85",
                    "court": "Cour de cassation",
                    "chamber": "Première chambre civile",
                    "formation": "Formation de section",
                    "date": "2020-12-02",
                    "number": "19-20.000",
                    "ecli": "ECLI:FR:CCASS:2020:C100001",
                    "solution": "Cassation",
                    "publication": "Publié au Bulletin",
                    "summary": "Une donation entre vifs est irrévocable.",
                    "themes": ["Donation"],
                    "citation": "Cour de cassation, Première chambre civile, 2 décembre 2020, n° 19-20.000"
                }],
                "attribution": ATTRIBUTION,
                "consulted_on": "2026-11-27",
                "case_law_note": CASE_LAW_NOTE,
            })
        );
    }

    #[test]
    fn absent_fields_are_omitted_and_the_note_is_passed_on() {
        let bare = CaseLawHit {
            mention: CaseLawMention {
                court: "Tribunal judiciaire".to_owned(),
                chamber: None,
                formation: None,
                date: "0201-02-24".to_owned(),
            },
            number: None,
            solution: None,
            excerpt: None,
            themes: Vec::new(),
            ..appeal()
        };
        let mut empty = output(vec![bare]);
        empty.has_more = false;
        empty.note = Some("Note.".to_owned());
        let value = json_of(empty);
        assert_eq!(
            value["hits"],
            json!([{
                "id": "63c8eefddc5b777c90992fd8",
                "court": "Tribunal judiciaire",
                "date": "0201-02-24",
                "citation": "Tribunal judiciaire, 0201-02-24"
            }])
        );
        assert_eq!(
            (value["has_more"].clone(), value["note"].clone()),
            (json!(false), json!("Note."))
        );
        assert!(json_of(output(Vec::new())).get("note").is_none());
    }

    /// Parité avec le calcul de taille de l'application (`hit_cost`, `envelope_cost` de
    /// `usecase/search_judilibre.rs`) : les mêmes littéraux JSON que ses tests, à l'octet. Un
    /// champ renommé, déplacé ou ajouté dans la vue casse ce test, et dit de reprendre le calcul.
    #[test]
    fn the_compact_json_is_the_one_the_application_counts() {
        let hit = cassation();
        let citation = hit.mention.citation(hit.number.as_deref());
        assert_eq!(
            serde_json::to_string(&CaseLawHitView::from(hit)).unwrap(),
            format!(
                r#"{{"id":"5fca624b7eeb384a40cafb85","court":"Cour de cassation","chamber":"Première chambre civile","formation":"Formation de section","date":"2020-12-02","number":"19-20.000","ecli":"ECLI:FR:CCASS:2020:C100001","solution":"Cassation","publication":"Publié au Bulletin","summary":"Une donation entre vifs est irrévocable.","themes":["Donation"],"citation":"{citation}"}}"#
            )
        );
        let hit = appeal();
        let citation = hit.mention.citation(hit.number.as_deref());
        assert_eq!(
            serde_json::to_string(&CaseLawHitView::from(hit)).unwrap(),
            format!(
                r#"{{"id":"63c8eefddc5b777c90992fd8","court":"Cour d'appel de Paris","chamber":"Pôle 3 - Chambre 1","date":"2023-01-18","number":"20/17369","solution":"Infirmation","excerpt":"la donation consentie entre époux","themes":["Donation"],"citation":"{citation}"}}"#
            )
        );
        // L'enveloppe : celle que l'application compte, plus les deux constantes de la vue.
        let mut noted = output(Vec::new());
        noted.has_more = false;
        noted.note = Some("Note « é ».".to_owned());
        let json = serde_json::to_string(&CaseLawSearchView::from(noted)).unwrap();
        assert_eq!(
            json,
            format!(
                r#"{{"total":1234,"page":12,"has_more":false,"hits":[],"attribution":{},"consulted_on":"2026-11-27","case_law_note":{},"note":"Note « é »."}}"#,
                serde_json::to_string(ATTRIBUTION).unwrap(),
                serde_json::to_string(CASE_LAW_NOTE).unwrap()
            )
        );
        let counted = r#"{"total":1234,"page":12,"has_more":false,"hits":[],"consulted_on":"2026-11-27","note":"Note « é »."}"#;
        assert_eq!(json.chars().count(), counted.chars().count() + constants().chars().count());
    }

    /// Les deux constantes que la vue ajoute à l'enveloppe, clés, guillemets et virgules compris.
    fn constants() -> String {
        format!(
            r#","attribution":{},"case_law_note":{}"#,
            serde_json::to_string(ATTRIBUTION).unwrap(),
            serde_json::to_string(CASE_LAW_NOTE).unwrap()
        )
    }

    /// Les constantes tiennent dans la réserve que l'application leur garde, sans échappement.
    #[test]
    fn the_constants_fit_in_the_reserve_of_the_application() {
        let size = constants().chars().count();
        assert!(size <= CASE_LAW_CONSTANTS_RESERVE_CHARS, "{size}");
        assert_eq!(CASE_LAW_CONSTANTS_RESERVE_CHARS, 1_000);
        for constant in [ATTRIBUTION, CASE_LAW_NOTE] {
            assert_eq!(serde_json::to_string(constant).unwrap(), format!("\"{constant}\""));
        }
    }

    /// V-242 : les mentions des CGU V8 (source, licence, pseudonymisation, données non altérées,
    /// interdiction de réidentifier et de profiler) figurent dans chaque réponse, sans adresse.
    #[test]
    fn the_mentions_of_the_terms_of_reuse_are_all_there() {
        assert!(ATTRIBUTION.contains("Cour de cassation"));
        assert!(ATTRIBUTION.contains("Judilibre"));
        assert!(ATTRIBUTION.contains("Licence Ouverte 2.0"));
        for words in [
            "pseudonymised",
            "as published",
            "re-identify",
            "L. 111-13",
            "judges and court clerks",
            "evaluate, analyse, compare or predict",
        ] {
            assert!(CASE_LAW_NOTE.contains(words), "{words}");
        }
        assert!(!CASE_LAW_NOTE.contains("http") && !ATTRIBUTION.contains("http"));
    }

    /// Une page pleine (50 résultats aux sommaires de 300 caractères) reste sous le plafond dur.
    #[test]
    fn a_full_page_stays_under_the_hard_cap() {
        let summary = "é".repeat(EXCERPT_MAX_CHARS);
        let hits: Vec<CaseLawHit> =
            (0..50).map(|_| CaseLawHit { summary: Some(summary.clone()), ..cassation() }).collect();
        let view = CaseLawSearchView::from(output(hits));
        assert_eq!(view.hits.len(), 50);
        let size = serde_json::to_string(&view).unwrap().chars().count();
        assert!(size <= ResponseBudget::HARD_CAP_CHARS, "{size}");
    }

    #[test]
    fn the_output_schema_names_the_fields_in_order() {
        let schema = Value::Object(output_schema::<CaseLawSearchView>().as_ref().clone());
        assert_eq!(
            schema["required"],
            json!([
                "total",
                "page",
                "has_more",
                "hits",
                "attribution",
                "consulted_on",
                "case_law_note"
            ])
        );
        let hit = &schema["properties"]["hits"]["items"];
        assert_eq!(hit["required"], json!(["id", "court", "date", "citation"]));
        assert!(schema.get("$schema").is_none());
    }

    #[test]
    fn output_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        let schema = Value::Object(output_schema::<CaseLawSearchView>().as_ref().clone());
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("output_schema", schema);
        });
    }
}
