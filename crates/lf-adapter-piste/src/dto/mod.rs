//! Structures de désérialisation des réponses de PISTE (04 §3.2, §8).
//!
//! Ces types ne servent qu'à lire du JSON : ils ne portent aucune règle métier. Ils sont
//! tolérants (champs facultatifs, nombres lus aussi en chaîne) parce que le format réel de
//! PISTE n'est pas garanti.

pub(crate) mod de;
pub(crate) mod oauth;
