//! Intégrité des fixtures de référence (04 §13.3 ; 06 §4.4 point 5, §4.6 ; D-35).
//!
//! Ce module ne contient que des tests et leurs aides. Il garde les seize réponses brutes de
//! `tests/fixtures/reference` et le golden de parité : chaque fichier a son sidecar `.meta.toml`
//! (source, endpoint, fond, `sha256_raw`, anomalies connues, environnement, date de capture), le
//! SHA-256 des octets des fixtures correspond à celui du sidecar (celui de la copie du golden est
//! contrôlé par `golden`), le répertoire ne contient rien d'autre, et ni les fixtures, ni les
//! dix-sept snapshots `insta`, ni le golden ne portent de chaîne qui ressemble à un secret
//! (D-35) : mot d'authentification (`bearer`, `access_token`…, quelle que soit la casse), UUID,
//! suite hexadécimale de 32 caractères ou plus, JWT, suite base64 de 40 caractères ou plus (deux
//! exemptions déclarées, `BASE64_LOOKALIKES`).
//!
//! [`sha256`] est écrit ici (FIPS 180-4) pour ne tirer aucune dépendance : il n'existe que pour
//! ces contrôles. Le module n'existe que sous `cfg(test)`.

use std::fmt::Write as _;

use serde_json::Value;

use crate::map::fixtures::{ALL, Fixture, Kind};
use crate::view::leaf_strings;

/// Valeurs initiales du hachage (FIPS 180-4, §5.3.3).
const H0: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// Constantes de ronde (FIPS 180-4, §4.2.2).
const K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// SHA-256 de `bytes` (FIPS 180-4) : les blocs entiers par `as_chunks`, puis le reste complété
/// (`0x80`, des zéros, la longueur en bits sur huit octets) en un ou deux derniers blocs.
pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut state = H0;
    let (blocks, tail) = bytes.as_chunks::<64>();
    for block in blocks {
        compress(&mut state, block);
    }
    let mut last = tail.to_vec();
    last.push(0x80);
    while last.len() % 64 != 56 {
        last.push(0);
    }
    let bit_len = u64::try_from(bytes.len()).expect("length fits in u64") << 3;
    last.extend_from_slice(&bit_len.to_be_bytes());
    for block in last.as_chunks::<64>().0 {
        compress(&mut state, block);
    }
    let mut digest = [0_u8; 32];
    for (out, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(state) {
        *out = word.to_be_bytes();
    }
    digest
}

/// Fonction de compression de FIPS 180-4 (§6.2.2) : mêle un bloc de 64 octets à l'état.
#[expect(clippy::many_single_char_names, reason = "notation de FIPS 180-4 : a à h et w")]
fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0_u32; 64];
    for (word, chunk) in w.iter_mut().zip(block.as_chunks::<4>().0) {
        *word = u32::from_be_bytes(*chunk);
    }
    for t in 16..64 {
        let s0 = w[t - 15].rotate_right(7) ^ w[t - 15].rotate_right(18) ^ (w[t - 15] >> 3);
        let s1 = w[t - 2].rotate_right(17) ^ w[t - 2].rotate_right(19) ^ (w[t - 2] >> 10);
        w[t] = w[t - 16].wrapping_add(s0).wrapping_add(w[t - 7]).wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for (k, word) in K.iter().zip(w) {
        let big_s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let t1 = h.wrapping_add(big_s1).wrapping_add(ch).wrapping_add(*k).wrapping_add(word);
        let big_s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = big_s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (slot, mixed) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *slot = slot.wrapping_add(mixed);
    }
}

/// Chiffres hexadécimaux minuscules de `digest`, deux par octet.
pub(crate) fn hex(digest: &[u8]) -> String {
    digest.iter().fold(String::with_capacity(digest.len() * 2), |mut out, byte| {
        write!(out, "{byte:02x}").expect("writing to a String cannot fail");
        out
    })
}

/// Nom de chaque sidecar `.meta.toml` et son contenu : les seize fixtures puis le golden.
const SIDECARS: [(&str, &str); 17] = [
    ("consult_acco", include_str!("../tests/fixtures/reference/consult_acco.meta.toml")),
    ("consult_article", include_str!("../tests/fixtures/reference/consult_article.meta.toml")),
    ("consult_cnil", include_str!("../tests/fixtures/reference/consult_cnil.meta.toml")),
    ("consult_juri", include_str!("../tests/fixtures/reference/consult_juri.meta.toml")),
    (
        "consult_juri_cetat",
        include_str!("../tests/fixtures/reference/consult_juri_cetat.meta.toml"),
    ),
    (
        "consult_juri_constit",
        include_str!("../tests/fixtures/reference/consult_juri_constit.meta.toml"),
    ),
    ("consult_kali", include_str!("../tests/fixtures/reference/consult_kali.meta.toml")),
    ("consult_loda", include_str!("../tests/fixtures/reference/consult_loda.meta.toml")),
    ("search_acco", include_str!("../tests/fixtures/reference/search_acco.meta.toml")),
    ("search_cetat", include_str!("../tests/fixtures/reference/search_cetat.meta.toml")),
    ("search_cnil", include_str!("../tests/fixtures/reference/search_cnil.meta.toml")),
    ("search_code", include_str!("../tests/fixtures/reference/search_code.meta.toml")),
    ("search_constit", include_str!("../tests/fixtures/reference/search_constit.meta.toml")),
    ("search_juri", include_str!("../tests/fixtures/reference/search_juri.meta.toml")),
    ("search_kali", include_str!("../tests/fixtures/reference/search_kali.meta.toml")),
    ("search_loda", include_str!("../tests/fixtures/reference/search_loda.meta.toml")),
    ("parite-golden", include_str!("../tests/fixtures/reference/parite-golden.meta.toml")),
];

/// Sidecar `.meta.toml` de la fixture `name` (ou `parite-golden`), lu comme table TOML.
///
/// # Panics
///
/// Si `name` n'est pas dans [`SIDECARS`] ou si le sidecar n'est pas du TOML : une faute de
/// frappe d'un test ou un fichier abîmé.
pub(crate) fn sidecar(name: &str) -> toml::Table {
    let (_, content) =
        SIDECARS.iter().find(|(known, _)| *known == name).expect("known sidecar name");
    toml::from_str(content).expect("sidecar is TOML")
}

/// Valeur texte de la clé `key` de `meta`, `None` si la clé manque ou si ce n'est pas un texte.
pub(crate) fn text_at<'a>(meta: &'a toml::Table, key: &str) -> Option<&'a str> {
    meta.get(key).and_then(toml::Value::as_str)
}

/// Textes de la liste `known_issues` de `meta` ; vide si la clé manque ou si ce n'est pas une
/// liste, les éléments qui ne sont pas des textes étant ignorés.
pub(crate) fn known_issues(meta: &toml::Table) -> Vec<&str> {
    meta.get("known_issues")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_str)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;

    use serde_json::json;

    use super::*;
    use crate::map::fixtures::{decoded, golden, text, value};

    #[test]
    fn sha256_matches_the_fips_180_vectors() {
        let million_a = "a".repeat(1_000_000);
        let vectors: [(&str, &str); 4] = [
            ("", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
            ("abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
            (
                "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            ),
            (&million_a, "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"),
        ];
        for (message, expected) in vectors {
            assert_eq!(hex(&sha256(message.as_bytes())), expected, "digest of {expected}");
        }
    }

    #[test]
    fn a_sidecar_reads_texts_and_lists() {
        let juri = sidecar("consult_juri");
        assert_eq!(text_at(&juri, "endpoint"), Some("/consult/juri"));
        assert_eq!(text_at(&juri, "absent"), None);
        assert_eq!(known_issues(&juri), ["mojibake"]);
        assert_eq!(known_issues(&sidecar("consult_acco")), Vec::<&str>::new());
    }

    /// Valeur attendue de `fond` dans le sidecar de `fixture`, `None` pour `/consult/getArticle`
    /// (seul endpoint sans fond).
    fn expected_fond(fixture: &Fixture) -> Option<&'static str> {
        match fixture.kind {
            Kind::Search(fond) => Some(fond.name()),
            Kind::LawDecree => Some("LODA"),
            Kind::KaliText => Some("KALI"),
            Kind::Cnil => Some("CNIL"),
            Kind::Acco => Some("ACCO"),
            Kind::GetArticle => None,
            Kind::Juri if fixture.requested.starts_with("CETAT") => Some("CETAT"),
            Kind::Juri if fixture.requested.starts_with("CONST") => Some("CONSTIT"),
            Kind::Juri => Some("JURI"),
        }
    }

    #[test]
    fn fixtures_have_meta_sidecar() {
        let mut mojibake = 0;
        for fixture in &ALL {
            let meta = sidecar(fixture.name);
            let fond = expected_fond(fixture);
            let mut expected_keys =
                vec!["source", "endpoint", "sha256_raw", "known_issues", "env", "captured_at"];
            if fond.is_some() {
                expected_keys.push("fond");
            }
            expected_keys.sort_unstable();
            let keys: Vec<&str> = meta.keys().map(String::as_str).collect();
            assert_eq!(keys, expected_keys, "keys of {}", fixture.name);

            assert_eq!(text_at(&meta, "source"), Some("matematicsolutions/fr-eli-mcp@d3b5b13"));
            assert_eq!(text_at(&meta, "endpoint"), Some(fixture.endpoint), "{}", fixture.name);
            assert_eq!(text_at(&meta, "fond"), fond, "fond of {}", fixture.name);
            assert_eq!(text_at(&meta, "env"), Some("sandbox"), "{}", fixture.name);
            assert_eq!(text_at(&meta, "captured_at"), Some("inconnu"), "{}", fixture.name);

            let repaired = decoded(fixture, true).repaired;
            let expected_issues: &[&str] = if repaired { &["mojibake"] } else { &[] };
            assert_eq!(known_issues(&meta), expected_issues, "known_issues of {}", fixture.name);
            mojibake += usize::from(repaired);
        }
        assert_eq!(mojibake, 6);
    }

    #[test]
    fn fixture_sha256_matches_meta() {
        for fixture in &ALL {
            let meta = sidecar(fixture.name);
            assert_eq!(
                Some(hex(&sha256(fixture.bytes())).as_str()),
                text_at(&meta, "sha256_raw"),
                "sha256_raw of {}",
                fixture.name
            );
        }
    }

    #[test]
    fn the_reference_directory_holds_the_fixtures_their_sidecars_and_the_golden() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/reference");
        let found: BTreeSet<String> = fs::read_dir(directory)
            .expect("reference directory is readable")
            .map(|entry| {
                entry.expect("entry is readable").file_name().to_string_lossy().into_owned()
            })
            .collect();
        let mut expected = BTreeSet::new();
        for fixture in &ALL {
            expected.insert(format!("{}.json", fixture.name));
            expected.insert(format!("{}.meta.toml", fixture.name));
        }
        expected.insert("parite-golden.json".to_owned());
        expected.insert("parite-golden.meta.toml".to_owned());
        assert_eq!(expected.len(), 34);
        assert_eq!(found, expected);
    }

    /// Longueur de la tête de `text` faite de lettres, chiffres, `-` et `_`.
    fn token_len(text: &str) -> usize {
        text.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .unwrap_or(text.len())
    }

    /// `text` contient `eyJ` suivi de deux segments `[A-Za-z0-9_-]+` séparés de points : la forme
    /// d'un JWT (en-tête JSON encodé en base64url, charge utile, signature).
    fn looks_like_jwt(text: &str) -> bool {
        text.match_indices("eyJ").any(|(start, _)| {
            let mut rest = &text[start..];
            let mut segments = 0;
            while segments < 3 {
                let len = token_len(rest);
                if len == 0 {
                    return false;
                }
                segments += 1;
                rest = &rest[len..];
                if segments < 3 {
                    match rest.strip_prefix('.') {
                        Some(after) => rest = after,
                        None => return false,
                    }
                }
            }
            true
        })
    }

    /// Mots dont la présence, quelle que soit la casse, dans une clé ou une valeur trahit un
    /// secret d'authentification (en minuscules).
    const SECRET_WORDS: [&str; 6] =
        ["bearer", "authorization", "accesstoken", "access_token", "clientsecret", "client_secret"];

    /// Longueur minimale d'une suite hexadécimale prise pour un secret (un condensat de 128 bits).
    const HEX_RUN: usize = 32;

    /// Longueur minimale d'une suite de l'alphabet base64 standard (`A-Za-z0-9+/`) prise pour un
    /// secret. L'alphabet des URL (`-` et `_`) n'est pas testé : les identifiants techniques de
    /// l'API (`index_DATA_NEXT_ACCORD_ACCOTEXT…`) en sont faits.
    const BASE64_RUN: usize = 40;

    /// Premier mot de [`SECRET_WORDS`] que contient `text`, sans tenir compte de la casse.
    fn secret_word(text: &str) -> Option<&'static str> {
        let lower = text.to_ascii_lowercase();
        SECRET_WORDS.into_iter().find(|word| lower.contains(word))
    }

    /// `text` contient un UUID (`8-4-4-4-12` chiffres hexadécimaux, majuscules ou minuscules).
    fn has_uuid(text: &str) -> bool {
        text.as_bytes().windows(36).any(|window| {
            window.iter().enumerate().all(|(index, byte)| {
                if matches!(index, 8 | 13 | 18 | 23) {
                    *byte == b'-'
                } else {
                    byte.is_ascii_hexdigit()
                }
            })
        })
    }

    /// `text` contient une suite d'au moins [`HEX_RUN`] chiffres hexadécimaux.
    fn has_hex_run(text: &str) -> bool {
        text.split(|c: char| !c.is_ascii_hexdigit()).any(|run| run.len() >= HEX_RUN)
    }

    /// `text` contient, en sous-chaîne, une suite d'au moins [`BASE64_RUN`] caractères de
    /// l'alphabet base64 standard (le `=` final n'en fait pas partie).
    fn has_base64_run(text: &str) -> bool {
        text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '+' || c == '/'))
            .any(|run| run.len() >= BASE64_RUN)
    }

    /// Valeurs JSON déclarées sûres bien qu'elles aient la forme d'un base64 : fixture et pointeur
    /// JSON. L'adresse de l'accord (`texte/acco/ACCO/TEXT/00/00/53/…/ACCOTEXT…`) et un chemin
    /// d'article de convention (`/KALISCTA…/KALIARTI…`) sont faits de lettres, de chiffres et de
    /// `/` sur plus de 40 caractères.
    const BASE64_LOOKALIKES: [(&str, &str); 2] =
        [("consult_acco", "/acco/url"), ("consult_kali", "/sections/0/articles/0/path")];

    /// Ce qui, dans le document `name` (texte brut `raw` et sa lecture JSON `json`), ressemble à un
    /// secret : un mot de [`SECRET_WORDS`], un UUID, une suite hexadécimale, un JWT ou une suite
    /// base64, sauf la valeur de `json` déclarée sûre dans `exempt` (comparée par adresse). Le texte
    /// brut couvre les clés comme les valeurs ; la lecture JSON, les valeurs une fois décodées.
    fn leaks(name: &str, raw: &str, json: &Value, exempt: &[&str]) -> Vec<String> {
        let mut found = Vec::new();
        if let Some(word) = secret_word(raw) {
            found.push(format!("{name}: secret word {word:?}"));
        }
        if has_uuid(raw) {
            found.push(format!("{name}: UUID"));
        }
        if has_hex_run(raw) {
            found.push(format!("{name}: hexadecimal run"));
        }
        for (path, text) in leaf_strings(json) {
            if secret_word(text).is_some() {
                found.push(format!("{name}: secret word at {path}"));
            }
            if looks_like_jwt(text) {
                found.push(format!("{name}: JWT-like string at {path}"));
            }
            let declared_safe = exempt.iter().any(|safe| std::ptr::eq(*safe, text));
            if has_base64_run(text) && !declared_safe {
                found.push(format!("{name}: base64-like string at {path}"));
            }
        }
        found
    }

    /// Lecture JSON du contenu d'un snapshot `insta` : ce qui suit l'en-tête entre deux `---`.
    fn snapshot_json(raw: &str) -> Value {
        let body = raw.splitn(3, "---\n").nth(2).expect("snapshot has a header");
        serde_json::from_str(body).expect("snapshot body is JSON")
    }

    #[test]
    fn fixtures_contain_no_token_like_strings() {
        for fixture in &ALL {
            let json = value(fixture.name);
            let declared: Vec<&str> = BASE64_LOOKALIKES
                .iter()
                .filter(|(name, _)| *name == fixture.name)
                .map(|(_, pointer)| *pointer)
                .collect();
            let exempt: Vec<&str> = declared
                .iter()
                .filter_map(|pointer| json.pointer(pointer))
                .filter_map(Value::as_str)
                .collect();
            assert_eq!(exempt.len(), declared.len(), "declared pointers of {} exist", fixture.name);
            assert!(exempt.iter().all(|text| has_base64_run(text)), "stale exemption");
            assert_eq!(
                leaks(fixture.name, text(fixture.name), &json, &exempt),
                Vec::<String>::new()
            );
        }
    }

    #[test]
    fn snapshots_and_the_golden_contain_no_token_like_strings() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/snapshots");
        let mut checked = 0;
        for entry in fs::read_dir(directory).expect("snapshot directory is readable") {
            let path = entry.expect("entry is readable").path();
            if path.extension().is_some_and(|extension| extension == "snap") {
                let raw =
                    fs::read_to_string(&path).expect("snapshot is UTF-8").replace("\r\n", "\n");
                let name = path.file_name().expect("file name").to_string_lossy().into_owned();
                assert_eq!(leaks(&name, &raw, &snapshot_json(&raw), &[]), Vec::<String>::new());
                checked += 1;
            }
        }
        assert_eq!(checked, 17);
        let raw = include_str!("../tests/fixtures/reference/parite-golden.json");
        assert_eq!(leaks("parite-golden", raw, &golden(), &[]), Vec::<String>::new());
    }

    #[test]
    fn the_secret_detectors_flag_their_witnesses() {
        // JWT.
        assert!(looks_like_jwt("x eyJhbGciOi.eyJzdWIiOi.c2ln"));
        assert!(!looks_like_jwt("eyJ seul"));
        assert!(!looks_like_jwt("eyJa.b"));
        assert!(!looks_like_jwt("eyJa..c"));
        // Mots d'authentification, quelle que soit la casse.
        for (text, word) in [
            ("Authorization: x", "authorization"),
            ("un BEARER", "bearer"),
            ("{\"accessToken\":1}", "accesstoken"),
            ("Access_Token", "access_token"),
            ("ClientSecret", "clientsecret"),
            ("client_SECRET", "client_secret"),
        ] {
            assert_eq!(secret_word(text), Some(word), "{text}");
        }
        assert_eq!(secret_word("autorisation du porteur"), None);
        // UUID.
        assert!(has_uuid("id 123E4567-e89b-12d3-A456-426614174000 fin"));
        assert!(!has_uuid("id 123e4567-e89b-12d3-a456-42661417400"));
        assert!(!has_uuid("id 123e4567-e89b-12d3-a456-42661417400g"));
        assert!(!has_uuid("123e4567e89b12d3a456426614174000"));
        // Suite hexadécimale.
        assert!(has_hex_run(&format!("a {}", "0123456789abcdefABCDEF".repeat(2))));
        assert!(has_hex_run(&"f".repeat(64)));
        assert!(!has_hex_run(&"f".repeat(31)));
        assert!(!has_hex_run(&format!("{}g{}", "f".repeat(20), "f".repeat(20))));
        // Suite base64 en sous-chaîne.
        assert!(has_base64_run(&"QUJD".repeat(10)));
        assert!(has_base64_run(&format!("jeton {}==", "QUJD".repeat(11))));
        assert!(has_base64_run("/KALISCTA000049886029/KALIARTI000049886059."));
        assert!(!has_base64_run(&"QUJD".repeat(9)));
        assert!(!has_base64_run(&format!("{}-{}", "QUJD".repeat(5), "QUJD".repeat(5))));
    }

    #[test]
    fn leaks_reports_each_kind_and_honours_the_exemption() {
        let clean = json!({"title": "Code du travail", "id": "KALIARTI000049886059"});
        assert_eq!(leaks("clean", &clean.to_string(), &clean, &[]), Vec::<String>::new());
        let base64 = "QUJD".repeat(11);
        let json = json!({
            "Authorization": "x",
            "uuid": "123e4567-e89b-12d3-a456-426614174000",
            "digest": "f".repeat(32),
            "jwt": "eyJhbGciOi.eyJzdWIiOi.c2ln",
            "value": "un Bearer",
            "lookalike": base64,
        });
        let found = leaks("doc", &json.to_string(), &json, &[]);
        assert_eq!(
            found,
            [
                "doc: secret word \"bearer\"",
                "doc: UUID",
                "doc: hexadecimal run",
                "doc: JWT-like string at jwt",
                "doc: base64-like string at lookalike",
                "doc: secret word at value",
            ]
        );
        let safe = json["lookalike"].as_str().unwrap();
        let found = leaks("doc", &json.to_string(), &json, &[safe]);
        assert_eq!(found.len(), 5);
        assert!(!found.iter().any(|line| line.contains("base64")));
    }
}
