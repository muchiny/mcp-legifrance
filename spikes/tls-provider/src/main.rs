//! Sonde TLS du spike T-M2-27 : une même requête vers une URL, avec le fournisseur TLS
//! choisi à la compilation (`aws-lc`, option A, ou `ring`, option B) et trois bornes de
//! version TLS (sans borne, TLS 1.2 seul, TLS 1.3 seul).
//!
//! Hors workspace (`spikes/tls-provider`), jamais publié ni lancé par la CI du dépôt : le
//! workflow `tls-spike.yml` le compile sur les 7 cibles livrées et en consigne le résultat.
//! Les réglages du client reprennent ceux de l'adapter (`http::build_client`).

#[cfg(all(feature = "aws-lc", feature = "ring"))]
compile_error!("les features `aws-lc` et `ring` sont exclusives : en choisir une seule");

#[cfg(not(any(feature = "aws-lc", feature = "ring")))]
compile_error!("choisir un fournisseur TLS : feature `aws-lc` (défaut) ou `ring`");

use std::error::Error;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use reqwest::header::{ACCEPT, HeaderMap, HeaderValue};
use reqwest::tls::Version;
use reqwest::{Client, redirect};

/// URL sondée quand aucun argument n'est donné : la racine du bac à sable PISTE.
const DEFAULT_URL: &str = "https://sandbox-api.piste.gouv.fr/";

/// Nom du fournisseur compilé, écrit en première ligne de la sortie.
const PROVIDER: &str = if cfg!(feature = "ring") { "ring" } else { "aws-lc" };

/// Mots qui, en minuscules, signalent une panne TLS dans la chaîne d'une erreur : ceux de
/// `TLS_WORDS` de l'adapter (`http.rs`), pour confirmer sur de vrais certificats refusés la
/// reconnaissance par le texte.
const TLS_WORDS: [&str; 5] = ["tls", "certificate", "handshake", "alert", "corrupt message"];

/// Délai de connexion de l'adapter (`PisteConfig`, valeur par défaut).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Délai total d'une requête de l'adapter (`PisteConfig`, valeur par défaut).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(40);

/// Option B : `reqwest/rustls-no-provider` n'en installe aucun, et `Client::build()` panique
/// tant qu'aucun n'est installé. À appeler avant toute construction de client ; sans effet
/// si un fournisseur est déjà installé.
#[cfg(feature = "ring")]
fn install_provider() {
    let _already_installed = rustls::crypto::ring::default_provider().install_default();
}

/// Option A : `reqwest/rustls` embarque aws-lc et l'installe de lui-même, rien à faire.
#[cfg(not(feature = "ring"))]
fn install_provider() {}

/// Borne de version TLS d'une tentative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pin {
    /// Aucune borne : ce que négocie le client par défaut.
    Auto,
    /// Une seule version admise (minimum et maximum égaux).
    Only(Version),
}

impl Pin {
    /// Les trois tentatives, dans l'ordre de la sortie.
    const ALL: [Self; 3] = [Self::Auto, Self::Only(Version::TLS_1_2), Self::Only(Version::TLS_1_3)];

    /// Libellé stable de la tentative dans la sortie.
    fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Only(version) if version == Version::TLS_1_2 => "tls1.2",
            Self::Only(_) => "tls1.3",
        }
    }
}

/// Résultat d'une tentative.
#[derive(Debug)]
struct Outcome {
    /// Borne de version de la tentative.
    pin: Pin,
    /// Statut HTTP obtenu, ou l'échec.
    result: Result<u16, Failure>,
    /// Durée de la tentative, construction du client comprise.
    elapsed: Duration,
}

/// Échec d'une tentative.
#[derive(Debug)]
struct Failure {
    /// Genre : `build`, `timeout`, `tls`, `connect` ou `other`.
    kind: &'static str,
    /// Chaîne des causes, de l'effet à la cause première.
    detail: String,
}

/// Construit le client de la sonde avec les réglages de celui de l'adapter
/// (`http::build_client`) ; `Pin::Only` borne la version TLS des deux côtés.
fn build_client(pin: Pin) -> Result<Client, reqwest::Error> {
    let mut defaults = HeaderMap::new();
    defaults.insert(ACCEPT, HeaderValue::from_static("application/json"));
    let mut builder = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .user_agent(concat!("tls-probe/", env!("CARGO_PKG_VERSION")))
        .default_headers(defaults)
        .gzip(true)
        .redirect(redirect::Policy::none())
        .https_only(true);
    if let Pin::Only(version) = pin {
        builder = builder.tls_version_min(version).tls_version_max(version);
    }
    builder.build()
}

/// Causes d'une erreur, de l'effet à la cause première, jointes par « : ».
fn describe(error: &(dyn Error + 'static)) -> String {
    std::iter::successors(Some(error), |link| (*link).source())
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(": ")
}

/// Genre d'un échec : construction, délai, TLS, connexion, sinon autre. L'ordre compte : un
/// échec TLS est aussi un échec de connexion.
fn classify(error: &reqwest::Error) -> Failure {
    let detail = describe(error);
    let text = detail.to_lowercase();
    let kind = if error.is_builder() {
        "build"
    } else if error.is_timeout() {
        "timeout"
    } else if TLS_WORDS.iter().any(|word| text.contains(word)) {
        "tls"
    } else if error.is_connect() {
        "connect"
    } else {
        "other"
    };
    Failure { kind, detail }
}

/// Une requête GET vers `url` avec la borne `pin` ; seul le statut est lu.
async fn attempt(url: &str, pin: Pin) -> Outcome {
    let started = Instant::now();
    let result = match build_client(pin) {
        Ok(client) => match client.get(url).send().await {
            Ok(response) => Ok(response.status().as_u16()),
            Err(error) => Err(classify(&error)),
        },
        Err(error) => Err(classify(&error)),
    };
    Outcome { pin, result, elapsed: started.elapsed() }
}

/// Ligne de sortie d'une tentative.
fn render(outcome: &Outcome) -> String {
    let label = outcome.pin.label();
    let ms = outcome.elapsed.as_millis();
    match &outcome.result {
        Ok(status) => format!("attempt={label} status={status} ms={ms}"),
        Err(failure) => {
            format!("attempt={label} error={} ms={ms} detail={}", failure.kind, failure.detail)
        }
    }
}

/// Plus haute version TLS dont la tentative bornée a abouti. La version négociée n'est pas
/// lisible (`reqwest::TlsInfo` n'expose que le certificat du pair), d'où cette déduction.
fn highest_accepted(outcomes: &[Outcome]) -> &'static str {
    let accepted = |version: Version| {
        outcomes.iter().any(|outcome| outcome.pin == Pin::Only(version) && outcome.result.is_ok())
    };
    if accepted(Version::TLS_1_3) {
        "tls1.3"
    } else if accepted(Version::TLS_1_2) {
        "tls1.2"
    } else {
        "none"
    }
}

/// Seul l'échec de la tentative sans borne fait échouer la sonde : qu'un serveur refuse une
/// version épinglée est une donnée, pas une erreur.
fn failed(outcomes: &[Outcome]) -> bool {
    outcomes.iter().any(|outcome| outcome.pin == Pin::Auto && outcome.result.is_err())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    install_provider();
    let url = std::env::args().nth(1).unwrap_or_else(|| DEFAULT_URL.to_owned());
    println!("provider={PROVIDER}");
    println!("url={url}");
    let mut outcomes = Vec::new();
    for pin in Pin::ALL {
        let outcome = attempt(&url, pin).await;
        println!("{}", render(&outcome));
        outcomes.push(outcome);
    }
    println!("highest_accepted={}", highest_accepted(&outcomes));
    if failed(&outcomes) { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

#[cfg(test)]
mod tests {
    use std::fmt;
    use std::io;

    use super::*;

    /// Tentative aboutie, 12 ms.
    fn ok(pin: Pin, status: u16) -> Outcome {
        Outcome { pin, result: Ok(status), elapsed: Duration::from_millis(12) }
    }

    /// Tentative en échec TLS, 7 ms.
    fn err(pin: Pin) -> Outcome {
        Outcome {
            pin,
            result: Err(Failure { kind: "tls", detail: "handshake failed".to_owned() }),
            elapsed: Duration::from_millis(7),
        }
    }

    /// Erreur à deux maillons : « outer » puis `io::Error` « inner ».
    #[derive(Debug)]
    struct Outer(io::Error);

    impl fmt::Display for Outer {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("outer")
        }
    }

    impl std::error::Error for Outer {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
        }
    }

    #[test]
    fn labels_are_stable() {
        let labels: Vec<&str> = Pin::ALL.iter().map(|pin| pin.label()).collect();
        assert_eq!(labels, ["auto", "tls1.2", "tls1.3"]);
    }

    #[test]
    fn render_shows_status_or_error() {
        assert_eq!(render(&ok(Pin::Auto, 404)), "attempt=auto status=404 ms=12");
        assert_eq!(
            render(&err(Pin::Only(Version::TLS_1_2))),
            "attempt=tls1.2 error=tls ms=7 detail=handshake failed"
        );
    }

    #[test]
    fn highest_accepted_is_the_best_pinned_success() {
        let v12 = Pin::Only(Version::TLS_1_2);
        let v13 = Pin::Only(Version::TLS_1_3);
        assert_eq!(highest_accepted(&[ok(Pin::Auto, 401), ok(v12, 401), ok(v13, 401)]), "tls1.3");
        assert_eq!(highest_accepted(&[ok(Pin::Auto, 401), ok(v12, 401), err(v13)]), "tls1.2");
        assert_eq!(highest_accepted(&[err(Pin::Auto), err(v12), err(v13)]), "none");
    }

    #[test]
    fn only_a_failed_unpinned_attempt_fails_the_run() {
        let v12 = Pin::Only(Version::TLS_1_2);
        assert!(!failed(&[ok(Pin::Auto, 401), err(v12)]));
        assert!(failed(&[err(Pin::Auto), ok(v12, 401)]));
    }

    #[test]
    fn describe_joins_the_causes_from_effect_to_cause() {
        assert_eq!(describe(&Outer(io::Error::other("inner"))), "outer: inner");
    }

    #[cfg(feature = "aws-lc")]
    #[test]
    fn aws_lc_builds_without_installation() {
        for pin in Pin::ALL {
            assert!(build_client(pin).is_ok(), "client for {}", pin.label());
        }
    }

    #[cfg(feature = "ring")]
    #[test]
    fn provider_installed_before_first_client_builds() {
        install_provider();
        assert!(rustls::crypto::CryptoProvider::get_default().is_some());
        install_provider();
        for pin in Pin::ALL {
            assert!(build_client(pin).is_ok(), "client for {}", pin.label());
        }
    }

    /// Enfant de `building_without_provider_panics` : sans la variable, ne fait rien. Avec
    /// elle, construit un client sans fournisseur installé et imprime le résultat.
    #[cfg(feature = "ring")]
    #[test]
    fn child_builds_without_provider() {
        if std::env::var_os("TLS_PROBE_CHILD").is_none() {
            return;
        }
        let line = match std::panic::catch_unwind(|| build_client(Pin::Auto).is_ok()) {
            Ok(built) => format!("CHILD_RESULT=built: {built}"),
            Err(payload) => {
                let message = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|text| (*text).to_owned()))
                    .unwrap_or_default();
                let normalized = message.split_whitespace().collect::<Vec<_>>().join(" ");
                format!("CHILD_RESULT=panic: {normalized}")
            }
        };
        println!("{line}");
    }

    /// Le fournisseur est un état de processus et `cargo test` fait tourner les tests en
    /// threads du même processus : la preuve se fait dans un sous-processus, qui n'a rien
    /// installé.
    #[cfg(feature = "ring")]
    #[test]
    fn building_without_provider_panics() {
        let exe = std::env::current_exe().expect("path of the test binary");
        let output = std::process::Command::new(exe)
            .args(["--exact", "tests::child_builds_without_provider", "--nocapture"])
            .env("TLS_PROBE_CHILD", "1")
            .output()
            .expect("child process runs");
        assert!(output.status.success(), "child status: {:?}", output.status);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let line = stdout.lines().find(|line| line.starts_with("CHILD_RESULT=panic:"));
        assert!(
            line.is_some_and(|line| line.to_lowercase().contains("crypto provider")),
            "child stdout: {stdout}"
        );
    }
}
