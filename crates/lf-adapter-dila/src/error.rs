//! Erreurs de l'ingestion des dumps DILA.
//!
//! Aucune erreur ne porte le contenu d'un membre d'archive : seuls des motifs typés, des
//! compteurs et des noms d'archives passent. Les erreurs d'entrée-sortie gardent leur cause
//! (`source`) pour l'opérateur ; elles ne citent pas de chemin du disque.

use std::io;

use thiserror::Error;

/// Échec du magasin de documents (écriture de l'index, par exemple).
///
/// L'adapter d'index (V3-08) enveloppe ses propres erreurs dans `detail`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("document store failure: {detail}")]
pub struct StoreError {
    /// Description de l'échec, pour les journaux de l'opérateur.
    pub detail: String,
}

impl StoreError {
    /// Construit une erreur de magasin.
    pub fn new(detail: impl Into<String>) -> Self {
        Self { detail: detail.into() }
    }
}

/// Échec de l'ingestion d'une archive, de la lecture du registre ou de la planification.
#[derive(Debug, Error)]
pub enum IngestError {
    /// L'archive est illisible (gzip ou tar corrompu, lecture interrompue).
    #[error("archive unreadable after {entries} entries: {source}")]
    Archive {
        /// Nombre de membres déjà lus quand l'erreur est survenue.
        entries: u64,
        /// Cause d'entrée-sortie (la couche gzip et la couche tar rendent des `io::Error`).
        #[source]
        source: io::Error,
    },
    /// Le magasin de documents a refusé une opération.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// Une opération sur le système de fichiers a échoué (répertoire, registre, archive).
    #[error("cannot {action}: {source}")]
    Io {
        /// Ce qui était tenté, en anglais court (`read the archive directory`, par exemple).
        action: &'static str,
        /// Cause d'entrée-sortie.
        #[source]
        source: io::Error,
    },
    /// Une ligne du registre des archives est illisible.
    #[error("archive registry line {line} is invalid")]
    Registry {
        /// Numéro de la ligne (à partir de 1).
        line: usize,
    },
    /// Ni le registre ni le répertoire ne portent de global : on ne bâtit pas un corpus à partir
    /// des seuls incréments.
    #[error("no global archive is available to build the corpus from")]
    NoGlobal,
    /// Une archive nécessaire au plan (le global à rejouer) n'est plus dans le répertoire.
    #[error("archive {name} is needed but missing from the archive directory")]
    MissingArchive {
        /// Nom de fichier de l'archive absente.
        name: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_error_keeps_its_detail() {
        let error = StoreError::new("disk full");
        assert_eq!(error.detail, "disk full");
        assert_eq!(error.to_string(), "document store failure: disk full");
        assert_eq!(IngestError::from(error).to_string(), "document store failure: disk full");
    }

    #[test]
    fn messages_name_the_cause_but_never_a_path() {
        let io_error = || io::Error::other("boom");
        let archive = IngestError::Archive { entries: 3, source: io_error() };
        assert_eq!(archive.to_string(), "archive unreadable after 3 entries: boom");
        let fs = IngestError::Io { action: "read the registry", source: io_error() };
        assert_eq!(fs.to_string(), "cannot read the registry: boom");
        assert_eq!(
            IngestError::Registry { line: 7 }.to_string(),
            "archive registry line 7 is invalid"
        );
        assert_eq!(
            IngestError::NoGlobal.to_string(),
            "no global archive is available to build the corpus from"
        );
        let missing = IngestError::MissingArchive { name: "LEGI_20261028-212713.tar.gz".into() };
        assert_eq!(
            missing.to_string(),
            "archive LEGI_20261028-212713.tar.gz is needed but missing from the archive directory"
        );
    }
}
