//! Structures de désérialisation des réponses de PISTE (04 §3.2, §8).
//!
//! Ces types ne servent qu'à lire du JSON : ils ne portent aucune règle métier. Ils sont
//! tolérants (champs facultatifs, nombres lus aussi en chaîne) parce que le format réel de
//! PISTE n'est pas garanti.

pub(crate) mod acco;
pub(crate) mod article;
pub(crate) mod budget;
pub(crate) mod consult_text;
pub(crate) mod de;
pub(crate) mod decode;
pub(crate) mod error;
pub(crate) mod juri;
pub(crate) mod oauth;
pub(crate) mod search;

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use super::decode::decode;
    use lf_application::ports::CorpusError;
    use serde::de::DeserializeOwned;

    /// Liste JSON de `count` fois `element`.
    pub(crate) fn repeated(element: &str, count: usize) -> String {
        format!("[{}]", vec![element; count].join(","))
    }

    /// Vérifie que `raw` se décode en `T` par l'erreur `Decode` d'un budget épuisé au `path` de
    /// l'`endpoint`. Le succès n'est jamais affiché : un DTO de 200 001 éléments remplirait la sortie.
    pub(crate) fn assert_too_many<T: DeserializeOwned>(raw: &str, endpoint: &str, path: &str) {
        let expected =
            CorpusError::Decode { detail: format!("{endpoint}: {path}: too many elements") };
        assert_eq!(decode::<T>(raw, endpoint).err(), Some(expected), "{path}");
    }

    /// Vérifie qu'un numéro (D-24) se lit en chaîne ou en nombre, rendu en chaîne : `field` est la
    /// clé JSON du champ et `get` le champ du DTO. Chaîne blanche, `null`, absent, flottant,
    /// booléen, tableau et objet donnent `None`.
    pub(crate) fn assert_number_reading<T: DeserializeOwned>(
        field: &str,
        endpoint: &str,
        get: fn(&T) -> &Option<String>,
    ) {
        let read = |raw: &str| {
            let dto = decode::<T>(&format!(r#"{{"{field}":{raw}}}"#), endpoint).unwrap();
            get(&dto).clone()
        };
        let text = |value: &str| Some(value.to_owned());
        assert_eq!(read("12500399"), text("12500399"), "entier");
        assert_eq!(read(r#""12500399""#), text("12500399"), "chaîne");
        assert_eq!(read(r#""0045""#), text("0045"), "zéros initiaux");
        for raw in [r#""""#, r#""  ""#, "null", "2.5", "true", "[1]", r#"{"a":1}"#] {
            assert_eq!(read(raw), None, "{raw}");
        }
        let absent = decode::<T>("{}", endpoint).unwrap();
        assert_eq!(get(&absent), &None, "absent");
    }

    /// Les fichiers de `dto/`, `mod.rs` compris, et leur source. Un fichier ajouté au dossier sans
    /// être listé ici fait échouer `every_dto_file_is_listed`.
    const SOURCES: [(&str, &str); 11] = [
        ("acco.rs", include_str!("acco.rs")),
        ("article.rs", include_str!("article.rs")),
        ("budget.rs", include_str!("budget.rs")),
        ("consult_text.rs", include_str!("consult_text.rs")),
        ("de.rs", include_str!("de.rs")),
        ("decode.rs", include_str!("decode.rs")),
        ("error.rs", include_str!("error.rs")),
        ("juri.rs", include_str!("juri.rs")),
        ("mod.rs", include_str!("mod.rs")),
        ("oauth.rs", include_str!("oauth.rs")),
        ("search.rs", include_str!("search.rs")),
    ];

    /// Types d'un conteneur que `serde` lirait sans payer le budget : listes, tranches, tables,
    /// ensembles, valeur JSON libre (`Map<` couvre `HashMap` et `BTreeMap`, `Set<` leurs ensembles).
    const CONTAINERS: [&str; 9] = [
        "Vec<",
        "VecDeque<",
        "Box<[",
        "Map<",
        "Set<",
        "json::Value",
        ": Value",
        "<Value",
        "(Value",
    ];

    /// Attributs qui paient chaque élément d'une liste sur le budget du document.
    const BOUNDED: [&str; 2] =
        [r#"deserialize_with = "de::bounded_list""#, r#"deserialize_with = "bounded_results""#];

    /// Implémentation des lecteurs bornés eux-mêmes, hors balayage : fichier et début de
    /// l'élément (fonction ou `impl`), qui s'étend jusqu'à la fin de son corps.
    const READERS: [(&str, &str); 4] = [
        ("de.rs", "fn bounded_list<"),
        ("de.rs", "fn capped_list<"),
        ("de.rs", "for ListVisitor<T>"),
        ("search.rs", "fn bounded_results<"),
    ];

    /// Champs hors budget admis : fichier et déclaration. `PisteErrorDto.status` n'est lu que dans
    /// un corps d'erreur borné à `MAX_ERROR_BODY_BYTES` (64 Kio, `error_body.rs`).
    const UNBUDGETED: [(&str, &str); 1] = [("error.rs", "pub status: Option<serde_json::Value>")];

    /// Code de production d'un fichier : tout ce qui précède son module de test. Rien ne le suit
    /// (`clippy::items_after_test_module`, refusé par `make lint`). Les fins de ligne CRLF d'une
    /// extraction Windows sont ramenées à LF : sans cela le module de test n'est pas trouvé.
    fn production(source: &str) -> String {
        let source = source.replace("\r\n", "\n");
        match source.split_once("\n#[cfg(test)]\nmod tests {") {
            Some((code, _)) => code.to_owned(),
            None => source,
        }
    }

    /// Octets de `rest` jusqu'au `close` non échappé compris ; s'il manque, tout `rest` suivi d'un
    /// délimiteur fictif (la chaîne court jusqu'à la fin).
    fn quoted(rest: &[u8], close: u8) -> usize {
        let mut index = 0;
        while index < rest.len() {
            if rest[index] == b'\\' {
                index += 2;
            } else if rest[index] == close {
                return index + 1;
            } else {
                index += 1;
            }
        }
        rest.len() + 1
    }

    /// `code` dont les commentaires, et si `strings` le contenu des chaînes et des caractères,
    /// sont remplacés par des espaces, octet pour octet : les positions ne changent pas.
    fn masked(code: &str, strings: bool) -> String {
        let bytes = code.as_bytes();
        let inner = |len: usize| if strings { 1..len - 1 } else { 0..0 };
        let mut out = bytes.to_vec();
        let mut index = 0;
        while index < bytes.len() {
            let (len, blank) = match &bytes[index..] {
                [b'/', b'/', rest @ ..] => {
                    let len = 2 + rest.iter().position(|&byte| byte == b'\n').unwrap_or(rest.len());
                    (len, 0..len)
                }
                [b'"', rest @ ..] => {
                    let len = 1 + quoted(rest, b'"');
                    (len, inner(len))
                }
                [b'\'', b'\\', _, rest @ ..] => {
                    let len = 3 + quoted(rest, b'\'');
                    (len, inner(len))
                }
                [b'\'', _, b'\'', ..] => (3, inner(3)),
                _ => (1, 0..0),
            };
            out[index + blank.start..index + blank.end].fill(b' ');
            index += len;
        }
        String::from_utf8(out).expect("only whole characters are blanked")
    }

    /// Position du délimiteur (`}`, `)` ou `]`) qui ferme celui de `open` dans `shape`.
    fn closing(shape: &str, open: usize) -> usize {
        let bytes = shape.as_bytes();
        let (opener, closer) = match bytes[open] {
            b'{' => (b'{', b'}'),
            b'(' => (b'(', b')'),
            _ => (b'[', b']'),
        };
        let mut depth = 0_i32;
        let offset = bytes[open..]
            .iter()
            .position(|&byte| {
                depth += i32::from(byte == opener) - i32::from(byte == closer);
                depth == 0
            })
            .expect("closing delimiter");
        open + offset
    }

    /// Champs de `shape[start..end]` coupés aux virgules de premier niveau, attributs compris ; la
    /// flèche `->` d'un type de fonction n'ouvre ni ne ferme rien.
    fn split_fields(shape: &str, start: usize, end: usize) -> Vec<Range<usize>> {
        let bytes = shape.as_bytes();
        let mut fields = Vec::new();
        let mut depth = 0_i32;
        let mut from = start;
        for index in start..end {
            match bytes[index] {
                b'(' | b'[' | b'{' | b'<' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b'>' if bytes[index - 1] != b'-' => depth -= 1,
                b',' if depth == 0 => {
                    fields.push(from..index);
                    from = index + 1;
                }
                _ => {}
            }
        }
        fields.push(from..end);
        fields.retain(|field| !shape[field.clone()].trim().is_empty());
        fields
    }

    /// Champs de toutes les structs de `shape`, nommés ou de tuple.
    fn fields(shape: &str) -> Vec<Range<usize>> {
        let mut fields = Vec::new();
        for (at, _) in shape.match_indices("struct ") {
            let after_ident = shape[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_');
            let body = shape[at..].find(['{', '(', ';']).map(|offset| at + offset);
            if let Some(open) = body.filter(|&open| !after_ident && shape.as_bytes()[open] != b';')
            {
                fields.extend(split_fields(shape, open + 1, closing(shape, open)));
            }
        }
        fields
    }

    /// Déclaration d'un champ sans ses attributs, blancs repliés (`pub status: Option<…>`).
    fn declaration(field: &str) -> String {
        let mut rest = field.trim_start();
        while rest.starts_with("#[") {
            rest = rest[closing(rest, 1) + 1..].trim_start();
        }
        rest.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Bilan du balayage des listes d'un fichier.
    #[derive(Debug, Default, PartialEq)]
    struct Scan {
        /// Champs à conteneur payés sur le budget.
        bounded: usize,
        /// Champs à conteneur admis hors budget ([`UNBUDGETED`]).
        exempted: usize,
        /// Lecteurs bornés exclus du balayage ([`READERS`]).
        readers: usize,
        /// Conteneurs hors budget : déclaration du champ, ou jeton hors de tout champ.
        unbounded: Vec<String>,
    }

    /// Balaye le code de production de `source` (fichier `file`) : chaque conteneur doit être un
    /// champ de struct payé sur le budget, un champ admis ou faire partie d'un lecteur borné.
    fn scan(file: &str, source: &str) -> Scan {
        let code = production(source);
        let shape = masked(&code, true);
        let attributes = masked(&code, false);
        let mut scan = Scan::default();
        let mut covered: Vec<Range<usize>> = Vec::new();
        for (_, header) in READERS.iter().filter(|(name, _)| *name == file) {
            let start = shape.find(header).expect("bounded reader");
            let open = start + shape[start..].find('{').expect("reader body");
            covered.push(start..closing(&shape, open));
            scan.readers += 1;
        }
        for field in fields(&shape) {
            covered.push(field.clone());
            let declared = declaration(&shape[field.clone()]);
            if !CONTAINERS.iter().any(|token| declared.contains(token)) {
                continue;
            }
            if BOUNDED.iter().any(|attribute| attributes[field.clone()].contains(attribute)) {
                scan.bounded += 1;
            } else if UNBUDGETED.contains(&(file, declared.as_str())) {
                scan.exempted += 1;
            } else {
                scan.unbounded.push(declared);
            }
        }
        for token in CONTAINERS {
            for (at, _) in shape.match_indices(token) {
                if !covered.iter().any(|range| range.contains(&at)) {
                    scan.unbounded.push(format!("{token} outside a struct field"));
                }
            }
        }
        scan
    }

    #[test]
    fn every_dto_file_is_listed() {
        let directory = concat!(env!("CARGO_MANIFEST_DIR"), "/src/dto");
        let mut on_disk: Vec<String> = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| std::path::Path::new(name).extension().is_some_and(|ext| ext == "rs"))
            .collect();
        on_disk.sort();
        let listed: Vec<&str> = SOURCES.iter().map(|(name, _)| *name).collect();
        assert_eq!(on_disk, listed);
    }

    #[test]
    fn production_cuts_the_test_module_under_crlf_line_endings() {
        let lf = "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n";
        assert_eq!(production(lf), "fn a() {}");
        assert_eq!(production(&lf.replace('\n', "\r\n")), "fn a() {}");
        assert_eq!(production("fn a() {}\r\n"), "fn a() {}\n");
    }

    #[test]
    fn every_dto_list_is_bounded() {
        let mut total = Scan::default();
        for (file, source) in SOURCES {
            let scan = scan(file, source);
            assert_eq!(scan.unbounded, Vec::<String>::new(), "{file}");
            total.bounded += scan.bounded;
            total.exempted += scan.exempted;
            total.readers += scan.readers;
        }
        assert_eq!(total, Scan { bounded: 25, exempted: 1, readers: 4, unbounded: Vec::new() });
    }

    #[test]
    fn the_list_scan_sees_every_hidden_form() {
        let source = r#"
/// Doc : `pub z: Vec<u8>` n'est pas du code.
pub(crate) struct Hidden<T> {
    #[serde(default)]
    pub(crate) a: Vec<u8>,
    b: Option<Vec<u8>>,
    pub c: Box<[u8]>,
    pub d: std::collections::HashMap<String, u8>,
    pub e: BTreeMap<String, u8>,
    pub f: serde_json::Value,
    pub g: Option<
        Vec<u8>,
    >,
    /// Une doc qui cite `deserialize_with = "de::bounded_list"` ne paie rien.
    pub h: Vec<u8>,
    #[serde(rename = "x", deserialize_with = "de::bounded_list")]
    pub i: Vec<u8>,
    marker: PhantomData<fn() -> T>,
    pub j: String,
}
pub(crate) struct Tuple(pub Vec<u8>, u8);
pub(crate) struct Unit;
pub(crate) enum Variant { A(Vec<u8>) }
fn destruct (x: u8) {}
#[cfg(test)]
mod tests {
    pub struct Test { pub k: Vec<u8> }
}
"#;
        assert_eq!(
            scan("synthetic.rs", source),
            Scan {
                bounded: 1,
                exempted: 0,
                readers: 0,
                unbounded: [
                    "pub(crate) a: Vec<u8>",
                    "b: Option<Vec<u8>>",
                    "pub c: Box<[u8]>",
                    "pub d: std::collections::HashMap<String, u8>",
                    "pub e: BTreeMap<String, u8>",
                    "pub f: serde_json::Value",
                    "pub g: Option< Vec<u8>, >",
                    "pub h: Vec<u8>",
                    "pub Vec<u8>",
                    "Vec< outside a struct field",
                ]
                .map(str::to_owned)
                .into(),
            }
        );
    }

    #[test]
    fn masking_keeps_positions_and_hides_comments_and_strings() {
        let code = "let a = \"x // \\\"y\"; // Vec<u8>\nlet b = ('\"', '\\'', 'x'); let c: &'static str = \"open";
        let shape = masked(code, true);
        assert_eq!(shape.len(), code.len());
        assert_eq!(
            shape,
            "let a = \"        \";           \nlet b = (' ', '  ', ' '); let c: &'static str = \"    "
        );
        assert_eq!(
            masked(code, false),
            "let a = \"x // \\\"y\";           \nlet b = ('\"', '\\'', 'x'); let c: &'static str = \"open"
        );
        assert_eq!(masked("// Vec<é>", true), " ".repeat(10));
    }

    #[test]
    fn null_default_serves_only_the_two_struct_fields() {
        let mut uses = 0;
        for (_, source) in SOURCES {
            let code = production(source);
            let lines: Vec<&str> = code.lines().collect();
            for (index, line) in lines.iter().enumerate() {
                if !line.contains(r#"deserialize_with = "de::null_default""#) {
                    continue;
                }
                uses += 1;
                let field = lines[index + 1..]
                    .iter()
                    .take(3)
                    .find(|next| next.trim_start().starts_with("pub "))
                    .unwrap();
                assert!(!field.contains("Vec<"), "liste en null_default : {field}");
            }
        }
        assert_eq!(uses, 2);
    }
}
