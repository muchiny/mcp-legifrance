//! Enregistre des réponses brutes de PISTE en fixtures (04 §13.4), par le vrai client de
//! l'adapter. Lancé par `cargo xtask record-fixture`, jamais par le binaire.
//!
//! ```text
//! record_fixture --endpoint /consult/juri --body '{"textId":"…"}' --name juri/… \
//!     [--env sandbox|production] [--allow-error] [--redact <pointeur>]… [--force] [--out <dir>]
//! record_fixture --plan <plan.toml> [--only <préfixe>] [--env …] [--force] [--out <dir>]
//! ```
//!
//! Identifiants : `LF_PISTE_CLIENT_ID` et `LF_PISTE_CLIENT_SECRET`, dans l'environnement
//! seulement, jamais en argument ni à l'écran. Sandbox par défaut ; `--env production` est un
//! choix explicite du mainteneur (Q3). Une requête par seconde au plus (limiteur de l'adapter,
//! rafale 1, une requête en vol), borne de corps relevée à 64 Mio pour mesurer les gros textes
//! (V-75, V-96). Glue d'E/S : la logique vit dans `lf_adapter_piste::record`.

use std::env;
use std::fs;
use std::num::{NonZeroU16, NonZeroU32};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lf_adapter_piste::record::{self, MAX_VERSIONED_BYTES, Plan, Probe};
use lf_adapter_piste::{
    PisteConfig, PisteCorpus, PisteCredentials, PisteEndpoints, PisteTuning, RateQuota,
};
use lf_domain::coverage::PisteEnvironment;
use secrecy::SecretString;

const USAGE: &str = "usage: record_fixture (--endpoint <path> --body <json> --name <name> [--allow-error] [--redact <pointer>]... | --plan <plan.toml> [--only <prefix>]) [--env sandbox|production] [--force] [--out <dir>]";

/// Arguments lus.
#[derive(Default)]
struct Args {
    probe: Probe,
    plan: Option<PathBuf>,
    only: Option<String>,
    env: Option<String>,
    force: bool,
    out: Option<PathBuf>,
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut parsed = Args::default();
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--endpoint" => parsed.probe.endpoint = value()?,
            "--body" => parsed.probe.body = value()?,
            "--name" => parsed.probe.name = value()?,
            "--redact" => parsed.probe.redact.push(value()?),
            "--allow-error" => parsed.probe.allow_error = true,
            "--plan" => parsed.plan = Some(PathBuf::from(value()?)),
            "--only" => parsed.only = Some(value()?),
            "--env" => parsed.env = Some(value()?),
            "--force" => parsed.force = true,
            "--out" => parsed.out = Some(PathBuf::from(value()?)),
            _ => return Err(format!("unknown argument {flag:?}\n{USAGE}")),
        }
    }
    Ok(parsed)
}

fn environment(name: Option<&str>) -> Result<PisteEnvironment, String> {
    match name.unwrap_or("sandbox") {
        "sandbox" => Ok(PisteEnvironment::Sandbox),
        "production" => Ok(PisteEnvironment::Production),
        other => Err(format!("--env must be sandbox or production, not {other:?}")),
    }
}

fn corpus(env: PisteEnvironment) -> Result<PisteCorpus, String> {
    let (Ok(id), Ok(secret)) = (env::var("LF_PISTE_CLIENT_ID"), env::var("LF_PISTE_CLIENT_SECRET"))
    else {
        return Err("set LF_PISTE_CLIENT_ID and LF_PISTE_CLIENT_SECRET in the environment".into());
    };
    let credentials = PisteCredentials::new(id, SecretString::from(secret));
    let user_agent = format!(
        "mcp-legifrance/{} (+https://github.com/muchiny/mcp-legifrance) record-fixture",
        env!("CARGO_PKG_VERSION")
    );
    let one = NonZeroU32::MIN;
    let tuning = PisteTuning {
        request_timeout: Duration::from_secs(120),
        tool_deadline: Duration::from_secs(180),
        rate: RateQuota::new(one, one),
        max_concurrency: NonZeroU16::MIN,
        max_body_bytes: 64 << 20,
        ..PisteTuning::default()
    };
    let config =
        PisteConfig::new(env, Ok(PisteEndpoints::official(env)), Some(credentials), user_agent)
            .with_tuning(tuning)
            .map_err(|error| error.to_string())?;
    Ok(PisteCorpus::new(config))
}

fn probes(args: &Args) -> Result<Vec<Probe>, String> {
    let Some(path) = &args.plan else {
        return Ok(vec![args.probe.clone()]);
    };
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let plan: Plan =
        toml::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    let prefix = args.only.as_deref().unwrap_or("");
    Ok(plan.probe.into_iter().filter(|probe| probe.name.starts_with(prefix)).collect())
}

fn now() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    record::rfc3339_utc(i64::try_from(secs).unwrap_or(i64::MAX))
}

async fn run(args: Args) -> Result<usize, String> {
    let env = environment(args.env.as_deref())?;
    let probes = probes(&args)?;
    for probe in &probes {
        probe.check().map_err(|error| format!("{}: {error}", probe.name))?;
    }
    let corpus = corpus(env)?;
    let root = args.out.clone().unwrap_or_else(|| {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/recorded"))
    });
    let mut failed = 0;
    for probe in &probes {
        let line = match corpus.exchange(&probe.endpoint, probe.body.as_bytes()).await {
            Ok(exchange) => {
                let head = format!(
                    "{}\t{}\t{} bytes\t{} ms\tmojibake={}",
                    probe.name,
                    exchange.status,
                    exchange.body.len(),
                    exchange.elapsed.as_millis(),
                    record::mojibake_detected(&exchange.body)
                );
                match record::write(&root, probe, &exchange, env, &now(), args.force) {
                    Ok(written) if written.bytes_file > MAX_VERSIONED_BYTES => {
                        format!("{head}\twritten (> 1 MiB: version a reduced derivative)")
                    }
                    Ok(_) => format!("{head}\twritten"),
                    Err(error) => {
                        failed += 1;
                        format!("{head}\tNOT WRITTEN: {error}")
                    }
                }
            }
            Err(error) => {
                failed += 1;
                format!("{}\terror: {error}", probe.name)
            }
        };
        println!("{line}");
    }
    Ok(failed)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let result = match parse(env::args().skip(1)) {
        Ok(args) => run(args).await,
        Err(message) => Err(message),
    };
    match result {
        Ok(0) => ExitCode::SUCCESS,
        Ok(failed) => {
            eprintln!("{failed} probe(s) not recorded");
            ExitCode::FAILURE
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}
