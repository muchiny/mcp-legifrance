#![no_main]

use lf_application::ports::CorpusError;
use libfuzzer_sys::fuzz_target;

/// Plus long détail possible : 127 niveaux de 64 caractères d'identifiant, un point et un indice.
const MAX_DETAIL: usize = 16 * 1024;

/// Sources de production des DTO de l'adapter PISTE, incluses telles quelles (D7) : ni feature ni
/// `#[doc(hidden)]` dans l'adapter. Leurs modules `#[cfg(test)]` ne sont pas compilés ici. La cible
/// ne lit que le résultat de `decode` : les champs des DTO et les lecteurs que seul l'adapter
/// emploie restent inutilisés, d'où `dead_code`.
#[allow(dead_code, unreachable_pub, reason = "la cible n'emploie qu'une part des DTO inclus")]
// Le `#[path]` d'un module en ligne fixe le dossier de ses sous-modules, relatif au dossier de ce
// fichier : deux `../` mènent de `fuzz/fuzz_targets/` à la racine du dépôt. Un `#[path]` par
// fichier serait relatif à `fuzz/fuzz_targets/dto/`, dossier absent que `..` ne traverse pas.
#[path = "../../crates/lf-adapter-piste/src/dto"]
mod dto {
    // Ni oauth.rs, qui exige secrecy, ni error.rs, qu'aucune réponse fuzzée ne lit.
    pub(crate) mod acco;
    pub(crate) mod article;
    pub(crate) mod budget;
    pub(crate) mod consult_text;
    pub(crate) mod de;
    pub(crate) mod decode;
    pub(crate) mod juri;
    pub(crate) mod search;
}

fuzz_target!(|data: &[u8]| {
    // Le corps a déjà passé l'UTF-8 strict avant `decode` (encoding.rs) : le reste est écarté.
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    check::<dto::search::SearchResponseDto>(text, "/search");
    check::<dto::consult_text::ConsultTextDto>(text, "/consult/lawDecree");
    check::<dto::article::GetArticleResponseDto>(text, "/consult/getArticle");
    check::<dto::juri::ConsultJuriResponseDto>(text, "/consult/juri");
    check::<dto::acco::ConsultAccoResponseDto>(text, "/consult/acco");
});

/// Décode `text` en `T` par `decode` (budget armé) : jamais de panique ; une erreur est un
/// `Decode` dont le détail commence par `<endpoint>: `, n'est fait que d'ASCII imprimable et
/// tient en [`MAX_DETAIL`] octets ; un second décodage du même texte sur le même fil rend le
/// même résultat (garde du budget tombée, décodage déterministe).
fn check<T: serde::de::DeserializeOwned>(text: &str, endpoint: &str) {
    let first = dto::decode::decode::<T>(text, endpoint).map(drop);
    match &first {
        Ok(()) => {}
        Err(CorpusError::Decode { detail }) => {
            assert!(detail.starts_with(&format!("{endpoint}: ")), "detail prefix: {detail}");
            assert!(
                detail.bytes().all(|byte| byte.is_ascii_graphic() || byte == b' '),
                "detail not printable ASCII: {detail:?}"
            );
            assert!(detail.len() <= MAX_DETAIL, "detail too long: {}", detail.len());
        }
        Err(other) => panic!("decode failed with a non-Decode error: {other:?}"),
    }
    assert_eq!(first, dto::decode::decode::<T>(text, endpoint).map(drop), "second decode");
}
