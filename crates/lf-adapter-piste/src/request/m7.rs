//! Corps des requêtes de M7 envoyées à PISTE : `canonicalArticleVersion`, `getArticleByCid` et
//! `relatedLinksArticle` (04 §7.3).
//!
//! Comme pour `request::consult`, l'identifiant va dans le corps, jamais dans le chemin : le
//! gabarit d'endpoint ne dépend que de la variante. L'identifiant vient du domaine, déjà normalisé
//! (casse, blancs, suffixe de version retirés). Les clés diffèrent selon l'endpoint : `id` pour
//! `canonicalArticleVersion`, `cid` pour `getArticleByCid`, `articleId` pour `relatedLinksArticle`.
//! `getArticleByCid` reçoit l'identifiant de l'article demandé comme `cid` : l'API rend toutes les
//! versions de l'article que cet identifiant désigne (fixture enregistrée de l'article 9 du Code
//! civil).

use lf_domain::id::ArticleId;
use serde_json::json;

use crate::map::m7::{CANONICAL_ARTICLE_VERSION, GET_ARTICLE_BY_CID, RELATED_LINKS_ARTICLE};

/// Une requête de M7 et l'article qu'elle vise, empruntés à l'appelant.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Navigation<'a> {
    /// Versions d'un article, métadonnées seules : `{id}`.
    CanonicalVersions(&'a ArticleId),
    /// Toutes les versions d'un article, texte compris (repli) : `{cid}`.
    ArticleByCid(&'a ArticleId),
    /// Liens d'un article, dans les deux sens : `{articleId}`.
    RelatedLinks(&'a ArticleId),
}

impl Navigation<'_> {
    /// Gabarit d'endpoint de `map::m7`, sans paramètre.
    pub(crate) fn endpoint(self) -> &'static str {
        match self {
            Self::CanonicalVersions(_) => CANONICAL_ARTICLE_VERSION,
            Self::ArticleByCid(_) => GET_ARTICLE_BY_CID,
            Self::RelatedLinks(_) => RELATED_LINKS_ARTICLE,
        }
    }

    /// Corps JSON de la requête, infaillible.
    pub(crate) fn body(self) -> Vec<u8> {
        let body = match self {
            Self::CanonicalVersions(id) => json!({ "id": id.as_str() }),
            Self::ArticleByCid(id) => json!({ "cid": id.as_str() }),
            Self::RelatedLinks(id) => json!({ "articleId": id.as_str() }),
        };
        body.to_string().into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use lf_domain::id::ArticleId;
    use serde_json::{Value, json};

    use super::*;

    fn body_of(navigation: Navigation<'_>) -> Value {
        serde_json::from_slice(&navigation.body()).expect("the body is JSON")
    }

    fn article(raw: &str) -> ArticleId {
        ArticleId::parse(raw).expect("an article identifier")
    }

    #[test]
    fn each_navigation_has_its_endpoint_and_its_body_key() {
        let legi = article("LEGIARTI000006419288");
        let kali = article("KALIARTI000049886059");
        let table = [
            (
                Navigation::CanonicalVersions(&legi),
                "/search/canonicalArticleVersion",
                json!({"id": "LEGIARTI000006419288"}),
            ),
            (
                Navigation::ArticleByCid(&legi),
                "/consult/getArticleByCid",
                json!({"cid": "LEGIARTI000006419288"}),
            ),
            (
                Navigation::RelatedLinks(&legi),
                "/consult/relatedLinksArticle",
                json!({"articleId": "LEGIARTI000006419288"}),
            ),
            (
                Navigation::RelatedLinks(&kali),
                "/consult/relatedLinksArticle",
                json!({"articleId": "KALIARTI000049886059"}),
            ),
        ];
        for (navigation, endpoint, body) in table {
            assert_eq!((navigation.endpoint(), body_of(navigation)), (endpoint, body));
        }
    }

    #[test]
    fn the_body_carries_the_normalized_identifier() {
        let id = article("  legiarti000006419288_01-01-2020 ");
        assert_eq!(
            body_of(Navigation::CanonicalVersions(&id)),
            json!({"id": "LEGIARTI000006419288"})
        );
    }

    #[test]
    fn an_endpoint_template_never_holds_an_identifier() {
        let id = article("LEGIARTI000006419288");
        for navigation in [
            Navigation::CanonicalVersions(&id),
            Navigation::ArticleByCid(&id),
            Navigation::RelatedLinks(&id),
        ] {
            let endpoint = navigation.endpoint();
            assert!(!endpoint.contains("LEGIARTI"));
            assert!(endpoint.chars().all(|c| c.is_ascii_alphabetic() || c == '/'));
        }
    }
}
