#![no_main]

use lf_application::ports::CorpusError;
use libfuzzer_sys::fuzz_target;

/// Plus long détail possible : 127 niveaux de 64 caractères d'identifiant, un point et un indice.
const MAX_DETAIL: usize = 16 * 1024;

/// Marqueur d'une valeur hostile du corps : présent dans l'entrée, il ne doit jamais ressortir
/// dans la catégorie d'un détail (04 §8.5 : aucune valeur du corps dans un message).
const MARKER: &str = "sekret";

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
/// `Decode` dont le détail a la forme `<endpoint>: <chemin>: <catégorie>`, n'est fait que d'ASCII
/// imprimable et tient en [`MAX_DETAIL`] octets ; il ne contient jamais `decoding budget not
/// armed` (une liste lue hors de la fenêtre armée par `decode`), et sa catégorie ne contient
/// jamais [`MARKER`] quand l'entrée le contient (une valeur du corps recopiée, même en ASCII). Un
/// second décodage du même texte sur le même fil rend le même résultat (garde du budget tombée,
/// décodage déterministe).
fn check<T: serde::de::DeserializeOwned>(text: &str, endpoint: &str) {
    let first = dto::decode::decode::<T>(text, endpoint).map(drop);
    match &first {
        Ok(()) => {}
        Err(CorpusError::Decode { detail }) => {
            assert!(
                detail.bytes().all(|byte| byte.is_ascii_graphic() || byte == b' '),
                "detail not printable ASCII: {detail:?}"
            );
            assert!(detail.len() <= MAX_DETAIL, "detail too long: {}", detail.len());
            assert!(!detail.contains(dto::budget::NOT_ARMED), "list read unarmed: {detail}");
            let category = category_of(detail, endpoint);
            if text.contains(MARKER) {
                assert!(!category.contains(MARKER), "body value in the detail: {detail}");
            }
        }
        Err(other) => panic!("decode failed with a non-Decode error: {other:?}"),
    }
    assert_eq!(first, dto::decode::decode::<T>(text, endpoint).map(drop), "second decode");
}

/// Catégorie du détail `<endpoint>: <chemin>: <catégorie>`.
///
/// Le chemin n'a jamais `": "` : il n'est fait que de noms de champs, de clés
/// `[A-Za-z0-9_-]{1,64}`, de `?`, de points et d'indices (`decode::render_path`). Il est exclu du
/// contrôle du marqueur, car une clé du corps de forme identifiant y entre par une limite assumée
/// (`{"sekret":[1,}` donne le chemin `sekret`, 04 §8.5) ; une valeur, elle, n'y entre jamais.
fn category_of<'a>(detail: &'a str, endpoint: &str) -> &'a str {
    detail
        .strip_prefix(endpoint)
        .and_then(|rest| rest.strip_prefix(": "))
        .and_then(|rest| rest.split_once(": "))
        .map(|(_path, category)| category)
        .unwrap_or_else(|| panic!("detail is not <endpoint>: <path>: <category>: {detail}"))
}
