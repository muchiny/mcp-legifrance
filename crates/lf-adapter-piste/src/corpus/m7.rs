//! Opérations de M7 de `PisteCorpus` : `article_versions` et `related_links` (04 §7.3).
//! `corpus::port` y délègue.
//!
//! Les trois endpoints sont des **consultations** (`EndpointKind::Consult`), y compris
//! `/search/canonicalArticleVersion` malgré son chemin : un identifiant inconnu y donne un 200 sans
//! aucun octet (fixture `inconnu`), que `classify` range en « introuvable » sans réessai pour une
//! consultation seulement, alors qu'il serait une erreur de décodage pour une recherche. Ce n'est
//! pas une recherche de textes : zéro version n'est pas un succès.

use lf_application::ports::CorpusError;
use lf_domain::{
    entity::{ArticleVersion, RelatedLinks},
    id::ArticleId,
};
use serde::de::DeserializeOwned;

use super::PisteCorpus;
use crate::classify::EndpointKind;
use crate::map::Diagnostics;
use crate::map::m7::{article_versions, listed_article_versions, related_links};
use crate::request::m7::Navigation;
use crate::time::{MonoClock, Sleeper};

impl<K: MonoClock + Clone, S: Sleeper + Clone> PisteCorpus<K, S> {
    /// Une requête de M7 : `fetch(EndpointKind::Consult, navigation.endpoint(),
    /// &navigation.body(), map)`.
    async fn navigate<D: DeserializeOwned, T>(
        &self,
        navigation: Navigation<'_>,
        map: impl FnOnce(D, &mut Diagnostics) -> Result<T, CorpusError>,
    ) -> Result<T, CorpusError> {
        self.fetch(EndpointKind::Consult, navigation.endpoint(), &navigation.body(), map).await
    }

    /// Versions d'un article (`canonicalArticleVersion`, repli `getArticleByCid`), de la plus
    /// ancienne à la plus récente.
    ///
    /// `canonicalArticleVersion` rend des métadonnées légères. Le repli sur `getArticleByCid`
    /// (toutes les versions avec leur texte, réduites à des `ArticleVersion`) joue quand la liste
    /// est vide, ou que l'appel est « introuvable » : un 404 de l'hôte officiel, un 400 au marqueur
    /// d'identifiant inconnu, ou un 200 sans aucun octet. Toute autre erreur est rendue telle
    /// quelle, sans repli.
    ///
    /// # Errors
    ///
    /// [`CorpusError::NotFound`] (`/consult/getArticleByCid`) si le repli ne trouve aucune
    /// version ; sinon l'erreur de l'appel qui a échoué (voir `PisteCorpus::fetch`).
    pub(super) async fn article_versions_impl(
        &self,
        id: &ArticleId,
    ) -> Result<Vec<ArticleVersion>, CorpusError> {
        match self.navigate(Navigation::CanonicalVersions(id), article_versions).await {
            Ok(versions) if !versions.is_empty() => Ok(versions),
            Ok(_) | Err(CorpusError::NotFound { .. }) => {
                self.navigate(Navigation::ArticleByCid(id), listed_article_versions).await
            }
            Err(error) => Err(error),
        }
    }

    /// Liens d'un article (`relatedLinksArticle`), dans les deux sens, sans borne : la borne de
    /// 100 liens relève de l'application (05 §5.11).
    ///
    /// # Errors
    ///
    /// Celles de `PisteCorpus::fetch`. Un article inconnu donne un 500 en sandbox (fixture
    /// `inconnu`) : [`CorpusError::Upstream`], jamais `NotFound` (04 §11.1).
    pub(super) async fn related_links_impl(
        &self,
        id: &ArticleId,
    ) -> Result<RelatedLinks, CorpusError> {
        self.navigate(Navigation::RelatedLinks(id), related_links).await
    }
}

#[cfg(test)]
mod tests {
    use lf_application::ports::{CorpusError, LegalCorpus};
    use lf_domain::entity::{ArticleVersion, RelatedLinks};
    use lf_domain::id::ArticleId;
    use lf_domain::status::LegalStatus;
    use serde_json::{Value, json};
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, ResponseTemplate};

    use crate::corpus::tests::{API_BASE, Rig, json_reply, never, requests_to, token_mock};
    use crate::map::m7::{CANONICAL_ARTICLE_VERSION, GET_ARTICLE_BY_CID, RELATED_LINKS_ARTICLE};

    /// Article 9 du Code civil, dont les trois fixtures ont été enregistrées.
    const ARTICLE: &str = "LEGIARTI000006419288";

    /// `canonicalArticleVersion` : trois versions de l'article 9.
    const CANONICAL: &str = include_str!(
        "../../tests/fixtures/recorded/search/canonicalArticleVersion/m7/canonicalArticleVersion-LEGIARTI000006419288.json"
    );
    /// `canonicalArticleVersion` sur un identifiant inconnu : un 200 sans aucun octet.
    const CANONICAL_UNKNOWN: &str = include_str!(
        "../../tests/fixtures/recorded/search/canonicalArticleVersion/m7/canonicalArticleVersion-inconnu.json"
    );
    /// `getArticleByCid` : les trois versions de l'article 9, texte compris.
    const BY_CID: &str = include_str!(
        "../../tests/fixtures/recorded/consult/getArticleByCid/m7/getArticleByCid-LEGIARTI000006419288.json"
    );
    /// `relatedLinksArticle` : quatorze liens « cité par ».
    const RELATED: &str = include_str!(
        "../../tests/fixtures/recorded/consult/relatedLinksArticle/m7/relatedLinksArticle-LEGIARTI000006419288.json"
    );
    /// `relatedLinksArticle` d'un article sans lien.
    const RELATED_NONE: &str = include_str!(
        "../../tests/fixtures/recorded/consult/relatedLinksArticle/m7/relatedLinksArticle-LEGIARTI000033205058.json"
    );
    /// `relatedLinksArticle` sur un identifiant inconnu : un 500 « exception non gérée ».
    const RELATED_UNKNOWN: &str = include_str!(
        "../../tests/fixtures/recorded/consult/relatedLinksArticle/m7/relatedLinksArticle-inconnu.json"
    );
    /// Le 400 de PISTE pour un identifiant inconnu (04 §11.1).
    const UNKNOWN_ID_400: &str = r#"{"timestamp":1793269810331,"error":400,"status":"Bad Request","message":"L'expression à valider est fausse.","errorCode":null,"exceptionDetails":null}"#;

    /// Identifiants des trois versions de l'article 9, de la plus ancienne à la plus récente.
    const OLDEST_FIRST: [&str; 3] =
        ["LEGIARTI000050396803", "LEGIARTI000050396821", "LEGIARTI000006419288"];

    fn article() -> ArticleId {
        ArticleId::parse(ARTICLE).unwrap()
    }

    fn route(endpoint: &str) -> String {
        format!("{API_BASE}{endpoint}")
    }

    /// Un 200 dont le corps ne compte aucun octet.
    fn no_byte() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_raw(CANONICAL_UNKNOWN, "application/json")
    }

    /// La requête de corps `request` à `endpoint` reçoit `reply`, une fois exactement.
    async fn serve(rig: &Rig, endpoint: &str, request: &Value, reply: ResponseTemplate) {
        Mock::given(method("POST"))
            .and(path(route(endpoint)))
            .and(body_json(request))
            .respond_with(reply)
            .up_to_n_times(1)
            .expect(1)
            .mount(&rig.server)
            .await;
    }

    /// Un `Rig` dont le jeton est prêt, pour recevoir les mocks de l'API.
    async fn rig() -> Rig {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        rig
    }

    fn ids(versions: &[ArticleVersion]) -> Vec<&str> {
        versions.iter().map(|version| version.id.as_str()).collect()
    }

    #[test]
    fn the_recorded_unknown_cases_are_what_the_tests_below_assume() {
        assert_eq!(CANONICAL_UNKNOWN, "");
        assert!(RELATED_UNKNOWN.contains(r#""error":500"#));
    }

    #[tokio::test]
    async fn article_versions_canonical_then_fallback_by_cid() {
        let rig = rig().await;
        serve(
            &rig,
            CANONICAL_ARTICLE_VERSION,
            &json!({"id": ARTICLE}),
            json_reply(200, r#"{"articleVersions":[]}"#),
        )
        .await;
        serve(&rig, GET_ARTICLE_BY_CID, &json!({"cid": ARTICLE}), json_reply(200, BY_CID)).await;
        let versions = rig.corpus.article_versions(&article()).await.unwrap();
        assert_eq!(ids(&versions), OLDEST_FIRST);
        // Le repli porte le numéro de l'article, que `canonicalArticleVersion` laisse à `null`.
        assert!(versions.iter().all(|version| version.num.as_deref() == Some("9")));
        assert_eq!(versions[2].etat, Some(LegalStatus::Vigueur));
        assert_eq!(versions[2].date_fin, None);
    }

    #[tokio::test]
    async fn article_versions_use_the_canonical_list_when_it_has_versions() {
        let rig = rig().await;
        serve(&rig, CANONICAL_ARTICLE_VERSION, &json!({"id": ARTICLE}), json_reply(200, CANONICAL))
            .await;
        never(&rig.server, &route(GET_ARTICLE_BY_CID)).await;
        let versions = rig.corpus.article_versions(&article()).await.unwrap();
        assert_eq!(ids(&versions), OLDEST_FIRST);
        assert!(versions.iter().all(|version| version.num.is_none()));
        assert_eq!(requests_to(&rig.server, &route(GET_ARTICLE_BY_CID)).await, 0);
    }

    #[tokio::test]
    async fn article_versions_fall_back_when_canonical_answers_without_a_byte() {
        let rig = rig().await;
        serve(&rig, CANONICAL_ARTICLE_VERSION, &json!({"id": ARTICLE}), no_byte()).await;
        serve(&rig, GET_ARTICLE_BY_CID, &json!({"cid": ARTICLE}), json_reply(200, BY_CID)).await;
        let versions = rig.corpus.article_versions(&article()).await.unwrap();
        assert_eq!(ids(&versions), OLDEST_FIRST);
    }

    #[tokio::test]
    async fn article_versions_fall_back_when_canonical_reports_an_unknown_id() {
        let rig = rig().await;
        serve(
            &rig,
            CANONICAL_ARTICLE_VERSION,
            &json!({"id": ARTICLE}),
            json_reply(400, UNKNOWN_ID_400),
        )
        .await;
        serve(&rig, GET_ARTICLE_BY_CID, &json!({"cid": ARTICLE}), json_reply(200, BY_CID)).await;
        let versions = rig.corpus.article_versions(&article()).await.unwrap();
        assert_eq!(ids(&versions), OLDEST_FIRST);
    }

    #[tokio::test]
    async fn article_versions_do_not_fall_back_on_any_other_error() {
        let rig = rig().await;
        serve(
            &rig,
            CANONICAL_ARTICLE_VERSION,
            &json!({"id": ARTICLE}),
            json_reply(400, r#"{"message":"Paramètre invalide"}"#),
        )
        .await;
        never(&rig.server, &route(GET_ARTICLE_BY_CID)).await;
        let error = rig.corpus.article_versions(&article()).await.unwrap_err();
        assert!(
            matches!(error, CorpusError::InvalidRequest { status: Some(400), .. }),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_404_from_a_custom_host_is_a_config_error_without_fallback() {
        // Hors hôte officiel, un 404 ne prouve pas l'absence (04 §11.2) : pas de repli.
        let rig = rig().await;
        serve(&rig, CANONICAL_ARTICLE_VERSION, &json!({"id": ARTICLE}), ResponseTemplate::new(404))
            .await;
        never(&rig.server, &route(GET_ARTICLE_BY_CID)).await;
        let error = rig.corpus.article_versions(&article()).await.unwrap_err();
        assert!(matches!(error, CorpusError::Config { .. }), "{error:?}");
    }

    #[tokio::test]
    async fn article_versions_are_not_found_when_the_fallback_has_none() {
        for fallback in [
            json_reply(200, r#"{"executionTime":0,"listArticle":[]}"#),
            no_byte(),
            json_reply(400, UNKNOWN_ID_400),
        ] {
            let rig = rig().await;
            serve(&rig, CANONICAL_ARTICLE_VERSION, &json!({"id": ARTICLE}), no_byte()).await;
            serve(&rig, GET_ARTICLE_BY_CID, &json!({"cid": ARTICLE}), fallback).await;
            assert_eq!(
                rig.corpus.article_versions(&article()).await,
                Err(CorpusError::NotFound { what: GET_ARTICLE_BY_CID.to_owned() })
            );
        }
    }

    #[tokio::test]
    async fn article_versions_of_a_kali_article_are_requested_by_that_identifier() {
        let kali = "KALIARTI000049886059";
        let rig = rig().await;
        serve(&rig, CANONICAL_ARTICLE_VERSION, &json!({"id": kali}), json_reply(200, "{}")).await;
        serve(&rig, GET_ARTICLE_BY_CID, &json!({"cid": kali}), json_reply(200, BY_CID)).await;
        let versions = rig.corpus.article_versions(&ArticleId::parse(kali).unwrap()).await;
        assert_eq!(ids(&versions.unwrap()), OLDEST_FIRST);
    }

    #[tokio::test]
    async fn related_links_come_from_the_recorded_response() {
        let rig = rig().await;
        serve(
            &rig,
            RELATED_LINKS_ARTICLE,
            &json!({"articleId": ARTICLE}),
            json_reply(200, RELATED),
        )
        .await;
        let links = rig.corpus.related_links(&article()).await.unwrap();
        assert_eq!(links.cites, Vec::new());
        assert_eq!(links.cited_by.len(), 14);
        assert_eq!(
            links.cited_by[0].title.as_deref(),
            Some("Délibération n° 2008-161 du 3 juin 2008, v. init.")
        );
    }

    #[tokio::test]
    async fn related_links_of_an_article_without_links_are_empty() {
        let rig = rig().await;
        let other = "LEGIARTI000033205058";
        serve(
            &rig,
            RELATED_LINKS_ARTICLE,
            &json!({"articleId": other}),
            json_reply(200, RELATED_NONE),
        )
        .await;
        let links = rig.corpus.related_links(&ArticleId::parse(other).unwrap()).await.unwrap();
        assert_eq!(links, RelatedLinks::default());
    }

    #[tokio::test]
    async fn related_links_of_an_unknown_article_are_the_recorded_500() {
        // Observé en sandbox le 2026-10-29 : un 500 « exception non gérée », jamais un 404 ni un
        // 400. Il reste une erreur amont (04 §11.1 : un 500 n'est jamais « introuvable »).
        let rig = rig().await;
        Mock::given(method("POST"))
            .and(path(route(RELATED_LINKS_ARTICLE)))
            .respond_with(json_reply(500, RELATED_UNKNOWN))
            .mount(&rig.server)
            .await;
        let error = rig.corpus.related_links(&article()).await.unwrap_err();
        assert!(
            matches!(error, CorpusError::Upstream { status: Some(500), retryable: false, .. }),
            "{error:?}"
        );
    }
}
