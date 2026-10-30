//! Les neuf scénarios de la référence joués sur le binaire `mcp-legifrance` (T-M4-13, P-060,
//! 06 §6.1, annexe A §2.3 : `test_smoke.py`), contre la sandbox PISTE.
//!
//! `live_e2e_reference_scenarios` porte `#[ignore]` et son nom commence par `live_` (profil
//! nextest `live`). Les identifiants viennent de l'environnement du processus de test
//! (`LF_PISTE_CLIENT_ID`, `LF_PISTE_CLIENT_SECRET`), jamais d'un fichier ; sans eux, ou hors
//! sandbox (`LF_PISTE_ENV` autre que `sandbox`), le test écrit un message sur stderr et réussit.
//! Aucune valeur d'identifiant n'est affichée.
//!
//! ```text
//! LF_PISTE_CLIENT_ID=… LF_PISTE_CLIENT_SECRET=… \
//!   cargo nextest run -p mcp-legifrance --test live_e2e --all-features --profile live --run-ignored only
//! ```
//!
//! Les mêmes neuf scénarios, avec les mêmes assertions, sont aussi joués sans réseau contre les
//! seize réponses enregistrées par la référence (`reference_scenarios_replay_on_fixtures`) : ce test
//! vérifie les assertions elles-mêmes sur la forme réelle des sorties du binaire. Seules deux
//! assertions dépendent des données vivantes (l'article du scénario 3 est un article de loi en
//! sandbox, un article de code dans les enregistrements) et sont donc relâchées pour lui.
//!
//! Cadence : une pause d'une seconde avant chaque appel en sandbox, pour ne provoquer aucun 429.
//!
//! Tout le fichier est sous `cfg(test)` : les aides partagent les règles de lint des tests
//! (`unwrap` et `expect` permis, `clippy.toml`).

#![expect(
    clippy::as_conversions,
    reason = "tests : tailles et indices bornés par les données du test"
)]

#[cfg(test)]
mod common;

#[cfg(test)]
mod scenarios {
    use std::{path::Path, time::Duration};

    use serde_json::{Value, json};

    use crate::common::{FakePiste, Launch, Session, call, first_text, is_tool_error};

    /// D'où viennent les réponses : la sandbox PISTE, ou les enregistrements de la référence.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Source {
        Sandbox,
        Fixtures,
    }

    /// Le lancement vers la vraie sandbox : identifiants du processus de test, réseau direct (le
    /// proxy et les certificats du processus sont transmis s'il y en a). `None`, avec un message
    /// sur stderr, sans identifiants ou hors sandbox.
    #[expect(
        clippy::disallowed_methods,
        clippy::print_stderr,
        reason = "test live : identifiants lus dans l'environnement et diagnostic sur stderr, sans jamais afficher de valeur"
    )]
    fn sandbox_launch(audit: &Path) -> Option<Launch> {
        let test = "live_e2e_reference_scenarios";
        if std::env::var("LF_PISTE_ENV").is_ok_and(|env| env != "sandbox") {
            eprintln!("{test}: skipped, live tests run against the sandbox only");
            return None;
        }
        let (Ok(id), Ok(secret)) =
            (std::env::var("LF_PISTE_CLIENT_ID"), std::env::var("LF_PISTE_CLIENT_SECRET"))
        else {
            eprintln!(
                "{test}: skipped, set LF_PISTE_CLIENT_ID and LF_PISTE_CLIENT_SECRET (sandbox)"
            );
            return None;
        };
        let mut launch = Launch::new(audit)
            .without_proxy()
            .credentials_of(&id, &secret)
            .env("LF_PISTE_ENV", "sandbox");
        for name in [
            "HTTPS_PROXY",
            "HTTP_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "https_proxy",
            "http_proxy",
            "all_proxy",
            "no_proxy",
            "SSL_CERT_FILE",
            "SSL_CERT_DIR",
        ] {
            if let Ok(value) = std::env::var(name) {
                launch = launch.env(name, &value);
            }
        }
        Some(launch)
    }

    /// Les appels d'une série de scénarios sur une session.
    struct Run<'a> {
        session: &'a mut Session,
        source: Source,
        next_id: u64,
    }

    impl<'a> Run<'a> {
        async fn start(session: &'a mut Session, source: Source) -> Self {
            session.handshake().await;
            Self { session, source, next_id: 2 }
        }

        /// Appelle `tool` et rend son `structuredContent`; un échec d'outil arrête le scénario en
        /// citant son texte.
        async fn tool(&mut self, scenario: &str, tool: &str, arguments: &Value) -> Value {
            if self.source == Source::Sandbox {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            let id = self.next_id;
            self.next_id += 1;
            let response = self.session.request(&call(id, tool, arguments)).await;
            assert!(
                !is_tool_error(&response),
                "scenario {scenario}: {tool} {arguments} failed: {}",
                first_text(&response)
            );
            let structured = response["result"]["structuredContent"].clone();
            assert!(
                structured.is_object(),
                "scenario {scenario}: {tool} has no structuredContent: {response:.500}"
            );
            structured
        }

        /// `fr_search` du fond et de la requête de la référence : au moins un résultat, du bon fond,
        /// avec citation et URL. Rend l'identifiant du premier résultat et la page entière.
        async fn search(
            &mut self,
            scenario: &str,
            fond: &str,
            query: &str,
            size: u32,
        ) -> (String, Value) {
            let arguments = json!({"fond": fond, "query": query, "page_size": size});
            let page = self.tool(scenario, "fr_search", &arguments).await;
            let hits = page["hits"].as_array().expect("hits is an array");
            assert!(
                page["total"].as_u64() >= Some(1) && !hits.is_empty(),
                "scenario {scenario}: {fond} {query}: {page:.500}"
            );
            assert!(
                hits.len() <= size as usize,
                "scenario {scenario}: {} hits for page_size {size}",
                hits.len()
            );
            for hit in hits {
                assert_eq!(hit["kind"], fond.to_lowercase(), "scenario {scenario}: {hit}");
                assert!(
                    !hit["citation"].as_str().unwrap_or_default().is_empty(),
                    "scenario {scenario}: {hit}"
                );
                assert!(
                    hit["source_url"].as_str().unwrap_or_default().starts_with("https://"),
                    "scenario {scenario}: {hit}"
                );
            }
            let json = page.to_string();
            assert!(
                !json.contains("<mark>") && !json.contains("</mark>"),
                "scenario {scenario}: marks left in {json:.300}"
            );
            assert!(
                !json.contains("/eli/"),
                "scenario {scenario}: a fabricated ELI in {json:.300}"
            );
            let id = hits[0]["id"].as_str().expect("a hit id").to_owned();
            (id, page)
        }
    }

    /// Texte ou résumé non vide, citation et URL : ce que tout outil de lecture rend (05 §6.2 :
    /// un texte long n'est plus rendu en entier par défaut, d'où « `text` ou `summary` »).
    fn assert_readable(scenario: &str, view: &Value) {
        let nonempty = |key: &str| view[key].as_str().is_some_and(|text| !text.trim().is_empty());
        assert!(
            nonempty("text") || nonempty("summary"),
            "scenario {scenario}: no text and no summary: {view:.500}"
        );
        assert!(nonempty("citation"), "scenario {scenario}: no citation: {view:.500}");
        assert!(nonempty("source_url"), "scenario {scenario}: no source_url: {view:.500}");
        assert!(!view.to_string().contains("/eli/"), "scenario {scenario}: a fabricated ELI");
    }

    /// Le premier article de la table des matières, `(article_id, num)`.
    fn first_toc_article(scenario: &str, view: &Value) -> String {
        let toc = view["toc"].as_array().expect("toc is an array");
        assert!(!toc.is_empty(), "scenario {scenario}: empty table of contents: {view:.500}");
        toc.iter()
            .find_map(|entry| entry["article_id"].as_str())
            .unwrap_or_else(|| {
                crate::common::fail(&format!(
                    "scenario {scenario}: no article in the table of contents"
                ))
            })
            .to_owned()
    }

    /// Les neuf scénarios de `test_smoke.py` (annexe A §2.3), dans l'ordre.
    async fn play(run: &mut Run<'_>) {
        let law = run.source == Source::Sandbox;

        // 1. LODA : `republique numerique`, 5.
        let (id, _) = run.search("1", "LODA", "republique numerique", 5).await;
        assert!(id.starts_with("LEGITEXT") && !id.contains('_'), "scenario 1: {id}");

        // 2. CODE : `vie privee`, 5 ; les articles trouvés sont lisibles par `fr_get_text`.
        let (id, page) = run.search("2", "CODE", "vie privee", 5).await;
        assert!(id.starts_with("LEGITEXT"), "scenario 2: {id}");
        let article_ids: Vec<&str> = page["hits"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|hit| hit["articles"].as_array().into_iter().flatten())
            .filter_map(|article| article["article_id"].as_str())
            .collect();
        assert!(!article_ids.is_empty(), "scenario 2: no article in the hits: {page:.500}");
        assert!(
            article_ids.iter().all(|id| id.starts_with("LEGIARTI")),
            "scenario 2: {article_ids:?}"
        );

        // 3. Loi : `LEGITEXT000033205014` (loi pour une République numérique), puis son premier
        //    article, dont l'URL est celle de l'article (E-19 attend `/loda/article_lc/`, mais Q17
        //    garde `/codes/article_lc/` tant que V-140 et V-141 sont ouvertes).
        let act = run.tool("3", "fr_get_act", &json!({"text_id": "LEGITEXT000033205014"})).await;
        assert_eq!(act["text_id"], "LEGITEXT000033205014", "scenario 3: {act:.500}");
        assert!(
            act["source_url"].as_str().unwrap_or_default().contains("/loda/id/"),
            "scenario 3: {act:.500}"
        );
        assert!(!act["citation"].as_str().unwrap_or_default().is_empty(), "scenario 3: {act:.500}");
        let article_id = first_toc_article("3", &act);
        let article = run.tool("3", "fr_get_text", &json!({"article_id": article_id})).await;
        assert_readable("3", &article);
        // Q17 : tant que V-140 et V-141 (ouverture des URL dans un navigateur, par le mainteneur)
        // sont ouvertes, tout `LEGIARTI` garde `/codes/article_lc/` (`ArticleText::contract`,
        // ADR-0009 point 6). La cible E-19 `/loda/article_lc/` n'est donc pas encore attendue : la
        // sonde exige l'URL d'un article de l'identifiant demandé sous l'un ou l'autre chemin
        // (constaté en sandbox le 2026-10-29 : `textTitles[].nature` d'un article de loi vaut
        // `LOI`, et l'URL rendue est `/codes/article_lc/`).
        let url = article["source_url"].as_str().unwrap_or_default();
        if law {
            assert!(
                url.ends_with(&format!("/article_lc/{article_id}")),
                "scenario 3: {url} for {article_id}"
            );
        } else {
            // Les enregistrements ne contiennent qu'un article de code.
            assert!(url.contains("/codes/article_lc/"), "scenario 3: {url}");
        }

        // 4. JURI : `responsabilite`, 3 ; lecture de la première décision.
        let (id, _) = run.search("4", "JURI", "responsabilite", 3).await;
        assert!(id.starts_with("JURITEXT"), "scenario 4: {id}");
        let decision = run.tool("4", "fr_get_decision", &json!({"decision_id": id})).await;
        assert_readable("4", &decision);

        // 5. CONSTIT : `liberte`, 3 ; la citation commence par « Cons. const., décision ».
        let (id, _) = run.search("5", "CONSTIT", "liberte", 3).await;
        assert!(id.starts_with("CONSTEXT"), "scenario 5: {id}");
        let decision = run.tool("5", "fr_get_decision", &json!({"decision_id": id})).await;
        assert_readable("5", &decision);
        assert!(
            decision["citation"].as_str().unwrap_or_default().starts_with("Cons. const., décision"),
            "scenario 5: {}",
            decision["citation"]
        );

        // 6. CETAT : `urbanisme`, 5.
        let (id, _) = run.search("6", "CETAT", "urbanisme", 5).await;
        assert!(id.starts_with("CETATEXT"), "scenario 6: {id}");
        let decision = run.tool("6", "fr_get_decision", &json!({"decision_id": id})).await;
        assert_readable("6", &decision);

        // 7. CNIL : `sanction`, 3.
        let (id, _) = run.search("7", "CNIL", "sanction", 3).await;
        assert!(id.starts_with("CNILTEXT"), "scenario 7: {id}");
        let deliberation =
            run.tool("7", "fr_get_deliberation", &json!({"deliberation_id": id})).await;
        assert_readable("7", &deliberation);
        assert!(
            deliberation["citation"]
                .as_str()
                .unwrap_or_default()
                .starts_with("CNIL, délibération n°"),
            "scenario 7: {}",
            deliberation["citation"]
        );

        // 8. KALI : `convention collective`, 3 ; sa table des matières mène à des articles.
        let (id, _) = run.search("8", "KALI", "convention collective", 3).await;
        assert!(id.starts_with("KALITEXT"), "scenario 8: {id}");
        let convention = run.tool("8", "fr_get_convention", &json!({"convention_id": id})).await;
        assert!(
            !convention["citation"].as_str().unwrap_or_default().is_empty(),
            "scenario 8: {convention:.500}"
        );
        assert!(
            first_toc_article("8", &convention).starts_with("KALIARTI"),
            "scenario 8: {convention:.500}"
        );

        // 9. ACCO : `teletravail`, 3 ; métadonnées seules (le texte est un .docx), société présente.
        let (id, _) = run.search("9", "ACCO", "teletravail", 3).await;
        assert!(id.starts_with("ACCOTEXT"), "scenario 9: {id}");
        let agreement =
            run.tool("9", "fr_get_company_agreement", &json!({"agreement_id": id})).await;
        for key in ["raison_sociale", "attachment_note", "citation", "source_url"] {
            assert!(
                agreement[key].as_str().is_some_and(|text| !text.trim().is_empty()),
                "scenario 9: no {key}: {agreement:.500}"
            );
        }
    }

    /// P-060, T-M4-13 : les neuf scénarios de la référence contre la sandbox PISTE, sur le binaire
    /// lancé comme un client MCP le lancerait (stdio, `LF_AUDIT_DIR` temporaire). Joué par
    /// l'intégrateur en F2 (D17) : il demande les identifiants de la sandbox.
    #[tokio::test]
    #[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]
    async fn live_e2e_reference_scenarios() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let Some(launch) = sandbox_launch(audit.path()) else { return };
        let mut session = launch.spawn();
        play(&mut Run::start(&mut session, Source::Sandbox).await).await;
        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        // Une ligne d'audit par appel : 8 recherches et 8 lectures.
        let lines = crate::common::audit_lines(audit.path());
        assert_eq!(lines.len(), 16, "{lines:?}");
        assert!(lines.iter().all(|line| line["status"] == "ok"), "{lines:?}");
    }

    /// Les mêmes scénarios, avec les mêmes assertions, contre les enregistrements de la référence
    /// servis par un faux PISTE : prouve que les assertions tiennent sur la forme réelle des sorties
    /// du binaire, sans réseau ni identifiants.
    #[tokio::test]
    async fn reference_scenarios_replay_on_fixtures() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let piste = FakePiste::reference().await;
        let mut session = Launch::new(audit.path()).credentials().piste(&piste).spawn();
        play(&mut Run::start(&mut session, Source::Fixtures).await).await;
        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        let lines = crate::common::audit_lines(audit.path());
        assert_eq!(lines.len(), 16, "{lines:?}");
        assert!(lines.iter().all(|line| line["status"] == "ok"), "{lines:?}");
    }
}
