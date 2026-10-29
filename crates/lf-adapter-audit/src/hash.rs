//! Empreinte `input_hash` d'une entrée canonique (06 §11.4, P-041).
//!
//! L'adapter entrant (`lf-adapter-mcp::canonical`) produit la forme canonique « Python » de
//! l'objet `arguments` brut (clés triées, séparateurs compacts, échappement `ensure_ascii`) ; ce
//! module n'en calcule que le SHA-256 hexadécimal minuscule. L'empreinte n'est pas salée : pour
//! une entrée courte, elle se retrouve par énumération (D-17, 00 §7).

use lf_application::input::CanonicalInput;
use sha2::{Digest, Sha256};

/// SHA-256 hexadécimal minuscule (64 caractères) des octets UTF-8 de la forme canonique.
pub(crate) fn input_hash(input: &CanonicalInput) -> String {
    hex::encode(Sha256::digest(input.as_str().as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash_of(canonical: &str) -> String {
        input_hash(&CanonicalInput::new(canonical))
    }

    /// Les 8 vecteurs de l'annexe A §6.6 (recherche 01 §8) : forme canonique `ensure_ascii`,
    /// avec ses `\uXXXX` en minuscules, puis SHA-256.
    #[test]
    fn input_hash_matches_p041_vectors() {
        let vectors = [
            ("{}", "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"),
            (
                r#"{"fond":"LODA","page_size":5,"query":"republique numerique"}"#,
                "44a8ee11979f621ab2584f279aca6523f357c5cd2d4adcb89550253865beb0d3",
            ),
            (
                r#"{"fond":"CODE","page_size":10,"query":"vie priv\u00e9e"}"#,
                "60f4c0810787e4b88d338d513b2306aacbba2ecdec2c44346cae299ce87ecd14",
            ),
            (
                r#"{"fond":"CODE","page_size":10,"query":"  vie priv\u00e9e "}"#,
                "8e247a13f8c46327e3e3940cbbf85a1c9af59212878927af334a424dce056709",
            ),
            (
                r#"{"fond":"JURI","page_size":3,"query":"emoji \ud83d\ude00"}"#,
                "b097b014240c833f1347c250d05463c855488144feb98eba1c36751007f65b79",
            ),
            (
                r#"{"article_id":"LEGIARTI000006419288"}"#,
                "ae5137eaa92ffcd7e3948ae676e95ef72bb9d6535df3f141d31f67344873568a",
            ),
            (
                r#"{"date":"2026-10-15","text_id":"LEGITEXT000033205014"}"#,
                "d8a8e991b913e763ce637f43b0b983a647d7d106aca8e6c13c4dabb1199e2183",
            ),
            (
                r#"{"decision_id":"CONSTEXT000052555275"}"#,
                "26eab19dad0d8885c5c5d5ab0716f3eccfb7f14ca193c26e8378e111203fad48",
            ),
        ];
        for (canonical, expected) in vectors {
            assert_eq!(hash_of(canonical), expected, "{canonical}");
        }
    }

    /// Vecteur d'échappement de 06 §11.4 : DEL (U+007F) et U+2028 en `\uXXXX` minuscules.
    #[test]
    fn input_hash_escape_vector() {
        assert_eq!(
            hash_of(r#"{"q":"a\u007fb\u2028c"}"#),
            "e572d0bdd4ef1a25a73c32790c9d221a7050b5684cbda7cc2c6c7f178fa44928"
        );
    }

    /// L'empreinte porte sur les octets UTF-8 de la chaîne reçue, sans la renormaliser : un `é`
    /// brut et son échappement `\u00e9` donnent deux empreintes (l'adapter entrant échappe).
    #[test]
    fn input_hash_is_over_the_received_bytes() {
        assert_ne!(hash_of(r#"{"q":"é"}"#), hash_of(r#"{"q":"\u00e9"}"#));
        let hash = hash_of("{}");
        assert_eq!(hash.len(), 64);
        assert!(hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')), "{hash}");
    }
}
