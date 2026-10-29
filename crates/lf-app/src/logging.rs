//! Journalisation `tracing-subscriber` sur stderr (jamais stdout, qui ne porte que du JSON-RPC).
//!
//! Seul `LF_LOG` règle le filtre ; `RUST_LOG` est ignoré (02 §3.7, §8.6). Le filtre par défaut
//! plafonne `rmcp` à `warn`, car rmcp journalise en `debug` la réponse complète de chaque
//! appel, donc les arguments et le texte des outils.

#![allow(
    dead_code,
    reason = "consommé par la racine de composition (V2-04, `wiring::run`) ; retirer à ce moment"
)]

use tracing_subscriber::{
    EnvFilter,
    filter::LevelFilter,
    fmt::{
        MakeWriter, SubscriberBuilder,
        format::{DefaultFields, Format},
    },
};

/// Filtre quand `LF_LOG` est absent, vide ou invalide (02 §8.2).
pub(crate) const DEFAULT_LOG_FILTER: &str =
    "info,rmcp=warn,hyper=warn,h2=warn,rustls=warn,reqwest=warn";

/// Remarque à journaliser une fois le journal en place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FilterNote {
    /// `LF_LOG` est invalide : le filtre par défaut s'applique.
    InvalidFilter,
    /// `LF_LOG` active `debug` ou `trace` pour `rmcp`, qui journalise alors les réponses entières.
    RmcpLogsFullResponses,
}

/// Filtre effectif et remarques sur `LF_LOG`.
#[derive(Debug)]
pub(crate) struct LogPlan {
    /// Directives du filtre effectif.
    pub(crate) directives: String,
    /// Filtre construit à partir des directives.
    pub(crate) filter: EnvFilter,
    /// Remarques, dans l'ordre.
    pub(crate) notes: Vec<FilterNote>,
}

impl LogPlan {
    fn new(directives: String, notes: Vec<FilterNote>) -> Self {
        // Les directives viennent de `DEFAULT_LOG_FILTER` ou ont déjà passé `try_new` : `new` n'a
        // donc aucune directive invalide à signaler sur stderr.
        let filter = EnvFilter::new(&directives);
        Self { directives, filter, notes }
    }
}

/// Calcule le filtre effectif d'une valeur de `LF_LOG` (02 §8.6).
///
/// - Vide ou blanc : le filtre par défaut.
/// - Invalide : le filtre par défaut et la remarque [`FilterNote::InvalidFilter`], sans recopier
///   la valeur.
/// - `debug` ou `trace` quelque part sans mention de `rmcp` : `rmcp=warn` est ajouté.
/// - `rmcp` mentionné : respecté, avec [`FilterNote::RmcpLogsFullResponses`] s'il ouvre `debug`.
pub(crate) fn plan_filter(raw: &str) -> LogPlan {
    let raw = raw.trim();
    if raw.is_empty() {
        return LogPlan::new(DEFAULT_LOG_FILTER.to_owned(), Vec::new());
    }
    let Ok(requested) = EnvFilter::try_new(raw) else {
        return LogPlan::new(DEFAULT_LOG_FILTER.to_owned(), vec![FilterNote::InvalidFilter]);
    };
    // `EnvFilter` sépare les directives à chaque virgule (builder.rs de tracing-subscriber 0.3) :
    // même découpage ici, pour voir les directives comme lui.
    let rmcp: Vec<&str> = raw.split(',').map(str::trim).filter(|d| targets_rmcp(d)).collect();
    if rmcp.is_empty() {
        let directives =
            if opens_debug(&requested) { format!("{raw},{RMCP_CAP}") } else { raw.to_owned() };
        return LogPlan::new(directives, Vec::new());
    }
    let notes = if rmcp.into_iter().any(|d| EnvFilter::try_new(d).is_ok_and(|f| opens_debug(&f))) {
        vec![FilterNote::RmcpLogsFullResponses]
    } else {
        Vec::new()
    };
    LogPlan::new(raw.to_owned(), notes)
}

/// Directive qui plafonne rmcp.
const RMCP_CAP: &str = "rmcp=warn";

/// Vrai si le filtre peut ouvrir `debug` ou `trace` (sans indication : on suppose que oui).
fn opens_debug(filter: &EnvFilter) -> bool {
    filter.max_level_hint().is_none_or(|level| level >= LevelFilter::DEBUG)
}

/// Vrai si la directive vise la cible `rmcp` ou l'un de ses modules (`rmcp::service`), et non un
/// nom voisin comme `rmcp_extra`.
fn targets_rmcp(directive: &str) -> bool {
    let target = directive.split(['[', '=']).next().unwrap_or_default().trim();
    target == "rmcp" || target.starts_with("rmcp::")
}

/// Constructeur de journal : sans ANSI (stderr n'est pas un terminal) et avec la cible de chaque
/// événement (02 §8.6).
pub(crate) fn builder<W>(
    filter: EnvFilter,
    writer: W,
) -> SubscriberBuilder<DefaultFields, Format, EnvFilter, W>
where
    W: for<'w> MakeWriter<'w> + 'static,
{
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(true)
        .with_env_filter(filter)
}

/// Journalise les remarques sur `LF_LOG`, sans jamais recopier sa valeur.
pub(crate) fn report(notes: &[FilterNote]) {
    for note in notes {
        match note {
            FilterNote::InvalidFilter => {
                tracing::warn!("invalid LF_LOG filter, using the default filter");
            }
            FilterNote::RmcpLogsFullResponses => {
                tracing::warn!("rmcp debug logs full responses");
            }
        }
    }
}

/// Installe le journal global sur stderr avec la valeur de `LF_LOG` (vide : filtre par défaut).
///
/// À appeler avant de charger le reste de la configuration, pour que ses avertissements passent
/// par stderr (02 §3.7). Sans effet si un journal global est déjà installé.
pub(crate) fn init_logging(filter: &str) {
    let plan = plan_filter(filter);
    // Seule erreur possible : un journal global existe déjà ; le premier reste en place.
    let _ = builder(plan.filter, std::io::stderr).try_init();
    report(&plan.notes);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::process::Command;
    use std::sync::{Arc, Mutex};

    /// Tampon partagé qui sert de sortie au journal.
    #[derive(Clone, Default)]
    struct SharedBuf(Arc<Mutex<Vec<u8>>>);

    impl io::Write for SharedBuf {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl SharedBuf {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    /// Ce que le journal écrit pour `LF_LOG=raw` pendant que `emit` produit des événements.
    fn captured(raw: &str, emit: impl FnOnce()) -> String {
        let buf = SharedBuf::default();
        let sink = buf.clone();
        let subscriber = builder(plan_filter(raw).filter, move || sink.clone()).finish();
        tracing::subscriber::with_default(subscriber, emit);
        buf.text()
    }

    fn directives(raw: &str) -> String {
        plan_filter(raw).directives
    }

    fn notes(raw: &str) -> Vec<FilterNote> {
        plan_filter(raw).notes
    }

    /// 02 §8.6 : `LF_LOG=debug` n'ouvre pas les journaux `debug` de rmcp, qui portent les réponses.
    #[test]
    fn default_log_filter_caps_rmcp() {
        assert!(DEFAULT_LOG_FILTER.contains("rmcp=warn"));
        assert_eq!(directives(""), DEFAULT_LOG_FILTER);
        for raw in ["", "debug", "trace"] {
            let out = captured(raw, || {
                tracing::debug!(target: "rmcp::service", "lf-rmcp-full-response");
                tracing::info!(target: "rmcp::service", "lf-rmcp-notification");
                tracing::warn!(target: "rmcp::service", "lf-rmcp-warning");
                tracing::debug!(target: "lf_app::wiring", "lf-own-debug");
            });
            assert!(!out.contains("lf-rmcp-full-response"), "LF_LOG={raw:?} : {out}");
            assert!(!out.contains("lf-rmcp-notification"), "LF_LOG={raw:?} : {out}");
            assert!(out.contains("lf-rmcp-warning"), "LF_LOG={raw:?} : {out}");
            assert_eq!(out.contains("lf-own-debug"), !raw.is_empty(), "LF_LOG={raw:?} : {out}");
        }
    }

    #[test]
    fn blank_filter_uses_the_default_without_a_note() {
        for raw in ["", "   ", "\t\n"] {
            assert_eq!(directives(raw), DEFAULT_LOG_FILTER);
            assert_eq!(notes(raw), []);
        }
    }

    #[test]
    fn invalid_filter_falls_back_to_the_default_with_a_note() {
        let plan = plan_filter("debug,hyper=loud");
        assert_eq!(plan.directives, DEFAULT_LOG_FILTER);
        assert_eq!(plan.notes, [FilterNote::InvalidFilter]);
        let out = captured(DEFAULT_LOG_FILTER, || report(&plan.notes));
        assert!(out.contains("WARN"), "{out}");
        assert!(out.contains("LF_LOG"), "{out}");
        assert!(!out.contains("loud"), "la valeur invalide n'est pas recopiée : {out}");
    }

    #[test]
    fn debug_and_trace_filters_get_the_rmcp_cap() {
        for raw in ["debug", "trace", "info,lf_app=debug", "reqwest", "warn,hyper=trace"] {
            assert_eq!(directives(raw), format!("{raw},rmcp=warn"), "{raw}");
            assert_eq!(notes(raw), [], "{raw}");
        }
    }

    #[test]
    fn debug_directives_with_span_fields_are_still_detected() {
        let raw = "info,lf_app[request{id=1}]=debug";
        assert_eq!(directives(raw), format!("{raw},rmcp=warn"));
    }

    #[test]
    fn quieter_filters_are_left_alone() {
        for raw in ["info", "warn", "error", "off", "info,lf_app=warn", " info "] {
            assert_eq!(directives(raw), raw.trim(), "{raw}");
            assert_eq!(notes(raw), [], "{raw}");
        }
    }

    /// Une mention explicite de rmcp est respectée ; elle ne prévient que si elle ouvre `debug`.
    #[test]
    fn explicit_rmcp_directive_is_respected() {
        for raw in ["debug,rmcp=debug", "rmcp::service=trace", "info,rmcp"] {
            assert_eq!(directives(raw), raw, "{raw}");
            assert_eq!(notes(raw), [FilterNote::RmcpLogsFullResponses], "{raw}");
        }
        for raw in ["debug,rmcp=warn", "debug,rmcp::service=info"] {
            assert_eq!(directives(raw), raw, "{raw}");
            assert_eq!(notes(raw), [], "{raw}");
        }
        let out = captured("debug,rmcp=debug", || {
            tracing::debug!(target: "rmcp::service", "lf-rmcp-full-response");
        });
        assert!(out.contains("lf-rmcp-full-response"), "{out}");
    }

    /// `rmcp_extra` n'est pas la cible `rmcp` : le plafond s'ajoute.
    #[test]
    fn lookalike_targets_do_not_count_as_mentioning_rmcp() {
        let raw = "debug,rmcp_extra=info";
        assert_eq!(directives(raw), format!("{raw},rmcp=warn"));
        assert_eq!(notes(raw), []);
    }

    #[test]
    fn the_warning_about_rmcp_is_reported() {
        let out = captured(DEFAULT_LOG_FILTER, || report(&[FilterNote::RmcpLogsFullResponses]));
        assert!(out.contains("rmcp debug logs full responses"), "{out}");
    }

    #[test]
    fn events_show_their_target_and_never_use_ansi() {
        let out = captured("info", || tracing::info!(target: "lf_test", "hello"));
        assert!(out.contains(" INFO lf_test: hello"), "{out}");
        assert!(!out.contains('\u{1b}'), "{out:?}");
    }

    #[test]
    fn init_logging_twice_is_harmless() {
        init_logging("info");
        init_logging("debug");
    }

    /// Fils de `init_logging_writes_to_stderr_only_and_ignores_rust_log` : à lancer par lui seul.
    #[test]
    fn child_emits_log_events() {
        init_logging("");
        tracing::info!("lf-info-marker");
        tracing::debug!("lf-debug-marker");
    }

    /// Le journal global écrit sur stderr, sans ANSI et sans rien sur stdout, et ignore
    /// `RUST_LOG` (le test relance ce binaire, car le journal global ne s'installe qu'une fois).
    #[test]
    fn init_logging_writes_to_stderr_only_and_ignores_rust_log() {
        let exe = std::env::current_exe().unwrap();
        let out = Command::new(exe)
            .args(["--exact", "logging::tests::child_emits_log_events", "--nocapture"])
            .env("RUST_LOG", "trace")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{stdout}\n{stderr}");
        assert!(stdout.contains("1 passed"), "le test fils n'a pas tourné : {stdout}");
        assert!(stderr.contains("lf-info-marker"), "{stderr}");
        assert!(!stderr.contains("lf-debug-marker"), "RUST_LOG doit être ignoré : {stderr}");
        assert!(!stderr.contains('\u{1b}'), "{stderr:?}");
        assert!(!stdout.contains("lf-info-marker"), "stdout ne porte jamais de journal : {stdout}");
    }
}
