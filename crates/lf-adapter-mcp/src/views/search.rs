//! Vue de sortie de `fr_search` (05 §5.1, §3.4) : `SearchView`, construite depuis le
//! `SearchOutput` de l'application.
//!
//! Les règles de rendu communes sont celles de `views::common` (titre omis s'il est dans la
//! citation, `etat` en code d'API, date sentinelle omise). `idcc` et `total_articles` sont des
//! ajouts de la v0.2 (classe « ajout », 00 §7) : ils ne sont rendus que si la surface annoncée
//! n'est pas la v0.1 ([`SURFACE`]). La mesure exacte du JSON compact suit la construction
//! (`views::fit`) : pour `fr_search`, l'estimation de l'application est exacte (elle compte le
//! JSON que cette vue écrit, échappements compris) et la reprise de la dégradation, dans le même
//! ordre, n'est qu'un filet (05 §6.4).

use lf_application::output::search::{HitOut, SearchOutput};
use lf_domain::entity::CodeArticleRef;
use schemars::JsonSchema;
use serde::Serialize;

use super::{
    common::{date_unless_sentinel, etat_code, title_unless_in_citation},
    fit::fit_under_cap,
};
use crate::text::{SURFACE, Surface};

/// Note ajoutée quand la vue doit encore retirer du contenu après l'estimation de l'application.
const FURTHER_NOTE: &str = "Further snippets and details were removed to keep the response \
                            under 45,000 characters; use a smaller page_size to get them.";

/// Une page de résultats de `fr_search`.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct SearchView {
    #[schemars(
        description = "Total results; for CODE, codes; total_articles (if present) counts articles."
    )]
    total: u64,
    /// Page servie, à partir de 1.
    page: u32,
    /// Vrai si la page suivante existe et est atteignable.
    #[schemars(description = "True if a next page exists and is reachable.")]
    has_more: bool,
    /// Nombre total d'articles correspondants (fond `CODE`), à partir de la v0.2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u64")]
    total_articles: Option<u64>,
    /// Résultats, dans l'ordre de l'API ; toujours présents, même vides.
    hits: Vec<HitView>,
    /// Indice facultatif (aucun résultat, fin de la fenêtre atteignable, articles à lire, contenu
    /// retiré).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        with = "String",
        description = "Hint: no match, window end, articles to page, or content removed."
    )]
    note: Option<String>,
}

/// Un résultat de recherche.
#[derive(Debug, Serialize, JsonSchema)]
struct HitView {
    /// Fond du résultat, en minuscules : `loda`, `code`, `juri`, `constit`, `cetat`, `cnil`,
    /// `kali` ou `acco`.
    kind: String,
    /// Identifiant sans suffixe de version ; son préfixe désigne l'outil de lecture.
    id: String,
    /// Titre, s'il n'est pas déjà dans `citation`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    title: Option<String>,
    /// Date `AAAA-MM-JJ` donnée par Légifrance pour le fond, pas toujours celle de l'acte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        with = "String",
        description = "YYYY-MM-DD as Légifrance gives it for the fond (CNIL: publication, ACCO: signature), not always the act date; absent if unknown."
    )]
    date: Option<String>,
    /// Nature du texte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    nature: Option<String>,
    /// État juridique, en code d'API (`VIGUEUR`, `ABROGE`...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String", description = "In force: VIGUEUR*, ABROGE_DIFF; other codes not.")]
    etat: Option<String>,
    /// IDCC (KALI et ACCO), à partir de la v0.2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    idcc: Option<String>,
    /// Extrait de 280 caractères au plus.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    snippet: Option<String>,
    /// Articles trouvés (fonds `CODE` et `KALI`), lisibles avec `fr_get_text`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    articles: Vec<ArticleView>,
    /// Citation selon l'usage français.
    citation: String,
    /// Page Légifrance de la ressource.
    source_url: String,
}

/// Un article trouvé dans un résultat de code.
#[derive(Debug, Serialize, JsonSchema)]
struct ArticleView {
    /// Identifiant de l'article.
    article_id: String,
    /// Numéro affiché.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    num: Option<String>,
    /// Extrait de l'article.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    snippet: Option<String>,
}

impl From<SearchOutput> for SearchView {
    /// Vue de la surface annoncée, ramenée sous le plafond dur de 45 000 caractères.
    fn from(output: SearchOutput) -> Self {
        let mut view = Self::build(output, SURFACE);
        view.fit();
        view
    }
}

impl SearchView {
    /// Vue de `output` pour `surface`, sans mesure du plafond.
    fn build(output: SearchOutput, surface: Surface) -> Self {
        let additions = surface != Surface::V01;
        Self {
            total: output.total,
            page: output.page,
            has_more: output.has_more,
            total_articles: output.total_articles.filter(|_| additions),
            hits: output.hits.into_iter().map(|hit| HitView::build(hit, additions)).collect(),
            note: output.note,
        }
    }

    /// Mesure la vue et la dégrade tant qu'elle dépasse le plafond dur (05 §6.4). Une note le dit
    /// dès le premier retrait.
    fn fit(&mut self) {
        let mut noted = false;
        fit_under_cap(self, |view| {
            let stripped = view.strip_one();
            if stripped && !noted {
                noted = true;
                view.note = Some(match view.note.take() {
                    Some(note) => format!("{note} {FURTHER_NOTE}"),
                    None => FURTHER_NOTE.to_owned(),
                });
            }
            stripped
        });
    }

    /// Retire un peu de contenu, en partant du dernier résultat : d'abord les extraits (un
    /// résultat à la fois), puis les listes d'articles et les champs optionnels. Jamais un
    /// résultat. Vrai si quelque chose a été retiré.
    fn strip_one(&mut self) -> bool {
        if let Some(hit) = self.hits.iter_mut().rev().find(|hit| hit.has_snippets()) {
            hit.strip_snippets();
            return true;
        }
        if let Some(hit) = self.hits.iter_mut().rev().find(|hit| hit.has_details()) {
            hit.strip_details();
            return true;
        }
        false
    }
}

impl HitView {
    fn build(out: HitOut, additions: bool) -> Self {
        let hit = out.hit;
        Self {
            kind: hit.fond.name().to_ascii_lowercase(),
            id: hit.id.as_str().to_owned(),
            title: title_unless_in_citation(hit.title.as_deref(), &out.citation),
            date: date_unless_sentinel(hit.date),
            nature: hit.nature,
            etat: etat_code(hit.etat.as_ref()),
            idcc: hit.idcc.filter(|_| additions).map(|idcc| idcc.to_string()),
            snippet: hit.snippet.map(|snippet| snippet.as_str().to_owned()),
            articles: hit.articles.into_iter().map(ArticleView::from).collect(),
            citation: out.citation,
            source_url: out.source_url,
        }
    }

    fn has_snippets(&self) -> bool {
        self.snippet.is_some() || self.articles.iter().any(|article| article.snippet.is_some())
    }

    fn strip_snippets(&mut self) {
        self.snippet = None;
        for article in &mut self.articles {
            article.snippet = None;
        }
    }

    fn has_details(&self) -> bool {
        !self.articles.is_empty()
            || [&self.title, &self.date, &self.nature, &self.etat]
                .iter()
                .any(|field| field.is_some())
    }

    fn strip_details(&mut self) {
        self.articles.clear();
        (self.title, self.date, self.nature, self.etat) = (None, None, None, None);
    }
}

impl From<CodeArticleRef> for ArticleView {
    fn from(article: CodeArticleRef) -> Self {
        Self {
            article_id: article.article_id.as_str().to_owned(),
            num: article.num,
            snippet: article.snippet.map(|snippet| snippet.as_str().to_owned()),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::as_conversions,
    reason = "tests : tailles et indices bornés par les données du test"
)]
mod tests {
    use lf_application::budget::ResponseBudget;
    use lf_domain::{
        contract::Idcc,
        date::CivilDate,
        entity::{SearchHit, Snippet},
        fond::Fond,
        id::{ArticleId, DocumentId},
        status::LegalStatus,
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::schema::output_schema;

    /// D-12 : la règle générale de `etat` est dans la description du champ (`VIGUEUR_ETEN`, `PERIME`,
    /// ANNULE... ne sont pas dans le glossaire des instructions).
    #[test]
    fn the_etat_field_carries_the_general_rule() {
        let schema = serde_json::Value::Object(output_schema::<SearchView>().as_ref().clone());
        let text = schema.to_string();
        assert!(text.contains("In force: VIGUEUR*, ABROGE_DIFF; other codes not."), "{text}");
    }

    const CAP: usize = ResponseBudget::HARD_CAP_CHARS;

    fn doc(id: &str) -> DocumentId {
        DocumentId::parse(id).unwrap()
    }

    fn hit(fond: Fond, id: &str) -> SearchHit {
        SearchHit::new(fond, doc(id), None).unwrap()
    }

    /// Résultat dont la citation est donnée : l'application la calcule avant la vue.
    fn out(hit: SearchHit, citation: &str) -> HitOut {
        let url = format!("https://www.legifrance.gouv.fr/x/id/{}", hit.id.as_str());
        HitOut { hit, citation: citation.to_owned(), source_url: url }
    }

    fn output(hits: Vec<HitOut>) -> SearchOutput {
        SearchOutput {
            total: hits.len() as u64,
            page: 1,
            has_more: false,
            total_articles: None,
            hits,
            note: None,
        }
    }

    fn json_of(view: &SearchView) -> Value {
        serde_json::to_value(view).unwrap()
    }

    fn compact_len(view: &SearchView) -> usize {
        serde_json::to_string(view).unwrap().chars().count()
    }

    fn validator() -> jsonschema::Validator {
        let schema = Value::Object(output_schema::<SearchView>().as_ref().clone());
        jsonschema::validator_for(&schema).unwrap()
    }

    #[test]
    fn an_empty_search_is_a_success_with_an_empty_hits_list() {
        let view = SearchView::build(output(vec![]), Surface::V01);
        assert_eq!(
            serde_json::to_string(&view).unwrap(),
            r#"{"total":0,"page":1,"has_more":false,"hits":[]}"#
        );
    }

    #[test]
    fn hit_fields_follow_the_rendering_rules() {
        let full = SearchHit {
            title: Some("Code civil".to_owned()),
            date: CivilDate::new(2024, 1, 2),
            nature: Some("CODE".to_owned()),
            etat: Some(LegalStatus::VigueurEtendue),
            snippet: Snippet::new("[...] vie privée [...]"),
            idcc: Idcc::parse("0275"),
            articles: vec![CodeArticleRef::new(
                ArticleId::parse("LEGIARTI000006419288").unwrap(),
                Some("9".to_owned()),
                "Chacun a droit",
            )],
            ..hit(Fond::Code, "LEGITEXT000006070721")
        };
        let page =
            SearchOutput { total_articles: Some(12), ..output(vec![out(full, "Code civil")]) };
        let expected_hit = |extra: Value| {
            let mut hit = json!({
                "kind": "code", "id": "LEGITEXT000006070721", "date": "2024-01-02",
                "nature": "CODE", "etat": "VIGUEUR_ETEN", "snippet": "[...] vie privée [...]",
                "articles": [{"article_id": "LEGIARTI000006419288", "num": "9", "snippet": "Chacun a droit"}],
                "citation": "Code civil",
                "source_url": "https://www.legifrance.gouv.fr/x/id/LEGITEXT000006070721"
            });
            hit.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            hit
        };

        // v0.1 : ni `idcc` ni `total_articles` ; le titre égal à la citation est omis.
        let v01 = json_of(&SearchView::build(page.clone(), Surface::V01));
        assert_eq!(
            v01,
            json!({"total": 1, "page": 1, "has_more": false, "hits": [expected_hit(json!({}))]})
        );
        // v0.2 et au-delà : les deux ajouts sont rendus.
        for surface in [Surface::V02, Surface::V03] {
            assert_eq!(
                json_of(&SearchView::build(page.clone(), surface)),
                json!({
                    "total": 1, "page": 1, "has_more": false, "total_articles": 12,
                    "hits": [expected_hit(json!({"idcc": "275"}))]
                })
            );
        }
    }

    #[test]
    fn the_serving_surface_decides_the_additions() {
        let with_idcc =
            SearchHit { idcc: Idcc::parse("275"), ..hit(Fond::Kali, "KALITEXT000005652329") };
        let page =
            SearchOutput { total_articles: Some(3), ..output(vec![out(with_idcc, "Convention")]) };
        let view = json_of(&SearchView::from(page));
        let additions = SURFACE != Surface::V01;
        assert_eq!(view.get("total_articles").is_some(), additions);
        assert_eq!(view["hits"][0].get("idcc").is_some(), additions);
    }

    #[test]
    fn title_etat_and_date_are_rendered_by_the_common_rules() {
        // Titre absent de la citation : rendu. Date sentinelle : omise.
        let titled = SearchHit {
            title: Some("Décision 2025-1173 QPC - Objet [Conformité]".to_owned()),
            date: CivilDate::new(2999, 1, 1),
            etat: Some(LegalStatus::Abroge),
            ..hit(Fond::Constit, "CONSTEXT000043596386")
        };
        let view = json_of(&SearchView::build(
            output(vec![out(titled, "Cons. const., décision n° 2025-1173 QPC")]),
            Surface::V01,
        ));
        let rendered = &view["hits"][0];
        assert_eq!(rendered["kind"], "constit");
        assert_eq!(rendered["title"], "Décision 2025-1173 QPC - Objet [Conformité]");
        assert_eq!(rendered["etat"], "ABROGE");
        assert!(rendered.get("date").is_none());
        assert!(rendered.get("articles").is_none(), "une liste vide est omise");
    }

    #[test]
    fn kinds_are_the_lowercase_fond_names() {
        let kinds: Vec<_> = Fond::ALL
            .iter()
            .map(|fond| {
                let id = format!("{}000012345678", fond.hit_prefix());
                out(hit(*fond, &id), "c")
            })
            .collect();
        let view = json_of(&SearchView::build(output(kinds), Surface::V01));
        let names: Vec<_> =
            view["hits"].as_array().unwrap().iter().map(|h| h["kind"].as_str().unwrap()).collect();
        assert_eq!(names, ["loda", "code", "juri", "constit", "cetat", "cnil", "kali", "acco"]);
    }

    #[test]
    fn the_note_and_the_flags_pass_through() {
        let page = SearchOutput {
            has_more: true,
            page: 7,
            total: 51_531,
            note: Some("a note".to_owned()),
            ..output(vec![])
        };
        assert_eq!(
            json_of(&SearchView::build(page, Surface::V01)),
            json!({"total": 51_531, "page": 7, "has_more": true, "hits": [], "note": "a note"})
        );
    }

    /// Les trois résultats de l'exemple de 05 §5.1 (`search_cetat.json`).
    fn cetat_example() -> SearchOutput {
        let rows = [
            (
                "CETATEXT000054242861",
                "[...] d'urbanisme, en fonction de la demande présentée : / a) Indique les dispositions d'urbanisme, les limitations administratives au droit de propriété et la liste des taxes et participations d'urbanisme [...] de l'urbanisme \". 10. [...]",
                "CAA de MARSEILLE, 4ème chambre, 09/06/2026, 24MA02429",
            ),
            (
                "CETATEXT000051655317",
                "[...] Aux termes de l'article L. 123-6 du code de l'urbanisme en vigueur à la date de la délibération prescrivant l'élaboration du PLUiH litigieux : \" Le plan local d'urbanisme est élaboré à l'initiative et [...] S'il est loisible aux auteurs des plans locaux d'urbanisme de…",
                "CAA de NANCY, 3ème chambre, 22/05/2025, 22NC02259",
            ),
            (
                "CETATEXT000054178503",
                "[...] Aux termes de l'article L. 600-4-1 du code de l'urbanisme : \" Lorsqu'elle annule pour excès de pouvoir un acte intervenu en matière d'urbanisme ou en ordonne la suspension, la juridiction administrative [...] En deuxième lieu, toutefois, aux termes de l'article L. 104-7…",
                "Conseil d'État, 2ème - 7ème chambres réunies, 29/05/2026, 506507",
            ),
        ];
        let hits = rows
            .iter()
            .map(|(id, snippet, citation)| {
                let hit = SearchHit {
                    title: Some((*citation).to_owned()),
                    nature: Some("texte".to_owned()),
                    snippet: Snippet::new(snippet),
                    ..hit(Fond::Cetat, id)
                };
                let url = format!("https://www.legifrance.gouv.fr/ceta/id/{id}");
                HitOut { hit, citation: (*citation).to_owned(), source_url: url }
            })
            .collect();
        SearchOutput { total: 51_531, has_more: true, ..output(hits) }
    }

    #[test]
    fn the_cetat_example_matches_the_published_size() {
        let view = SearchView::build(cetat_example(), Surface::V01);
        // 05 §5.1 : 1 501 caractères compacts contre 1 949 pour la référence.
        assert_eq!(compact_len(&view), 1_501);
        insta::assert_json_snapshot!("cetat_three_hits", view);
    }

    #[test]
    fn a_code_hit_with_articles_snapshot() {
        let code = SearchHit {
            nature: Some("code".to_owned()),
            etat: Some(LegalStatus::Vigueur),
            articles: vec![CodeArticleRef::new(
                ArticleId::parse("LEGIARTI000006419288").unwrap(),
                Some("9".to_owned()),
                "[...]Chacun a droit au respect de sa vie privée. Les juges peuvent, sans préjudice de la réparation [...]",
            )],
            title: Some("Code civil".to_owned()),
            ..hit(Fond::Code, "LEGITEXT000006070721")
        };
        let mut item = out(code, "Code civil");
        item.source_url =
            "https://www.legifrance.gouv.fr/codes/texte_lc/LEGITEXT000006070721".to_owned();
        let page = SearchOutput { total_articles: Some(4), ..output(vec![item]) };
        let view = SearchView::build(page, Surface::V02);
        // 05 §6.5 : 408 caractères pour la réponse entière (sans `total_articles` ni `total` 1).
        insta::assert_json_snapshot!("code_hit_with_articles", view);
    }

    // ---------------------------------------------------------------------------------------
    // Schéma de sortie
    // ---------------------------------------------------------------------------------------

    #[test]
    fn the_output_schema_is_an_object_that_requires_the_main_fields() {
        let schema = Value::Object(output_schema::<SearchView>().as_ref().clone());
        assert_eq!(schema["type"], "object");
        assert!(schema.get("$schema").is_none());
        assert_eq!(schema["required"], json!(["total", "page", "has_more", "hits"]));
        let hit = &schema["$defs"]["HitView"];
        assert_eq!(hit["required"], json!(["kind", "id", "citation", "source_url"]));
        assert_eq!(hit["properties"]["title"]["type"], "string");
        assert_eq!(schema["$defs"]["ArticleView"]["required"], json!(["article_id"]));
        assert!(schema.get("additionalProperties").is_none(), "la sortie reste ouverte");
    }

    /// D-12 (search-2, search-13) : `date` dit d'où elle vient, `total` ce qu'il compte pour CODE.
    #[test]
    fn the_output_schema_says_what_date_and_total_are() {
        let schema = Value::Object(output_schema::<SearchView>().as_ref().clone());
        let date =
            schema["$defs"]["HitView"]["properties"]["date"]["description"].as_str().unwrap();
        assert!(date.contains("as Légifrance gives it for the fond"), "{date}");
        assert!(date.contains("not always the act date"), "{date}");
        let total = schema["properties"]["total"]["description"].as_str().unwrap();
        assert!(total.contains("for CODE, codes"), "{total}");
        assert!(total.contains("total_articles (if present) counts articles"), "{total}");
        // `note` n'est plus décrite comme exceptionnelle : D-10 la pose à chaque recherche vide.
        let note = schema["properties"]["note"]["description"].as_str().unwrap();
        assert!(note.contains("no match") && note.contains("articles to page"), "{note}");
    }

    #[test]
    fn structured_content_validates_against_the_output_schema() {
        let validator = validator();
        // Une page vide, une page à résultats nus (toutes les listes omissibles vides), une page
        // à tous les champs.
        let bare = out(hit(Fond::Loda, "LEGITEXT000020445850"), "Loi");
        let full = SearchHit {
            title: Some("T".to_owned()),
            date: CivilDate::new(2024, 1, 2),
            nature: Some("N".to_owned()),
            etat: Some(LegalStatus::Vigueur),
            snippet: Snippet::new("extrait"),
            idcc: Idcc::parse("275"),
            articles: vec![CodeArticleRef::new(
                ArticleId::parse("LEGIARTI000006419288").unwrap(),
                Some("9".to_owned()),
                "a",
            )],
            ..hit(Fond::Kali, "KALITEXT000005652329")
        };
        let pages = [
            output(vec![]),
            output(vec![bare]),
            SearchOutput {
                total_articles: Some(2),
                note: Some("n".to_owned()),
                ..output(vec![out(full, "Convention")])
            },
        ];
        for page in pages {
            let value = json_of(&SearchView::build(page, Surface::V03));
            assert!(validator.is_valid(&value), "{value}");
        }
    }

    // ---------------------------------------------------------------------------------------
    // Plafond dur
    // ---------------------------------------------------------------------------------------

    /// `count` résultats dont l'extrait de 280 guillemets s'échappe en 560 caractères de JSON.
    fn quoted_snippets(count: u32) -> SearchOutput {
        let hits = (1..=count)
            .map(|n| {
                let item = SearchHit {
                    snippet: Snippet::new(&"\"".repeat(280)),
                    ..hit(Fond::Juri, &format!("JURITEXT{n:012}"))
                };
                out(item, "Cour de cassation")
            })
            .collect();
        output(hits)
    }

    #[test]
    fn nothing_is_removed_when_the_view_fits() {
        let view = SearchView::from(quoted_snippets(10));
        assert_eq!(view.hits.len(), 10);
        assert!(view.hits.iter().all(|hit| hit.snippet.is_some()));
        assert_eq!(view.note, None);
    }

    #[test]
    fn snippets_with_escapes_are_removed_from_the_last_hits_until_under_the_cap() {
        let before = SearchView::build(quoted_snippets(80), Surface::V01);
        assert!(compact_len(&before) > CAP, "le test suppose que les échappements font déborder");

        let mut view = SearchView::from(quoted_snippets(80));
        assert!(compact_len(&view) <= CAP);
        assert_eq!(view.hits.len(), 80, "aucun résultat n'est retiré");
        let stripped = view.hits.iter().filter(|hit| hit.snippet.is_none()).count();
        assert!(stripped > 0 && stripped < 80, "{stripped}");
        // En partant du dernier, sans trou.
        let first = 80 - stripped;
        assert!(view.hits[..first].iter().all(|hit| hit.snippet.is_some()));
        assert!(view.hits[first..].iter().all(|hit| hit.snippet.is_none()));
        assert_eq!(view.note.as_deref(), Some(FURTHER_NOTE));
        // Le seul retrait nécessaire : rendre son extrait au premier résultat dégradé ferait
        // déborder.
        view.hits[first].snippet = Some("\"".repeat(280));
        assert!(compact_len(&view) > CAP);
    }

    #[test]
    fn the_note_of_the_application_is_kept_before_ours() {
        let mut page = quoted_snippets(80);
        page.note = Some("Results beyond.".to_owned());
        let view = SearchView::from(page);
        assert_eq!(view.note, Some(format!("Results beyond. {FURTHER_NOTE}")));
    }

    #[test]
    fn details_go_after_every_snippet() {
        // Aucun extrait : 80 titres de 800 caractères (absents de la citation) font déborder.
        let hits = (1..=80)
            .map(|n| {
                let item = SearchHit {
                    title: Some("x".repeat(800)),
                    nature: Some("ARRET".to_owned()),
                    ..hit(Fond::Juri, &format!("JURITEXT{n:012}"))
                };
                out(item, "Cour de cassation")
            })
            .collect();
        let view = SearchView::from(output(hits));
        assert!(compact_len(&view) <= CAP);
        assert_eq!(view.hits.len(), 80);
        let stripped = view.hits.iter().filter(|hit| hit.title.is_none()).count();
        assert!(stripped > 0 && stripped < 80, "{stripped}");
        let first = 80 - stripped;
        assert!(view.hits[..first].iter().all(|hit| hit.title.is_some() && hit.nature.is_some()));
        assert!(view.hits[first..].iter().all(|hit| hit.title.is_none() && hit.nature.is_none()));
        assert_eq!(view.note.as_deref(), Some(FURTHER_NOTE));
    }

    #[test]
    fn snippets_of_a_hit_and_of_its_articles_go_together_then_its_details() {
        let item = SearchHit {
            title: Some("T".to_owned()),
            snippet: Snippet::new("hit snippet"),
            articles: vec![CodeArticleRef::new(
                ArticleId::parse("LEGIARTI000006419288").unwrap(),
                Some("9".to_owned()),
                "article snippet",
            )],
            ..hit(Fond::Code, "LEGITEXT000006070721")
        };
        let mut view = SearchView::build(output(vec![out(item, "Code civil")]), Surface::V01);
        assert!(view.strip_one());
        assert_eq!(view.hits[0].snippet, None);
        assert_eq!(view.hits[0].articles[0].snippet, None);
        assert_eq!(view.hits[0].articles[0].num.as_deref(), Some("9"), "l'article reste");
        assert!(view.hits[0].title.is_some());
        assert!(view.strip_one());
        assert!(view.hits[0].articles.is_empty() && view.hits[0].title.is_none());
        assert!(!view.strip_one(), "plus rien à retirer");
    }

    #[test]
    fn a_page_that_cannot_shrink_is_sent_as_it_is_without_a_note() {
        // Résultats nus mais citations de 1 000 caractères : 60 résultats dépassent le plafond, et
        // il n'y a plus rien à retirer.
        let hits = (1..=60)
            .map(|n| out(hit(Fond::Juri, &format!("JURITEXT{n:012}")), &"c".repeat(1_000)))
            .collect();
        let view = SearchView::from(output(hits));
        assert!(compact_len(&view) > CAP);
        assert_eq!(view.hits.len(), 60);
        assert_eq!(view.note, None, "rien n'a été retiré, donc rien à dire");
    }
}
