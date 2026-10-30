//! Intégrité des fixtures de référence (04 §13.3 ; 06 §4.4 point 5, §4.6 ; D-35).
//!
//! Ce module ne contient que des tests et leurs aides. Il garde les seize réponses brutes de
//! `tests/fixtures/reference` et le golden de parité : chaque fichier a son sidecar `.meta.toml`
//! (source, endpoint, fond, `sha256_raw`, anomalies connues, environnement, date de capture), le
//! SHA-256 des octets des fixtures correspond à celui du sidecar (celui de la copie du golden est
//! contrôlé par `golden`), le répertoire ne contient rien d'autre, et ni les fixtures, ni les
//! dix-sept snapshots `insta`, ni le golden ne portent de chaîne qui ressemble à un secret
//! (D-35) : mot d'authentification (`bearer`, `access_token`…, quelle que soit la casse), UUID,
//! suite hexadécimale de 32 caractères ou plus, JWT, suite base64 de 40 caractères ou plus (sauf
//! sous les clés `path`, `url`, `eli` et `pdfFilePath`, `BASE64_KEYS`, où chaque segment séparé par `/` reste
//! contrôlé).
//!
//! Les fixtures **enregistrées** (`tests/fixtures/recorded/<endpoint>/<nom>.json`, écrites par
//! `cargo xtask record-fixture`, 04 §13.4) passent les mêmes détecteurs, plus le contrôle de leur
//! sidecar (clés de 04 §13.4, `sha256_file` recalculé, environnement nommé, chemin sous
//! l'endpoint, licence, 1 Mio au plus) et la cohérence de `sha256_raw` et de `mojibake_detected`
//! quand rien n'a été caviardé. Le répertoire est absent tant qu'aucune fixture n'est
//! enregistrée ; le décompte est épinglé (`RECORDED`).
//!
//! Le SHA-256 vient de [`crate::sha256`] (FIPS 180-4, sans dépendance), partagé avec
//! l'enregistrement des fixtures (feature `record`). Le module n'existe que sous `cfg(test)`.

use serde_json::Value;

use crate::map::fixtures::{ALL, Fixture, Kind};
use crate::sha256::{hex, sha256};
use crate::view::leaf_strings;

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
    use std::path::{Path, PathBuf};

    use serde_json::json;

    use super::*;
    use crate::map::fixtures::{decoded, golden, text, value};

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

    /// Clés dont la valeur texte n'est pas soumise au détecteur base64 : `path`, `url`, `eli` et
    /// `pdfFilePath`. Les chemins de la table des matières (`/LEGISCTA…/LEGIARTI…`), l'adresse d'un
    /// accord (`texte/acco/ACCO/TEXT/00/00/53/…/ACCOTEXT…`), un lien de téléchargement, un ELI
    /// (`/eli/loi/2025/8/11/AGRX2502679L/jo/texte`) et le chemin du PDF d'un code
    /// (`/LEGI/TEXT/00/…`) sont faits de
    /// lettres, de chiffres et de `/` sur plus de 40 caractères ; il y en a des milliers par
    /// fixture enregistrée, trop pour une exemption par pointeur (D10). L'exemption est par la
    /// DERNIÈRE clé de la feuille, jamais par la valeur : la même suite sous toute autre clé, ou
    /// comme élément d'un tableau, reste signalée. Sous ces clés, le détecteur s'applique encore
    /// à chaque segment séparé par `/` : seule la longueur cumulée d'un chemin est exemptée. Les
    /// autres détecteurs (mots d'authentification, UUID, suite hexadécimale, JWT) s'appliquent
    /// partout, y compris sous ces clés.
    const BASE64_KEYS: [&str; 4] = ["path", "url", "eli", "pdfFilePath"];

    /// Ajoute à `exempt` les valeurs texte de `value` portées directement par une clé de
    /// [`BASE64_KEYS`] (`key` est la clé de `value` dans son objet, `None` à la racine ou dans un
    /// tableau). Le parcours se fait sur le JSON lui-même : une clé qui contient un point ou des
    /// crochets ne peut pas se faire passer pour `path`.
    fn base64_exempt<'a>(value: &'a Value, key: Option<&str>, exempt: &mut Vec<&'a str>) {
        match value {
            Value::String(text) if key.is_some_and(|key| BASE64_KEYS.contains(&key)) => {
                exempt.push(text);
            }
            Value::Array(items) => {
                for item in items {
                    base64_exempt(item, None, exempt);
                }
            }
            Value::Object(entries) => {
                for (key, item) in entries {
                    base64_exempt(item, Some(key), exempt);
                }
            }
            Value::String(_) | Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }

    /// Ce qui, dans le document `name` (texte brut `raw` et sa lecture JSON `json`), ressemble à un
    /// secret : un mot de [`SECRET_WORDS`], un UUID, une suite hexadécimale, un JWT ou une suite
    /// base64, sauf pour une feuille de `json` portée par une clé de [`BASE64_KEYS`] (comparée par
    /// adresse : le détecteur base64 seul est levé). Le texte brut couvre les clés comme les
    /// valeurs ; la lecture JSON, les valeurs une fois décodées.
    fn leaks(name: &str, raw: &str, json: &Value) -> Vec<String> {
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
        let mut exempt = Vec::new();
        base64_exempt(json, None, &mut exempt);
        for (path, text) in leaf_strings(json) {
            if secret_word(text).is_some() {
                found.push(format!("{name}: secret word at {path}"));
            }
            if looks_like_jwt(text) {
                found.push(format!("{name}: JWT-like string at {path}"));
            }
            let by_key = exempt.iter().any(|safe| std::ptr::eq(*safe, text));
            // Sous une clé exemptée, le détecteur ne regarde que chaque segment séparé par `/` :
            // un chemin ou une adresse (suite de segments courts) passe, un jeton glissé dans un
            // segment (`?sig=…`) reste signalé (L4).
            let base64 =
                if by_key { text.split('/').any(has_base64_run) } else { has_base64_run(text) };
            if base64 {
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
            assert_eq!(leaks(fixture.name, text(fixture.name), &json), Vec::<String>::new());
        }
    }

    #[test]
    fn snapshots_and_the_golden_contain_no_token_like_strings() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/snapshots");
        let mut checked = 0;
        for entry in fs::read_dir(directory).expect("snapshot directory is readable") {
            let path = entry.expect("entry is readable").path();
            // Un `.snap.new` laissé par un essai local échoue ici, au lieu d'échapper au scan.
            assert_eq!(path.extension().and_then(|extension| extension.to_str()), Some("snap"));
            let raw = fs::read_to_string(&path).expect("snapshot is UTF-8").replace("\r\n", "\n");
            let name = path.file_name().expect("file name").to_string_lossy().into_owned();
            assert_eq!(leaks(&name, &raw, &snapshot_json(&raw)), Vec::<String>::new());
            checked += 1;
        }
        assert_eq!(checked, 17);
        let raw = include_str!("../tests/fixtures/reference/parite-golden.json");
        assert_eq!(leaks("parite-golden", raw, &golden()), Vec::<String>::new());
    }

    // ---- Fixtures enregistrées (04 §13.4, plan 2h) ----------------------------------------

    /// Nombre de fixtures enregistrées versionnées (fichiers `.json`, sidecars non comptés).
    const RECORDED: usize = 118;

    /// Clés obligatoires d'un sidecar enregistré (04 §13.4) ; `content_type` et
    /// `content_encoding` sont facultatives (absentes si l'en-tête l'était).
    const RECORDED_KEYS: [&str; 13] = [
        "endpoint",
        "request",
        "env",
        "captured_at",
        "http_status",
        "elapsed_ms",
        "bytes_raw",
        "sha256_raw",
        "sha256_file",
        "mojibake_detected",
        "redactions",
        "tool",
        "license",
    ];

    /// Licence du sidecar (`record::LICENSE`, répétée ici : le module d'enregistrement n'existe
    /// que sous la feature `record`).
    const RECORDED_LICENSE: &str = "Licence Ouverte 2.0 — source : Légifrance / DILA";

    /// Taille maximale d'une fixture enregistrée : 1 Mio.
    const RECORDED_MAX_BYTES: usize = 1 << 20;

    /// Fichiers sous `dir`, récursivement, triés ; vide si `dir` n'existe pas.
    fn files_under(dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut pending = vec![dir.to_path_buf()];
        while let Some(next) = pending.pop() {
            let Ok(entries) = fs::read_dir(&next) else { continue };
            for entry in entries {
                let path = entry.expect("entry is readable").path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    files.push(path);
                }
            }
        }
        files.sort();
        files
    }

    /// Problèmes de la fixture enregistrée `json` (chemin sous `root`) et de son sidecar.
    fn recorded_problems(root: &Path, json: &Path) -> Vec<String> {
        let name =
            json.strip_prefix(root).expect("under root").to_string_lossy().replace('\\', "/");
        let bytes = fs::read(json).expect("fixture is readable");
        let Ok(meta) = fs::read_to_string(json.with_extension("meta.toml")) else {
            return vec![format!("{name}: no sidecar")];
        };
        let Ok(meta) = toml::from_str::<toml::Table>(&meta.replace("\r\n", "\n")) else {
            return vec![format!("{name}: sidecar is not TOML")];
        };
        let mut found: Vec<String> = RECORDED_KEYS
            .iter()
            .filter(|key| !meta.contains_key(**key))
            .map(|key| format!("{name}: missing {key}"))
            .collect();
        let text = |key: &str| text_at(&meta, key).unwrap_or("");
        let digest = hex(&sha256(&bytes));
        if text("sha256_file") != digest {
            found.push(format!("{name}: sha256_file differs"));
        }
        // Sans caviardage, le fichier est la réponse reçue : même condensat, même indicateur.
        // Seule une liste non vide dispense de ce contrôle ; une valeur d'un autre type est
        // signalée (une clé absente l'est déjà par `missing`) et ne dispense de rien.
        let redactions = meta.get("redactions");
        if redactions.is_some_and(|value| !value.is_array()) {
            found.push(format!("{name}: redactions must be a list"));
        }
        let redacted =
            redactions.and_then(toml::Value::as_array).is_some_and(|list| !list.is_empty());
        let raw = String::from_utf8_lossy(&bytes);
        let mojibake = lf_domain::text::encoding::has_latin1_mojibake(&raw);
        if !redacted
            && (text("sha256_raw") != digest
                || meta.get("mojibake_detected").and_then(toml::Value::as_bool) != Some(mojibake))
        {
            found.push(format!("{name}: raw digest or mojibake flag differs from the file"));
        }
        if !matches!(text("env"), "sandbox" | "production") {
            found.push(format!("{name}: env must be sandbox or production"));
        }
        if !name.starts_with(&format!("{}/", text("endpoint").trim_start_matches('/'))) {
            found.push(format!("{name}: endpoint differs from the directory"));
        }
        if text("license") != RECORDED_LICENSE {
            found.push(format!("{name}: license"));
        }
        if bytes.len() > RECORDED_MAX_BYTES {
            found.push(format!("{name}: larger than 1 MiB"));
        }
        // Un fichier illisible comme JSON est signalé : sans lui, les détecteurs par feuille (JWT,
        // mot d'authentification, base64) ne tourneraient pas. Les contrôles du texte brut (mot
        // d'authentification, UUID, suite hexadécimale) s'appliquent quand même.
        // Une réponse 2xx sans aucun octet (`canonicalArticleVersion` sur un identifiant inconnu)
        // est une fixture légitime : elle n'est pas signalée.
        let json = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            if !bytes.is_empty() {
                found.push(format!("{name}: not JSON"));
            }
            Value::Null
        });
        found.extend(leaks(&name, &raw, &json));
        found
    }

    /// Problèmes de l'arbre `root` : fichier hors `.json`/`.meta.toml`, sidecar orphelin, puis
    /// ceux de chaque fixture ; et le nombre de fixtures.
    fn recorded_tree(root: &Path) -> (Vec<String>, usize) {
        let files = files_under(root);
        let mut found = Vec::new();
        let mut fixtures = 0;
        for path in &files {
            let shown =
                path.strip_prefix(root).expect("under root").to_string_lossy().replace('\\', "/");
            if shown.ends_with(".meta.toml") {
                if !files.contains(&path.with_extension("").with_extension("json")) {
                    found.push(format!("{shown}: orphan sidecar"));
                }
            } else if path.extension().is_some_and(|extension| extension == "json") {
                fixtures += 1;
                found.extend(recorded_problems(root, path));
            } else {
                found.push(format!("{shown}: unexpected file"));
            }
        }
        (found, fixtures)
    }

    #[test]
    fn recorded_fixtures_pass_integrity() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/recorded");
        assert_eq!(recorded_tree(&root), (Vec::<String>::new(), RECORDED));
    }

    /// Sidecar sain (les quinze clés de `record::render`, moins les deux facultatives) de la
    /// fixture dont les octets ont pour condensat `digest`.
    fn healthy_meta(digest: &str) -> String {
        format!(
            "endpoint = \"/consult/juri\"\nrequest = \"{{}}\"\nenv = \"sandbox\"\ncaptured_at = \"x\"\n\
             http_status = 200\nelapsed_ms = 1\nbytes_raw = 28\nsha256_raw = \"{digest}\"\n\
             sha256_file = \"{digest}\"\nmojibake_detected = false\nredactions = []\ntool = \"t\"\n\
             license = \"Licence Ouverte 2.0 — source : Légifrance / DILA\"\n"
        )
    }

    #[test]
    fn recorded_tree_flags_each_problem() {
        let root = std::env::temp_dir().join(format!("lf-recorded-{}", std::process::id()));
        // Un reste d'un essai interrompu ne doit pas fausser le décompte.
        let _ = fs::remove_dir_all(&root);
        let dir = root.join("consult/juri");
        fs::create_dir_all(&dir).unwrap();
        let body = br#"{"id":"JURITEXT000000000001"}"#;
        let meta = healthy_meta(&hex(&sha256(body)));
        fs::write(dir.join("ok.json"), body).unwrap();
        fs::write(dir.join("ok.meta.toml"), &meta).unwrap();
        assert_eq!(recorded_tree(&root), (vec![], 1));

        // Un fichier tronqué (donc illisible comme JSON) à sidecar sain, et une liste de
        // caviardages d'un mauvais type dans un fichier dont le condensat brut est faux.
        let truncated = br#"{"k":"#;
        fs::write(dir.join("trunc.json"), truncated).unwrap();
        fs::write(dir.join("trunc.meta.toml"), healthy_meta(&hex(&sha256(truncated)))).unwrap();
        let typed = br#"{"k":1}"#;
        fs::write(dir.join("typed.json"), typed).unwrap();
        fs::write(
            dir.join("typed.meta.toml"),
            healthy_meta(&hex(&sha256(typed)))
                .replace("redactions = []", "redactions = \"all\"")
                .replace("sha256_raw = \"", "sha256_raw = \"0"),
        )
        .unwrap();

        // Plus de 1 Mio, une suite hexadécimale de 32 caractères ; sidecar sans `tool`, avec un
        // environnement inconnu, un autre endpoint, une licence altérée et le condensat d'un
        // autre fichier.
        let mut big = br#"{"k":"0123456789abcdef0123456789abcdef"}"#.to_vec();
        big.resize((1 << 20) + 1, b' ');
        fs::write(dir.join("bad.json"), &big).unwrap();
        fs::write(
            dir.join("bad.meta.toml"),
            meta.replace("tool = \"t\"\n", "")
                .replace("sandbox", "prod")
                .replace("/consult/juri", "/consult/kali")
                .replace("Ouverte 2.0", "Ouverte 1.0"),
        )
        .unwrap();
        fs::write(dir.join("lone.meta.toml"), "").unwrap();
        fs::write(dir.join("notes.txt"), "").unwrap();
        fs::write(dir.join("nometa.json"), "{}").unwrap();
        fs::write(dir.join("broken.json"), "{}").unwrap();
        fs::write(dir.join("broken.meta.toml"), "= x").unwrap();
        let (found, fixtures) = recorded_tree(&root);
        assert_eq!(
            found,
            [
                "consult/juri/bad.json: missing tool",
                "consult/juri/bad.json: sha256_file differs",
                "consult/juri/bad.json: raw digest or mojibake flag differs from the file",
                "consult/juri/bad.json: env must be sandbox or production",
                "consult/juri/bad.json: endpoint differs from the directory",
                "consult/juri/bad.json: license",
                "consult/juri/bad.json: larger than 1 MiB",
                "consult/juri/bad.json: hexadecimal run",
                "consult/juri/broken.json: sidecar is not TOML",
                "consult/juri/lone.meta.toml: orphan sidecar",
                "consult/juri/nometa.json: no sidecar",
                "consult/juri/notes.txt: unexpected file",
                "consult/juri/trunc.json: not JSON",
                "consult/juri/typed.json: redactions must be a list",
                "consult/juri/typed.json: raw digest or mojibake flag differs from the file",
            ]
        );
        assert_eq!(fixtures, 6);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn recorded_checks_the_mojibake_flag_only_without_redaction() {
        let root = std::env::temp_dir().join(format!("lf-recorded-moji-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = root.join("consult/juri");
        fs::create_dir_all(&dir).unwrap();
        // Un mojibake que le sidecar ne signale pas : rapporté tant que rien n'est caviardé.
        let body = "{\"t\":\"R\u{c3}\u{a9}publique\"}".as_bytes();
        let digest = hex(&sha256(body));
        fs::write(dir.join("m.json"), body).unwrap();
        fs::write(dir.join("m.meta.toml"), healthy_meta(&digest)).unwrap();
        assert_eq!(
            recorded_tree(&root).0,
            ["consult/juri/m.json: raw digest or mojibake flag differs from the file"]
        );
        // Avec un caviardage, le condensat brut et l'indicateur portent sur la réponse d'origine.
        fs::write(
            dir.join("m.meta.toml"),
            healthy_meta(&digest).replace("redactions = []", "redactions = [\"/a\"]").replace(
                &format!("sha256_raw = \"{digest}\""),
                &format!("sha256_raw = \"{}\"", "0".repeat(64)),
            ),
        )
        .unwrap();
        assert_eq!(recorded_tree(&root), (vec![], 1));
        // Une liste de caviardages d'un mauvais type est signalée et ne dispense d'aucun contrôle.
        fs::write(
            dir.join("m.meta.toml"),
            healthy_meta(&digest).replace("redactions = []", "redactions = 1"),
        )
        .unwrap();
        assert_eq!(
            recorded_tree(&root).0,
            [
                "consult/juri/m.json: redactions must be a list",
                "consult/juri/m.json: raw digest or mojibake flag differs from the file",
            ]
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_absent_directory_has_no_files() {
        let absent = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/absent-directory");
        assert_eq!(files_under(&absent), Vec::<PathBuf>::new());
    }

    #[test]
    fn base64_lookalikes_are_exempt_by_key_only() {
        assert_eq!(BASE64_KEYS, ["path", "url", "eli", "pdfFilePath"]);
        // Forme de chemin : cinq segments de 12 caractères séparés par `/` (64 caractères). La
        // suite entière est une « suite base64 » (le `/` est dans l'alphabet), aucun segment seul
        // n'en est une.
        let lookalike = ["QUJDQUJDQUJD"; 5].join("/");
        assert_eq!(lookalike.len(), 64);
        // Un `path` et un `url`, à la racine, imbriqués ou dans un tableau d'objets : exemptés.
        let exempt = json!({
            "path": lookalike,
            "acco": {"url": lookalike},
            "articles": [{"path": lookalike}, {"url": lookalike}],
            "eli": lookalike,
            "results": [{"pdfFilePath": lookalike}],
        });
        assert_eq!(leaks("exempt", &exempt.to_string(), &exempt), Vec::<String>::new());
        // La même valeur sous toute autre clé est rapportée, y compris une clé qui ne fait que
        // contenir ou terminer par le nom exempté, ou une clé à point.
        let flagged = json!({
            "texte": lookalike,
            "paths": lookalike,
            "my_url": lookalike,
            "x.path": lookalike,
            "nested": {"texte": lookalike},
        });
        assert_eq!(
            leaks("flagged", &flagged.to_string(), &flagged),
            [
                "flagged: base64-like string at my_url",
                "flagged: base64-like string at nested.texte",
                "flagged: base64-like string at paths",
                "flagged: base64-like string at texte",
                "flagged: base64-like string at x.path",
            ]
        );
        // Sous une clé exemptée, un segment séparé par `/` qui est lui-même une suite base64 reste
        // rapporté : un jeton glissé dans un paramètre d'adresse ne passe pas (L4).
        let token = format!("{}QUJ", "QUJD".repeat(10));
        assert_eq!(token.len(), 43);
        let signed = json!({"url": format!("https://x/dl?sig={token}")});
        assert_eq!(
            leaks("signed", &signed.to_string(), &signed),
            ["signed: base64-like string at url"]
        );
        // Un élément de tableau n'a pas de clé : même sous `path`, il est rapporté.
        let listed = json!({"path": [lookalike]});
        assert_eq!(
            leaks("listed", &listed.to_string(), &listed),
            ["listed: base64-like string at path[0]"]
        );
        // Une valeur exemptée ne désactive que le détecteur base64 : un mot d'authentification, un
        // JWT ou une suite hexadécimale sous `url` restent rapportés.
        let others = json!({"url": "Bearer eyJhbGciOi.eyJzdWIiOi.c2ln", "path": "f".repeat(32)});
        assert_eq!(
            leaks("others", &others.to_string(), &others),
            [
                "others: secret word \"bearer\"",
                "others: hexadecimal run",
                "others: secret word at url",
                "others: JWT-like string at url",
            ]
        );
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
        assert_eq!(leaks("clean", &clean.to_string(), &clean), Vec::<String>::new());
        let base64 = "QUJD".repeat(11);
        let path_shaped = ["QUJDQUJDQUJD"; 5].join("/");
        let json = json!({
            "Authorization": "x",
            "uuid": "123e4567-e89b-12d3-a456-426614174000",
            "digest": "f".repeat(32),
            "jwt": "eyJhbGciOi.eyJzdWIiOi.c2ln",
            "value": "un Bearer",
            "lookalike": base64,
        });
        let found = leaks("doc", &json.to_string(), &json);
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
        // Un mot sensible dans une clé seulement (valeur anodine) est aussi rapporté.
        let key_only = json!({"Authorization": "x"});
        assert_eq!(
            leaks("key", &key_only.to_string(), &key_only),
            ["key: secret word \"authorization\""]
        );
        // La même suite sous `path` n'est plus rapportée comme base64 (le reste l'est).
        let exempt = json!({"path": path_shaped, "Authorization": "x"});
        assert_eq!(
            leaks("doc", &exempt.to_string(), &exempt),
            ["doc: secret word \"authorization\""]
        );
    }
}
