//! Synchronisation d'un répertoire d'archives avec le magasin (T-M9-02, rapport 10 §10.2).
//!
//! [`sync_directory`] lit le répertoire où le script de téléchargement dépose les dumps, consulte
//! le registre ([`crate::registry`]), planifie, puis ingère les archives une à une :
//!
//! - un **global** vide d'abord le magasin. Le registre est écrit `building` **avant** ce
//!   vidage et `applied` après l'ingestion : un arrêt à n'importe quel moment laisse un registre
//!   qui demande la reconstruction, jamais un magasin à moitié rempli déclaré appliqué ;
//! - un **incrément** s'applique par-dessus le magasin et n'entre au registre qu'une fois
//!   ingéré en entier. Rejoué après un arrêt, il donne le même état (idempotence).
//!
//! La première erreur arrête la synchronisation et remonte : les archives suivantes ne sont pas
//! essayées (l'ordre compte). Le registre garde les archives déjà appliquées.
//!
//! La fonction est synchrone et bloquante : V3-08 l'appelle depuis `spawn_blocking`.

use std::{fs, path::Path};

use crate::{
    archive::{ArchiveKind, ArchiveName},
    error::IngestError,
    ingest::{IngestLimits, IngestReport, ingest_archive},
    registry::{RebuildReason, Registry},
    store::DocStore,
};

/// Une archive ingérée par la synchronisation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedArchive {
    /// Nom de l'archive.
    pub name: ArchiveName,
    /// Bilan de son ingestion.
    pub report: IngestReport,
}

/// Bilan d'une synchronisation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Raison de la reconstruction, si le magasin a été refait depuis un global.
    pub rebuild: Option<RebuildReason>,
    /// Archives ingérées, dans l'ordre.
    pub applied: Vec<AppliedArchive>,
}

impl SyncReport {
    /// Vrai si la synchronisation n'a rien eu à faire.
    pub fn is_up_to_date(&self) -> bool {
        self.applied.is_empty()
    }
}

/// Les archives LEGI du répertoire `dir` ; tout autre fichier (autre fonds, nom non UTF-8,
/// sous-répertoire) est laissé de côté.
fn list_archives(dir: &Path) -> Result<Vec<ArchiveName>, IngestError> {
    let entries = fs::read_dir(dir)
        .map_err(|source| IngestError::Io { action: "list the archive directory", source })?;
    // une entrée illisible ne peut pas être une archive utilisable : elle est laissée de côté
    let mut found: Vec<ArchiveName> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter_map(|name| ArchiveName::parse(&name))
        .collect();
    found.sort();
    Ok(found)
}

/// Applique au magasin les archives du répertoire `dir` qui ne l'ont pas encore été.
///
/// `registry_path` est le fichier du registre (créé au premier passage). Rend la liste de ce qui
/// a été ingéré ; un passage sans rien de nouveau rend un [`SyncReport`] vide.
///
/// # Errors
///
/// [`IngestError::NoGlobal`] si le magasin est à bâtir et que le répertoire n'a pas de global ;
/// [`IngestError::MissingArchive`] si le global à rejouer a disparu ; [`IngestError::Io`] pour
/// le répertoire, le registre ou une archive illisibles ; [`IngestError::Archive`] et
/// [`IngestError::Store`] depuis l'ingestion. Le registre reflète alors ce qui est appliqué.
pub fn sync_directory<S: DocStore>(
    dir: &Path,
    registry_path: &Path,
    store: &mut S,
    limits: &IngestLimits,
) -> Result<SyncReport, IngestError> {
    let available = list_archives(dir)?;
    let mut registry = Registry::load(registry_path)?;
    let plan = registry.plan(&available)?;
    let mut report = SyncReport { rebuild: plan.rebuild, applied: Vec::new() };
    for name in plan.steps {
        let file = fs::File::open(dir.join(name.file_name()))
            .map_err(|source| IngestError::Io { action: "open an archive", source })?;
        if name.kind() == ArchiveKind::Global {
            registry.begin_global(name.clone());
            registry.save(registry_path)?;
            store.reset()?;
        }
        let ingested = ingest_archive(file, store, limits)?;
        // Redondant avec le commit de l'ingestion, sauf pour une archive vide (qui rend la main
        // avant) : le registre ne déclare jamais appliquée une archive que le magasin n'a pas
        // rendue durable.
        store.commit()?;
        if name.kind() == ArchiveKind::Global {
            registry.finish_global();
        } else {
            registry.record_increment(name.clone());
        }
        registry.save(registry_path)?;
        report.applied.push(AppliedArchive { name, report: ingested });
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::*;
    use crate::{
        registry::ArchiveStatus,
        store::MemoryStore,
        testing::{
            TestStore, article_member, article_xml, fixture, global_archive, increment_archive,
        },
    };

    const G1: &str = "Freemium_legi_global_20250713-140000.tar.gz";
    const G2: &str = "Freemium_legi_global_20261001-140000.tar.gz";
    const I1: &str = "LEGI_20250714-100000.tar.gz";
    const I2: &str = "LEGI_20250715-100000.tar.gz";
    const I3: &str = "LEGI_20250716-100000.tar.gz";
    const I_AFTER_G2: &str = "LEGI_20261002-212713.tar.gz";

    const CID: &str = "LEGITEXT000000000001";
    const SUB: &str = "code_en_vigueur";

    fn id(n: u32) -> String {
        format!("LEGIARTI{n:012}")
    }

    fn member(n: u32) -> String {
        article_member(SUB, CID, &id(n))
    }

    fn key(n: u32) -> String {
        member(n).trim_end_matches(".xml").to_owned()
    }

    fn doc(n: u32, html: &str) -> Vec<u8> {
        article_xml(CID, &id(n), "1", html).into_bytes()
    }

    /// Global synthétique : `(numéro, contenu)` pour chaque article.
    fn global(articles: &[(u32, &str)]) -> Vec<u8> {
        let members: Vec<(String, Vec<u8>)> =
            articles.iter().map(|(n, html)| (member(*n), doc(*n, html))).collect();
        let files: Vec<(&str, &[u8])> =
            members.iter().map(|(p, d)| (p.as_str(), d.as_slice())).collect();
        global_archive(&files)
    }

    /// Incrément synthétique : suppressions de fichiers (numéros), puis articles.
    fn increment(stamp: &str, deleted: &[u32], articles: &[(u32, &str)]) -> Vec<u8> {
        let keys: Vec<String> = deleted.iter().map(|n| key(*n)).collect();
        let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
        let members: Vec<(String, Vec<u8>)> =
            articles.iter().map(|(n, html)| (member(*n), doc(*n, html))).collect();
        let files: Vec<(&str, &[u8])> =
            members.iter().map(|(p, d)| (p.as_str(), d.as_slice())).collect();
        increment_archive(stamp, &keys, &[], &files)
    }

    struct Setup {
        dir: TempDir,
        registry: PathBuf,
    }

    impl Setup {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let registry = dir.path().join("registry.tsv");
            Self { dir, registry }
        }

        fn put(&self, file_name: &str, bytes: &[u8]) {
            std::fs::write(self.dir.path().join(file_name), bytes).unwrap();
        }

        fn remove(&self, file_name: &str) {
            std::fs::remove_file(self.dir.path().join(file_name)).unwrap();
        }

        fn sync(&self, store: &mut impl DocStore) -> Result<SyncReport, IngestError> {
            sync_directory(self.dir.path(), &self.registry, store, &IngestLimits::default())
        }

        fn registry(&self) -> Registry {
            Registry::load(&self.registry).unwrap()
        }
    }

    fn content(store: &MemoryStore, n: u32) -> Option<String> {
        store.get(&key(n)).map(|doc| doc.document.as_article().unwrap().content.clone())
    }

    fn applied(report: &SyncReport) -> Vec<&str> {
        report.applied.iter().map(|a| a.name.file_name()).collect()
    }

    fn name(file_name: &str) -> ArchiveName {
        ArchiveName::parse(file_name).unwrap()
    }

    #[test]
    fn rebuild_on_new_global() {
        let setup = Setup::new();
        setup.put(G1, &global(&[(1, "g1"), (2, "g1")]));
        setup.put(I1, &increment("20250714-100000", &[], &[(1, "i1")]));
        let mut store = TestStore::new();

        let first = setup.sync(&mut store).unwrap();
        assert_eq!(first.rebuild, Some(RebuildReason::FirstBuild));
        assert_eq!(applied(&first), [G1, I1]);
        assert_eq!(content(&store, 1).as_deref(), Some("i1"));
        assert_eq!(content(&store, 2).as_deref(), Some("g1"));

        // la DILA publie un nouveau global : l'article 2 en a disparu, le 3 est nouveau
        setup.put(G2, &global(&[(1, "g2"), (3, "g2")]));
        setup.put(I_AFTER_G2, &increment("20261002-212713", &[], &[(4, "i4")]));
        let second = setup.sync(&mut store).unwrap();
        assert_eq!(second.rebuild, Some(RebuildReason::NewGlobal { previous: name(G1) }));
        assert_eq!(applied(&second), [G2, I_AFTER_G2], "I1 predates the new global");
        assert_eq!(content(&store, 1).as_deref(), Some("g2"), "the old increment is not replayed");
        assert_eq!(content(&store, 2), None, "the old corpus was emptied first");
        assert_eq!(content(&store, 3).as_deref(), Some("g2"));
        assert_eq!(content(&store, 4).as_deref(), Some("i4"));
        assert_eq!(store.len(), 3);

        let registry = setup.registry();
        assert_eq!(registry.global().unwrap().0.file_name(), G2);
        assert_eq!(registry.last_increment().unwrap().file_name(), I_AFTER_G2);
        assert_eq!(registry.increment_count(), 1, "the increments of the old global are forgotten");
        assert!(setup.sync(&mut store).unwrap().is_up_to_date());
    }

    #[test]
    fn a_pass_without_news_does_nothing_and_a_new_increment_is_applied_alone() {
        let setup = Setup::new();
        setup.put(G1, &global(&[(1, "g1"), (2, "g1")]));
        let mut store = TestStore::new();
        assert_eq!(applied(&setup.sync(&mut store).unwrap()), [G1]);
        let idle = setup.sync(&mut store).unwrap();
        assert!(idle.is_up_to_date());
        assert_eq!(idle, SyncReport::default());

        setup.put(I1, &increment("20250714-100000", &[2], &[(3, "i1")]));
        let report = setup.sync(&mut store).unwrap();
        assert_eq!(report.rebuild, None, "an increment never rebuilds");
        assert_eq!(applied(&report), [I1]);
        assert_eq!(report.applied[0].report.deleted_files_removed, 1);
        assert_eq!(content(&store, 1).as_deref(), Some("g1"));
        assert_eq!(content(&store, 2), None);
        assert_eq!(content(&store, 3).as_deref(), Some("i1"));
    }

    #[test]
    fn the_global_may_leave_the_directory_once_applied() {
        let setup = Setup::new();
        setup.put(G1, &global(&[(1, "g1")]));
        let mut store = TestStore::new();
        setup.sync(&mut store).unwrap();
        setup.remove(G1);
        setup.put(I1, &increment("20250714-100000", &[], &[(2, "i1")]));
        let report = setup.sync(&mut store).unwrap();
        assert_eq!(applied(&report), [I1]);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn a_late_increment_replays_everything_in_order() {
        let setup = Setup::new();
        setup.put(G1, &global(&[(1, "g1")]));
        setup.put(I1, &increment("20250714-100000", &[], &[(2, "i1")]));
        // I3 recrée l'article 1 que I2, arrivé après coup, supprime
        setup.put(I3, &increment("20250716-100000", &[], &[(1, "i3")]));
        let mut store = TestStore::new();
        assert_eq!(applied(&setup.sync(&mut store).unwrap()), [G1, I1, I3]);
        assert_eq!(content(&store, 1).as_deref(), Some("i3"));

        setup.put(I2, &increment("20250715-100000", &[1], &[]));
        let report = setup.sync(&mut store).unwrap();
        assert_eq!(report.rebuild, Some(RebuildReason::LateIncrement { late: name(I2) }));
        assert_eq!(applied(&report), [G1, I1, I2, I3]);
        // appliqué à la suite, I2 aurait effacé l'article 1 ; dans l'ordre, I3 le recrée
        assert_eq!(content(&store, 1).as_deref(), Some("i3"));
        assert_eq!(content(&store, 2).as_deref(), Some("i1"));
        assert_eq!(setup.registry().increment_count(), 3);
    }

    #[test]
    fn the_real_mini_archives_are_synchronised_from_their_dila_names() {
        let setup = Setup::new();
        setup.put(
            "Freemium_legi_global_20250713-140000.tar.gz",
            &fixture("archives/legi-global-mini.tar.gz"),
        );
        setup.put(
            "LEGI_20261028-212713.tar.gz",
            &fixture("archives/legi-incr-20261028-mini.tar.gz"),
        );
        setup.put("KALI_20261028-212713.tar.gz", b"another fund, never read");
        let mut store = TestStore::new();
        let report = setup.sync(&mut store).unwrap();
        assert_eq!(report.applied.len(), 2);
        assert_eq!(report.applied[0].report.documents(), 23);
        assert_eq!(report.applied[1].report.deleted_files_removed, 5);
        assert_eq!(store.len(), 23);
        assert_eq!(setup.registry().last_increment().unwrap().stamp().as_str(), "20261028-212713");
    }

    #[test]
    fn a_stop_between_the_registry_and_the_reset_is_recovered_by_a_rebuild() {
        let setup = Setup::new();
        setup.put(G1, &global(&[(1, "g1")]));
        setup.put(I1, &increment("20250714-100000", &[], &[(2, "i1")]));
        let mut store = TestStore::failing("reset");

        let error = setup.sync(&mut store).unwrap_err();
        assert_eq!(error.to_string(), "document store failure: reset refused");
        let registry = setup.registry();
        assert_eq!(
            registry.global().map(|(n, s)| (n.file_name().to_owned(), s)),
            Some((G1.to_owned(), ArchiveStatus::Building)),
            "the global is declared building before the store is touched"
        );

        store.fail_on = None;
        let report = setup.sync(&mut store).unwrap();
        assert_eq!(report.rebuild, Some(RebuildReason::InterruptedGlobal));
        assert_eq!(applied(&report), [G1, I1]);
        assert_eq!(store.inner.len(), 2);
        assert_eq!(setup.registry().global().unwrap().1, ArchiveStatus::Applied);
    }

    #[test]
    fn an_archive_is_applied_only_once_the_store_has_made_it_durable() {
        let setup = Setup::new();
        setup.put(G1, &global(&[(1, "g1")]));
        let mut store = TestStore::failing("commit");
        let error = setup.sync(&mut store).unwrap_err();
        assert_eq!(error.to_string(), "document store failure: commit refused");
        assert_eq!(
            setup.registry().global().unwrap().1,
            ArchiveStatus::Building,
            "the registry never says applied before the store is durable"
        );

        store.fail_on = None;
        let report = setup.sync(&mut store).unwrap();
        assert_eq!(report.rebuild, Some(RebuildReason::InterruptedGlobal));
        assert_eq!(store.commits, 2, "one by the ingestion, one by the synchronisation");

        // le commit de l'ingestion passe, celui de la synchronisation est refusé
        let late = Setup::new();
        late.put(G1, &global(&[(1, "g1")]));
        let mut store = TestStore::new();
        store.refuse_commit_after = Some(1);
        let error = late.sync(&mut store).unwrap_err();
        assert_eq!(error.to_string(), "document store failure: commit refused");
        assert_eq!(late.registry().global().unwrap().1, ArchiveStatus::Building);

        // une archive vide rend la main avant le commit de l'ingestion : celui de la
        // synchronisation la couvre
        let empty = Setup::new();
        empty.put(G1, &[]);
        let mut store = TestStore::new();
        empty.sync(&mut store).unwrap();
        assert_eq!(store.commits, 1);
    }

    #[test]
    fn a_registry_that_cannot_be_written_after_an_ingestion_is_an_io_error() {
        let setup = Setup::new();
        setup.put(G1, &global(&[(1, "g1")]));
        let mut store = TestStore::new();
        setup.sync(&mut store).unwrap();

        // le fichier voisin dans lequel `save` écrit est occupé par un dossier
        std::fs::create_dir(setup.dir.path().join("registry.tsv.tmp")).unwrap();
        setup.put(I1, &increment("20250714-100000", &[], &[(2, "i1")]));
        let error = setup.sync(&mut store).unwrap_err();
        assert!(error.to_string().starts_with("cannot write the archive registry: "), "{error}");
        assert_eq!(
            content(&store, 2).as_deref(),
            Some("i1"),
            "ingested before the registry failed"
        );
        assert!(!setup.registry().has_applied(&name(I1)), "but not declared applied");

        // le prochain passage rejoue l'incrément, sans effet de bord (idempotence)
        std::fs::remove_dir(setup.dir.path().join("registry.tsv.tmp")).unwrap();
        let report = setup.sync(&mut store).unwrap();
        assert_eq!(applied(&report), [I1]);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn a_corrupt_global_stays_building_until_a_good_copy_arrives() {
        let setup = Setup::new();
        let good = global(&[(1, "g1")]);
        setup.put(G1, good.get(..good.len() / 2).unwrap());
        let mut store = TestStore::new();
        let error = setup.sync(&mut store).unwrap_err();
        assert!(error.to_string().starts_with("archive unreadable after "), "{error}");
        assert_eq!(setup.registry().global().unwrap().1, ArchiveStatus::Building);

        setup.put(G1, &good);
        let report = setup.sync(&mut store).unwrap();
        assert_eq!(report.rebuild, Some(RebuildReason::InterruptedGlobal));
        assert_eq!(content(&store, 1).as_deref(), Some("g1"));
        assert!(setup.sync(&mut store).unwrap().is_up_to_date());
    }

    #[test]
    fn a_corrupt_increment_stops_the_pass_and_is_retried_alone() {
        let setup = Setup::new();
        setup.put(G1, &global(&[(1, "g1")]));
        setup.put(I1, &increment("20250714-100000", &[], &[(2, "i1")]));
        setup.put(I2, b"truncated download");
        setup.put(I3, &increment("20250716-100000", &[], &[(3, "i3")]));
        let mut store = TestStore::new();

        let error = setup.sync(&mut store).unwrap_err();
        assert!(error.to_string().starts_with("archive unreadable after 0 entries: "), "{error}");
        let registry = setup.registry();
        assert!(registry.has_applied(&name(I1)));
        assert!(!registry.has_applied(&name(I2)));
        assert!(!registry.has_applied(&name(I3)), "the order matters: nothing after the failure");
        assert_eq!(content(&store, 3), None);

        setup.put(I2, &increment("20250715-100000", &[], &[(4, "i2")]));
        let report = setup.sync(&mut store).unwrap();
        assert_eq!(report.rebuild, None, "I2 is the next in line, not late");
        assert_eq!(applied(&report), [I2, I3]);
        assert_eq!(store.len(), 4);
    }

    #[test]
    fn files_that_are_not_legi_archives_are_left_alone() {
        let setup = Setup::new();
        setup.put(G1, &global(&[(1, "g1")]));
        setup.put("notes.txt", b"hello");
        setup.put("KALI_20261028-212713.tar.gz", b"another fund");
        setup.put("LEGI_20261028.tar.gz", b"bad stamp");
        // un répertoire qui porte un nom d'archive n'est pas une archive
        std::fs::create_dir(setup.dir.path().join(I1)).unwrap();
        let mut store = TestStore::new();
        let report = setup.sync(&mut store).unwrap();
        assert_eq!(applied(&report), [G1]);
    }

    // APFS refuse de créer un fichier dont le nom n'est pas de l'UTF-8 (EILSEQ, « Illegal byte
    // sequence ») : le cas ne peut pas se construire sous macOS.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn a_file_name_that_is_not_utf8_is_left_alone() {
        use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

        let setup = Setup::new();
        setup.put(G1, &global(&[(1, "g1")]));
        let odd = setup.dir.path().join(OsStr::from_bytes(b"LEGI_\xff.tar.gz"));
        std::fs::write(odd, b"x").unwrap();
        let mut store = TestStore::new();
        assert_eq!(applied(&setup.sync(&mut store).unwrap()), [G1]);
    }

    #[test]
    fn without_any_global_nothing_is_built() {
        let setup = Setup::new();
        setup.put(I1, &increment("20250714-100000", &[], &[(2, "i1")]));
        let mut store = TestStore::new();
        let error = setup.sync(&mut store).unwrap_err();
        assert_eq!(error.to_string(), "no global archive is available to build the corpus from");
        assert!(!setup.registry.exists(), "no registry is written for a pass that did nothing");
        assert!(store.is_empty());
    }

    #[test]
    fn directory_and_registry_problems_are_io_errors() {
        let setup = Setup::new();
        let mut store = TestStore::new();
        let missing = setup.dir.path().join("absent");
        let error = sync_directory(&missing, &setup.registry, &mut store, &IngestLimits::default())
            .unwrap_err();
        assert!(error.to_string().starts_with("cannot list the archive directory: "), "{error}");

        setup.put(G1, &global(&[(1, "g1")]));
        std::fs::write(&setup.registry, "applied\tnope\n").unwrap();
        let error = setup.sync(&mut store).unwrap_err();
        assert_eq!(error.to_string(), "archive registry line 1 is invalid");

        std::fs::remove_file(&setup.registry).unwrap();
        let blocked = Setup {
            dir: tempfile::tempdir().unwrap(),
            registry: PathBuf::from("/nonexistent/registry.tsv"),
        };
        blocked.put(G1, &global(&[(1, "g1")]));
        let error = blocked.sync(&mut store).unwrap_err();
        assert!(error.to_string().starts_with("cannot write the archive registry: "), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn an_archive_that_cannot_be_opened_is_an_io_error() {
        use std::os::unix::fs::PermissionsExt;

        let setup = Setup::new();
        setup.put(G1, &global(&[(1, "g1")]));
        let path = setup.dir.path().join(G1);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        // un administrateur ouvre tout : alors la synchronisation réussit et le test le constate
        let denied = std::fs::File::open(&path).is_err();
        let outcome = setup.sync(&mut TestStore::new());
        let message = outcome.err().map(|error| error.to_string());
        let expected = "cannot open an archive: Permission denied (os error 13)";
        assert_eq!(message.as_deref(), denied.then_some(expected));
        assert_eq!(
            setup.registry.exists(),
            !denied,
            "nothing is declared before the archive opens"
        );
    }
}
