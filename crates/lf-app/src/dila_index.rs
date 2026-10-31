//! `build-dila-index` (M9, feature `dila`) : bâtit ou met à jour l'index local DILA depuis des
//! archives LEGI déjà téléchargées.
//!
//! - **Aucun accès réseau** : le binaire ne contacte pas `echanges.dila.gouv.fr`. Les archives
//!   viennent d'un répertoire que l'utilisateur a rempli (script `scripts/fetch-dila-dumps.sh`,
//!   hors binaire) ; la commande les lit en flux, sans rien extraire sur le disque.
//! - **Reprise** : le registre des archives appliquées vit dans `<index>/registry.tsv`. Rejouer
//!   la commande n'applique que ce qui est nouveau ; un nouveau global refait l'index.
//! - **Un seul écrivain** : un autre processus qui tient le verrou de l'index fait échouer la
//!   commande (code 1), jamais corrompre l'index.
//!
//! Codes de sortie : 0 (index à jour), 1 (ingestion en échec), 2 (usage : répertoire d'archives
//! absent, ou aucun répertoire d'index).

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::Context as _;
use lf_adapter_dila::{ArchiveKind, ArchiveName, DilaCorpus, IngestLimits, SyncReport};

use crate::{cli::BuildDilaIndexArgs, config::CorpusSource};

/// Nom du registre des archives appliquées, dans le répertoire de l'index.
const REGISTRY_FILE: &str = "registry.tsv";

/// Code de sortie d'une erreur d'usage (02 §8.1).
const EXIT_USAGE: u8 = 2;

/// Exécute la commande et écrit son bilan sur `out`.
///
/// `source` est la source lue dans `LF_DILA_DIR` : elle donne le répertoire de l'index quand
/// `--index` est absent, pour que l'index bâti soit celui que `serve` ouvrira.
pub(crate) async fn run(
    source: &CorpusSource,
    args: &BuildDilaIndexArgs,
    out: &mut impl Write,
) -> anyhow::Result<ExitCode> {
    let Some(index) = index_dir(source, args) else {
        tracing::error!("no index directory: pass --index, or set LF_DILA_DIR to an absolute path");
        return Ok(ExitCode::from(EXIT_USAGE));
    };
    // `LF_DILA_DIR` doit être absolu : le chemin affiché à la fin l'est donc aussi.
    let index = std::path::absolute(&index).context("cannot resolve the index directory")?;
    if !args.archives.is_dir() {
        tracing::error!("--archives does not name an existing directory");
        return Ok(ExitCode::from(EXIT_USAGE));
    }
    // Ouvrir l'index le crée : une faute de frappe dans `--index` ne doit pas laisser un dossier
    // vide quand il n'y a de toute façon rien à bâtir (aucun global dans les archives).
    if !index.exists() && !has_global(&args.archives) {
        tracing::error!("--archives holds no LEGI global archive: the index cannot be built");
        return Ok(ExitCode::from(EXIT_USAGE));
    }
    let corpus = DilaCorpus::open(&index).context("cannot open the local DILA index")?;
    let report = corpus
        .sync(args.archives.clone(), index.join(REGISTRY_FILE), IngestLimits::default())
        .await
        .context("cannot apply the DILA archives")?;
    summary(&index, &report, corpus.engine().len(), out)
        .context("cannot write the build-dila-index report")?;
    Ok(ExitCode::SUCCESS)
}

/// Vrai si `dir` contient une archive globale LEGI (le nom suffit : le contenu est lu à l'ingestion).
fn has_global(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter_map(|name| ArchiveName::parse(&name))
            .any(|name| name.kind() == ArchiveKind::Global)
    })
}

/// Répertoire de l'index : `--index`, sinon `LF_DILA_DIR` s'il est utilisable.
fn index_dir(source: &CorpusSource, args: &BuildDilaIndexArgs) -> Option<PathBuf> {
    match (&args.index, source) {
        (Some(dir), _) | (None, CorpusSource::Dila(dir)) => Some(dir.clone()),
        (None, CorpusSource::Piste | CorpusSource::Unusable(_)) => None,
    }
}

/// Le bilan : de quoi savoir si l'index est à jour, et quelle valeur donner à `LF_DILA_DIR`.
fn summary(
    index: &Path,
    report: &SyncReport,
    documents: u64,
    out: &mut impl Write,
) -> std::io::Result<()> {
    if report.is_up_to_date() {
        writeln!(out, "The index is up to date: no new archive to apply.")?;
    } else {
        if report.rebuild.is_some() {
            writeln!(out, "The index was rebuilt from a new global archive.")?;
        }
        for applied in &report.applied {
            writeln!(
                out,
                "Applied {}: {} documents, {} rejected members.",
                applied.name.file_name(),
                applied.report.documents(),
                applied.report.rejected,
            )?;
        }
    }
    writeln!(out, "The index holds {documents} documents.")?;
    writeln!(out, "Use it with: LF_DILA_DIR={}", index.display())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::config::DilaDirProblem;

    /// Mini-archives réelles de `lf-adapter-dila` (extraits LEGI), sous les noms de la DILA.
    const FIXTURES: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../lf-adapter-dila/tests/fixtures/archives");
    const GLOBAL: (&str, &str) =
        ("legi-global-mini.tar.gz", "Freemium_legi_global_20250713-140000.tar.gz");
    const INCREMENT: (&str, &str) =
        ("legi-incr-20261028-mini.tar.gz", "LEGI_20261028-212713.tar.gz");

    fn archives(names: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (fixture, dila_name) in names {
            fs::copy(Path::new(FIXTURES).join(fixture), dir.path().join(dila_name)).unwrap();
        }
        dir
    }

    fn args(archives: &TempDir, index: Option<&TempDir>) -> BuildDilaIndexArgs {
        BuildDilaIndexArgs {
            archives: archives.path().to_path_buf(),
            index: index.map(|dir| dir.path().to_path_buf()),
        }
    }

    async fn build(source: &CorpusSource, args: &BuildDilaIndexArgs) -> (ExitCode, String) {
        let mut out = Vec::new();
        let code = run(source, args, &mut out).await.unwrap();
        (code, String::from_utf8(out).unwrap())
    }

    fn code(value: u8) -> String {
        format!("{:?}", ExitCode::from(value))
    }

    #[tokio::test]
    async fn a_global_then_an_increment_build_the_index_and_a_rerun_is_up_to_date() {
        let index = TempDir::new().unwrap();
        let source = CorpusSource::Piste;

        let first = archives(&[GLOBAL, INCREMENT]);
        let (exit, text) = build(&source, &args(&first, Some(&index))).await;
        assert_eq!(format!("{exit:?}"), format!("{:?}", ExitCode::SUCCESS), "{text}");
        assert!(text.contains(&format!("Applied {}", GLOBAL.1)), "{text}");
        assert!(text.contains(&format!("Applied {}", INCREMENT.1)), "{text}");
        assert!(text.contains("was rebuilt from a new global archive"), "{text}");
        assert!(text.contains("The index holds 23 documents."), "{text}");
        assert!(text.contains(&format!("LF_DILA_DIR={}", index.path().display())), "{text}");
        assert!(index.path().join(REGISTRY_FILE).is_file());

        let (exit, text) = build(&source, &args(&first, Some(&index))).await;
        assert_eq!(format!("{exit:?}"), format!("{:?}", ExitCode::SUCCESS), "{text}");
        assert!(text.contains("up to date"), "{text}");
        assert!(!text.contains("Applied"), "{text}");
    }

    #[tokio::test]
    async fn the_index_directory_defaults_to_the_one_of_lf_dila_dir() {
        let index = TempDir::new().unwrap();
        let source = CorpusSource::Dila(index.path().to_path_buf());
        let from = archives(&[GLOBAL]);
        let (exit, text) = build(&source, &args(&from, None)).await;
        assert_eq!(format!("{exit:?}"), format!("{:?}", ExitCode::SUCCESS), "{text}");
        assert!(index.path().join(REGISTRY_FILE).is_file());
    }

    #[tokio::test]
    async fn no_index_directory_is_a_usage_error() {
        let from = archives(&[GLOBAL]);
        for source in [CorpusSource::Piste, CorpusSource::Unusable(DilaDirProblem::NotAbsolute)] {
            let (exit, text) = build(&source, &args(&from, None)).await;
            assert_eq!(format!("{exit:?}"), code(EXIT_USAGE));
            assert!(text.is_empty(), "{text}");
        }
    }

    #[tokio::test]
    async fn a_missing_archive_directory_is_a_usage_error() {
        let index = TempDir::new().unwrap();
        let missing = BuildDilaIndexArgs {
            archives: index.path().join("absent"),
            index: Some(index.path().to_path_buf()),
        };
        let (exit, _) = build(&CorpusSource::Piste, &missing).await;
        assert_eq!(format!("{exit:?}"), code(EXIT_USAGE));
    }

    #[tokio::test]
    async fn archives_without_a_global_are_a_usage_error_and_create_no_index() {
        let parent = TempDir::new().unwrap();
        let index = parent.path().join("typo");
        let from = archives(&[INCREMENT]);
        let wanted =
            BuildDilaIndexArgs { archives: from.path().to_path_buf(), index: Some(index.clone()) };
        let (exit, text) = build(&CorpusSource::Piste, &wanted).await;
        assert_eq!(format!("{exit:?}"), code(EXIT_USAGE));
        assert!(text.is_empty(), "{text}");
        assert!(!index.exists(), "un index vide a été créé");
    }

    #[tokio::test]
    async fn an_increment_without_a_global_fails_when_the_index_is_new_to_the_registry() {
        let index = TempDir::new().unwrap();
        let from = archives(&[INCREMENT]);
        let mut out = Vec::new();
        let error =
            run(&CorpusSource::Piste, &args(&from, Some(&index)), &mut out).await.unwrap_err();
        assert!(format!("{error:#}").contains("cannot apply the DILA archives"), "{error:#}");
        assert_eq!(out, Vec::<u8>::new());
    }

    #[tokio::test]
    async fn an_unusable_index_path_fails_with_an_error() {
        let file = TempDir::new().unwrap();
        let blocker = file.path().join("blocker");
        fs::write(&blocker, b"not a directory").unwrap();
        let from = archives(&[GLOBAL]);
        let broken = BuildDilaIndexArgs {
            archives: from.path().to_path_buf(),
            index: Some(blocker.join("index")),
        };
        let error = run(&CorpusSource::Piste, &broken, &mut Vec::new()).await.unwrap_err();
        assert!(format!("{error:#}").contains("cannot open the local DILA index"), "{error:#}");
    }

    #[test]
    fn a_failing_output_is_reported_not_swallowed() {
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("closed"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        assert!(summary(Path::new("/index"), &SyncReport::default(), 0, &mut Closed).is_err());
    }
}
