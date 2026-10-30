//! Composant interne de mcp-legifrance : journal d'audit JSONL du port `AuditLog` (`JsonlAuditLog`).
//!
//! Aucune garantie de stabilité d'API. Le crate n'ajoute que des lignes (jamais de réécriture ni
//! de suppression) au journal `mcp-legifrance.jsonl` d'un répertoire (`LF_AUDIT_DIR`). Spécification :
//! 06 §11 ; 02 §3.5 ; 00 §7.
//!
//! # Écriture
//!
//! - **Aucune E/S avant le premier `record()`** : [`JsonlAuditLog::open`] ne contrôle que ses
//!   arguments. Le répertoire, le fil d'écriture et les fichiers naissent au premier appel.
//! - **Un fil dédié** (`std::thread`) reçoit les lignes par un canal borné et répond par un
//!   accusé après `sync_data` : `record` ne rend la main qu'une fois la ligne durable, ce
//!   qu'exige le mode `strict` (D-16). Une ligne remise au fil est écrite en entier même si le
//!   futur de `record` est abandonné ensuite (contrat du port).
//! - **Pour chaque ligne**, le fil prend le verrou consultatif `fs4` sur le fichier dédié
//!   `.mcp-legifrance.lock`, ouvre le journal en ajout, écrit d'un seul `write_all`, appelle
//!   `sync_data`, puis ferme le fichier et relâche le verrou. Aucun descripteur ne reste ouvert
//!   entre deux lignes : plusieurs processus peuvent écrire le même répertoire sans qu'aucune
//!   ligne soit coupée (V-164), et Windows peut renommer le journal.
//! - **Rotation sous le même verrou** : à 16 Mio ou au changement de date UTC, le journal actif
//!   est renommé en `mcp-legifrance-AAAA-MM-JJ[-n].jsonl`. Rien n'est supprimé.
//! - **Attentes bornées** : le verrou est tenté de façon non bloquante pendant 10 s au plus, puis
//!   l'attente de l'accusé est bornée à son tour ; au-delà, `record` rend
//!   `AuditErrorKind::WriterGone` au lieu de se figer.
//!
//! # Permissions
//!
//! Sous Unix, le répertoire est créé en `0700` et les fichiers en `0600`. Un répertoire ou un
//! fichier préexistant aux droits plus larges n'est pas modifié : `warn!` au premier `record`.
//! Un journal qui est un lien symbolique est refusé.
//!
//! **Windows** : aucune ACL n'est posée (`unsafe_code = "forbid"`, pas de dépendance de
//! sécurité). Les fichiers héritent des ACL du dossier de profil, a priori privées ; ce point
//! reste **non vérifié** (V-165, D22). Le test `windows_acl_report`, ignoré par défaut, relève
//! les ACL réelles avec `icacls` et les affiche.

mod file;
mod hash;
mod line;
mod rotation;

pub use file::JsonlAuditLog;

/// Enregistrements d'essai communs aux modules de test.
#[cfg(test)]
pub(crate) mod fixtures {
    use chrono::{DateTime, TimeZone, Utc};
    use lf_application::{
        input::CanonicalInput,
        ports::{AuditRecord, AuditStatus, PisteEnvironment},
        tools::ToolName,
    };

    /// Instant d'essai : `2026-10-15T15:09:12.345Z`.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "tests : valeurs bornées par les données du test"
    )]
    pub(crate) fn instant() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 15, 15, 9, 12).unwrap()
            + chrono::TimeDelta::milliseconds(345)
    }

    /// Un appel réussi de `fr_search`, à `instant()`.
    pub(crate) fn record() -> AuditRecord {
        record_at(instant())
    }

    /// Le même appel, daté de `ts`.
    pub(crate) fn record_at(ts: DateTime<Utc>) -> AuditRecord {
        AuditRecord {
            ts,
            tool: ToolName::FR_SEARCH,
            input: CanonicalInput::new(r#"{"query":"bail"}"#),
            output_count_or_size: 0,
            duration_ms: 3,
            status: AuditStatus::Ok,
            error_code: None,
            error: None,
            env: PisteEnvironment::Sandbox,
            server_version: "0.1.0",
        }
    }
}
