//! Arbitrage des chevauchements et post-traitement (annexe B §4.15, §4.16, §4.18).
//!
//! L'arbitrage trie les candidats (longueur décroissante, puis priorité), retient les candidats qui
//! ne chevauchent aucun span déjà accepté, applique la règle des puits et celle des
//! énumérations, puis la pile de contexte (dernier code, dernier texte, dernière décision) et les
//! anaphores. Les plages des candidats sont déjà des offsets du texte d'origine.
//!
//! STUB(V0) → V2-13 : [`arbitrate`] ne rend aucune citation pour l'instant.

use crate::citation::ParseOptions;
use crate::citation::grammar::Candidate;
use crate::citation::types::{CitationRef, Spanned};

/// Citations retenues, triées par début de span puis par longueur décroissante.
pub(crate) fn arbitrate(
    _candidates: Vec<Candidate>,
    _opts: &ParseOptions,
) -> Vec<Spanned<CitationRef>> {
    // STUB(V0) → V2-13
    Vec::new()
}
