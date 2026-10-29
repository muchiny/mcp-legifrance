//! Plafond dur des réponses de `fr_verify_citations`, `fr_get_related` et `fr_resolve`, mesuré sur
//! le fil (T-M6-05, T-M6-06, 05 §6.4, 06 §12.2, D-33) : le pendant de `budgets.rs` pour les outils
//! de M6 et de M7 lus par citation. Un vrai client rmcp appelle chaque outil sur des données
//! synthétiques extrêmes, et la taille **exacte** du `structuredContent` (JSON compact, en
//! caractères) ne dépasse jamais 45 000 caractères, bloc texte compris.
//!
//! - `every_citation_tool_response_under_hard_cap` : les combinaisons extrêmes de 05 §6.4. Cent
//!   citations de 1 000 caractères riches en guillemets et en antislash, cent cinquante liens de
//!   500 caractères du même genre, cinq candidats de 1 000 caractères. Chaque cas vérifie d'abord
//!   qu'il est extrême (sans dégradation, sa réponse dépasserait le plafond), puis ce que 05 §6.4
//!   prescrit : ordre de dégradation (les listes se tronquent par la fin, `truncated` et `note` le
//!   disent), compteurs exacts et ordre du texte gardé. `case_report` fige ce qui reste de chaque
//!   cas.
//! - `every_citation_tool_is_registered` : les trois outils sont enregistrés, et aucun n'échappe au
//!   contrôle.
//!
//! Le harnais en processus n'existe qu'avec la feature `test-support` (`--all-features`). Tout le
//! fichier est sous `cfg(test)` : les aides partagent les règles de lint des tests (`unwrap` et
//! `expect` permis, `clippy.toml`).

// Le harnais en processus n'existe qu'avec la feature `test-support`.
#[cfg(test)]
#[cfg(feature = "test-support")]
mod budgets_m6 {
    use std::collections::BTreeSet;

    use lf_adapter_mcp::testing::Fixture;
    use lf_application::budget::ResponseBudget;
    use lf_domain::{
        contract::Citable,
        date::CivilDate,
        entity::{ArticleText, CodeListing, RelatedLink, RelatedLinks, RelatedNature, SearchHit},
        fond::Fond,
        id::{ArticleId, DocumentId, LegiTextId},
        status::LegalStatus,
        text::LongText,
    };
    use rmcp::model::{CallToolRequestParams, CallToolResult};
    use serde_json::{Value, json};

    const CAP: usize = ResponseBudget::HARD_CAP_CHARS;
    const TARGET: usize = ResponseBudget::ESTIMATE_TARGET_CHARS;

    /// Début de la note que l'application pose quand elle raccourcit la liste de `fr_verify_citations`.
    const VERIFY_NOTE: &str = "List shortened to";

    // ---- données synthétiques ------------------------------------------------------------------

    /// Un titre de `n` suivi de guillemets et d'antislash : 1 000 caractères une fois la citation
    /// coupée (`CitationText`), mais 2 000 caractères de JSON. Les contrôles et les blancs seraient
    /// retirés par le domaine, les guillemets et les antislash non.
    fn heavy(n: u32) -> String {
        format!("{n} {}", "\"\\".repeat(600))
    }

    /// Le titre d'un lien de `fr_get_related`, borné à 500 caractères (`CITATION_MAX_CHARS` de
    /// `citation/related.rs`) : sans coupe, pour que le cas pèse ce qu'il dit, soit 1 000 caractères
    /// de JSON.
    fn heavy_link(n: u32) -> String {
        format!("{n} {}", "\"\\".repeat(247))
    }

    /// Ce que pèserait le contenu extrême, sans aucune dégradation : caractères de son JSON compact.
    #[derive(Debug, Clone, Copy)]
    struct Raw {
        chars: usize,
    }

    impl Raw {
        /// Le coût des chaînes `parts` sérialisées en JSON (guillemets et échappements compris).
        fn of<'a>(parts: impl IntoIterator<Item = &'a str>) -> Self {
            let chars = parts
                .into_iter()
                .map(|part| serde_json::to_string(part).unwrap().chars().count())
                .sum();
            Self { chars }
        }
    }

    /// Le caractère extrême d'un cas, vérifié avant toute autre chose : un cas qui ne l'est pas ne
    /// prouverait rien.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Extreme {
        /// Sans dégradation, la réponse dépasserait le plafond (en caractères de JSON).
        OverTheCap,
        /// Les bornes de l'outil sont pleines, et la réponse tient d'elle-même sous le plafond.
        Ordinary,
    }

    // ---- appels et mesures ---------------------------------------------------------------------

    async fn call(fixture: &Fixture, tool: &str, arguments: &Value) -> CallToolResult {
        let client = fixture.connect().await;
        let mut params = CallToolRequestParams::new(tool.to_owned());
        params.arguments = arguments.as_object().cloned();
        client.call_tool(params).await.unwrap()
    }

    /// Ce qu'un appel a rendu : une réponse réussie, bloc texte identique au `structuredContent`.
    struct Observed {
        value: Value,
        /// Caractères du JSON compact de `value`.
        chars: usize,
    }

    impl Observed {
        fn of(result: &CallToolResult) -> Self {
            let text = &result.content[0].as_text().unwrap().text;
            assert_eq!(result.is_error, Some(false), "{text}");
            let value = result.structured_content.clone().expect("a structured content");
            // Le bloc texte porte le même JSON, sous le même plafond (parité des clients sans
            // `structuredContent`).
            assert_eq!(&serde_json::from_str::<Value>(text).unwrap(), &value);
            let chars = serde_json::to_string(&value).unwrap().chars().count();
            assert!(text.chars().count() <= CAP, "text block: {} characters", text.chars().count());
            Self { value, chars }
        }

        fn list(&self, field: &str) -> &Vec<Value> {
            self.value[field].as_array().expect("a list")
        }

        fn note(&self) -> &str {
            self.value["note"].as_str().unwrap_or("")
        }
    }

    // ---- les constructeurs d'entités -----------------------------------------------------------

    fn civil_code_listed(fixture: &Fixture) {
        fixture.corpus.push_list_codes(Ok(vec![CodeListing {
            id: LegiTextId::parse("LEGITEXT000006070721").unwrap(),
            title: "Code civil".to_owned(),
            etat: Some(LegalStatus::Vigueur),
        }]));
    }

    /// Un article dont la citation (le titre du code, faute de numéro) est `code_title`.
    fn article(n: u32, code_title: &str) -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(&format!("LEGIARTI{n:012}")).unwrap(),
            num: None,
            etat: Some(LegalStatus::Vigueur),
            code_title: Some(code_title.to_owned()),
            parent: None,
            path: vec![],
            full_sections_titre: None,
            date_debut: CivilDate::new(2016, 10, 1),
            date_fin: None,
            text: LongText::from_plain("Chacun a droit au respect de sa vie privée.").unwrap(),
            nota: None,
        }
    }

    /// Le texte de `fr_verify_citations` : `count` articles du code civil, dans l'ordre.
    fn citing_text(count: u32) -> String {
        (1..=count).map(|n| format!("article {n} du code civil")).collect::<Vec<_>>().join(" ; ")
    }

    /// Programme `count` articles trouvés, de citation `code_title(n)`, et rend leur coût brut.
    fn serve_articles(fixture: &Fixture, count: u32, code_title: impl Fn(u32) -> String) -> Raw {
        civil_code_listed(fixture);
        let articles: Vec<ArticleText> = (1..=count).map(|n| article(n, &code_title(n))).collect();
        let citations: Vec<String> = articles
            .iter()
            .map(|article| article.contract().citation.as_str().to_owned())
            .collect();
        for article in articles {
            fixture.corpus.push_find_article(Ok(article));
        }
        Raw::of(citations.iter().map(String::as_str))
    }

    const RELATED_ID: &str = "LEGIARTI000006419288";

    /// Un lien vers un article, de titre `title`.
    fn article_link(n: u32, title: &str) -> RelatedLink {
        RelatedLink {
            target: DocumentId::parse(&format!("LEGIARTI{n:012}")).ok(),
            nature: RelatedNature::Code,
            title: Some(title.to_owned()),
            date: None,
        }
    }

    /// Un lien vers le Journal officiel, sans cible lisible : ni `id` ni `source_url`.
    fn jorf_link(n: u32) -> RelatedLink {
        RelatedLink {
            target: None,
            nature: RelatedNature::JorfCont,
            title: Some(format!("Journal officiel, numéro {n}")),
            date: None,
        }
    }

    /// Programme l'article demandé et ses liens `cites` puis `cited_by`.
    fn serve_related(fixture: &Fixture, cites: Vec<RelatedLink>, cited_by: Vec<RelatedLink>) {
        fixture.corpus.push_get_article(Ok(article(1, "Code civil")));
        fixture.corpus.push_related_links(Ok(RelatedLinks { cites, cited_by }));
    }

    /// Un texte LODA par numéro de `1` à `count`, de titre `title(n)`.
    fn texts(count: u32, title: impl Fn(u32) -> String) -> Vec<SearchHit> {
        (1..=count)
            .map(|n| {
                let id = DocumentId::parse(&format!("LEGITEXT{n:012}")).unwrap();
                SearchHit { title: Some(title(n)), ..SearchHit::new(Fond::Loda, id, None).unwrap() }
            })
            .collect()
    }

    // ---- les cas -------------------------------------------------------------------------------

    /// Un appel extrême : la donnée programmée dans le corpus factice, l'appel, ce qu'on y attend.
    struct Case {
        name: &'static str,
        tool: &'static str,
        arguments: Value,
        extreme: Extreme,
        /// Programme le corpus et rend le coût brut du contenu extrême.
        setup: fn(&Fixture) -> Raw,
        /// Contrôles propres au cas ; rend ce qui reste de la réponse, pour `case_report`.
        check: fn(&Observed) -> Value,
    }

    /// Les entrées gardées d'une liste tronquée par la fin : les premières, dans l'ordre, sans trou.
    fn first_in_order(entries: &[Value], field: &str, expected: impl Fn(usize) -> String) {
        for (index, entry) in entries.iter().enumerate() {
            assert_eq!(entry[field], expected(index + 1), "entry {index}");
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "la table des combinaisons extrêmes de 05 §6.4, un cas par entrée"
    )]
    fn cases() -> Vec<Case> {
        vec![
            // ---- fr_verify_citations ----------------------------------------------------------
            Case {
                name: "verify_100_citations_with_1000_char_records",
                tool: "fr_verify_citations",
                arguments: json!({"text": citing_text(100), "max_citations": 100}),
                extreme: Extreme::OverTheCap,
                setup: |fixture| serve_articles(fixture, 100, heavy),
                check: |observed| {
                    let citations = observed.list("citations");
                    let kept = citations.len();
                    // L'estimation de l'application raccourcit la liste, puis la mesure exacte de
                    // la vue, qui compte les échappements, retire encore des citations par la fin.
                    assert!((1..100).contains(&kept), "{kept}");
                    assert_eq!(observed.value["found"], 100);
                    assert_eq!(observed.value["checked"], kept);
                    assert_eq!(observed.value["truncated"], true);
                    assert!(observed.note().starts_with(VERIFY_NOTE), "{}", observed.note());
                    first_in_order(citations, "raw", |n| format!("article {n} du code civil"));
                    assert!(citations.iter().all(|c| c["resolved"]["citation"].is_string()));
                    json!({"found": 100, "kept": kept})
                },
            },
            Case {
                name: "verify_50_plain_citations_by_default",
                tool: "fr_verify_citations",
                arguments: json!({"text": citing_text(50)}),
                extreme: Extreme::Ordinary,
                setup: |fixture| serve_articles(fixture, 50, |_| "Code civil".to_owned()),
                check: |observed| {
                    // Les cinquante citations du défaut tiennent d'elles-mêmes (05 §5.9) : ni
                    // troncature ni remarque.
                    let citations = observed.list("citations");
                    assert_eq!(citations.len(), 50);
                    assert_eq!(observed.value["found"], 50);
                    assert_eq!(observed.value["checked"], 50);
                    assert!(observed.value.get("truncated").is_none());
                    assert_eq!(observed.note(), "");
                    assert!(observed.chars <= TARGET, "{}", observed.chars);
                    json!({"found": 50, "kept": citations.len()})
                },
            },
            // ---- fr_get_related ---------------------------------------------------------------
            Case {
                name: "related_150_links_with_500_char_citations",
                tool: "fr_get_related",
                arguments: json!({"id": RELATED_ID}),
                extreme: Extreme::OverTheCap,
                setup: |fixture| {
                    let titles: Vec<String> = (1..=150).map(heavy_link).collect();
                    let links = |range: std::ops::Range<usize>| {
                        range
                            .map(|i| article_link(u32::try_from(i + 1).unwrap(), &titles[i]))
                            .collect::<Vec<_>>()
                    };
                    serve_related(fixture, links(0..75), links(75..150));
                    Raw::of(titles.iter().map(String::as_str))
                },
                check: |observed| {
                    let links = observed.list("links");
                    let kept = links.len();
                    // Cent liens au plus sans suite, et moins encore : à 500 caractères de citation
                    // riche en échappements, la liste est raccourcie par la fin.
                    assert!((1..100).contains(&kept), "{kept}");
                    assert_eq!(observed.value["total"], 150);
                    assert_eq!(observed.value["truncated"], true);
                    let first = links[0]["citation"].as_str().unwrap();
                    assert!(first.contains('"') && first.contains('\\'), "{first}");
                    assert!(first.chars().count() <= 500, "{}", first.chars().count());
                    // Les liens gardés sont les premiers ; ceux qu'il cite d'abord.
                    assert!(links.iter().all(|link| link["relation"] == "cites"), "{links:?}");
                    for (index, link) in links.iter().enumerate() {
                        let citation = link["citation"].as_str().unwrap();
                        assert!(citation.starts_with(&format!("{} ", index + 1)), "{index}");
                        assert!(link["id"].is_string() && link["source_url"].is_string());
                    }
                    json!({"total": 150, "kept": kept})
                },
            },
            Case {
                name: "related_120_short_links",
                tool: "fr_get_related",
                arguments: json!({"id": RELATED_ID}),
                extreme: Extreme::Ordinary,
                setup: |fixture| {
                    let cites = (1..=60).map(jorf_link).collect();
                    let cited_by = (61..=120).map(jorf_link).collect();
                    serve_related(fixture, cites, cited_by);
                    Raw::of(std::iter::empty::<&str>())
                },
                check: |observed| {
                    // Plus de cent liens : les cent premiers, sans suite, et `truncated` le dit.
                    let links = observed.list("links");
                    assert_eq!(links.len(), 100);
                    assert_eq!(observed.value["total"], 120);
                    assert_eq!(observed.value["truncated"], true);
                    assert_eq!(links[0]["citation"], "Journal officiel, numéro 1");
                    assert_eq!(links[99]["citation"], "Journal officiel, numéro 100");
                    assert_eq!(links[60]["relation"], "cited_by");
                    assert!(observed.chars <= TARGET, "{}", observed.chars);
                    json!({"total": 120, "kept": links.len()})
                },
            },
            // ---- fr_resolve -------------------------------------------------------------------
            Case {
                name: "resolve_5_candidates_with_1000_char_citations",
                tool: "fr_resolve",
                arguments: json!({"citation": "loi n° 2016-1321"}),
                extreme: Extreme::Ordinary,
                setup: |fixture| {
                    let hits = texts(8, heavy);
                    let titles: Vec<String> = hits
                        .iter()
                        .map(|hit| hit.contract().citation.as_str().to_owned())
                        .collect();
                    fixture.corpus.push_find_by_number(Ok(hits));
                    Raw::of(titles.iter().map(String::as_str))
                },
                check: |observed| {
                    // Huit textes de 1 000 caractères de citation riche en échappements : cinq
                    // candidats au plus, et la réponse reste très en deçà du plafond.
                    let candidates = observed.list("candidates");
                    assert_eq!(candidates.len(), 5);
                    first_in_order(candidates, "id", |n| format!("LEGITEXT{n:012}"));
                    assert!(candidates.iter().all(|c| c["confidence"] == "low"));
                    assert!(
                        candidates
                            .iter()
                            .all(|c| c["citation"].as_str().unwrap().chars().count() <= 1_000)
                    );
                    assert!(observed.chars <= 12_000, "{}", observed.chars);
                    json!({"candidates": candidates.len()})
                },
            },
        ]
    }

    // ---- les tests -----------------------------------------------------------------------------

    /// Régression : D-33 (T-M6-05, T-M6-06). Chaque combinaison extrême tient sous 45 000
    /// caractères, listes tronquées par la fin et dites telles.
    #[tokio::test]
    async fn every_citation_tool_response_under_hard_cap() {
        let mut covered = BTreeSet::new();
        let mut report = Vec::new();
        for case in cases() {
            let fixture = Fixture::default();
            let raw = (case.setup)(&fixture);
            if case.extreme == Extreme::OverTheCap {
                assert!(
                    raw.chars > CAP,
                    "{}: the case is not extreme ({} characters)",
                    case.name,
                    raw.chars
                );
            }

            let result = call(&fixture, case.tool, &case.arguments).await;
            let observed = Observed::of(&result);
            assert!(observed.chars <= CAP, "{}: {} characters", case.name, observed.chars);
            let kept = (case.check)(&observed);

            covered.insert(case.tool);
            report.push(json!({
                "case": case.name,
                "tool": case.tool,
                "under_estimation_target": observed.chars <= TARGET,
                "kept": kept,
            }));
        }

        assert_eq!(
            covered,
            BTreeSet::from(["fr_get_related", "fr_resolve", "fr_verify_citations"])
        );
        insta::assert_json_snapshot!("case_report", report);
    }

    /// Les trois outils de citation sont enregistrés : aucun n'échappe au contrôle de plafond.
    #[tokio::test]
    async fn every_citation_tool_is_registered() {
        let client = Fixture::default().connect().await;
        let registered: BTreeSet<String> =
            client.list_all_tools().await.unwrap().iter().map(|t| t.name.to_string()).collect();
        for name in ["fr_verify_citations", "fr_get_related", "fr_resolve"] {
            assert!(registered.contains(name), "{name} is not registered");
        }
    }
}
