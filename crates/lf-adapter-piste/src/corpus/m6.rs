//! Opérations de M6 de `PisteCorpus` : `find_article`, `find_by_number` et `list_codes`
//! (04 §7.1 et §7.2, annexe B §6.6).
//!
//! STUB(V0) → V2-08 : signatures gelées par la vague 0, corps `todo!()`. `corpus::port` y délègue.

use lf_application::ports::{ArticleAt, CorpusError};
use lf_domain::{
    entity::{ArticleText, CodeListing, SearchHit},
    id::LegiTextId,
    search::NumberQuery,
};

use super::PisteCorpus;
use crate::time::{MonoClock, Sleeper};

impl<K: MonoClock + Clone, S: Sleeper + Clone> PisteCorpus<K, S> {
    /// Article d'un texte par son numéro : `getArticleWithIdAndNum` pour [`ArticleAt::Today`],
    /// recherche exacte sur `NUM_ARTICLE` et `DATE_VERSION` pour [`ArticleAt::On`].
    #[expect(clippy::todo, reason = "STUB(V0) → V2-08")]
    pub(super) async fn find_article_impl(
        &self,
        _text: &LegiTextId,
        _num: &str,
        _at: ArticleAt,
    ) -> Result<ArticleText, CorpusError> {
        todo!("STUB(V0) → V2-08")
    }

    /// Recherche par numéro (affaire, décision, délibération, IDCC, NOR, ECLI, ELI...).
    #[expect(clippy::todo, reason = "STUB(V0) → V2-08")]
    pub(super) async fn find_by_number_impl(
        &self,
        _query: &NumberQuery,
    ) -> Result<Vec<SearchHit>, CorpusError> {
        todo!("STUB(V0) → V2-08")
    }

    /// Liste des codes (`/list/code`), abrogés compris.
    #[expect(clippy::todo, reason = "STUB(V0) → V2-08")]
    pub(super) async fn list_codes_impl(&self) -> Result<Vec<CodeListing>, CorpusError> {
        todo!("STUB(V0) → V2-08")
    }
}
