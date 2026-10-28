//! Port sortant du journal d'audit (02 §5.2, D-16).

use std::future::Future;

use chrono::{DateTime, Utc};
pub use lf_domain::coverage::PisteEnvironment;
use thiserror::Error;

use crate::{error::ErrorCode, input::CanonicalInput, tools::ToolName};

/// Port sortant du journal d'audit.
///
/// Aucune méthode par défaut. Le futur est `+ Send` (rmcp boxe les futurs d'outils, 02 §3.2).
///
/// Contrat de toute implémentation (F-5 ; `JsonlAuditLog` au plan 3c) :
///
/// 1. une ligne remise à l'écrivain est écrite en entier, même si le futur de `record` est
///    abandonné ensuite (tâche de l'appel annulée) : l'abandon ne perd ni ne tronque une ligne
///    déjà remise ;
/// 2. aucune attente sans borne : au-delà d'une borne propre à l'adapter (écrivain figé, verrou
///    jamais rendu), `record` rend [`AuditErrorKind::WriterGone`] plutôt que de se figer, car
///    `Audited` attend l'accusé sans délai à lui (D-16).
pub trait AuditLog: Send + Sync + 'static {
    /// Écrit une ligne et ne rend la main qu'une fois l'écriture terminée (accusé, D-16).
    fn record(&self, record: &AuditRecord) -> impl Future<Output = Result<(), AuditError>> + Send;
}

/// Une ligne d'audit, construite par `Audited` à la fin de chaque appel d'outil.
#[derive(Debug, Clone)]
pub struct AuditRecord {
    /// `Clock::now_utc()` à l'entrée de la façade.
    pub ts: DateTime<Utc>,
    /// `"fr_search"`, … ; la lecture d'une ressource (M7) s'inscrit `resource:<nom du gabarit>`.
    pub tool: ToolName,
    /// JSON canonique des arguments ; l'adapter en calcule `input_hash`.
    pub input: CanonicalInput,
    /// `AuditMetric` (00 §7) ; 0 en erreur.
    pub output_count_or_size: u64,
    /// Durée réelle, y compris en erreur (D-03).
    pub duration_ms: u64,
    /// Issue de l'appel.
    pub status: AuditStatus,
    /// `None` si `Ok` ou en cas d'annulation (00 §7).
    pub error_code: Option<ErrorCode>,
    /// Message final de `AppError` sans le préfixe `[code]`, 500 caractères au plus, sans secret ;
    /// `cancelled by client` pour une annulation.
    pub error: Option<String>,
    /// Environnement PISTE interrogé.
    pub env: PisteEnvironment,
    /// Version du serveur (`CARGO_PKG_VERSION` de `lf-app`).
    pub server_version: &'static str,
}

/// Issue d'un appel d'outil.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditStatus {
    /// L'appel a abouti.
    Ok,
    /// L'appel a échoué ou a été annulé.
    Error,
}

/// Échec d'écriture du journal. Le message ne nomme que le genre. Le `detail` (il peut citer un
/// chemin) est journalisé par l'adapter d'audit lui-même (plan 3c, span `audit_write`, 02 §11.1) ;
/// `Audited` n'en journalise que le genre (`kind`) et le modèle ne le voit jamais.
#[derive(Debug, Error)]
#[error("audit log unavailable ({kind:?})")]
pub struct AuditError {
    /// Genre de l'échec.
    pub kind: AuditErrorKind,
    /// Détail pour le journal de l'adapter d'audit (span `audit_write`) seulement.
    pub detail: String,
}

/// Genre d'échec d'écriture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditErrorKind {
    /// Erreur d'entrée-sortie autre que celles ci-dessous.
    Io,
    /// Disque plein.
    DiskFull,
    /// Droits insuffisants.
    PermissionDenied,
    /// La ligne n'a pas pu être encodée.
    Encode,
    /// La tâche d'écriture a disparu.
    WriterGone,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{error::ErrorCode, input::CanonicalInput, tools::ToolName};
    use pretty_assertions::assert_eq;
    use std::{
        future::{Future, ready},
        pin::pin,
        task::{Context, Poll, Waker},
    };

    #[test]
    fn audit_error_display_names_the_kind_only() {
        let error = AuditError {
            kind: AuditErrorKind::DiskFull,
            detail: "write /var/log/lf/audit.jsonl: no space left on device".to_owned(),
        };
        assert_eq!(error.to_string(), "audit log unavailable (DiskFull)");
    }

    struct Memory;

    impl AuditLog for Memory {
        async fn record(&self, record: &AuditRecord) -> Result<(), AuditError> {
            ready(match record.status {
                AuditStatus::Ok => Ok(()),
                AuditStatus::Error => Err(AuditError {
                    kind: AuditErrorKind::WriterGone,
                    detail: record.error.clone().unwrap_or_default(),
                }),
            })
            .await
        }
    }

    /// Exige un futur `Send` et le sonde une fois : le faux journal n'attend rien.
    fn poll_once<F: Future + Send>(future: F) -> Poll<F::Output> {
        pin!(future).poll(&mut Context::from_waker(Waker::noop()))
    }

    fn record(status: AuditStatus) -> AuditRecord {
        AuditRecord {
            ts: chrono::DateTime::UNIX_EPOCH,
            tool: ToolName::FR_SEARCH,
            input: CanonicalInput::new("{}"),
            output_count_or_size: 0,
            duration_ms: 3,
            status,
            error_code: matches!(status, AuditStatus::Error).then_some(ErrorCode::AuditError),
            error: matches!(status, AuditStatus::Error).then(|| "boom".to_owned()),
            env: PisteEnvironment::Sandbox,
            server_version: "0.0.0",
        }
    }

    /// Écrit une ligne et ne garde de l'échec que le genre et le détail (`AuditError` n'est
    /// pas comparable).
    async fn written(record: AuditRecord) -> Result<(), (AuditErrorKind, String)> {
        Memory.record(&record).await.map_err(|error| (error.kind, error.detail))
    }

    #[test]
    fn port_traits_are_implementable_with_send_futures() {
        assert_eq!(poll_once(written(record(AuditStatus::Ok))), Poll::Ready(Ok(())));
        assert_eq!(
            poll_once(written(record(AuditStatus::Error))),
            Poll::Ready(Err((AuditErrorKind::WriterGone, "boom".to_owned())))
        );
    }

    #[test]
    fn records_are_cloneable_and_debuggable() {
        let original = record(AuditStatus::Error);
        let copy = original.clone();
        assert_eq!(format!("{copy:?}"), format!("{original:?}"));
    }

    #[test]
    fn record_debug_never_shows_the_arguments() {
        // L'adapter d'audit hache `input` ; un `Debug` journalisé n'en montre que la longueur.
        let sentinel = "sentinel-argument-value";
        let record = AuditRecord {
            input: CanonicalInput::new(format!(r#"{{"query":"{sentinel}"}}"#)),
            ..record(AuditStatus::Ok)
        };
        for rendered in [format!("{record:?}"), format!("{record:#?}")] {
            assert!(!rendered.contains(sentinel), "{rendered}");
            assert!(rendered.contains("CanonicalInput(<35 bytes>)"), "{rendered}");
        }
    }
}
