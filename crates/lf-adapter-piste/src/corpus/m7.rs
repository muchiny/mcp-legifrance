//! Opérations de M7 de `PisteCorpus` : `article_versions` et `related_links` (04 §7.3).
//!
//! STUB(V0) → V2-09 : signatures gelées par la vague 0, corps `todo!()`. `corpus::port` y délègue.

use lf_application::ports::CorpusError;
use lf_domain::{
    entity::{ArticleVersion, RelatedLinks},
    id::ArticleId,
};

use super::PisteCorpus;
use crate::time::{MonoClock, Sleeper};

impl<K: MonoClock + Clone, S: Sleeper + Clone> PisteCorpus<K, S> {
    /// Versions d'un article (`canonicalArticleVersion`, repli `getArticleByCid`).
    #[expect(clippy::todo, reason = "STUB(V0) → V2-09")]
    pub(super) async fn article_versions_impl(
        &self,
        _id: &ArticleId,
    ) -> Result<Vec<ArticleVersion>, CorpusError> {
        todo!("STUB(V0) → V2-09")
    }

    /// Liens d'un article (`relatedLinksArticle`).
    #[expect(clippy::todo, reason = "STUB(V0) → V2-09")]
    pub(super) async fn related_links_impl(
        &self,
        _id: &ArticleId,
    ) -> Result<RelatedLinks, CorpusError> {
        todo!("STUB(V0) → V2-09")
    }
}
