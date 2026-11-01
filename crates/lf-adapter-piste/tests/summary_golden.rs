//! Golden du résumé (`detail="summary"`, 03 §7.9, T-M5-02) : le texte et le sommaire officiel
//! des décisions de référence, lus par `PisteCorpus` sur ses fixtures (`wiremock`), passent par
//! `lf_domain::text::summary`.
//!
//! Les longueurs sont celles que mesure 03 §7.9 sur les fixtures : dispositif du Conseil
//! constitutionnel (551 caractères) et du Conseil d'État (411), sommaire officiel du Conseil d'État
//! (736) et de la Cour de cassation (829). Les textes eux-mêmes sont figés par un snapshot `insta`.
//!
//! Tout le fichier est sous `cfg(test)` : les aides partagent les règles de lint des tests
//! (`unwrap` permis, `clippy.toml`).

#[cfg(test)]
mod golden {
    use std::num::NonZeroU32;

    use lf_adapter_piste::{
        PisteConfig, PisteCorpus, PisteCredentials, PisteEndpoints, PisteTuning, RateQuota,
    };
    use lf_application::ports::LegalCorpus;
    use lf_domain::coverage::PisteEnvironment;
    use lf_domain::entity::{Decision, Deliberation, OfficialSummary};
    use lf_domain::id::{CnilTextId, DecisionId};
    use lf_domain::text::{DispositifRule, SUMMARY_CAP_CHARS, SummarySource, summary};
    use secrecy::SecretString;
    use serde_json::{Value, json};
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const API_BASE: &str = "/dila/legifrance/lf-engine-app";
    const TOKEN_PATH: &str = "/oauth/token";
    /// `max_chars` d'un appel `fr_get_decision` en mode résumé : le plafond du résumé s'applique.
    const MAX_CHARS: usize = 4_000;

    /// Décision de référence, sa fixture et la règle de dispositif de sa juridiction.
    struct Case {
        /// Nom de la fixture, sans extension.
        fixture: &'static str,
        /// Identifiant demandé à la capture.
        id: &'static str,
        /// Contenu de la fixture.
        body: &'static [u8],
        /// Règle choisie par l'appelant d'après le préfixe de l'identifiant.
        rule: DispositifRule,
    }

    const CASES: [Case; 3] = [
        Case {
            fixture: "consult_juri",
            id: "JURITEXT000051743650",
            body: include_bytes!("fixtures/reference/consult_juri.json"),
            rule: DispositifRule::None,
        },
        Case {
            fixture: "consult_juri_constit",
            id: "CONSTEXT000052555275",
            body: include_bytes!("fixtures/reference/consult_juri_constit.json"),
            rule: DispositifRule::ConseilConstitutionnel,
        },
        Case {
            fixture: "consult_juri_cetat",
            id: "CETATEXT000054178503",
            body: include_bytes!("fixtures/reference/consult_juri_cetat.json"),
            rule: DispositifRule::ConseilEtat,
        },
    ];

    /// `PisteCorpus` vers un serveur simulé qui sert `body` sur `endpoint` pour l'identifiant `id`.
    async fn corpus_serving(
        endpoint: &str,
        id: &str,
        body: &'static [u8],
    ) -> (MockServer, PisteCorpus) {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(TOKEN_PATH))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(
                    br#"{"access_token":"golden-token","token_type":"Bearer","expires_in":3600}"#
                        .as_slice(),
                    "application/json",
                ),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("{API_BASE}{endpoint}")))
            .and(body_partial_json(json!({"textId": id})))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
            .expect(1)
            .mount(&server)
            .await;
        let uri = server.uri();
        let (endpoints, _warnings) = PisteEndpoints::with_overrides(
            PisteEnvironment::Sandbox,
            Some(&format!("{uri}{TOKEN_PATH}")),
            Some(&format!("{uri}{API_BASE}")),
        )
        .unwrap();
        let credentials =
            PisteCredentials::new("golden-client".to_owned(), SecretString::from("golden-secret"));
        let hundred = NonZeroU32::new(100).unwrap();
        let tuning =
            PisteTuning { rate: RateQuota::new(hundred, hundred), ..PisteTuning::default() };
        let config = PisteConfig::new(
            PisteEnvironment::Sandbox,
            Ok(endpoints),
            Some(credentials),
            "lf-golden-tests/0".to_owned(),
        )
        .with_tuning(tuning)
        .unwrap();
        (server, PisteCorpus::new(config))
    }

    /// Décision servie pour `case`.
    async fn decision(case: &Case) -> Decision {
        let (_server, corpus) = corpus_serving("/consult/juri", case.id, case.body).await;
        corpus.get_decision(&DecisionId::parse(case.id).unwrap()).await.unwrap()
    }

    /// Résumé de `decision` : avec ou sans son sommaire officiel, comme le cas d'usage de
    /// `fr_get_decision` qui passe `Some(official)` ou `None` selon que la liste est vide.
    fn summarize(
        decision: &Decision,
        official: Option<&OfficialSummary>,
        rule: DispositifRule,
    ) -> (SummarySource, usize, bool, String) {
        let got = summary(&decision.text, official, rule, MAX_CHARS);
        (got.source, got.text.chars().count(), got.truncated, got.text)
    }

    async fn by_fixture(name: &str) -> (Decision, DispositifRule) {
        let case = CASES.iter().find(|case| case.fixture == name).unwrap();
        (decision(case).await, case.rule)
    }

    #[tokio::test]
    async fn summary_matches_fixture_lengths() {
        // Conseil constitutionnel : pas de sommaire officiel, le dispositif de deux articles.
        let (constit, rule) = by_fixture("consult_juri_constit").await;
        let (source, chars, truncated, _) =
            summarize(&constit, Some(&constit.official_summary), rule);
        assert_eq!((source, chars, truncated), (SummarySource::Dispositif, 551, false));

        // Conseil d'État : le sommaire officiel (analyse dédoublonnée) passe avant le dispositif,
        // qui est le repli sans sommaire (trois articles, ligne de tirets écartée).
        let (cetat, rule) = by_fixture("consult_juri_cetat").await;
        let (source, chars, truncated, _) = summarize(&cetat, Some(&cetat.official_summary), rule);
        assert_eq!((source, chars, truncated), (SummarySource::Official, 736, false));
        let (source, chars, truncated, _) = summarize(&cetat, None, rule);
        assert_eq!((source, chars, truncated), (SummarySource::Dispositif, 411, false));

        // Cour de cassation : l'abstrat de 829 caractères, sans règle de dispositif.
        let (juri, rule) = by_fixture("consult_juri").await;
        let (source, chars, truncated, _) = summarize(&juri, Some(&juri.official_summary), rule);
        assert_eq!((source, chars, truncated), (SummarySource::Official, 829, false));

        // Les quatre sont sous le plafond de 1 200 caractères : aucune coupure.
        assert!([551, 411, 829, 736].into_iter().all(|chars| chars <= SUMMARY_CAP_CHARS));
    }

    #[tokio::test]
    async fn summary_texts_are_golden() {
        let mut texts: Vec<Value> = Vec::new();
        for (fixture, official, label) in [
            ("consult_juri_constit", true, "with the official summary"),
            ("consult_juri_cetat", true, "with the official summary"),
            ("consult_juri_cetat", false, "without the official summary"),
            ("consult_juri", true, "with the official summary"),
        ] {
            let (decision, rule) = by_fixture(fixture).await;
            let official = official.then_some(&decision.official_summary);
            let (source, chars, truncated, text) = summarize(&decision, official, rule);
            texts.push(json!({
                "fixture": fixture,
                "mode": label,
                "source": source.as_str(),
                "chars": chars,
                "truncated": truncated,
                "text": text,
            }));
        }
        insta::assert_json_snapshot!("summaries", texts);
    }
    /// Délibération de la CNIL de référence : fixture enregistrée, nature attendue.
    struct CnilCase {
        /// Libellé court dans le snapshot.
        label: &'static str,
        /// Identifiant demandé à la capture.
        id: &'static str,
        /// Contenu de la fixture.
        body: &'static [u8],
        /// Source attendue du résumé.
        source: SummarySource,
    }

    const CNIL_CASES: [CnilCase; 6] = [
        CnilCase {
            label: "sanction SAN-2025-008",
            id: "CNILTEXT000052266505",
            body: include_bytes!(
                "fixtures/recorded/consult/cnil/c110/D09-cnil-consult-SAN-2025-008-CNILTEXT000052266505.json"
            ),
            source: SummarySource::Dispositif,
        },
        CnilCase {
            label: "sanction SAN-2018-008",
            id: "CNILTEXT000037273346",
            body: include_bytes!(
                "fixtures/recorded/consult/cnil/c110/D09-cnil-consult-SAN-2018-008-CNILTEXT000037273346.json"
            ),
            source: SummarySource::Dispositif,
        },
        CnilCase {
            label: "sanction SAN-2026-002",
            id: "CNILTEXT000053352643",
            body: include_bytes!(
                "fixtures/recorded/consult/cnil/complet/cnil-CNILTEXT000053352643.json"
            ),
            source: SummarySource::Dispositif,
        },
        CnilCase {
            label: "mise en demeure MED-2024-150",
            id: "CNILTEXT000050719865",
            body: include_bytes!(
                "fixtures/recorded/consult/cnil/c110/D09-cnil-consult-MED-2024-150-CNILTEXT000050719865.json"
            ),
            source: SummarySource::Dispositif,
        },
        CnilCase {
            label: "avis 2025-083",
            id: "CNILTEXT000053018448",
            body: include_bytes!(
                "fixtures/recorded/consult/cnil/c110/D09-cnil-consult-avis-2025-083-CNILTEXT000053018448.json"
            ),
            source: SummarySource::Lead,
        },
        CnilCase {
            label: "avis 2025-107",
            id: "CNILTEXT000053308330",
            body: include_bytes!(
                "fixtures/recorded/consult/cnil/c110/D09-cnil-consult-avis-CNILTEXT000053308330.json"
            ),
            source: SummarySource::Lead,
        },
    ];

    /// Délibération servie pour `case`.
    async fn deliberation(case: &CnilCase) -> Deliberation {
        let (_server, corpus) = corpus_serving("/consult/cnil", case.id, case.body).await;
        corpus.get_deliberation(&CnilTextId::parse(case.id).unwrap()).await.unwrap()
    }

    #[tokio::test]
    async fn cnil_summary_texts_are_golden() {
        // D-09 : les sanctions et la mise en demeure donnent leur dispositif, les avis leur début.
        // La CNIL ne publie pas de sommaire officiel : `official` est toujours `None`.
        let mut texts: Vec<Value> = Vec::new();
        for case in &CNIL_CASES {
            let deliberation = deliberation(case).await;
            let got = summary(&deliberation.text, None, DispositifRule::Cnil, MAX_CHARS);
            assert_eq!(got.source, case.source, "{}", case.label);
            assert!(got.text.chars().count() <= SUMMARY_CAP_CHARS);
            texts.push(json!({
                "case": case.label,
                "source": got.source.as_str(),
                "chars": got.text.chars().count(),
                "truncated": got.truncated,
                "text": got.text,
            }));
        }
        insta::assert_json_snapshot!("summaries_cnil", texts);
    }
}
