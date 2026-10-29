//! Sous-commande `check-config` : valide la configuration sans appel réseau (02 §3.7, §8.1).
//!
//! La commande charge la configuration (`config::load`), puis affiche une ligne par variable de la
//! liste fermée de 00 §1 : nom, source (`LF_*`, `FR_ELI_*` ou défaut), valeur et état. Les secrets
//! n'y figurent jamais (« set (n characters) »), l'identifiant client est tronqué à 4 caractères,
//! et aucune valeur refusée n'est recopiée. Elle liste ensuite les avertissements et les problèmes.
//!
//! Avec `--online`, une seule requête de jeton OAuth part vers PISTE (`check_credentials`), jamais
//! d'appel à l'API (04 §2.4). Elle est omise, sans réseau, quand la configuration est invalide.
//!
//! Codes de sortie (02 §8.1) : 0 si tout est valide, 2 si la configuration est invalide, 3 si PISTE
//! refuse les identifiants, 4 en cas d'erreur réseau.
//!
//! `check-config` est la seule commande qui écrit sur stdout : la sortie passe par un
//! `impl io::Write` que l'appelant ouvre sur `io::stdout().lock()`, ce que `clippy::print_stdout`
//! ne voit pas (02 §3.7, piège 3).

#![allow(dead_code, reason = "binaire sans appelant avant le câblage de `main` (V2-04)")]

use std::{fmt::Write as _, io, process::ExitCode};

use lf_adapter_piste::PisteCorpus;
use lf_application::ports::CorpusError;

use crate::config::{Loaded, SettingLine};

/// Code de sortie d'une configuration invalide (celui de clap pour une erreur d'usage).
const EXIT_INVALID_CONFIG: u8 = 2;
/// Code de sortie quand PISTE refuse les identifiants.
const EXIT_CREDENTIALS_REFUSED: u8 = 3;
/// Code de sortie en cas d'erreur réseau.
const EXIT_NETWORK: u8 = 4;

/// Format de la sortie (`--format text|json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    /// Tableau lisible.
    Text,
    /// Un objet JSON sur une ligne.
    Json,
}

/// Options de `check-config`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Options {
    /// Format de la sortie.
    pub(crate) format: Format,
    /// Tente d'obtenir un jeton OAuth (`--online`).
    pub(crate) online: bool,
}

/// Résultat de la vérification en ligne.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Online {
    /// `--online` n'est pas demandé.
    NotRequested,
    /// Demandé mais omis : la configuration est invalide, aucune requête n'est partie.
    Skipped,
    /// PISTE a délivré un jeton.
    TokenObtained,
    /// PISTE a refusé les identifiants.
    Refused {
        /// Statut HTTP reçu, s'il est connu.
        status: Option<u16>,
    },
    /// PISTE n'a pas répondu (réseau, délai, quota).
    Unreachable,
}

impl Online {
    /// Clé stable pour la sortie JSON.
    fn key(self) -> &'static str {
        match self {
            Self::NotRequested => "not_requested",
            Self::Skipped => "skipped",
            Self::TokenObtained => "token_obtained",
            Self::Refused { .. } => "credentials_refused",
            Self::Unreachable => "network_error",
        }
    }

    /// Phrase de la sortie texte.
    fn sentence(self) -> String {
        match self {
            Self::NotRequested => "not requested".to_owned(),
            Self::Skipped => "skipped (the configuration is invalid)".to_owned(),
            Self::TokenObtained => "token obtained".to_owned(),
            Self::Refused { status: Some(status) } => {
                format!("credentials refused by PISTE (HTTP {status})")
            }
            Self::Refused { status: None } => "credentials refused by PISTE".to_owned(),
            Self::Unreachable => "PISTE could not be reached".to_owned(),
        }
    }

    /// Statut HTTP du refus, s'il y en a un.
    fn http_status(self) -> Option<u16> {
        match self {
            Self::Refused { status } => status,
            _ => None,
        }
    }
}

/// Code de sortie : une configuration invalide l'emporte sur le résultat en ligne.
pub(crate) fn exit_code(loaded: &Loaded, online: Online) -> u8 {
    if !loaded.is_valid() {
        return EXIT_INVALID_CONFIG;
    }
    match online {
        Online::Refused { .. } => EXIT_CREDENTIALS_REFUSED,
        Online::Unreachable => EXIT_NETWORK,
        Online::NotRequested | Online::Skipped | Online::TokenObtained => 0,
    }
}

/// Vérification en ligne : une requête de jeton, jamais d'appel à l'API. Rien ne part si elle n'est
/// pas demandée ou si la configuration est invalide.
pub(crate) async fn probe(loaded: &Loaded, requested: bool) -> Online {
    if !requested {
        return Online::NotRequested;
    }
    if !loaded.is_valid() {
        return Online::Skipped;
    }
    match PisteCorpus::new(loaded.config.piste.clone()).check_credentials().await {
        Ok(()) => Online::TokenObtained,
        Err(CorpusError::Auth { status, .. }) => Online::Refused { status },
        // Le `detail` des erreurs de l'adapter est réservé aux journaux : il n'est pas affiché.
        Err(_) => Online::Unreachable,
    }
}

/// Exécute `check-config` : vérifie en ligne si demandé, écrit le rapport dans `out` et rend le
/// code de sortie.
///
/// # Errors
///
/// L'erreur d'écriture de `out`.
pub(crate) async fn run(
    loaded: &Loaded,
    options: Options,
    out: &mut impl io::Write,
) -> io::Result<ExitCode> {
    let online = probe(loaded, options.online).await;
    out.write_all(render(loaded, online, options.format).as_bytes())?;
    out.flush()?;
    Ok(ExitCode::from(exit_code(loaded, online)))
}

/// Rapport complet, terminé par un saut de ligne.
pub(crate) fn render(loaded: &Loaded, online: Online, format: Format) -> String {
    match format {
        Format::Text => render_text(loaded, online),
        Format::Json => render_json(loaded, online),
    }
}

/// Messages des avertissements, puis des problèmes.
fn messages(loaded: &Loaded) -> (Vec<String>, Vec<String>) {
    let (problems, warnings): (Vec<_>, Vec<_>) =
        loaded.notices.iter().partition(|notice| notice.is_problem());
    (
        warnings.iter().map(ToString::to_string).collect(),
        problems.iter().map(ToString::to_string).collect(),
    )
}

/// Tableau lisible : une ligne par variable, puis avertissements, problèmes et résultat.
fn render_text(loaded: &Loaded, online: Online) -> String {
    let widths = columns(&loaded.lines);
    let mut lines = vec![
        "mcp-legifrance check-config".to_owned(),
        String::new(),
        row(widths, "VARIABLE", "SOURCE", "VALUE", "STATE"),
    ];
    for line in &loaded.lines {
        let state = line.state.label();
        lines.push(row(widths, line.name, line.source.label(), &line.value, state));
    }
    let (warnings, problems) = messages(loaded);
    for (title, items) in [("Warnings", warnings), ("Problems", problems)] {
        if !items.is_empty() {
            lines.push(String::new());
            lines.push(format!("{title}:"));
            lines.extend(items.into_iter().map(|item| format!("  - {item}")));
        }
    }
    if online != Online::NotRequested {
        lines.push(String::new());
        lines.push(format!("Online check: {}", online.sentence()));
    }
    lines.push(String::new());
    lines.push(if loaded.is_valid() { "Result: valid" } else { "Result: invalid" }.to_owned());
    lines.push(String::new());
    lines.join("\n")
}

/// Largeurs des trois premières colonnes, titres compris.
fn columns(lines: &[SettingLine]) -> [usize; 3] {
    let mut widths = ["VARIABLE".len(), "SOURCE".len(), "VALUE".len()];
    for line in lines {
        widths[0] = widths[0].max(line.name.len());
        widths[1] = widths[1].max(line.source.label().len());
        widths[2] = widths[2].max(line.value.chars().count());
    }
    widths
}

/// Une ligne du tableau, sans blanc final.
fn row(widths: [usize; 3], name: &str, source: &str, value: &str, state: &str) -> String {
    let [name_width, source_width, value_width] = widths;
    format!("{name:<name_width$}  {source:<source_width$}  {value:<value_width$}  {state}")
        .trim_end()
        .to_owned()
}

/// Un objet JSON sur une ligne : `valid`, `variables`, `warnings`, `problems` et `online`.
fn render_json(loaded: &Loaded, online: Online) -> String {
    let variables = loaded.lines.iter().map(|line| {
        format!(
            r#"{{"name":{},"source":{},"value":{},"state":{}}}"#,
            json_string(line.name),
            json_string(line.source.label()),
            json_string(&line.value),
            json_string(line.state.key()),
        )
    });
    let (warnings, problems) = messages(loaded);
    let status =
        online.http_status().map_or_else(|| "null".to_owned(), |status| status.to_string());
    format!(
        r#"{{"valid":{},"variables":{},"warnings":{},"problems":{},"online":{{"result":{},"http_status":{}}}}}"#,
        loaded.is_valid(),
        json_array(variables),
        json_array(warnings.iter().map(|message| json_string(message))),
        json_array(problems.iter().map(|message| json_string(message))),
        json_string(online.key()),
        status,
    ) + "\n"
}

/// Tableau JSON des éléments déjà sérialisés.
fn json_array(items: impl Iterator<Item = String>) -> String {
    format!("[{}]", items.collect::<Vec<_>>().join(","))
}

/// Chaîne JSON entre guillemets : `"`, `\` et les caractères de contrôle sont échappés.
fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if u32::from(control) < 0x20 => {
                // Écrire dans une `String` ne peut pas échouer.
                let _ = write!(out, "\\u{:04x}", u32::from(control));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{load, test_env::MapEnv};
    use serde_json::Value;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    const ID: &str = "11112222-3333-4444-5555-666677778888";
    const SECRET: &str = "99990000-aaaa-bbbb-cccc-ddddeeeeffff";

    fn loaded(pairs: &[(&str, &str)]) -> Loaded {
        load(&MapEnv::new(pairs))
    }

    fn valid() -> Loaded {
        loaded(&[("LF_PISTE_CLIENT_ID", ID), ("LF_PISTE_CLIENT_SECRET", SECRET)])
    }

    fn json_of(loaded: &Loaded, online: Online) -> Value {
        let text = render(loaded, online, Format::Json);
        assert!(text.ends_with('\n') && text.matches('\n').count() == 1, "{text:?}");
        serde_json::from_str(&text).unwrap()
    }

    // --- Codes de sortie ------------------------------------------------------------------------

    #[test]
    fn check_config_exit_codes() {
        let ok = valid();
        assert_eq!(exit_code(&ok, Online::NotRequested), 0);
        assert_eq!(exit_code(&ok, Online::TokenObtained), 0);
        assert_eq!(exit_code(&ok, Online::Skipped), 0);
        assert_eq!(exit_code(&ok, Online::Refused { status: Some(401) }), 3);
        assert_eq!(exit_code(&ok, Online::Refused { status: None }), 3);
        assert_eq!(exit_code(&ok, Online::Unreachable), 4);
        // Un avertissement n'invalide pas la configuration.
        let warned = loaded(&[
            ("LF_PISTE_CLIENT_ID", ID),
            ("LF_PISTE_CLIENT_SECRET", SECRET),
            ("FR_ELI_ENV", "production"),
        ]);
        assert_eq!(exit_code(&warned, Online::NotRequested), 0);
        // Configuration invalide : 2, quel que soit le résultat en ligne.
        let degraded = loaded(&[]);
        for online in [
            Online::NotRequested,
            Online::Skipped,
            Online::TokenObtained,
            Online::Refused { status: Some(401) },
            Online::Unreachable,
        ] {
            assert_eq!(exit_code(&degraded, online), 2, "{online:?}");
        }
        let bad_env = loaded(&[
            ("LF_PISTE_CLIENT_ID", ID),
            ("LF_PISTE_CLIENT_SECRET", SECRET),
            ("LF_PISTE_ENV", "prod"),
        ]);
        assert_eq!(exit_code(&bad_env, Online::NotRequested), 2);
    }

    // --- Contenu --------------------------------------------------------------------------------

    #[test]
    fn check_config_masks_secret() {
        let loaded = valid();
        for format in [Format::Text, Format::Json] {
            let shown = render(&loaded, Online::NotRequested, format);
            assert!(!shown.contains(SECRET), "{shown}");
            assert!(!shown.contains(ID), "{shown}");
            assert!(shown.contains("set (36 characters)"), "{shown}");
            assert!(shown.contains("1111..."), "{shown}");
        }
    }

    #[test]
    fn check_config_lists_every_variable() {
        let names = [
            "LF_PISTE_ENV",
            "LF_PISTE_CLIENT_ID",
            "LF_PISTE_CLIENT_SECRET",
            "LF_PISTE_OAUTH_URL",
            "LF_PISTE_BASE_URL",
            "LF_AUDIT_DIR",
            "LF_AUDIT_MODE",
            "LF_LOG",
            "LF_HTTP_TIMEOUT_S",
            "LF_TOOL_DEADLINE_S",
            "LF_PISTE_RPS",
            "LF_PISTE_BURST",
            "LF_PISTE_CONCURRENCY",
            "LF_PISTE_MOJIBAKE_REPAIR",
        ];
        let ignored = [
            "FR_ELI_CACHE_DIR",
            "FR_ELI_RUNTIME_URL",
            "FR_ELI_ENV",
            "PISTE_CLIENT_ID",
            "PISTE_CLIENT_SECRET",
        ];
        let mut pairs = vec![("LF_PISTE_CLIENT_ID", ID), ("LF_PISTE_CLIENT_SECRET", SECRET)];
        pairs.extend(ignored.iter().map(|name| (*name, "x")));
        let loaded = loaded(&pairs);

        let text = render(&loaded, Online::NotRequested, Format::Text);
        for name in names {
            let rows = text.lines().filter(|line| line.starts_with(name)).count();
            assert_eq!(rows, 1, "{name} : {text}");
        }
        // Une ligne « ignored » par variable héritée définie.
        for name in ignored {
            let hits =
                text.lines().filter(|line| line.contains(&format!("{name} ignored"))).count();
            assert_eq!(hits, 1, "{name} : {text}");
        }

        let json = json_of(&loaded, Online::NotRequested);
        let listed: Vec<_> = json["variables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|variable| variable["name"].as_str().unwrap())
            .collect();
        assert_eq!(listed, names);
        let warnings = json["warnings"].as_array().unwrap();
        for name in ignored {
            let hits = warnings
                .iter()
                .filter(|warning| warning.as_str().unwrap().contains(&format!("{name} ignored")))
                .count();
            assert_eq!(hits, 1, "{name}");
        }
        assert_eq!(json["problems"], Value::Array(Vec::new()));
    }

    #[test]
    fn text_report_shape() {
        let text = render(&valid(), Online::NotRequested, Format::Text);
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines[0], "mcp-legifrance check-config");
        assert_eq!(lines[1], "");
        assert!(lines[2].starts_with("VARIABLE"), "{text}");
        assert!(lines[2].ends_with("STATE"), "{text}");
        assert!(text.ends_with("Result: valid\n"), "{text}");
        // Pas de section vide, pas de ligne en ligne sans demande.
        assert!(!text.contains("Warnings:") && !text.contains("Problems:"), "{text}");
        assert!(!text.contains("Online check"), "{text}");
        // Colonnes alignées, sans blanc final.
        assert!(lines.iter().all(|line| line == &line.trim_end()), "{text}");
        let env_row = lines.iter().find(|line| line.starts_with("LF_PISTE_ENV")).unwrap();
        assert!(
            env_row.contains("default") && env_row.contains("sandbox") && env_row.ends_with("ok")
        );
        let value_column = lines[2].find("VALUE").unwrap();
        assert_eq!(env_row.find("sandbox"), Some(value_column), "{text}");
    }

    #[test]
    fn text_report_lists_warnings_and_problems() {
        let loaded = loaded(&[
            ("LF_PISTE_CLIENT_ID", ID),
            ("LF_PISTE_CLIENT_SECRET", SECRET),
            ("LF_PISTE_ENV", "prod"),
            ("FR_ELI_CACHE_DIR", "x"),
        ]);
        let text = render(&loaded, Online::Skipped, Format::Text);
        let warnings = text.find("Warnings:").unwrap();
        let problems = text.find("Problems:").unwrap();
        assert!(warnings < problems, "{text}");
        assert!(text.contains("  - FR_ELI_CACHE_DIR ignored: it is never read"), "{text}");
        assert!(text.contains("  - LF_PISTE_ENV='prod' is invalid"), "{text}");
        assert!(text.contains("Online check: skipped (the configuration is invalid)"), "{text}");
        assert!(text.ends_with("Result: invalid\n"), "{text}");
    }

    #[test]
    fn online_sentences_are_distinct() {
        assert_eq!(Online::NotRequested.sentence(), "not requested");
        assert_eq!(Online::TokenObtained.sentence(), "token obtained");
        assert_eq!(
            Online::Refused { status: Some(401) }.sentence(),
            "credentials refused by PISTE (HTTP 401)"
        );
        assert_eq!(Online::Refused { status: None }.sentence(), "credentials refused by PISTE");
        assert_eq!(Online::Unreachable.sentence(), "PISTE could not be reached");
        assert_eq!(Online::Refused { status: Some(403) }.http_status(), Some(403));
        assert_eq!(Online::TokenObtained.http_status(), None);
        let keys = [
            Online::NotRequested.key(),
            Online::Skipped.key(),
            Online::TokenObtained.key(),
            Online::Refused { status: None }.key(),
            Online::Unreachable.key(),
        ];
        assert_eq!(
            keys,
            ["not_requested", "skipped", "token_obtained", "credentials_refused", "network_error"]
        );
    }

    #[test]
    fn json_report_is_one_valid_object() {
        let loaded = loaded(&[("FR_ELI_ENV", "x"), ("LF_PISTE_CLIENT_ID", ID)]);
        let json = json_of(&loaded, Online::Refused { status: Some(401) });
        assert_eq!(json["valid"], Value::Bool(false));
        assert_eq!(json["online"]["result"], "credentials_refused");
        assert_eq!(json["online"]["http_status"], 401);
        assert_eq!(
            json["problems"][0],
            "PISTE credentials missing: set LF_PISTE_CLIENT_ID and LF_PISTE_CLIENT_SECRET"
        );
        assert_eq!(json["warnings"][0], "FR_ELI_ENV ignored: it is never read");
        let first = &json["variables"][0];
        assert_eq!(first["name"], "LF_PISTE_ENV");
        assert_eq!(first["source"], "default");
        assert_eq!(first["value"], "sandbox");
        assert_eq!(first["state"], "ok");
        let secret = json["variables"]
            .as_array()
            .unwrap()
            .iter()
            .find(|variable| variable["name"] == "LF_PISTE_CLIENT_SECRET")
            .unwrap();
        assert_eq!(secret["state"], "unset");
        let valid = json_of(&valid(), Online::TokenObtained);
        assert_eq!(valid["valid"], Value::Bool(true));
        assert_eq!(valid["online"]["http_status"], Value::Null);
    }

    #[test]
    fn json_strings_escape_everything_that_needs_it() {
        for text in [
            "plain",
            "quote \" and backslash \\",
            "line\nbreak\r\ttab",
            "control \u{1} \u{1f} end",
            "accents é€ 日本 \u{2028} \u{1f600}",
            "",
        ] {
            let encoded = json_string(text);
            let decoded: String = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, text, "{encoded}");
        }
        assert_eq!(json_string("\u{1}"), r#""\u0001""#);
        assert_eq!(json_string("\u{1f}"), r#""\u001f""#);
        assert_eq!(json_string("\u{20}"), r#"" ""#);
        assert_eq!(json_array(["1".to_owned(), "2".to_owned()].into_iter()), "[1,2]");
        assert_eq!(json_array(std::iter::empty()), "[]");
    }

    // --- Vérification en ligne ------------------------------------------------------------------

    /// Serveur de jeton qui répond `status` et attend exactement `requests` requêtes.
    async fn token_server(status: u16, body: Value, requests: u64) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/oauth/token"))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .expect(requests)
            .mount(&server)
            .await;
        server
    }

    fn online_config(server_uri: &str) -> Loaded {
        let oauth = format!("{server_uri}/api/oauth/token");
        loaded(&[
            ("LF_PISTE_CLIENT_ID", ID),
            ("LF_PISTE_CLIENT_SECRET", SECRET),
            ("LF_PISTE_OAUTH_URL", &oauth),
        ])
    }

    async fn run_to_string(loaded: &Loaded, options: Options) -> (String, String) {
        let mut out = Vec::new();
        let code = run(loaded, options, &mut out).await.unwrap();
        (String::from_utf8(out).unwrap(), format!("{code:?}"))
    }

    fn code(value: u8) -> String {
        format!("{:?}", ExitCode::from(value))
    }

    #[tokio::test]
    async fn check_config_online_makes_one_token_request() {
        let body = serde_json::json!({
            "access_token": "tok-LEAK-0123456789",
            "token_type": "Bearer",
            "expires_in": 3600,
        });
        let server = token_server(200, body, 1).await;
        let loaded = online_config(&server.uri());
        let (text, exit) =
            run_to_string(&loaded, Options { format: Format::Text, online: true }).await;
        assert!(text.contains("Online check: token obtained"), "{text}");
        assert!(!text.contains("LEAK") && !text.contains(SECRET), "{text}");
        assert_eq!(exit, code(0));
        // `expect(1)` de wiremock vérifie, à la fermeture du serveur, qu'une seule requête est partie.
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn check_config_online_reports_refused_credentials() {
        let body = serde_json::json!({"error": "invalid_client", "error_description": "LEAK"});
        // Deux exécutions (texte puis JSON) : une requête de jeton chacune, jamais davantage.
        let server = token_server(401, body, 2).await;
        let loaded = online_config(&server.uri());
        let (text, exit) =
            run_to_string(&loaded, Options { format: Format::Text, online: true }).await;
        assert!(text.contains("Online check: credentials refused by PISTE (HTTP 401)"), "{text}");
        assert!(!text.contains("LEAK"), "{text}");
        assert_eq!(exit, code(3));
        let (json, _) =
            run_to_string(&loaded, Options { format: Format::Json, online: true }).await;
        assert!(json.contains(r#""http_status":401"#), "{json}");
        assert!(!json.contains("LEAK"), "{json}");
    }

    #[tokio::test]
    async fn check_config_online_reports_a_network_error() {
        // Un port qui vient d'être libéré refuse la connexion ; le délai d'un appel est de 1 s.
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let oauth = format!("http://127.0.0.1:{port}/api/oauth/token");
        let loaded = loaded(&[
            ("LF_PISTE_CLIENT_ID", ID),
            ("LF_PISTE_CLIENT_SECRET", SECRET),
            ("LF_PISTE_OAUTH_URL", &oauth),
            ("LF_TOOL_DEADLINE_S", "1"),
        ]);
        let (text, exit) =
            run_to_string(&loaded, Options { format: Format::Text, online: true }).await;
        assert!(text.contains("Online check: PISTE could not be reached"), "{text}");
        assert_eq!(exit, code(4));
    }

    #[tokio::test]
    async fn online_check_is_skipped_without_network_when_the_configuration_is_invalid() {
        // Aucun identifiant : une requête vers ce serveur ferait échouer `expect(0)`.
        let server = token_server(200, serde_json::json!({}), 0).await;
        let oauth = format!("{}/api/oauth/token", server.uri());
        let loaded = loaded(&[("LF_PISTE_OAUTH_URL", &oauth)]);
        let (text, exit) =
            run_to_string(&loaded, Options { format: Format::Text, online: true }).await;
        assert!(text.contains("Online check: skipped"), "{text}");
        assert_eq!(exit, code(2));
        assert_eq!(server.received_requests().await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn offline_run_sends_nothing_and_ignores_the_network() {
        let server = token_server(200, serde_json::json!({}), 0).await;
        let loaded = online_config(&server.uri());
        let (text, exit) =
            run_to_string(&loaded, Options { format: Format::Text, online: false }).await;
        assert!(!text.contains("Online check"), "{text}");
        assert_eq!(exit, code(0));
        assert_eq!(server.received_requests().await.unwrap().len(), 0);
        assert_eq!(probe(&loaded, false).await, Online::NotRequested);
    }

    #[tokio::test]
    async fn run_propagates_write_errors() {
        struct Broken;
        impl io::Write for Broken {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("closed"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let options = Options { format: Format::Json, online: false };
        assert!(run(&valid(), options, &mut Broken).await.is_err());
    }
}
