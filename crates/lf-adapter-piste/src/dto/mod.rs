//! Structures de désérialisation des réponses de PISTE (04 §3.2, §8).
//!
//! Ces types ne servent qu'à lire du JSON : ils ne portent aucune règle métier. Ils sont
//! tolérants (champs facultatifs, nombres lus aussi en chaîne) parce que le format réel de
//! PISTE n'est pas garanti.

pub(crate) mod acco;
pub(crate) mod article;
pub(crate) mod consult_text;
pub(crate) mod de;
pub(crate) mod decode;
pub(crate) mod error;
pub(crate) mod juri;
pub(crate) mod oauth;
pub(crate) mod search;
