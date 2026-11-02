//! Vue de sortie de `fr_search_all` (05 §5.13) : `SearchAllView`.
//!
//! Construite depuis le [`SearchAllOutput`] de l'application. Chaque groupe porte son `fond`, puis
//! soit `total`, `has_more` et `hits` (de la forme de ceux de `fr_search`), soit `error` au format
//! `[code] message` des échecs d'outil. Les règles de rendu communes sont celles de
//! `views::common` ; `idcc` est un ajout de la v0.2, rendu seulement si la surface annoncée n'est
//! pas la v0.1 ([`SURFACE`]).
//!
//! La mesure exacte du JSON compact suit la construction (`views::fit`) : l'estimation de
//! l'application ignore les échappements JSON, et la vue reprend la dégradation dans le même
//! ordre, du dernier résultat du dernier fond au premier : extraits, puis listes d'articles et
//! champs optionnels. Jamais un résultat (05 §6.4). Les commentaires des champs sont des `//` : un
//! doc comment partirait dans l'`outputSchema`, dont la taille compte (05 §3.4).

use lf_application::{
    error::AppError,
    output::{
        search::HitOut,
        search_all::{FondHits, GroupOut, SearchAllOutput},
    },
};
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
                            under 45,000 characters; use fr_search on one fond to get them.";

/// Ce que `fr_search_all` rend : un groupe par fond demandé, dans l'ordre de `fonds`.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct SearchAllView {
    // Un groupe par fond, toujours présent.
    groups: Vec<GroupView>,
    // Indice facultatif (aucun résultat, contenu retiré).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String", description = "Hint: no match, or content removed.")]
    note: Option<String>,
}

// Un fond et ce qu'il a donné : ses résultats, ou l'erreur qu'il a rendue.
#[derive(Debug, Serialize, JsonSchema)]
struct GroupView {
    // Nom du fond : LODA, CODE, JURI, CONSTIT, CETAT, CNIL, KALI ou ACCO.
    fond: String,
    // Nombre total de résultats annoncé par la source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u64")]
    total: Option<u64>,
    // Vrai si `fr_search` peut donner d'autres résultats de ce fond.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool", description = "True if fr_search can page further in this fond.")]
    has_more: Option<bool>,
    // Les premiers résultats du fond (présents, parfois vides, si le fond a répondu).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<HitView>")]
    hits: Option<Vec<HitView>>,
    // `[code] message` : le fond a échoué, les autres non.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    error: Option<String>,
}

// Un résultat de recherche : la forme de ceux de `fr_search`.
#[derive(Debug, Serialize, JsonSchema)]
struct HitView {
    // Fond du résultat, en minuscules.
    kind: String,
    // Identifiant sans suffixe de version ; son préfixe désigne l'outil de lecture.
    id: String,
    // Titre, s'il n'est pas déjà dans `citation`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    nature: Option<String>,
    // État juridique, en code d'API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    etat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    idcc: Option<String>,
    // Extrait de 280 caractères au plus.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    snippet: Option<String>,
    // Articles trouvés (fonds `CODE` et `KALI`), lisibles avec `fr_get_text`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    articles: Vec<ArticleView>,
    citation: String,
    source_url: String,
}

// Un article trouvé dans un résultat de code.
#[derive(Debug, Serialize, JsonSchema)]
struct ArticleView {
    article_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    num: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    snippet: Option<String>,
}

impl From<SearchAllOutput> for SearchAllView {
    /// Vue de la surface annoncée, ramenée sous le plafond dur de 45 000 caractères.
    fn from(output: SearchAllOutput) -> Self {
        let mut view = Self::build(output, SURFACE);
        view.fit();
        view
    }
}

impl SearchAllView {
    /// Vue de `output` pour `surface`, sans mesure du plafond.
    fn build(output: SearchAllOutput, surface: Surface) -> Self {
        let additions = surface != Surface::V01;
        Self {
            groups: output
                .groups
                .into_iter()
                .map(|group| GroupView::build(group, additions))
                .collect(),
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

    /// Les résultats de tous les groupes, du dernier au premier.
    fn hits_from_the_end(&mut self) -> impl Iterator<Item = &mut HitView> {
        self.groups.iter_mut().rev().flat_map(|group| group.hits.iter_mut().flatten().rev())
    }

    /// Retire un peu de contenu, en partant du dernier résultat : d'abord les extraits (un
    /// résultat à la fois), puis les listes d'articles et les champs optionnels. Jamais un
    /// résultat. Vrai si quelque chose a été retiré.
    fn strip_one(&mut self) -> bool {
        self.hits_from_the_end().any(HitView::strip_snippets)
            || self.hits_from_the_end().any(HitView::strip_details)
    }
}

impl GroupView {
    fn build(group: GroupOut, additions: bool) -> Self {
        let fond = group.fond.name().to_owned();
        match group.result {
            Ok(answer) => Self::answered(fond, answer, additions),
            Err(error) => Self {
                fond,
                total: None,
                has_more: None,
                hits: None,
                error: Some(error_text(&error)),
            },
        }
    }

    fn answered(fond: String, answer: FondHits, additions: bool) -> Self {
        Self {
            fond,
            total: Some(answer.total),
            has_more: Some(answer.has_more),
            hits: Some(answer.hits.into_iter().map(|hit| HitView::build(hit, additions)).collect()),
            error: None,
        }
    }
}

/// `[code] message`, comme l'échec d'un outil ; un appel annulé n'a pas de code (le message seul).
fn error_text(error: &AppError) -> String {
    match error.code() {
        Some(code) => format!("[{}] {}", code.as_str(), error.message()),
        None => error.message().to_owned(),
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

    /// Retire l'extrait du résultat et ceux de ses articles ; vrai s'il y en avait un.
    fn strip_snippets(&mut self) -> bool {
        let mut stripped = self.snippet.take().is_some();
        for article in &mut self.articles {
            stripped |= article.snippet.take().is_some();
        }
        stripped
    }

    /// Retire la liste d'articles et les champs `title`, `date`, `nature` et `etat` ; vrai s'il y
    /// avait quelque chose à retirer.
    fn strip_details(&mut self) -> bool {
        let had_articles = !self.articles.is_empty();
        self.articles.clear();
        let had_fields = [
            self.title.take().is_some(),
            self.date.take().is_some(),
            self.nature.take().is_some(),
            self.etat.take().is_some(),
        ];
        had_articles || had_fields.contains(&true)
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
mod tests {
    use lf_application::{
        budget::ResponseBudget,
        error::{ErrorCode, UpstreamCause},
    };
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

    const CAP: usize = ResponseBudget::HARD_CAP_CHARS;

    fn hit(fond: Fond, n: u32) -> SearchHit {
        let id = DocumentId::parse(&format!("{}{n:012}", fond.hit_prefix())).unwrap();
        SearchHit::new(fond, id, None).unwrap()
    }

    /// Résultat dont la citation est donnée : l'application la calcule avant la vue.
    fn out(hit: SearchHit, citation: &str) -> HitOut {
        let url = format!("https://www.legifrance.gouv.fr/x/id/{}", hit.id.as_str());
        HitOut { hit, citation: citation.to_owned(), source_url: url }
    }

    fn found(fond: Fond, total: u64, has_more: bool, hits: Vec<HitOut>) -> GroupOut {
        GroupOut { fond, result: Ok(FondHits { total, has_more, hits }) }
    }

    fn failed(fond: Fond, error: AppError) -> GroupOut {
        GroupOut { fond, result: Err(error) }
    }

    fn output(groups: Vec<GroupOut>) -> SearchAllOutput {
        SearchAllOutput { groups, note: None }
    }

    fn json_of(output: SearchAllOutput) -> Value {
        serde_json::to_value(SearchAllView::from(output)).unwrap()
    }

    fn compact_len(view: &SearchAllView) -> usize {
        serde_json::to_string(view).unwrap().chars().count()
    }

    #[test]
    fn a_group_that_answered_has_total_has_more_and_hits() {
        let cetat = SearchHit {
            nature: Some("texte".to_owned()),
            snippet: Snippet::new("[...] Aux termes de l'article L. 600-4-1 [...]"),
            ..hit(Fond::Cetat, 1)
        };
        let view = json_of(output(vec![found(
            Fond::Cetat,
            51_531,
            true,
            vec![out(cetat, "Conseil d'État, 29/05/2026, 506507")],
        )]));
        assert_eq!(
            view,
            json!({"groups": [{
                "fond": "CETAT",
                "total": 51_531,
                "has_more": true,
                "hits": [{
                    "kind": "cetat",
                    "id": "CETATEXT000000000001",
                    "nature": "texte",
                    "snippet": "[...] Aux termes de l'article L. 600-4-1 [...]",
                    "citation": "Conseil d'État, 29/05/2026, 506507",
                    "source_url": "https://www.legifrance.gouv.fr/x/id/CETATEXT000000000001"
                }]
            }]})
        );
    }

    #[test]
    fn a_group_with_no_hit_keeps_an_empty_hit_list() {
        let view = json_of(output(vec![found(Fond::Juri, 0, false, vec![])]));
        assert_eq!(
            view,
            json!({"groups": [{"fond": "JURI", "total": 0, "has_more": false, "hits": []}]})
        );
    }

    #[test]
    fn a_failed_group_carries_only_its_error_with_the_code_prefix() {
        let timeout = AppError::Upstream {
            message: "Legifrance/PISTE did not answer in time".to_owned(),
            cause: UpstreamCause::Timeout,
        };
        let view =
            json_of(output(vec![found(Fond::Loda, 1, false, vec![]), failed(Fond::Juri, timeout)]));
        assert_eq!(
            view["groups"][1],
            json!({"fond": "JURI", "error": "[upstream_error] Legifrance/PISTE did not answer in time"})
        );
        assert_eq!(view["groups"][0]["fond"], "LODA", "l'ordre des fonds est conservé");
    }

    #[test]
    fn an_error_without_a_code_is_rendered_as_its_message() {
        let cancelled =
            AppError::Upstream { message: "cancelled".to_owned(), cause: UpstreamCause::Cancelled };
        assert_eq!(error_text(&cancelled), "cancelled");
        let invalid = AppError::invalid_argument("bad");
        assert_eq!(invalid.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(error_text(&invalid), "[invalid_arg] bad");
    }

    #[test]
    fn hits_follow_the_search_rendering_rules() {
        let code = SearchHit {
            title: Some("Code civil".to_owned()),
            date: CivilDate::new(2222, 2, 22),
            etat: Some(LegalStatus::Vigueur),
            articles: vec![CodeArticleRef::new(
                ArticleId::parse("LEGIARTI000006419288").unwrap(),
                Some("9".to_owned()),
                "Chacun a droit au respect de sa vie privée.",
            )],
            ..hit(Fond::Code, 1)
        };
        let view =
            json_of(output(vec![found(Fond::Code, 1, false, vec![out(code, "Code civil")])]));
        let first = &view["groups"][0]["hits"][0];
        assert!(first.get("title").is_none(), "le titre est déjà la citation");
        assert!(first.get("date").is_none(), "la sentinelle vaut absente");
        assert_eq!(first["etat"], "VIGUEUR");
        assert_eq!(
            first["articles"],
            json!([{
                "article_id": "LEGIARTI000006419288",
                "num": "9",
                "snippet": "Chacun a droit au respect de sa vie privée."
            }])
        );
    }

    #[test]
    fn idcc_is_an_addition_of_the_second_surface() {
        let kali = SearchHit { idcc: Idcc::parse("1486"), ..hit(Fond::Kali, 1) };
        let build = |surface| {
            let groups = vec![found(Fond::Kali, 1, false, vec![out(kali.clone(), "Convention")])];
            serde_json::to_value(SearchAllView::build(output(groups), surface)).unwrap()
        };
        assert!(build(Surface::V01)["groups"][0]["hits"][0].get("idcc").is_none());
        for surface in [Surface::V02, Surface::V03] {
            assert_eq!(build(surface)["groups"][0]["hits"][0]["idcc"], "1486");
        }
    }

    #[test]
    fn the_note_of_the_application_is_rendered() {
        let mut out = output(vec![]);
        out.note = Some("Snippets were removed.".to_owned());
        let view = json_of(out);
        assert_eq!(view, json!({"groups": [], "note": "Snippets were removed."}));
    }

    /// Dix résultats de 280 caractères d'extrait (plus un article de CODE) dans chacun de `fonds`.
    fn heavy(fonds: &[Fond], snippet: &str) -> SearchAllOutput {
        let groups = fonds
            .iter()
            .map(|&fond| {
                let hits = (1..=10)
                    .map(|n| {
                        let article = CodeArticleRef::new(
                            ArticleId::parse(&format!("LEGIARTI{n:012}")).unwrap(),
                            Some(n.to_string()),
                            snippet,
                        );
                        let heavy = SearchHit {
                            title: Some(format!("{} {n}", fond.name())),
                            nature: Some("TEXTE".to_owned()),
                            snippet: Snippet::new(snippet),
                            articles: vec![article],
                            ..hit(fond, n)
                        };
                        out(heavy, &format!("{} {n}", fond.name()))
                    })
                    .collect();
                found(fond, 500, true, hits)
            })
            .collect();
        output(groups)
    }

    #[test]
    fn a_response_over_the_hard_cap_loses_snippets_from_the_last_hit_back_and_says_so() {
        // Huit fonds de dix résultats, chacun avec deux extraits de 280 caractères : environ
        // 65 000 caractères de JSON, que seule la vue peut mesurer.
        let output = heavy(&Fond::ALL, &"mot ".repeat(70));
        let mut view = SearchAllView::build(output, Surface::V03);
        assert!(compact_len(&view) > CAP);
        view.fit();

        assert!(compact_len(&view) <= CAP);
        let hits: Vec<&HitView> =
            view.groups.iter().flat_map(|group| group.hits.iter().flatten()).collect();
        assert_eq!(hits.len(), 80, "aucun résultat n'est retiré");
        // En partant du dernier, sans trou : les premiers gardent leur extrait.
        let without: Vec<bool> = hits.iter().map(|hit| hit.snippet.is_none()).collect();
        let first_removed = without.iter().position(|gone| *gone).expect("some snippets go");
        assert!(first_removed > 0);
        assert!(without[first_removed..].iter().all(|gone| *gone));
        assert!(without[..first_removed].iter().all(|gone| !gone));
        assert_eq!(view.note.as_deref(), Some(FURTHER_NOTE));
    }

    #[test]
    fn titles_and_article_lists_go_once_the_snippets_are_gone() {
        // Pas d'extrait à retirer, mais des champs optionnels énormes.
        let mut output = heavy(&[Fond::Code], "x");
        for group in &mut output.groups {
            for hit in &mut group.result.as_mut().unwrap().hits {
                hit.hit.nature = Some("n".repeat(5_000));
                hit.hit.snippet = None;
                hit.hit.articles.clear();
            }
        }
        output.note = Some("D'abord.".to_owned());
        let mut view = SearchAllView::build(output, Surface::V03);
        assert!(compact_len(&view) > CAP);
        view.fit();
        assert!(compact_len(&view) <= CAP);
        let hits = view.groups[0].hits.as_ref().unwrap();
        assert_eq!(hits.len(), 10);
        assert!(hits[0].nature.is_some() && hits[9].nature.is_none());
        assert_eq!(view.note, Some(format!("D'abord. {FURTHER_NOTE}")));
    }

    #[test]
    fn a_view_that_cannot_be_reduced_is_sent_as_it_is() {
        // Ni extrait ni détail à retirer : la dégradation s'arrête, la réponse part telle quelle.
        let bare = hit(Fond::Juri, 1);
        let mut view = SearchAllView::build(
            output(vec![found(Fond::Juri, 1, false, vec![out(bare, &"c".repeat(50_000))])]),
            Surface::V03,
        );
        view.fit();
        assert!(compact_len(&view) > CAP);
        assert_eq!(view.note, None, "rien n'a été retiré, donc rien à dire");
    }

    #[test]
    fn strip_helpers_report_whether_they_removed_something() {
        let mut full = HitView::build(
            out(
                SearchHit {
                    title: Some("Un autre titre".to_owned()),
                    snippet: Snippet::new("extrait"),
                    ..hit(Fond::Juri, 1)
                },
                "citation",
            ),
            true,
        );
        assert!(full.strip_snippets());
        assert!(!full.strip_snippets());
        assert!(full.strip_details());
        assert!(!full.strip_details());
    }

    #[test]
    fn output_schema_snapshot() {
        let schema = Value::Object(output_schema::<SearchAllView>().as_ref().clone());
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"], json!(["groups"]));
        assert!(schema.get("$schema").is_none());
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("output_schema", schema);
        });
    }
}
