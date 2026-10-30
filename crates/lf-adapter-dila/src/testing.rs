//! Aides de test communes (compilées pour les tests seulement) : lecture des fixtures de V1-16,
//! constructeur d'archives `tar.gz` en mémoire et générateur d'XML d'articles synthétiques.

use std::{
    cell::Cell,
    io::{self, Cursor, Read, Write},
    ops::Deref,
    path::PathBuf,
    rc::Rc,
};

use flate2::{Compression, write::GzEncoder};
use tar::{EntryType, Header};

use crate::{
    error::StoreError,
    index::Engine,
    model::Document,
    path::{DocKind, doc_for_key},
    store::{DocStore, MemoryStore, StoredDoc},
};

/// Magasin de test : un [`MemoryStore`] qui peut refuser une opération et noter où en est la
/// lecture de l'archive quand le premier document arrive.
pub(crate) struct TestStore {
    pub(crate) inner: MemoryStore,
    /// Opération refusée (`reset`, `put`, `remove_file` ou `remove_text_dir`).
    pub(crate) fail_on: Option<&'static str>,
    /// Compteur d'octets lus de l'archive, à relever au premier `put`.
    pub(crate) probe: Option<Rc<Cell<usize>>>,
    pub(crate) first_put_at: Option<usize>,
    /// Nombre d'appels à `commit` (un par archive ingérée).
    pub(crate) commits: usize,
    /// Refuse les `commit` une fois ce nombre atteint.
    pub(crate) refuse_commit_after: Option<usize>,
}

/// Lecteur de test : compte les octets lus et peut échouer dès la première lecture.
///
/// Tous les tests d'ingestion passent par ce seul type de lecteur et par [`TestStore`] : la
/// couverture d'une fonction générique se mesure sur son instanciation la mieux couverte, qui
/// doit donc parcourir tous les chemins, erreurs comprises.
pub(crate) struct TestReader {
    inner: Cursor<Vec<u8>>,
    pub(crate) consumed: Rc<Cell<usize>>,
    fail: bool,
}

impl TestReader {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self { inner: Cursor::new(bytes), consumed: Rc::new(Cell::new(0)), fail: false }
    }

    pub(crate) fn failing() -> Self {
        Self { fail: true, ..Self::new(Vec::new()) }
    }
}

impl Read for TestReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.fail {
            return Err(io::Error::other("disk gone"));
        }
        let read = self.inner.read(out).unwrap();
        self.consumed.set(self.consumed.get() + read);
        Ok(read)
    }
}

impl Deref for TestStore {
    type Target = MemoryStore;

    fn deref(&self) -> &MemoryStore {
        &self.inner
    }
}

impl TestStore {
    pub(crate) fn new() -> Self {
        Self {
            inner: MemoryStore::new(),
            fail_on: None,
            probe: None,
            first_put_at: None,
            commits: 0,
            refuse_commit_after: None,
        }
    }

    pub(crate) fn failing(operation: &'static str) -> Self {
        Self { fail_on: Some(operation), ..Self::new() }
    }

    fn check(&self, operation: &str) -> Result<(), StoreError> {
        match self.fail_on {
            Some(refused) if refused == operation => {
                Err(StoreError::new(format!("{operation} refused")))
            }
            _ => Ok(()),
        }
    }
}

impl DocStore for TestStore {
    fn reset(&mut self) -> Result<(), StoreError> {
        self.check("reset")?;
        self.inner.reset()
    }

    fn put(&mut self, doc: StoredDoc) -> Result<(), StoreError> {
        self.check("put")?;
        if let Some(probe) = &self.probe {
            self.first_put_at.get_or_insert(probe.get());
        }
        self.inner.put(doc)
    }

    fn remove_file(&mut self, key: &str) -> Result<bool, StoreError> {
        self.check("remove_file")?;
        self.inner.remove_file(key)
    }

    fn remove_text_dir(&mut self, text_dir: &str) -> Result<u64, StoreError> {
        self.check("remove_text_dir")?;
        self.inner.remove_text_dir(text_dir)
    }

    fn commit(&mut self) -> Result<(), StoreError> {
        self.check("commit")?;
        if self.refuse_commit_after == Some(self.commits) {
            return Err(StoreError::new("commit refused"));
        }
        self.commits += 1;
        self.inner.commit()
    }
}

/// Chemin d'une fixture, relatif à `tests/fixtures/`.
pub(crate) fn fixture_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(relative)
}

/// Contenu d'une fixture.
pub(crate) fn fixture(relative: &str) -> Vec<u8> {
    std::fs::read(fixture_path(relative)).unwrap()
}

/// Rejoue `run` en faisant échouer tour à tour chaque accès à l'index, du premier au dernier : une
/// panne doit toujours remonter en erreur, jamais être avalée. Rend le nombre d'accès (et donc de
/// pannes) de `run`, qui doit réussir quand aucune panne ne l'atteint.
pub(crate) fn every_fault<T, E>(engine: &Engine, mut run: impl FnMut() -> Result<T, E>) -> u64 {
    let mut faults = 0;
    loop {
        let access = faults + 1;
        engine.fail_nth_access(access);
        let result = run();
        if engine.fault_pending() {
            engine.fail_nth_access(0);
            assert!(result.is_ok(), "the run fails by itself");
            return faults;
        }
        assert!(result.is_err(), "the failure of access {access} was swallowed");
        faults += 1;
    }
}

/// Dossier d'un texte, `sub` valant `code_en_vigueur`, `TNC_non_vigueur`, etc.
pub(crate) fn text_dir(sub: &str, cid: &str) -> String {
    let group = if sub.ends_with("non_vigueur") {
        "code_et_TNC_non_vigueur"
    } else {
        "code_et_TNC_en_vigueur"
    };
    let source = if cid.starts_with("JORF") { "JORF" } else { "LEGI" };
    format!("legi/global/{group}/{sub}/{source}/TEXT/{}/{cid}", levels(cid))
}

/// Les cinq niveaux `NN/NN/NN/NN/NN` d'un identifiant (ses dix premiers chiffres, par paires).
fn levels(id: &str) -> String {
    let digits = &id[8..18];
    (0..5).map(|i| &digits[2 * i..2 * i + 2]).collect::<Vec<_>>().join("/")
}

/// Clé de stockage d'un document du texte `cid`, sous `sub` (`code_en_vigueur`, etc.).
pub(crate) fn doc_key(sub: &str, cid: &str, kind: DocKind, id: &str) -> String {
    let dir = text_dir(sub, cid);
    match kind {
        DocKind::Article => format!("{dir}/article/LEGI/ARTI/{}/{id}", levels(id)),
        DocKind::Section => format!("{dir}/section_ta/LEGI/SCTA/{}/{id}", levels(id)),
        DocKind::TextStruct => format!("{dir}/texte/struct/{id}"),
        DocKind::TextVersion => format!("{dir}/texte/version/{id}"),
    }
}

/// Le document rangé sous la clé que lui donne sa place dans le texte `cid`.
pub(crate) fn stored_doc(sub: &str, cid: &str, document: Document, mtime: u64) -> StoredDoc {
    let key = doc_key(sub, cid, document.kind(), document.id());
    StoredDoc { path: doc_for_key(&key).unwrap(), mtime, document }
}

/// Chemin d'un membre `article/…` (sans préfixe d'incrément).
pub(crate) fn article_member(sub: &str, cid: &str, id: &str) -> String {
    format!("{}/article/LEGI/ARTI/{}/{id}.xml", text_dir(sub, cid), levels(id))
}

/// `ARTICLE` minimal et valide, au contenu HTML donné.
pub(crate) fn article_xml(cid: &str, id: &str, num: &str, html: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ARTICLE><META><META_COMMUN><ID>{id}</ID><NATURE>Article</NATURE></META_COMMUN><META_SPEC><META_ARTICLE><NUM>{num}</NUM><ETAT>VIGUEUR</ETAT><DATE_DEBUT>2020-01-01</DATE_DEBUT><DATE_FIN>2999-01-01</DATE_FIN><TYPE>AUTONOME</TYPE></META_ARTICLE></META_SPEC></META><CONTEXTE><TEXTE cid="{cid}" nature="CODE"><TITRE_TXT>Code</TITRE_TXT></TEXTE></CONTEXTE><VERSIONS><VERSION etat="VIGUEUR"><LIEN_ART debut="2020-01-01" etat="VIGUEUR" fin="2999-01-01" id="{id}" num="{num}" origine="LEGI"/></VERSION></VERSIONS><NOTA><CONTENU/></NOTA><BLOC_TEXTUEL><CONTENU>{html}</CONTENU></BLOC_TEXTUEL><LIENS/></ARTICLE>"#
    )
}

/// Global synthétique : les fichiers donnés, sans préfixe.
pub(crate) fn global_archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut archive = TestArchive::new();
    for (path, data) in files {
        archive.file(path, data);
    }
    archive.gz()
}

/// Incrément synthétique : préfixe `stamp/`, listes de suppression en tête (comme la DILA),
/// puis les fichiers donnés.
pub(crate) fn increment_archive(
    stamp: &str,
    delete_files: &[&str],
    delete_dirs: &[&str],
    files: &[(&str, &[u8])],
) -> Vec<u8> {
    let mut archive = TestArchive::new();
    archive.dir(stamp);
    let list = delete_files.join("\n");
    archive.file(&format!("{stamp}/liste_suppression_legi.dat"), list.as_bytes());
    let dirs: Vec<String> = delete_dirs.iter().map(|dir| format!("{dir} D")).collect();
    archive
        .file(&format!("{stamp}/liste_suppression_legi_dossier.dat"), dirs.join("\n").as_bytes());
    for (path, data) in files {
        archive.file(&format!("{stamp}/{path}"), data);
    }
    archive.gz()
}

/// Constructeur d'archives `tar` (puis `tar.gz`) en mémoire, GNU tar comme la DILA.
pub(crate) struct TestArchive {
    builder: tar::Builder<Vec<u8>>,
}

impl TestArchive {
    pub(crate) fn new() -> Self {
        Self { builder: tar::Builder::new(Vec::new()) }
    }

    fn header(size: usize, mtime: u64, kind: EntryType) -> Header {
        let mut header = Header::new_gnu();
        header.set_size(u64::try_from(size).unwrap());
        header.set_mtime(mtime);
        header.set_mode(0o600);
        header.set_entry_type(kind);
        header
    }

    /// Fichier ordinaire daté de `mtime`.
    pub(crate) fn file_at(&mut self, path: &str, data: &[u8], mtime: u64) -> &mut Self {
        let mut header = Self::header(data.len(), mtime, EntryType::Regular);
        self.builder.append_data(&mut header, path, data).unwrap();
        self
    }

    /// Fichier ordinaire daté de 0.
    pub(crate) fn file(&mut self, path: &str, data: &[u8]) -> &mut Self {
        self.file_at(path, data, 0)
    }

    /// Fichier dont le nom (100 octets au plus) est écrit tel quel, sans aucune validation.
    pub(crate) fn raw_file(&mut self, name: &[u8], data: &[u8]) -> &mut Self {
        let mut header = Self::header(data.len(), 0, EntryType::Regular);
        header.as_old_mut().name[..name.len()].copy_from_slice(name);
        header.set_cksum();
        self.builder.append(&header, data).unwrap();
        self
    }

    /// Dossier.
    pub(crate) fn dir(&mut self, path: &str) -> &mut Self {
        let mut header = Self::header(0, 0, EntryType::Directory);
        self.builder.append_data(&mut header, path, std::io::empty()).unwrap();
        self
    }

    /// Lien (symbolique ou physique, selon `kind`).
    pub(crate) fn link(&mut self, path: &str, target: &str, kind: EntryType) -> &mut Self {
        let mut header = Self::header(0, 0, kind);
        self.builder.append_link(&mut header, path, target).unwrap();
        self
    }

    /// Membre qui n'est ni un fichier, ni un dossier, ni un lien (tube nommé).
    pub(crate) fn fifo(&mut self, path: &str) -> &mut Self {
        let mut header = Self::header(0, 0, EntryType::Fifo);
        self.builder.append_data(&mut header, path, std::io::empty()).unwrap();
        self
    }

    /// L'archive `tar` non compressée.
    pub(crate) fn tar(self) -> Vec<u8> {
        self.builder.into_inner().unwrap()
    }

    /// L'archive `tar.gz`.
    pub(crate) fn gz(self) -> Vec<u8> {
        self.gz_at(Compression::fast())
    }

    /// L'archive `tar.gz` sans compression : les décalages du gzip suivent ceux du tar.
    pub(crate) fn gz_stored(self) -> Vec<u8> {
        self.gz_at(Compression::none())
    }

    fn gz_at(self, level: Compression) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), level);
        encoder.write_all(&self.tar()).unwrap();
        encoder.finish().unwrap()
    }
}
