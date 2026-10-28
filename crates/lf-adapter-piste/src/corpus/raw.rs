//! API brute de la feature `record` (04 §1.4, §13.4) : un échange avec l'API par le vrai client
//! de l'adapter, sans classification, sans réessai, sans réparation du mojibake.
//!
//! Même chemin qu'une tentative de `PisteCorpus::send` (mode dégradé sans E/S, client unique,
//! barrière de charge, jeton OAuth, délai de l'appel, borne de corps), mais le statut et les
//! octets sont rendus tels quels : un 400, un 404 ou un 500 sont des résultats, pas des erreurs.
//! Un 401 n'est pas rejoué. Le permis de concurrence tombe dès la lecture du corps.
//!
//! Le jeton, le secret client et le `client_id` ne sortent jamais d'ici : seul un booléen dit si
//! le corps reçu recopie la valeur exacte de l'un d'eux (`Exchange::echoes_credentials`), et
//! `record::write` refuse alors d'écrire quoi que ce soit.

use lf_application::ports::CorpusError;
use secrecy::ExposeSecret;

use super::{PisteCorpus, api_url, ready};
use crate::deadline::CallDeadline;
use crate::http::{Attempt, Payload, send_once};
use crate::record::Exchange;
use crate::time::{MonoClock, Sleeper};

impl<K: MonoClock + Clone, S: Sleeper + Clone> PisteCorpus<K, S> {
    /// Envoie `body` en POST sur `endpoint` (gabarit de chemin) et rend la réponse brute.
    ///
    /// # Errors
    ///
    /// [`CorpusError::Config`] en mode dégradé (sans E/S) ; [`CorpusError::InvalidRequest`] si
    /// `endpoint` ne commence pas par `/` ; l'erreur de la barrière de charge ou du jeton ; une
    /// erreur de transport (`Timeout`, `Upstream` sans statut, `Decode` pour un corps au-delà de
    /// la borne). Un statut HTTP, quel qu'il soit, n'est jamais une erreur.
    pub async fn exchange(&self, endpoint: &str, body: &[u8]) -> Result<Exchange, CorpusError> {
        let inner = &*self.inner;
        let endpoints = ready(&inner.config)?;
        let url = api_url(endpoints.base_url(), endpoint)?;
        let tuning = inner.config.tuning();
        let deadline = CallDeadline::from_task_local(&inner.clock, tuning.tool_deadline);
        let client = inner.client.get(&inner.config).await?;
        let _permit = inner.gate.acquire(&deadline).await?;
        let lease = inner.tokens.token(&deadline).await?;
        let attempt = Attempt {
            url: &url,
            endpoint,
            payload: Payload::Json(body),
            bearer: Some(&lease.value),
            timeout: deadline.cap(&inner.clock, tuning.request_timeout),
            max_body_bytes: tuning.max_body_bytes,
        };
        let started = inner.clock.now();
        let response = send_once(client, &attempt).await.map_err(CorpusError::from)?;
        let elapsed = inner.clock.now().saturating_sub(started);
        let credentials = inner.config.credentials();
        let echoes_credentials = echoes(
            &response.body,
            &[
                lease.value.expose_secret(),
                credentials.map_or("", |c| c.client_secret().expose_secret()),
                credentials.map_or("", |c| c.client_id()),
            ],
        );
        Ok(Exchange {
            status: response.status,
            content_type: response.content_type,
            content_encoding: response.content_encoding,
            retry_after: response.retry_after,
            elapsed,
            echoes_credentials,
            body: response.body,
        })
    }
}

/// Vrai si `body` contient, octet pour octet, l'une des valeurs non vides de `secrets`.
fn echoes(body: &[u8], secrets: &[&str]) -> bool {
    secrets
        .iter()
        .filter(|secret| !secret.is_empty())
        .any(|secret| body.windows(secret.len()).any(|window| window == secret.as_bytes()))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use wiremock::matchers::{body_string, header, method, path};
    use wiremock::{Mock, ResponseTemplate};

    use super::*;
    use crate::corpus::tests::{
        Rig, SECRET, TOKEN_PATH, json_reply, ms, rate, requests_to, token_mock,
    };

    const ROUTE: &str = "/dila/legifrance/lf-engine-app/consult/juri";
    const BODY: &[u8] = br#"{"textId":"JURITEXT000000000000"}"#;
    const MARKER: &str = r#"{"error":400,"message":"L'expression à valider est fausse."}"#;

    async fn api(rig: &Rig, template: ResponseTemplate, times: u64) {
        Mock::given(method("POST"))
            .and(path(ROUTE))
            .and(header("authorization", "Bearer tok-1"))
            .and(body_string(String::from_utf8(BODY.to_vec()).unwrap()))
            .respond_with(template)
            .up_to_n_times(times)
            .expect(times)
            .mount(&rig.server)
            .await;
    }

    #[test]
    fn echoes_finds_any_non_empty_secret_byte_for_byte() {
        assert!(echoes(b"x tok-1 y", &["", "tok-1"]));
        assert!(echoes(b"tok-1", &["tok-1"]));
        assert!(!echoes(b"tok-", &["tok-1", ""]));
        assert!(!echoes(b"anything", &[]));
    }

    #[tokio::test]
    async fn an_error_status_is_returned_raw_without_classification() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        api(&rig, json_reply(400, MARKER).insert_header("Retry-After", "7"), 1).await;
        let reply = rig.corpus.exchange("/consult/juri", BODY).await.unwrap();
        assert_eq!(
            (reply.status, reply.body.as_slice(), reply.content_type.as_deref()),
            (400, MARKER.as_bytes(), Some("application/json"))
        );
        assert_eq!((reply.retry_after.as_deref(), reply.echoes_credentials), (Some("7"), false));
        assert_eq!((reply.content_encoding, reply.elapsed), (None, Duration::ZERO));
    }

    #[tokio::test]
    async fn a_503_is_not_retried_and_mojibake_is_not_repaired() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let mojibake = "R\u{c3}\u{a9}publique".as_bytes().to_vec();
        api(&rig, ResponseTemplate::new(503).set_body_raw(mojibake.clone(), "text/plain"), 1).await;
        let reply = rig.corpus.exchange("/consult/juri", BODY).await.unwrap();
        assert_eq!((reply.status, reply.body), (503, mojibake));
        assert_eq!(requests_to(&rig.server, ROUTE).await, 1);
        assert_eq!(rig.corpus.stats(), crate::StatsSnapshot::default());
    }

    #[tokio::test]
    async fn a_body_that_echoes_a_credential_is_flagged() {
        for (echoed, flagged) in
            [("tok-1", true), (SECRET, true), ("my-client-id", true), ("tok-2", false)]
        {
            let rig = Rig::new().await;
            token_mock(&rig.server, "tok-1", 1).await;
            api(&rig, json_reply(403, &format!(r#"{{"echo":"{echoed}"}}"#)), 1).await;
            let reply = rig.corpus.exchange("/consult/juri", BODY).await.unwrap();
            assert_eq!(reply.echoes_credentials, flagged, "{echoed}");
        }
    }

    #[tokio::test]
    async fn the_degraded_mode_and_a_bad_template_make_no_request() {
        let rig = Rig::tuned(|setup| setup.credentials = None).await;
        let missing = rig.corpus.exchange("/consult/juri", BODY).await.unwrap_err();
        assert_eq!(
            missing.to_string(),
            "configuration error: PISTE credentials missing: set LF_PISTE_CLIENT_ID and LF_PISTE_CLIENT_SECRET (from your piste.gouv.fr application, section OAuth Credentials)"
        );
        assert_eq!(rig.server.received_requests().await.unwrap().len(), 0);
        let rig = Rig::new().await;
        assert_eq!(
            rig.corpus.exchange("consult/juri", BODY).await,
            Err(CorpusError::InvalidRequest {
                status: None,
                detail: "endpoint template must start with a slash".to_owned()
            })
        );
        assert_eq!(rig.server.received_requests().await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn an_unbuildable_client_or_an_exhausted_quota_is_an_error() {
        let rig = Rig::tuned(|setup| setup.user_agent = "bad\nagent".to_owned()).await;
        assert_eq!(
            rig.corpus.exchange("/consult/juri", BODY).await,
            Err(CorpusError::Config {
                detail:
                    "could not build the HTTP client: builder error: failed to parse header value"
                        .to_owned()
            })
        );
        let rig = Rig::tuned(|setup| {
            setup.tuning.rate = rate(1, 1);
            setup.tuning.tool_deadline = ms(500);
        })
        .await;
        token_mock(&rig.server, "tok-1", 1).await;
        api(&rig, json_reply(200, "{}"), 1).await;
        assert_eq!(
            rig.corpus.exchange("/consult/juri", BODY).await.map(|reply| reply.status),
            Ok(200)
        );
        assert_eq!(
            rig.corpus.exchange("/consult/juri", BODY).await,
            Err(CorpusError::RateLimited { retry_after: Some(Duration::from_secs(1)) })
        );
    }

    #[tokio::test]
    async fn a_token_failure_or_an_oversized_body_is_an_error() {
        let rig = Rig::new().await;
        Mock::given(method("POST"))
            .and(path(TOKEN_PATH))
            .respond_with(json_reply(401, r#"{"error":"invalid_client"}"#))
            .mount(&rig.server)
            .await;
        let refused = rig.corpus.exchange("/consult/juri", BODY).await.unwrap_err();
        assert_eq!(refused.to_string(), "credentials rejected");
        assert_eq!(requests_to(&rig.server, ROUTE).await, 0);
        let rig = Rig::tuned(|setup| setup.tuning.max_body_bytes = 100).await;
        token_mock(&rig.server, "tok-1", 1).await;
        api(&rig, json_reply(200, &" ".repeat(101)), 1).await;
        assert_eq!(
            rig.corpus.exchange("/consult/juri", BODY).await,
            Err(CorpusError::Decode {
                detail: "response too large (> 100 bytes) on /consult/juri".to_owned()
            })
        );
    }
}
