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
    use lf_domain::entity::{Decision, OfficialSummary};
    use lf_domain::id::DecisionId;
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

    /// `PisteCorpus` vers un serveur simulé qui sert `case` pour son identifiant.
    async fn decision(case: &Case) -> Decision {
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
            .and(path(format!("{API_BASE}/consult/juri")))
            .and(body_partial_json(json!({"textId": case.id})))
            .respond_with(ResponseTemplate::new(200).set_body_raw(case.body, "application/json"))
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
        let corpus = PisteCorpus::new(config);
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
}
