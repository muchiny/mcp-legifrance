//! Plafond dur des réponses, mesuré sur le fil (T-M5-04, 05 §6.4, 06 §12.2, D-33) : un vrai
//! client rmcp appelle chaque outil de la v0.1 et du M7 sur des données synthétiques extrêmes, et
//! la taille **exacte** du `structuredContent` (JSON compact, en caractères) ne dépasse jamais
//! 45 000 caractères, bloc texte compris.
//!
//! - `every_tool_response_under_hard_cap` : les combinaisons extrêmes de 05 §6.4 (100 résultats
//!   avec extraits, 100 résultats à citations de 1 000 caractères, 8 fonds de 10 résultats, 500
//!   entrées de table des matières à titres longs, fenêtre de 40 000 caractères riche en
//!   échappements JSON, texte hors BMP). Chaque cas vérifie d'abord qu'il est extrême (sans
//!   dégradation, sa réponse dépasserait le plafond, ou ses octets le dépasseraient alors que ses
//!   caractères non), puis ce que 05 §6.4 prescrit : ordre de dégradation, `note`, `next_*` exacts
//!   et aucun résultat perdu. `case_report` fige ce qui reste de chaque cas.
//! - `a_refusal_names_a_page_size_that_fits`, `escape_rich_text_pages_partition_the_text` et
//!   `toc_pages_partition_the_toc` : les suites des dégradations (la `page_size` proposée tient ; les
//!   pages d'une lecture et d'une table des matières se partitionnent sans perte).
//! - `descriptor_size_budget` : le coût fixe de `tools/list` (06 §12.2).
//!
//! Le harnais en processus n'existe qu'avec la feature `test-support` (`--all-features`). Tout le
//! fichier est sous `cfg(test)` : les aides partagent les règles de lint des tests (`unwrap` et
//! `expect` permis, `clippy.toml`).

#![expect(
    clippy::arithmetic_side_effects,
    reason = "tests : valeurs bornées par les données du test"
)]
#![expect(
    clippy::as_conversions,
    reason = "tests : tailles et indices bornés par les données du test"
)]

// Le harnais en processus n'existe qu'avec la feature `test-support`.
#[cfg(test)]
#[cfg(feature = "test-support")]
mod budgets {
    use std::{collections::BTreeSet, fmt::Write as _};

    use lf_adapter_mcp::testing::Fixture;
    use lf_application::budget::ResponseBudget;
    use lf_domain::{
        contract::Idcc,
        date::CivilDate,
        entity::{
            Act, ArticleText, ArticleVersion, CodeArticleRef, CompanyAgreement, Convention,
            Decision, Deliberation, OfficialSummary, ParentText, SearchHit, SearchPage,
            SectionDraft, Snippet, TextBody, Toc, TocDraft,
        },
        fond::{Fond, PaginationMode},
        id::{
            AccoTextId, ArticleId, CnilTextId, DecisionId, DocumentId, KaliTextId, LegiArtiId,
            LegiTextId,
        },
        status::LegalStatus,
        text::LongText,
    };
    use rmcp::model::{CallToolRequestParams, CallToolResult};
    use serde_json::{Value, json};

    const CAP: usize = ResponseBudget::HARD_CAP_CHARS;
    const TARGET: usize = ResponseBudget::ESTIMATE_TARGET_CHARS;

    /// Note de repli de la vue, quand l'estimation de l'application a été prise en défaut
    /// (`views/search.rs`).
    const FURTHER_NOTE: &str = "Further snippets and details were removed";

    // ---- données synthétiques ------------------------------------------------------------------

    /// Sept caractères qui coûtent treize caractères de JSON : trois guillemets et trois antislash
    /// (deux chacun), et un émoji hors BMP (un seul caractère, mais quatre octets en UTF-8).
    const NASTY: &str = "\"\\\"\\\"\\\u{1F600}";

    /// Un texte riche en échappements de `chars` caractères exactement (un seul mot).
    fn nasty(chars: usize) -> String {
        NASTY.chars().cycle().take(chars).collect()
    }

    /// `paragraphs` paragraphes de [`NASTY`] : 8 caractères canoniques pour 15 de JSON.
    fn nasty_paragraphs(paragraphs: usize) -> String {
        vec![NASTY; paragraphs].join("\n")
    }

    /// `paragraphs` paragraphes de 99 émojis hors BMP : 100 caractères pour 400 octets.
    fn emoji_paragraphs(paragraphs: usize) -> String {
        vec!["\u{1F600}".repeat(99); paragraphs].join("\n")
    }

    /// `sentences` phrases de 74 caractères, en un seul paragraphe.
    fn prose(sentences: usize) -> String {
        "Considérant que l'article L. 1-1 du code civil prévoit une règle précise. "
            .repeat(sentences)
    }

    /// Un titre long et sans échappement, d'environ 340 caractères.
    fn long_title(n: u32) -> String {
        let body = "Des dispositions générales relatives à l'organisation du service public.";
        format!("Titre {n} - {}", [body; 5].join(" "))
    }

    /// Ce que pèserait le contenu extrême, sans aucune dégradation : caractères et octets de son
    /// JSON compact.
    #[derive(Debug, Clone, Copy)]
    struct Raw {
        chars: usize,
        bytes: usize,
    }

    impl Raw {
        /// Le coût des chaînes `parts` sérialisées en JSON (guillemets et échappements compris).
        fn of<'a>(parts: impl IntoIterator<Item = &'a str>) -> Self {
            let mut raw = Self { chars: 0, bytes: 0 };
            for part in parts {
                let json = serde_json::to_string(part).unwrap();
                raw.chars += json.chars().count();
                raw.bytes += json.len();
            }
            raw
        }

        /// Plus le JSON fixe de `hits` résultats (clés, `kind`, `id`, `date`, `nature`, `etat`,
        /// `source_url`).
        fn with_hits(self, hits: usize) -> Self {
            self.with_each(hits, HIT_FIXED_CHARS)
        }

        /// Plus `count` fois `each` caractères de JSON fixe.
        fn with_each(self, count: usize, each: usize) -> Self {
            let fixed = count * each;
            Self { chars: self.chars + fixed, bytes: self.bytes + fixed }
        }

        fn plus(self, other: Self) -> Self {
            Self { chars: self.chars + other.chars, bytes: self.bytes + other.bytes }
        }
    }

    /// JSON d'un résultat de recherche hors citation et extrait, arrondi bas (environ 200).
    const HIT_FIXED_CHARS: usize = 190;

    /// Le caractère extrême d'un cas, vérifié avant toute autre chose : un cas qui ne l'est pas ne
    /// prouverait rien.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Extreme {
        /// Sans dégradation, la réponse dépasserait le plafond (en caractères de JSON).
        OverTheCap,
        /// Sous le plafond en caractères, au-dessus en octets : la mesure compte des caractères.
        OverTheCapInBytesOnly,
        /// Un appel ordinaire du harnais (couverture) : rien à prouver sur l'extrême.
        Ordinary,
    }

    // ---- appels et mesures ---------------------------------------------------------------------

    async fn call(fixture: &Fixture, tool: &str, arguments: &Value) -> CallToolResult {
        let client = fixture.connect().await;
        let mut params = CallToolRequestParams::new(tool.to_owned());
        params.arguments = arguments.as_object().cloned();
        client.call_tool(params).await.unwrap()
    }

    fn compact_chars(value: &Value) -> usize {
        serde_json::to_string(value).unwrap().chars().count()
    }

    /// Ce qu'un appel a rendu.
    struct Observed {
        /// Le message d'une erreur en bande (`[code] message`).
        error: Option<String>,
        /// Le `structuredContent` d'une réponse réussie.
        value: Value,
        /// Caractères du JSON compact de `value` (0 pour une erreur).
        chars: usize,
    }

    impl Observed {
        fn of(result: &CallToolResult) -> Self {
            let text = &result.content[0].as_text().unwrap().text;
            if result.is_error == Some(true) {
                assert!(result.structured_content.is_none(), "{text}");
                assert!(text.starts_with('['), "{text}");
                return Self { error: Some(text.clone()), value: Value::Null, chars: 0 };
            }
            let value = result.structured_content.clone().expect("a structured content");
            // Le bloc texte porte le même JSON, sous le même plafond (parité des clients sans
            // `structuredContent`).
            assert_eq!(&serde_json::from_str::<Value>(text).unwrap(), &value);
            let chars = compact_chars(&value);
            assert!(text.chars().count() <= CAP, "text block: {} characters", text.chars().count());
            Self { error: None, value, chars }
        }

        fn note(&self) -> &str {
            self.value["note"].as_str().unwrap_or("")
        }
    }

    // ---- les constructeurs d'entités -----------------------------------------------------------

    fn hit(fond: Fond, n: u32, title: &str, snippet: &str) -> SearchHit {
        let id = DocumentId::parse(&format!("{}{n:012}", fond.hit_prefix())).unwrap();
        SearchHit {
            title: Some(title.to_owned()),
            date: CivilDate::new(2024, 1, 2),
            nature: Some("ARRET".to_owned()),
            snippet: Snippet::new(snippet),
            ..SearchHit::new(fond, id, None).unwrap()
        }
    }

    fn page(fond: Fond, hits: Vec<SearchHit>) -> SearchPage {
        SearchPage::new(fond, "bail", Some(5_000), None, hits, 0, PaginationMode::Defaut)
    }

    /// Cent résultats JURI : leurs titres (`title(n)`) et leurs extraits de 280 caractères.
    fn juri_page(hits: u32, title: impl Fn(u32) -> String, snippet: &str) -> (SearchPage, Raw) {
        let hits: Vec<SearchHit> =
            (1..=hits).map(|n| hit(Fond::Juri, n, &title(n), snippet)).collect();
        let raw = Raw::of(hits.iter().flat_map(|hit| {
            [hit.title.as_deref().unwrap_or(""), hit.snippet.as_ref().map_or("", Snippet::as_str)]
        }))
        .with_hits(hits.len());
        (page(Fond::Juri, hits), raw)
    }

    const ARTICLE_ID: &str = "LEGIARTI000006419288";

    fn article(text: &str) -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(ARTICLE_ID).unwrap(),
            num: Some("9".to_owned()),
            etat: LegalStatus::parse("VIGUEUR"),
            code_title: Some("Code civil".to_owned()),
            parent: Some(ParentText::new(
                DocumentId::parse("LEGITEXT000006070721").unwrap(),
                Some("CODE"),
            )),
            path: vec!["Livre Ier : Des personnes".to_owned()],
            full_sections_titre: None,
            date_debut: CivilDate::new(1970, 7, 19),
            date_fin: None,
            text: LongText::from_plain(text).unwrap(),
            nota: None,
        }
    }

    const DECISION_ID: &str = "CONSTEXT000052555275";

    fn decision(text: &str, summary: OfficialSummary) -> Decision {
        Decision {
            decision_id: DecisionId::parse(DECISION_ID).unwrap(),
            ecli: None,
            num: Some("2025-1173".to_owned()),
            juridiction: Some("Conseil constitutionnel".to_owned()),
            formation: None,
            solution: Some("Conformité".to_owned()),
            numero_affaire: Vec::new(),
            date_texte: CivilDate::new(2025, 11, 7),
            nature: Some("QPC".to_owned()),
            title: Some("Décision 2025-1173 QPC - 07 novembre 2025".to_owned()),
            text: LongText::from_plain(text).unwrap(),
            official_summary: summary,
            cited_case_law: None,
            applied_texts: vec![],
        }
    }

    const DELIBERATION_ID: &str = "CNILTEXT000053352643";

    fn deliberation(text: &str) -> Deliberation {
        Deliberation {
            deliberation_id: CnilTextId::parse(DELIBERATION_ID).unwrap(),
            num: Some("SAN-2026-002".to_owned()),
            nature: Some("Délibération".to_owned()),
            nature_delib: Some("Sanction".to_owned()),
            etat: LegalStatus::parse("VIGUEUR"),
            date_texte: CivilDate::new(2026, 1, 8),
            date_publication: CivilDate::new(2026, 1, 14),
            title: Some("Délibération SAN-2026-002 du 8 janvier 2026".to_owned()),
            title_long: None,
            text: LongText::from_plain(text).unwrap(),
        }
    }

    const ACT_ID: &str = "LEGITEXT000033205014";
    const CONVENTION_ID: &str = "KALITEXT000049878950";

    /// `entries` sections de table des matières, de titres `title(n)` ; rend aussi leur coût brut.
    fn toc(prefix: &str, entries: u32, title: impl Fn(u32) -> String) -> (Toc, Raw) {
        let titles: Vec<String> = (1..=entries).map(title).collect();
        let sections = titles
            .iter()
            .zip(1..=entries)
            .map(|(title, n)| SectionDraft {
                id: Some(format!("{prefix}{n:012}")),
                title: Some(title.clone()),
                ..SectionDraft::default()
            })
            .collect();
        let toc = Toc::build(TocDraft { sections, articles: Vec::new() }).unwrap();
        (toc, Raw::of(titles.iter().map(String::as_str)))
    }

    fn act(toc: Toc) -> Act {
        Act {
            text_id: LegiTextId::parse(ACT_ID).unwrap(),
            body: TextBody {
                title: Some("LOI n° 2016-1321 du 7 octobre 2016".to_owned()),
                nature: Some("LOI".to_owned()),
                etat: LegalStatus::parse("VIGUEUR"),
                toc,
                ..TextBody::default()
            },
        }
    }

    fn convention(toc: Toc) -> Convention {
        Convention {
            text_id: KaliTextId::parse(CONVENTION_ID).unwrap(),
            body: TextBody {
                title: Some("Annexe IX - Stipulations spécifiques".to_owned()),
                etat: LegalStatus::parse("VIGUEUR_ETEN"),
                toc,
                ..TextBody::default()
            },
            idcc: Idcc::parse("2121"),
            container_title: Some("Convention collective nationale de l'édition".to_owned()),
            container_id: Some("KALICONT000005635096".to_owned()),
            bulletin: Some("BO n°2024-20".to_owned()),
        }
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

    /// Les cent résultats gardent un identifiant et une citation : 05 §6.4, « jamais un résultat ».
    fn hits_of(observed: &Observed) -> &Vec<Value> {
        observed.value["hits"].as_array().expect("a hits list")
    }

    fn hits_with_snippet(hits: &[Value]) -> usize {
        hits.iter().filter(|hit| hit.get("snippet").is_some()).count()
    }

    /// Les extraits retirés le sont en partant du dernier résultat, sans trou (05 §6.4, point 2).
    fn snippets_go_from_the_end(hits: &[Value]) {
        let kept = hits_with_snippet(hits);
        assert!(hits[..kept].iter().all(|hit| hit.get("snippet").is_some()));
        assert!(hits[kept..].iter().all(|hit| hit.get("snippet").is_none()));
    }

    fn arguments_of_search(fond: &str) -> Value {
        json!({"query": "bail", "fond": fond, "page_size": 100})
    }

    #[expect(
        clippy::too_many_lines,
        reason = "la table des combinaisons extrêmes de 05 §6.4, un cas par entrée"
    )]
    fn cases() -> Vec<Case> {
        vec![
            // ---- fr_search --------------------------------------------------------------------
            Case {
                name: "search_100_hits_with_snippets",
                tool: "fr_search",
                arguments: arguments_of_search("JURI"),
                extreme: Extreme::OverTheCap,
                setup: |fixture| {
                    let (page, raw) = juri_page(
                        100,
                        |n| format!("Cour de cassation, chambre civile 1, {n}"),
                        &"mot ".repeat(70),
                    );
                    fixture.corpus.push_search(Ok(page));
                    raw
                },
                check: |observed| {
                    let hits = hits_of(observed);
                    assert_eq!(hits.len(), 100, "aucun résultat n'est retiré");
                    snippets_go_from_the_end(hits);
                    let kept = hits_with_snippet(hits);
                    // L'estimation compte le JSON de la vue : les derniers extraits partent, les
                    // premiers survivent. Ce que la note annonce est exactement ce qui a été retiré.
                    assert!(kept < 100, "{kept}");
                    assert!(
                        observed.note().starts_with(&format!(
                            "Snippets were removed from the last {} hits",
                            100 - kept
                        )),
                        "{}",
                        observed.note()
                    );
                    assert!(!observed.note().contains(FURTHER_NOTE), "{}", observed.note());
                    assert!(observed.chars <= TARGET + 3_000, "{}", observed.chars);
                    json!({"hits": hits.len(), "snippets_kept": kept})
                },
            },
            Case {
                name: "search_100_hits_with_1000_char_citations",
                tool: "fr_search",
                arguments: arguments_of_search("JURI"),
                extreme: Extreme::OverTheCap,
                setup: |fixture| {
                    let (page, raw) =
                        juri_page(100, |n| format!("{n} {}", "x".repeat(997)), &"mot ".repeat(70));
                    fixture.corpus.push_search(Ok(page));
                    raw
                },
                check: |observed| {
                    // 100 citations de 1 000 caractères ne tiennent pas : la page est refusée avec la
                    // `page_size` qui tient (05 §6.4, point 4), jamais tronquée en silence.
                    let message = observed.error.as_deref().expect("a refusal");
                    assert!(message.starts_with("[invalid_arg] page_size=100 would exceed"));
                    let suggested = suggested_page_size(message);
                    assert!((1..100).contains(&suggested), "{message}");
                    json!({"refused": true, "suggested_page_size": suggested})
                },
            },
            Case {
                name: "search_100_hits_with_escape_rich_citations",
                tool: "fr_search",
                arguments: arguments_of_search("JURI"),
                extreme: Extreme::OverTheCap,
                setup: |fixture| {
                    let (page, raw) = juri_page(100, |_| nasty(300), &nasty(280));
                    fixture.corpus.push_search(Ok(page));
                    raw
                },
                check: |observed| {
                    // La longueur écrite en JSON (guillemets et antislash doublés) est comptée
                    // exactement : la page est refusée, avec la `page_size` qui tient, avant toute
                    // mesure de la vue.
                    let message = observed.error.as_deref().expect("a refusal");
                    assert!(message.starts_with("[invalid_arg] page_size=100 would exceed"));
                    let suggested = suggested_page_size(message);
                    assert!((1..100).contains(&suggested), "{message}");
                    json!({"refused": true, "suggested_page_size": suggested})
                },
            },
            Case {
                name: "search_100_hits_with_escape_rich_citations_that_fit",
                tool: "fr_search",
                arguments: arguments_of_search("JURI"),
                extreme: Extreme::OverTheCap,
                setup: |fixture| {
                    let (page, raw) = juri_page(100, |_| nasty(150), &nasty(280));
                    fixture.corpus.push_search(Ok(page));
                    raw
                },
                check: |observed| {
                    // Le titre est dans la citation, donc omis par la vue et non compté : les cent
                    // résultats tiennent, sans extrait ; l'estimation exacte n'a pas besoin du
                    // repli de la vue.
                    let hits = hits_of(observed);
                    assert_eq!(hits.len(), 100);
                    snippets_go_from_the_end(hits);
                    assert!(!observed.note().contains(FURTHER_NOTE), "{}", observed.note());
                    assert!(observed.chars <= TARGET, "{}", observed.chars);
                    json!({"hits": hits.len(), "snippets_kept": hits_with_snippet(hits)})
                },
            },
            Case {
                name: "search_30_hits_with_non_bmp_citations",
                tool: "fr_search",
                arguments: json!({"query": "bail", "fond": "JURI", "page_size": 30}),
                extreme: Extreme::OverTheCapInBytesOnly,
                setup: |fixture| {
                    let (page, raw) = juri_page(30, |_| "\u{1F600}".repeat(380), "");
                    fixture.corpus.push_search(Ok(page));
                    raw
                },
                check: |observed| {
                    // 30 × 380 émojis : 45 600 octets de citations pour 11 400 caractères. La mesure
                    // compte des caractères : rien n'est retiré.
                    let hits = hits_of(observed);
                    assert_eq!(hits.len(), 30);
                    assert!(observed.value.get("note").is_none(), "{}", observed.note());
                    let bytes = serde_json::to_string(&observed.value).unwrap().len();
                    assert!(observed.chars <= CAP && bytes > CAP, "{} {bytes}", observed.chars);
                    json!({"hits": hits.len(), "note": false})
                },
            },
            Case {
                name: "search_code_100_hits_with_article_lists",
                tool: "fr_search",
                arguments: arguments_of_search("CODE"),
                extreme: Extreme::OverTheCap,
                setup: |fixture| {
                    let hits: Vec<SearchHit> = (1..=100)
                        .map(|n| {
                            let articles = (1..=10)
                                .map(|k| {
                                    CodeArticleRef::new(
                                        ArticleId::parse(&format!("LEGIARTI{:012}", n * 100 + k))
                                            .unwrap(),
                                        Some(format!("L. {n}-{k}")),
                                        &"mot ".repeat(70),
                                    )
                                })
                                .collect();
                            SearchHit {
                                articles,
                                nature: Some("CODE".to_owned()),
                                ..hit(Fond::Code, n, &format!("Code du travail {n}"), "")
                            }
                        })
                        .collect();
                    let raw = Raw::of(hits.iter().flat_map(|hit| {
                        hit.articles.iter().map(|a| a.snippet.as_ref().map_or("", Snippet::as_str))
                    }))
                    .with_hits(hits.len());
                    fixture.corpus.push_search(Ok(page(Fond::Code, hits)));
                    raw
                },
                check: |observed| {
                    // Les extraits des articles (280 000 caractères) sont retirés d'abord, puis les
                    // listes d'articles elles-mêmes (05 §6.4) : la page tient alors, et la note dit
                    // les deux retraits. Aucun résultat n'est perdu.
                    assert!(observed.error.is_none(), "{:?}", observed.error);
                    let hits = hits_of(observed);
                    assert_eq!(hits.len(), 100);
                    let lists = hits.iter().filter(|hit| hit.get("articles").is_some()).count();
                    // Les listes retirées le sont en partant du dernier résultat, sans trou.
                    assert!(hits[..lists].iter().all(|hit| hit.get("articles").is_some()));
                    assert!(hits[lists..].iter().all(|hit| hit.get("articles").is_none()));
                    let articles = hits[0]["articles"].as_array().expect("the first list is kept");
                    assert!(articles.iter().all(|article| article.get("snippet").is_none()));
                    let note = observed.note();
                    assert!(
                        note.starts_with("Snippets were removed from the last 100 hits"),
                        "{note}"
                    );
                    let removed = format!("removed from the last {} hits", 100 - lists);
                    assert!(note.contains(&removed), "{note}");
                    json!({"hits": hits.len(), "snippets_kept": 0, "article_lists_kept": lists})
                },
            },
            // ---- fr_search_all ----------------------------------------------------------------
            Case {
                name: "search_all_8_fonds_x_10_hits",
                tool: "fr_search_all",
                arguments: json!({
                    "query": "bail",
                    "fonds": ["LODA", "CODE", "JURI", "CONSTIT", "CETAT", "CNIL", "KALI", "ACCO"],
                    "per_fond": 10
                }),
                extreme: Extreme::OverTheCap,
                setup: |fixture| {
                    let mut raw = Raw { chars: 0, bytes: 0 };
                    for fond in Fond::ALL {
                        let hits: Vec<SearchHit> = (1..=10)
                            .map(|n| hit(fond, n, &long_title(n), &"mot ".repeat(70)))
                            .collect();
                        let part = Raw::of(hits.iter().flat_map(|hit| {
                            [
                                hit.title.as_deref().unwrap_or(""),
                                hit.snippet.as_ref().map_or("", Snippet::as_str),
                            ]
                        }))
                        .with_hits(hits.len());
                        raw = raw.plus(part);
                        fixture.corpus.push_search(Ok(page(fond, hits)));
                    }
                    raw
                },
                check: search_all_groups,
            },
            Case {
                name: "search_all_8_fonds_x_10_hits_escape_rich",
                tool: "fr_search_all",
                arguments: json!({
                    "query": "bail",
                    "fonds": ["LODA", "CODE", "JURI", "CONSTIT", "CETAT", "CNIL", "KALI", "ACCO"],
                    "per_fond": 10
                }),
                extreme: Extreme::OverTheCap,
                setup: |fixture| {
                    let mut raw = Raw { chars: 0, bytes: 0 };
                    for fond in Fond::ALL {
                        let hits: Vec<SearchHit> =
                            (1..=10).map(|n| hit(fond, n, &nasty(150), &nasty(280))).collect();
                        let part = Raw::of(hits.iter().flat_map(|hit| {
                            [
                                hit.title.as_deref().unwrap_or(""),
                                hit.snippet.as_ref().map_or("", Snippet::as_str),
                            ]
                        }))
                        .with_hits(hits.len());
                        raw = raw.plus(part);
                        fixture.corpus.push_search(Ok(page(fond, hits)));
                    }
                    raw
                },
                check: search_all_groups,
            },
            // ---- fr_get_act et fr_get_convention ----------------------------------------------
            Case {
                name: "act_500_toc_entries_with_long_titles",
                tool: "fr_get_act",
                arguments: json!({"text_id": ACT_ID, "toc_limit": 500}),
                extreme: Extreme::OverTheCap,
                setup: |fixture| {
                    let (toc, raw) = toc("LEGISCTA", 500, long_title);
                    fixture.corpus.push_get_act(Ok(act(toc)));
                    raw
                },
                check: |observed| toc_page(observed, 500),
            },
            Case {
                name: "act_500_toc_entries_with_escape_rich_titles",
                tool: "fr_get_act",
                arguments: json!({"text_id": ACT_ID, "toc_limit": 500}),
                extreme: Extreme::OverTheCap,
                setup: |fixture| {
                    let (toc, raw) = toc("LEGISCTA", 500, |_| nasty(300));
                    fixture.corpus.push_get_act(Ok(act(toc)));
                    raw
                },
                check: |observed| toc_page(observed, 500),
            },
            Case {
                name: "convention_500_toc_entries_with_long_titles",
                tool: "fr_get_convention",
                arguments: json!({"convention_id": CONVENTION_ID, "toc_limit": 500}),
                extreme: Extreme::OverTheCap,
                setup: |fixture| {
                    let (toc, raw) = toc("KALISCTA", 500, long_title);
                    fixture.corpus.push_get_convention(Ok(convention(toc)));
                    raw
                },
                check: |observed| toc_page(observed, 500),
            },
            Case {
                name: "convention_500_toc_entries_with_escape_rich_titles",
                tool: "fr_get_convention",
                arguments: json!({"convention_id": CONVENTION_ID, "toc_limit": 500}),
                extreme: Extreme::OverTheCap,
                setup: |fixture| {
                    let (toc, raw) = toc("KALISCTA", 500, |_| nasty(300));
                    fixture.corpus.push_get_convention(Ok(convention(toc)));
                    raw
                },
                check: |observed| toc_page(observed, 500),
            },
            // ---- fr_get_text ------------------------------------------------------------------
            Case {
                name: "text_40000_char_escape_rich_window",
                tool: "fr_get_text",
                arguments: json!({"article_id": ARTICLE_ID, "detail": "full", "max_chars": 40_000}),
                extreme: Extreme::OverTheCap,
                setup: |fixture| {
                    let text = nasty_paragraphs(6_500);
                    let raw = Raw::of([window_of(&text, 40_000).as_str()]);
                    fixture.corpus.push_get_article(Ok(article(&text)));
                    raw
                },
                check: reading_window,
            },
            Case {
                name: "text_40000_char_non_bmp_window",
                tool: "fr_get_text",
                arguments: json!({"article_id": ARTICLE_ID, "detail": "full", "max_chars": 40_000}),
                extreme: Extreme::OverTheCapInBytesOnly,
                setup: |fixture| {
                    let text = emoji_paragraphs(600);
                    let raw = Raw::of([window_of(&text, 40_000).as_str()]);
                    fixture.corpus.push_get_article(Ok(article(&text)));
                    raw
                },
                check: |observed| {
                    // 40 000 émojis : 160 000 octets, mais 40 000 caractères : la fenêtre demandée
                    // tient entière, sans note de dégradation.
                    let text = observed.value["text"].as_str().unwrap();
                    assert!(text.chars().count() >= 20_000, "{}", text.chars().count());
                    assert!(observed.value.get("note").is_none());
                    json!({"window_chars": text.chars().count()})
                },
            },
            Case {
                name: "text_40000_char_prose_window",
                tool: "fr_get_text",
                arguments: json!({"article_id": ARTICLE_ID, "detail": "full", "max_chars": 40_000}),
                extreme: Extreme::Ordinary,
                setup: |fixture| {
                    fixture.corpus.push_get_article(Ok(article(&prose(600))));
                    Raw { chars: 0, bytes: 0 }
                },
                check: |observed| {
                    let text = observed.value["text"].as_str().unwrap();
                    assert!(text.chars().count() >= 20_000, "{}", text.chars().count());
                    json!({"window_chars_at_least_half_of_max_chars": true})
                },
            },
            Case {
                name: "text_focus_escape_rich_passages",
                tool: "fr_get_text",
                arguments: json!({
                    "article_id": ARTICLE_ID,
                    "focus": "règle",
                    "passages": 10,
                    "max_chars": 40_000
                }),
                extreme: Extreme::OverTheCap,
                setup: |fixture| {
                    // Dix paragraphes qui contiennent le mot cherché, de 5 000 caractères chacun.
                    let paragraph = format!("règle {}", nasty(4_994));
                    let text = vec![paragraph; 10].join("\n");
                    let raw = Raw::of([text.as_str()]);
                    fixture.corpus.push_get_article(Ok(article(&text)));
                    raw
                },
                check: |observed| {
                    let passages = observed.value["passages"].as_array().expect("passages");
                    assert!(!passages.is_empty() && passages.len() <= 10);
                    json!({"passages": passages.len()})
                },
            },
            // ---- fr_get_decision et fr_get_deliberation ---------------------------------------
            Case {
                name: "decision_40000_char_escape_rich_window",
                tool: "fr_get_decision",
                arguments: json!({"decision_id": DECISION_ID, "detail": "full", "max_chars": 40_000}),
                extreme: Extreme::OverTheCap,
                setup: |fixture| {
                    let text = nasty_paragraphs(6_500);
                    let raw = Raw::of([window_of(&text, 40_000).as_str()]);
                    fixture
                        .corpus
                        .push_get_decision(Ok(decision(&text, OfficialSummary::default())));
                    raw
                },
                check: reading_window,
            },
            Case {
                name: "decision_with_a_huge_official_summary",
                tool: "fr_get_decision",
                arguments: json!({"decision_id": DECISION_ID}),
                extreme: Extreme::OverTheCap,
                setup: |fixture| {
                    let long = nasty(3_000);
                    let analyses: Vec<String> = (0..40).map(|_| long.clone()).collect();
                    let raw = Raw::of(analyses.iter().map(String::as_str));
                    let summary = OfficialSummary::new(analyses, [""; 0]);
                    fixture.corpus.push_get_decision(Ok(decision(&prose(2_000), summary)));
                    raw
                },
                check: |observed| {
                    // Le résumé rendu tient en 1 200 caractères au plus, quelle que soit la source.
                    let summary = observed.value["summary"].as_str().expect("a summary");
                    assert!(summary.chars().count() <= 1_200, "{}", summary.chars().count());
                    json!({"summary_chars_at_most_1200": true})
                },
            },
            Case {
                name: "deliberation_40000_char_escape_rich_window",
                tool: "fr_get_deliberation",
                arguments: json!({
                    "deliberation_id": DELIBERATION_ID,
                    "detail": "full",
                    "max_chars": 40_000
                }),
                extreme: Extreme::OverTheCap,
                setup: |fixture| {
                    let text = nasty_paragraphs(6_500);
                    let raw = Raw::of([window_of(&text, 40_000).as_str()]);
                    fixture.corpus.push_get_deliberation(Ok(deliberation(&text)));
                    raw
                },
                check: reading_window,
            },
            // ---- fr_get_company_agreement -----------------------------------------------------
            Case {
                name: "agreement_with_long_metadata",
                tool: "fr_get_company_agreement",
                arguments: json!({"agreement_id": "ACCOTEXT000053643828"}),
                extreme: Extreme::Ordinary,
                setup: |fixture| {
                    fixture.corpus.push_get_company_agreement(Ok(CompanyAgreement {
                        agreement_id: AccoTextId::parse("ACCOTEXT000053643828").unwrap(),
                        nature: Some("ACCORD".to_owned()),
                        title: Some(long_title(1)),
                        raison_sociale: Some(long_title(2)),
                        siret: Some("32862158600143".to_owned()),
                        idcc: Idcc::parse("275"),
                        code_ape: Some("5110Z".to_owned()),
                        secteur: Some(long_title(3)),
                        themes: (1..=20).map(long_title).collect(),
                        syndicats: (1..=20).map(|n| format!("Syndicat {n}")).collect(),
                        date_texte: CivilDate::new(2025, 12, 19),
                        date_effet: CivilDate::new(2025, 12, 20),
                        date_depot: CivilDate::new(2026, 2, 27),
                    }));
                    Raw { chars: 0, bytes: 0 }
                },
                check: |observed| json!({"themes": observed.value["themes"].as_array().map_or(0, Vec::len)}),
            },
            // ---- fr_diff_article --------------------------------------------------------------
            Case {
                name: "diff_500_changes_in_escape_rich_versions",
                tool: "fr_diff_article",
                arguments: json!({
                    "article_id": "LEGIARTI000000000001",
                    "date_a": "2020-01-01",
                    "date_b": "2026-01-01"
                }),
                extreme: Extreme::OverTheCap,
                setup: |fixture| {
                    let (old, new) = (changing_words('o'), changing_words('n'));
                    fixture.corpus.push_article_versions(Ok(vec![
                        version(1, (2016, 10, 9), Some((2023, 9, 7))),
                        version(2, (2023, 9, 8), None),
                    ]));
                    fixture.corpus.push_get_article(Ok(diff_article(1, &old)));
                    fixture.corpus.push_get_article(Ok(diff_article(2, &new)));
                    // 400 changements au plus, de 120 caractères de JSON chacun (`at`, `context`,
                    // `removed`, `added`) : 48 000, plus les deux versions.
                    Raw::of([old.as_str(), new.as_str()]).with_each(400, 120)
                },
                check: |observed| {
                    let hunks = observed.value["hunks"].as_array().expect("hunks");
                    assert!(hunks.len() > 50, "{}", hunks.len());
                    // Les changements retirés le sont par la fin, et `truncated` le dit (05 §5.12).
                    assert_eq!(observed.value["truncated"], true);
                    json!({"hunks": hunks.len(), "truncated": true})
                },
            },
            // ---- fr_coverage ------------------------------------------------------------------
            Case {
                name: "coverage",
                tool: "fr_coverage",
                arguments: json!({}),
                extreme: Extreme::Ordinary,
                setup: |_| Raw { chars: 0, bytes: 0 },
                check: |observed| json!({"fonds_listed": observed.value.is_object()}),
            },
        ]
    }

    /// Cinq cents mots qui diffèrent d'une version à l'autre (`tag` : `o` ou `n`), séparés par cinq
    /// cents ancres communes, avec guillemets et antislash : un changement par mot.
    fn changing_words(tag: char) -> String {
        let mut text = String::new();
        for i in 0..500 {
            write!(text, "ancre{i} \"{tag}{i}\\\\\" ").unwrap();
        }
        text
    }

    /// La `page_size` de « use `page_size` <= N. ».
    fn suggested_page_size(message: &str) -> usize {
        let tail = message.split("page_size <= ").nth(1).expect("a suggested page_size");
        tail.trim_end_matches('.').parse().expect("a number")
    }

    /// Les `max_chars` premiers caractères de `text` : la fenêtre que l'outil lirait sans dégrader.
    fn window_of(text: &str, max_chars: usize) -> String {
        LongText::from_plain(text).unwrap().as_str().chars().take(max_chars).collect()
    }

    /// Une fenêtre de lecture dégradée : elle est plus courte que demandée mais jamais vide,
    /// `next_offset` est exact et la remarque est celle de la lecture.
    fn reading_window(observed: &Observed) -> Value {
        let text = observed.value["text"].as_str().expect("a text window");
        let length = observed.value["text_length"].as_u64().expect("a text_length");
        let next = observed.value["next_offset"].as_u64().expect("a next_offset");
        let chars = text.chars().count() as u64;
        assert!(0 < chars && chars < 40_000, "{chars}");
        // La fenêtre commence à 0 : `next_offset` est exactement ce qui a été rendu (05 §6.4, 4).
        assert_eq!(next, chars, "next_offset must point right after the rendered window");
        assert!(next < length);
        json!({"window_chars": chars, "text_length": length})
    }

    /// Une table des matières coupée : les entrées rendues sont un préfixe, `next_toc_offset` en
    /// est la suite exacte et `toc_total` dit tout ce qui existe (05 §6.4, point 1).
    fn toc_page(observed: &Observed, total: u64) -> Value {
        let toc = observed.value["toc"].as_array().expect("a toc");
        assert!(!toc.is_empty() && (toc.len() as u64) < total, "{}", toc.len());
        assert_eq!(observed.value["toc_total"], total);
        assert_eq!(observed.value["next_toc_offset"], toc.len());
        json!({"toc_entries": toc.len(), "toc_total": total})
    }

    /// Les groupes de `fr_search_all` : tous les fonds répondent, aucun résultat n'est perdu.
    fn search_all_groups(observed: &Observed) -> Value {
        let groups = observed.value["groups"].as_array().expect("groups");
        assert_eq!(groups.len(), 8);
        let hits: usize = groups.iter().map(|g| g["hits"].as_array().map_or(0, Vec::len)).sum();
        assert_eq!(hits, 80, "aucun résultat n'est retiré");
        let snippets: usize = groups
            .iter()
            .flat_map(|g| g["hits"].as_array().unwrap())
            .filter(|hit| hit.get("snippet").is_some())
            .count();
        json!({"groups": groups.len(), "hits": hits, "snippets_kept": snippets})
    }

    fn version(n: u32, from: (u16, u8, u8), to: Option<(u16, u8, u8)>) -> ArticleVersion {
        ArticleVersion {
            id: LegiArtiId::parse(&format!("LEGIARTI{n:012}")).unwrap(),
            num: Some("222-1".to_owned()),
            etat: Some(LegalStatus::Vigueur),
            date_debut: CivilDate::new(from.0, from.1, from.2),
            date_fin: to.and_then(|(y, m, d)| CivilDate::new(y, m, d)),
        }
    }

    fn diff_article(n: u32, text: &str) -> ArticleText {
        ArticleText {
            article_id: ArticleId::Legi(LegiArtiId::parse(&format!("LEGIARTI{n:012}")).unwrap()),
            num: Some("222-1".to_owned()),
            etat: None,
            code_title: Some("Code pénal".to_owned()),
            parent: Some(ParentText::new(
                DocumentId::parse("LEGITEXT000006070719").unwrap(),
                Some("CODE"),
            )),
            path: vec![],
            full_sections_titre: None,
            date_debut: None,
            date_fin: None,
            text: LongText::from_plain(text).unwrap(),
            nota: None,
        }
    }

    // ---- les tests -----------------------------------------------------------------------------

    /// Régression : D-33 (T-M5-04). Chaque combinaison extrême tient sous 45 000 caractères, ou est
    /// refusée en bande avec la `page_size` qui tient ; les outils de la v0.1 et du M7 sont tous
    /// couverts.
    #[tokio::test]
    async fn every_tool_response_under_hard_cap() {
        let mut covered = BTreeSet::new();
        let mut report = Vec::new();
        for case in cases() {
            let fixture = Fixture::default();
            let raw = (case.setup)(&fixture);
            match case.extreme {
                Extreme::OverTheCap => assert!(
                    raw.chars > CAP,
                    "{}: the case is not extreme ({} characters)",
                    case.name,
                    raw.chars
                ),
                Extreme::OverTheCapInBytesOnly => assert!(
                    raw.chars <= CAP && raw.bytes > CAP,
                    "{}: {} characters, {} bytes",
                    case.name,
                    raw.chars,
                    raw.bytes
                ),
                Extreme::Ordinary => {}
            }

            let result = call(&fixture, case.tool, &case.arguments).await;
            let observed = Observed::of(&result);
            assert!(observed.chars <= CAP, "{}: {} characters", case.name, observed.chars);
            let kept = (case.check)(&observed);

            covered.insert(case.tool);
            report.push(json!({
                "case": case.name,
                "tool": case.tool,
                "outcome": if observed.error.is_some() { "refused" } else { "rendered" },
                "under_estimation_target": observed.error.is_none().then_some(observed.chars <= TARGET),
                "kept": kept,
            }));
        }

        // Aucun outil enregistré n'échappe au contrôle, sauf ceux du M6 et du M7 de lecture, que
        // porte V4-01.
        let client = Fixture::default().connect().await;
        let registered: BTreeSet<String> =
            client.list_all_tools().await.unwrap().iter().map(|t| t.name.to_string()).collect();
        let later: BTreeSet<&str> =
            ["fr_verify_citations", "fr_resolve", "fr_get_related"].into_iter().collect();
        for name in &registered {
            assert!(
                covered.contains(name.as_str()) || later.contains(name.as_str()),
                "{name} is registered but no extreme case covers it"
            );
        }

        insta::assert_json_snapshot!("case_report", report);
    }

    /// L4-06 (search-5). Cent résultats JURI aux citations typiques (une soixantaine de
    /// caractères) ne sont plus refusés : l'estimation à 64 par champ les refusait à partir de
    /// `page_size=97`, alors que la réponse exacte tient largement sous le plafond.
    #[tokio::test]
    async fn search_juri_100_typical() {
        let typical: fn(u32) -> String =
            |n| format!("Cour de cassation, civile, Chambre civile 1, 12 mars 2020, 19-{n:05}");
        // Sans extrait : rien n'est retiré, aucune note.
        let fixture = Fixture::default();
        fixture.corpus.push_search(Ok(juri_page(100, typical, "").0));
        let bare = Observed::of(&call(&fixture, "fr_search", &arguments_of_search("JURI")).await);
        assert!(bare.error.is_none(), "{:?}", bare.error);
        assert_eq!(hits_of(&bare).len(), 100);
        assert!(hits_of(&bare).iter().all(|hit| hit["date"].is_string()), "détails conservés");
        assert!(bare.value.get("note").is_none(), "{}", bare.note());
        assert!(bare.chars < CAP, "{}", bare.chars);

        // Avec des extraits de 280 caractères : les derniers les perdent, les premiers les gardent.
        let fixture = Fixture::default();
        fixture.corpus.push_search(Ok(juri_page(100, typical, &"mot ".repeat(70)).0));
        let rich = Observed::of(&call(&fixture, "fr_search", &arguments_of_search("JURI")).await);
        assert!(rich.error.is_none(), "{:?}", rich.error);
        let hits = hits_of(&rich);
        assert_eq!(hits.len(), 100);
        snippets_go_from_the_end(hits);
        let kept = hits_with_snippet(hits);
        assert!((1..100).contains(&kept), "{kept}");
        assert!(
            rich.note()
                .starts_with(&format!("Snippets were removed from the last {} hits", 100 - kept))
        );
        assert!(!rich.note().contains(FURTHER_NOTE), "{}", rich.note());
        assert!(rich.chars <= CAP, "{}", rich.chars);
    }

    /// Réponses réelles du bac à sable (rejeu E2E de la 1.0.1, search-5) : hits JURI et CODE
    /// recopiés tels quels, extraits retirés par la 1.0.1 complétés.
    const SANDBOX_SEARCH_5: &str = include_str!("fixtures/search-5-sandbox.json");

    /// Les hits `key` (`juri` ou `code`) du relevé, reconstruits comme l'amont les donne : le titre
    /// d'un hit est sa citation (la vue l'omet donc).
    fn sandbox_hits(key: &str, fond: Fond) -> Vec<SearchHit> {
        let relevé: Value = serde_json::from_str(SANDBOX_SEARCH_5).unwrap();
        let text = |hit: &Value, field: &str| hit[field].as_str().map(str::to_owned);
        relevé[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|hit| SearchHit {
                title: text(hit, "citation"),
                date: hit["date"].as_str().map(|date| CivilDate::parse(date).unwrap()),
                nature: text(hit, "nature"),
                etat: hit["etat"].as_str().map(|_| LegalStatus::Vigueur),
                snippet: hit["snippet"].as_str().and_then(Snippet::new),
                articles: hit["articles"]
                    .as_array()
                    .map(|articles| {
                        articles
                            .iter()
                            .map(|article| {
                                CodeArticleRef::new(
                                    ArticleId::parse(article["article_id"].as_str().unwrap())
                                        .unwrap(),
                                    text(article, "num"),
                                    article["snippet"].as_str().unwrap_or_default(),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                ..SearchHit::new(
                    fond,
                    DocumentId::parse(hit["id"].as_str().unwrap()).unwrap(),
                    None,
                )
                .unwrap()
            })
            .collect()
    }

    /// L4-06 (search-5, rejeu de la 1.0.1). L'estimation comptait le `title` des hits JURI, que la
    /// vue omet puisqu'il est dans la citation : cent résultats réels étaient rendus sans aucun
    /// extrait (24 394 caractères pour un plafond de 45 000) et le dernier perdait sa `nature`.
    /// Ne part que le nécessaire : des extraits en partant du dernier, jusqu'à la cible, sans
    /// toucher aux détails.
    #[tokio::test]
    async fn search_5_sandbox_juri_100_loses_only_the_snippets_it_must() {
        let fixture = Fixture::default();
        let hits = sandbox_hits("juri", Fond::Juri);
        assert_eq!(hits.len(), 100);
        fixture.corpus.push_search(Ok(page(Fond::Juri, hits)));
        let observed =
            Observed::of(&call(&fixture, "fr_search", &arguments_of_search("JURI")).await);
        assert!(observed.error.is_none(), "{:?}", observed.error);
        let hits = hits_of(&observed);
        assert_eq!(hits.len(), 100);
        snippets_go_from_the_end(hits);
        let kept = hits_with_snippet(hits);
        assert!((50..100).contains(&kept), "{kept} extraits gardés");
        assert!(hits.iter().all(|hit| hit.get("nature").is_some()), "les détails restent");
        assert!(
            observed
                .note()
                .starts_with(&format!("Snippets were removed from the last {} hits", 100 - kept)),
            "{}",
            observed.note()
        );
        assert!(!observed.note().contains("Article lists"), "{}", observed.note());
        assert!(!observed.note().contains(FURTHER_NOTE), "{}", observed.note());
        // Sous la cible, et à moins de deux extraits de la cible : rien n'est retiré pour rien.
        assert!(observed.chars <= TARGET, "{}", observed.chars);
        assert!(observed.chars + 600 > TARGET, "{}", observed.chars);
    }

    /// L4-06 (search-5, CODE `page_size=30`). La page réelle tient sous la cible avec tous ses
    /// extraits (31 376 caractères avec 8 retirés en 1.0.1) : aucun n'est retiré, aucune note.
    #[tokio::test]
    async fn search_5_sandbox_code_30_keeps_every_snippet() {
        let fixture = Fixture::default();
        let hits = sandbox_hits("code", Fond::Code);
        assert_eq!(hits.len(), 30);
        fixture.corpus.push_search(Ok(page(Fond::Code, hits)));
        let arguments = json!({"query": "vie privée", "fond": "CODE", "page_size": 30, "page": 2});
        let observed = Observed::of(&call(&fixture, "fr_search", &arguments).await);
        assert!(observed.error.is_none(), "{:?}", observed.error);
        let hits = hits_of(&observed);
        assert_eq!(hits.len(), 30);
        assert_eq!(hits_with_snippet(hits), 30);
        assert!(hits.iter().all(|hit| {
            hit["articles"].as_array().is_some_and(|articles| {
                articles.iter().all(|article| article.get("snippet").is_some())
            })
        }));
        assert!(observed.value.get("note").is_none(), "{}", observed.note());
        assert!(observed.chars <= TARGET, "{}", observed.chars);
    }

    /// Caractères que l'application réserve aux notes de dégradation dans son estimation
    /// (`DEGRADE_NOTE_RESERVE` de `usecase/search.rs`, privé) : la page n'est dégradée que si son
    /// JSON dépasse la cible moins cette réserve.
    const NOTE_RESERVE: usize = 411;

    /// Une page de `fr_search` dont tous les champs rendus sont présents (titre hors citation, date,
    /// `nature`, `etat`, extrait, et pour CODE une liste d'articles) ; la `nature` du dernier résultat
    /// fait `padding` caractères, ce qui règle la taille au caractère.
    fn full_page(fond: Fond, padding: usize) -> SearchPage {
        let hits = (1..=20)
            .map(|n| {
                let nature = if n == 20 { "n".repeat(padding) } else { "ARRET".to_owned() };
                let articles = if fond == Fond::Code {
                    (1..=3)
                        .map(|a| {
                            CodeArticleRef::new(
                                ArticleId::parse(&format!("LEGIARTI{:012}", n * 10 + a)).unwrap(),
                                Some(format!("L{a}\"")),
                                "Le texte de l'article \"cité\".",
                            )
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                SearchHit {
                    nature: Some(nature),
                    etat: Some(LegalStatus::Vigueur),
                    articles,
                    ..hit(fond, n, &long_title(n), &prose(2))
                }
            })
            .collect();
        page(fond, hits)
    }

    /// Dérive de l'estimateur de l'application (`usecase/search.rs`), qui recopie les champs de
    /// `HitView` et `ArticleView`. Sur une page sans dégradation, la taille mesurée de la réponse
    /// est celle que l'estimateur compte : elle passe tout juste à `cible - réserve` caractères et
    /// un caractère de plus déclenche la note d'extraits retirés. Un champ ajouté à la vue sans
    /// l'estimateur, ou l'inverse, déplace ce seuil.
    #[tokio::test]
    async fn search_estimate_matches_the_measured_size_to_the_character() {
        for (fond, name) in [(Fond::Juri, "JURI"), (Fond::Code, "CODE")] {
            let render = async |padding: usize| {
                let fixture = Fixture::default();
                fixture.corpus.push_search(Ok(full_page(fond, padding)));
                let arguments = json!({"query": "bail", "fond": name, "page_size": 20});
                let observed = Observed::of(&call(&fixture, "fr_search", &arguments).await);
                assert!(observed.error.is_none(), "{name}: {:?}", observed.error);
                observed
            };
            let (one, two) = (render(1_000).await, render(1_001).await);
            assert_eq!(two.chars, one.chars + 1, "{name}: un caractère de nature, un de réponse");
            assert!(one.note().is_empty(), "{name}: {}", one.note());
            let edge = 1_000 + (ResponseBudget::ESTIMATE_TARGET_CHARS - NOTE_RESERVE - one.chars);
            let fits = render(edge).await;
            assert_eq!(fits.chars, ResponseBudget::ESTIMATE_TARGET_CHARS - NOTE_RESERVE, "{name}");
            assert!(fits.note().is_empty(), "{name}: {}", fits.note());
            assert_eq!(hits_with_snippet(hits_of(&fits)), 20, "{name}");
            let over = render(edge + 1).await;
            assert!(over.note().starts_with("Snippets were removed"), "{name}: {}", over.note());
            assert!(!over.note().contains(FURTHER_NOTE), "{name}: {}", over.note());
            assert!(over.chars <= TARGET, "{name}: {}", over.chars);
        }
    }

    /// Régression : D-33. La `page_size` proposée par un refus tient : la page ainsi demandée est
    /// rendue sous le plafond, sans remarque de repli.
    #[tokio::test]
    async fn a_refusal_names_a_page_size_that_fits() {
        let plain: fn(u32) -> String = |n| format!("{n} {}", "x".repeat(997));
        let escape_rich: fn(u32) -> String = |_| nasty(300);
        for (title, snippet) in [(plain, "mot ".repeat(70)), (escape_rich, nasty(280))] {
            let page_of = |hits: usize| {
                let hits = u32::try_from(hits).unwrap();
                Ok(juri_page(hits, title, &snippet).0)
            };
            let ask = |size: usize| async move {
                let fixture = Fixture::default();
                fixture.corpus.push_search(page_of(size));
                let arguments = json!({"query": "bail", "fond": "JURI", "page_size": size});
                Observed::of(&call(&fixture, "fr_search", &arguments).await)
            };

            let message = ask(100).await.error.expect("a refusal");
            let suggested = suggested_page_size(&message);

            // La `page_size` proposée tient, sans le repli de la vue...
            let fits = ask(suggested).await;
            assert!(fits.error.is_none(), "page_size={suggested}: {:?}", fits.error);
            assert_eq!(hits_of(&fits).len(), suggested);
            assert!(fits.chars <= CAP, "{} characters for page_size={suggested}", fits.chars);
            assert!(!fits.note().contains(FURTHER_NOTE), "{}", fits.note());
            // ... et c'est la plus grande : la suivante est refusée à son tour.
            let next = ask(suggested + 1).await;
            assert!(next.error.is_some(), "page_size={} should be refused too", suggested + 1);
        }
    }

    /// Régression : D-33. Les pages d'un texte riche en échappements se partitionnent : suivre
    /// `next_offset` redonne le texte entier, et chaque page tient sous le plafond.
    #[tokio::test]
    async fn escape_rich_text_pages_partition_the_text() {
        let raw = nasty_paragraphs(3_000);
        let canonical = LongText::from_plain(&raw).unwrap().as_str().to_owned();
        let fixture = Fixture::default();
        let mut rebuilt = String::new();
        let mut offset = 0_u64;
        let mut pages = 0;
        loop {
            fixture.corpus.push_get_article(Ok(article(&raw)));
            let arguments = json!({
                "article_id": ARTICLE_ID,
                "detail": "full",
                "offset": offset,
                "max_chars": 40_000
            });
            let observed = Observed::of(&call(&fixture, "fr_get_text", &arguments).await);
            assert!(observed.error.is_none(), "{:?}", observed.error);
            assert!(observed.chars <= CAP, "page {pages}: {} characters", observed.chars);
            rebuilt.push_str(observed.value["text"].as_str().unwrap());
            pages += 1;
            match observed.value.get("next_offset") {
                Some(next) => offset = next.as_u64().unwrap(),
                None => break,
            }
            assert!(pages < 20, "the reading does not end");
        }
        assert_eq!(rebuilt, canonical, "the pages must rebuild the text without loss");
        assert!(pages >= 2, "{pages}");
    }

    /// Régression : D-47, D-33. Les pages d'une table des matières coupée se partitionnent :
    /// suivre `next_toc_offset` redonne les 500 entrées, dans l'ordre, sans doublon.
    #[tokio::test]
    async fn toc_pages_partition_the_toc() {
        let fixture = Fixture::default();
        let mut seen = Vec::new();
        let mut offset = 0_u64;
        let mut pages = 0;
        loop {
            fixture.corpus.push_get_act(Ok(act(toc("LEGISCTA", 500, long_title).0)));
            let arguments = json!({"text_id": ACT_ID, "toc_offset": offset, "toc_limit": 500});
            let observed = Observed::of(&call(&fixture, "fr_get_act", &arguments).await);
            assert!(observed.error.is_none(), "{:?}", observed.error);
            assert!(observed.chars <= CAP, "page {pages}: {} characters", observed.chars);
            seen.extend(observed.value["toc"].as_array().unwrap().iter().cloned());
            pages += 1;
            match observed.value.get("next_toc_offset") {
                Some(next) => offset = next.as_u64().unwrap(),
                None => break,
            }
            assert!(pages < 20, "the table of contents does not end");
        }
        assert_eq!(seen.len(), 500);
        let titles: Vec<&str> = seen.iter().map(|entry| entry["title"].as_str().unwrap()).collect();
        let expected: Vec<String> = (1..=500).map(long_title).collect();
        assert!(titles.iter().zip(&expected).all(|(a, b)| a == b), "entries out of order");
        assert!(pages >= 3, "{pages}");
    }

    /// Régression : D-33. Le coût fixe de `tools/list` reste sous le budget de 06 §12.2 de la plus
    /// riche des surfaces (v0.3), et aucune description ne dépasse 1 000 caractères. Le budget
    /// initial de 29 000 octets avait été recalibré sur la mesure plus 10 % (05 §6.6) ; F1 le pose
    /// une dernière fois sur la surface finale, les treize outils : 31 853 octets mesurés, plus 3 %
    /// (32 808,6), arrondis à la centaine supérieure, soit 32 900 octets ; la 1.1.0 le relève à
    /// 34 500 (Q27 = A, 05 §6.6) : 32 608 octets mesurés à la vague V2b, D-12 ajoute des
    /// descriptions. La même valeur est dans `TOOLS_LIST_BUDGETS` de `xtask` (`lock-descriptors`), qui mesure le même JSON canonique.
    #[tokio::test]
    async fn descriptor_size_budget() {
        let client = Fixture::default().connect().await;
        let tools = client.list_all_tools().await.unwrap();
        let bytes = serde_json::to_vec(&tools).unwrap().len();
        assert!(bytes <= 34_500, "tools/list: {bytes} bytes");
        for tool in &tools {
            let description = tool.description.as_deref().unwrap_or("");
            assert!(description.chars().count() <= 1_000, "{}: {description}", tool.name);
        }
    }
}
