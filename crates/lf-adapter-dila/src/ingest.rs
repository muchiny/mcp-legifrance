//! Ingestion d'une archive en flux : `flate2` → `tar` → `quick-xml` (T-M9-02, rapport 10 §10).
//!
//! L'archive est lue **une seule fois, dans l'ordre**, sans rien extraire sur le disque (le
//! global fait 5,25 millions de membres et 25,9 Go une fois extrait). Pour chaque membre :
//!
//! 1. dossiers, liens symboliques et physiques, fichiers spéciaux et membres vides sont comptés
//!    et laissés de côté (un lien n'est jamais suivi) ;
//! 2. le chemin est classé ([`crate::path`]) ; l'arbre `eli/` et tout chemin inattendu sont
//!    comptés et ignorés ; un chemin dangereux est refusé ;
//! 3. un document XML est lu en entier (16 Mio au plus), analysé ([`crate::xml`]), vérifié (son
//!    identifiant doit être celui de son nom de fichier) puis rangé dans le [`DocStore`] sous son
//!    chemin ;
//! 4. les deux listes de suppression sont lues quand elles passent (elles ouvrent l'archive) mais
//!    **appliquées après tout le contenu** (annexe C §7.4-4) : d'abord les fichiers, puis les
//!    dossiers de texte.
//!
//! `tar` est lu en **mode brut** : sans lui, il lit en mémoire et en entier, avant tout plafond,
//! les en-têtes GNU de noms longs et les en-têtes PAX (un en-tête annonçant 20 Gio épuiserait la
//! mémoire). Le nom long GNU (`L`, près de la moitié des en-têtes de la DILA) est lu sur
//! `MAX_PATH_BYTES + 1` octets au plus ; `K`, `x` et `g` ne sont ni lus ni interprétés, et chaque
//! liste de suppression n'est lue qu'une fois par archive.
//!
//! Un membre défectueux (XML invalide, trop gros, identifiant incohérent, chemin dangereux) est
//! compté dans le [`IngestReport`] et n'arrête pas l'archive. Une archive illisible (gzip ou tar
//! corrompu, lecture interrompue) arrête l'ingestion avec [`IngestError::Archive`]. Le magasin
//! garde alors le contenu déjà rangé : comme tout est idempotent, il suffit de rejouer l'archive
//! (le registre la marque à rejouer, voir [`crate::registry`]).
//!
//! La fonction est **synchrone** et bloquante : un appelant asynchrone (V3-08) l'enveloppe dans
//! `tokio::task::spawn_blocking`.

use std::io::{self, BufRead, BufReader, Read};

use flate2::read::MultiGzDecoder;
use tar::{Archive, Entry, EntryType};

use crate::{
    error::IngestError,
    path::{DocKind, DocPath, Ignored, MAX_PATH_BYTES, Member, PathReject, classify_member},
    store::{DocStore, StoredDoc},
    suppression::Deletions,
    xml::{self, XmlError},
};

/// Taille maximale d'un membre, en octets : le plus gros membre observé fait 2,9 Mo.
pub const DEFAULT_MAX_MEMBER_BYTES: u64 = 16 * 1024 * 1024;

/// Nombre de refus gardés en détail dans un rapport (le total reste exact).
pub const MAX_KEPT_REJECTIONS: usize = 32;

/// Octets lus après le dernier membre pour atteindre la fin du gzip (le bourrage d'un tar GNU
/// fait au plus 10 Kio).
const MAX_TRAILING_BYTES: u64 = 64 * 1024 * 1024;

/// Longueur maximale, en caractères, du chemin recopié dans un [`Rejection`].
const MAX_REJECTION_PATH_CHARS: usize = 200;

/// Bornes de l'ingestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IngestLimits {
    /// Taille maximale d'un document XML ; au-delà, le membre est refusé sans être lu.
    pub max_member_bytes: u64,
    /// Taille maximale d'une liste de suppression (quelques dizaines de Kio en pratique).
    pub max_list_bytes: u64,
}

impl Default for IngestLimits {
    fn default() -> Self {
        Self {
            max_member_bytes: DEFAULT_MAX_MEMBER_BYTES,
            max_list_bytes: DEFAULT_MAX_MEMBER_BYTES,
        }
    }
}

/// Pourquoi un membre a été refusé.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// Chemin non UTF-8, trop long ou dangereux.
    Path(PathReject),
    /// Membre plus gros que la borne.
    MemberTooLarge,
    /// XML refusé par l'analyse.
    Xml(XmlError),
    /// L'identifiant du XML n'est pas celui du nom de fichier.
    IdMismatch,
    /// Une deuxième liste de suppression du même type : chacune est lue en entier en mémoire, et
    /// l'archive n'en porte qu'une de chaque (rapport 10 §6).
    DuplicateList,
}

/// Un membre refusé : son chemin (nettoyé des caractères de contrôle, tronqué) et le motif.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    /// Chemin du membre dans l'archive.
    pub member: String,
    /// Motif du refus.
    pub reason: RejectReason,
}

/// Bilan de l'ingestion d'une archive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IngestReport {
    /// Membres lus dans le tar.
    pub entries: u64,
    /// Dossiers.
    pub directories: u64,
    /// Liens symboliques et physiques (jamais suivis).
    pub links: u64,
    /// Autres membres qui ne sont pas des fichiers (tubes, périphériques…).
    pub other_entries: u64,
    /// Documents vides (95 dans le global, tous dans `eli/`).
    pub empty_members: u64,
    /// Membres de l'arbre `eli/`, ignorés.
    pub ignored_eli: u64,
    /// Membres à un chemin inattendu, ignorés.
    pub ignored_other: u64,
    /// `ARTICLE` rangés.
    pub articles: u64,
    /// `SECTION_TA` rangés.
    pub sections: u64,
    /// `TEXTELR` rangés.
    pub text_structs: u64,
    /// `TEXTE_VERSION` rangés.
    pub text_versions: u64,
    /// Octets de contenu lus (documents et listes), après décompression.
    pub content_bytes: u64,
    /// Membres refusés, tous motifs confondus.
    pub rejected: u64,
    /// Les premiers refus, au plus [`MAX_KEPT_REJECTIONS`].
    pub rejections: Vec<Rejection>,
    /// Chemins de fichiers listés pour suppression.
    pub deleted_files_listed: u64,
    /// Fichiers effectivement retirés du magasin.
    pub deleted_files_removed: u64,
    /// Dossiers de texte listés pour suppression.
    pub deleted_dirs_listed: u64,
    /// Documents retirés par la suppression de dossiers entiers.
    pub deleted_dir_docs_removed: u64,
    /// Lignes de suppression illisibles ou hors corpus.
    pub invalid_deletion_lines: u64,
}

impl IngestReport {
    /// Nombre de documents rangés, toutes natures confondues.
    pub fn documents(&self) -> u64 {
        self.articles
            .saturating_add(self.sections)
            .saturating_add(self.text_structs)
            .saturating_add(self.text_versions)
    }

    fn count(&mut self, kind: DocKind) {
        let counter = match kind {
            DocKind::Article => &mut self.articles,
            DocKind::Section => &mut self.sections,
            DocKind::TextStruct => &mut self.text_structs,
            DocKind::TextVersion => &mut self.text_versions,
        };
        *counter = counter.saturating_add(1);
    }

    fn reject(&mut self, raw_path: &[u8], reason: RejectReason) {
        self.rejected = self.rejected.saturating_add(1);
        if self.rejections.len() < MAX_KEPT_REJECTIONS {
            let member = String::from_utf8_lossy(raw_path)
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_REJECTION_PATH_CHARS)
                .collect();
            self.rejections.push(Rejection { member, reason });
        }
    }
}

/// État d'une ingestion en cours.
struct Run<'a, S: DocStore> {
    store: &'a mut S,
    limits: &'a IngestLimits,
    report: IngestReport,
    deletions: Deletions,
    buf: Vec<u8>,
    /// Les deux listes de suppression ne sont lues qu'une fois chacune.
    seen_file_list: bool,
    seen_dir_list: bool,
}

impl<S: DocStore> Run<'_, S> {
    fn archive_error(&self, source: std::io::Error) -> IngestError {
        IngestError::Archive { entries: self.report.entries, source }
    }

    /// Le nom long GNU que porte l'entrée `L`, lu jusqu'à `MAX_PATH_BYTES + 1` octets : un nom plus
    /// long reste plus long que la borne, donc refusé comme tel par `classify_member`, sans que
    /// l'entrée soit lue en entier (le reste est sauté par `tar`).
    fn long_name<R: Read>(&self, entry: &mut Entry<'_, R>) -> Result<Vec<u8>, IngestError> {
        let limit = u64::try_from(MAX_PATH_BYTES).map_or(u64::MAX, |bytes| bytes.saturating_add(1));
        let mut name = Vec::new();
        entry
            .by_ref()
            .take(limit)
            .read_to_end(&mut name)
            .map_err(|source| self.archive_error(source))?;
        while name.last() == Some(&0) {
            name.pop();
        }
        Ok(name)
    }

    /// Lit le contenu du membre dans `self.buf` ; `false` (membre refusé) s'il dépasse `limit`.
    fn read<R: Read>(
        &mut self,
        entry: &mut Entry<'_, R>,
        limit: u64,
        raw_path: &[u8],
    ) -> Result<bool, IngestError> {
        let size = entry.size();
        if size > limit {
            self.report.reject(raw_path, RejectReason::MemberTooLarge);
            return Ok(false);
        }
        self.buf.clear();
        entry.read_to_end(&mut self.buf).map_err(|source| self.archive_error(source))?;
        self.report.content_bytes = self.report.content_bytes.saturating_add(size);
        Ok(true)
    }

    fn visit<R: Read>(
        &mut self,
        entry: &mut Entry<'_, R>,
        long_name: Option<Vec<u8>>,
    ) -> Result<(), IngestError> {
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            self.report.directories = self.report.directories.saturating_add(1);
        } else if kind.is_symlink() || kind.is_hard_link() {
            self.report.links = self.report.links.saturating_add(1);
        } else if !kind.is_file() {
            self.report.other_entries = self.report.other_entries.saturating_add(1);
        } else {
            let raw_path = long_name.unwrap_or_else(|| entry.path_bytes().into_owned());
            match classify_member(&raw_path) {
                Err(reason) => self.report.reject(&raw_path, RejectReason::Path(reason)),
                Ok(Member::Ignored(Ignored::Eli)) => {
                    self.report.ignored_eli = self.report.ignored_eli.saturating_add(1);
                }
                Ok(Member::Ignored(Ignored::Other)) => {
                    self.report.ignored_other = self.report.ignored_other.saturating_add(1);
                }
                Ok(list @ (Member::FileList | Member::DirList)) => {
                    let seen = match list {
                        Member::FileList => &mut self.seen_file_list,
                        _ => &mut self.seen_dir_list,
                    };
                    if std::mem::replace(seen, true) {
                        self.report.reject(&raw_path, RejectReason::DuplicateList);
                        return Ok(());
                    }
                    let read = self.read(entry, self.limits.max_list_bytes, &raw_path)?;
                    match (read, list) {
                        (false, _) => {} // trop grosse : déjà comptée comme refusée
                        (true, Member::FileList) => self.deletions.read_file_list(&self.buf),
                        (true, _) => self.deletions.read_dir_list(&self.buf),
                    }
                }
                Ok(Member::Doc(path)) => self.document(entry, path, &raw_path)?,
            }
        }
        Ok(())
    }

    fn document<R: Read>(
        &mut self,
        entry: &mut Entry<'_, R>,
        path: DocPath,
        raw_path: &[u8],
    ) -> Result<(), IngestError> {
        if entry.size() == 0 {
            self.report.empty_members = self.report.empty_members.saturating_add(1);
            return Ok(());
        }
        if !self.read(entry, self.limits.max_member_bytes, raw_path)? {
            return Ok(());
        }
        let document = match xml::parse(&self.buf, path.kind) {
            Ok(document) if document.id() == path.id => document,
            Ok(_) => {
                self.report.reject(raw_path, RejectReason::IdMismatch);
                return Ok(());
            }
            Err(error) => {
                self.report.reject(raw_path, RejectReason::Xml(error));
                return Ok(());
            }
        };
        self.report.count(path.kind);
        let mtime = entry.header().mtime().unwrap_or(0);
        self.store.put(StoredDoc { path, mtime, document })?;
        Ok(())
    }

    /// Applique les suppressions lues : fichiers d'abord, puis dossiers de texte.
    fn apply_deletions(&mut self) -> Result<(), IngestError> {
        let Deletions { files, dirs, invalid } = std::mem::take(&mut self.deletions);
        self.report.invalid_deletion_lines = invalid;
        self.report.deleted_files_listed = u64::try_from(files.len()).unwrap_or(u64::MAX);
        self.report.deleted_dirs_listed = u64::try_from(dirs.len()).unwrap_or(u64::MAX);
        for file in &files {
            if self.store.remove_file(&file.key)? {
                self.report.deleted_files_removed =
                    self.report.deleted_files_removed.saturating_add(1);
            }
        }
        for dir in &dirs {
            let removed = self.store.remove_text_dir(dir)?;
            self.report.deleted_dir_docs_removed =
                self.report.deleted_dir_docs_removed.saturating_add(removed);
        }
        Ok(())
    }
}

/// Ingère une archive `tar.gz` du fonds LEGI (global ou incrément) dans `store`.
///
/// `reader` est lu une seule fois, dans l'ordre ; rien n'est écrit sur le disque. Une entrée
/// vide (zéro octet) est une archive vide, tolérée : elle ne change rien. Une fois les
/// suppressions appliquées, le magasin est validé par [`DocStore::commit`] : c'est ce qui rend
/// l'archive durable avant que le registre ne la déclare appliquée.
///
/// # Errors
///
/// [`IngestError::Archive`] si le gzip ou le tar est illisible, [`IngestError::Store`] si le
/// magasin refuse une opération. Les membres défectueux ne sont pas des erreurs : ils figurent
/// dans le [`IngestReport`].
pub fn ingest_archive<R: Read, S: DocStore>(
    reader: R,
    store: &mut S,
    limits: &IngestLimits,
) -> Result<IngestReport, IngestError> {
    let mut input = BufReader::new(reader);
    let empty =
        input.fill_buf().map_err(|source| IngestError::Archive { entries: 0, source })?.is_empty();
    if empty {
        return Ok(IngestReport::default());
    }
    let mut archive = Archive::new(MultiGzDecoder::new(input));
    let mut run = Run {
        store,
        limits,
        report: IngestReport::default(),
        deletions: Deletions::default(),
        buf: Vec::new(),
        seen_file_list: false,
        seen_dir_list: false,
    };
    // Mode brut : sans lui, `tar` lit en mémoire, en entier et avant tout plafond, les en-têtes
    // GNU de noms longs et les en-têtes PAX (un en-tête `L` annonçant 20 Gio suffit à épuiser la
    // mémoire). On lit donc nous-mêmes le nom long, borné, et on laisse de côté le reste.
    // `entries()` n'échoue que sur une archive déjà entamée, ce qu'une archive neuve n'est pas.
    let mut long_name: Option<Vec<u8>> = None;
    for entry in archive.entries().map(|entries| entries.raw(true)).into_iter().flatten() {
        let mut entry = entry.map_err(|source| run.archive_error(source))?;
        match entry.header().entry_type() {
            EntryType::GNULongName => long_name = Some(run.long_name(&mut entry)?),
            // Cibles de lien longues et en-têtes PAX : jamais interprétés (les archives de la
            // DILA n'en portent aucun, rapport 10 §3.2), ni lus.
            EntryType::GNULongLink | EntryType::XHeader | EntryType::XGlobalHeader => {
                run.report.other_entries = run.report.other_entries.saturating_add(1);
            }
            _ => {
                run.report.entries = run.report.entries.saturating_add(1);
                run.visit(&mut entry, long_name.take())?;
            }
        }
    }
    // La fin du tar (blocs nuls) n'est pas la fin du gzip : lire jusqu'au bout vérifie la somme
    // de contrôle et révèle une archive tronquée juste après son dernier membre.
    let mut rest = archive.into_inner().take(MAX_TRAILING_BYTES);
    io::copy(&mut rest, &mut io::sink()).map_err(|source| run.archive_error(source))?;
    run.apply_deletions()?;
    run.store.commit()?;
    Ok(run.report)
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use tar::EntryType;

    use super::*;
    use crate::{
        path::Validity,
        store::MemoryStore,
        testing::{
            TestArchive, TestReader, TestStore, article_member, article_xml, fixture,
            global_archive, increment_archive, text_dir,
        },
        xml::XmlError,
    };

    const CID: &str = "JORFTEXT000000000001";
    const ID_A: &str = "LEGIARTI000000000001";
    const ID_B: &str = "LEGIARTI000000000002";

    fn ingest(bytes: Vec<u8>, store: &mut TestStore) -> Result<IngestReport, IngestError> {
        ingest_archive(TestReader::new(bytes), store, &IngestLimits::default())
    }

    fn ingest_with(
        bytes: Vec<u8>,
        store: &mut TestStore,
        limits: &IngestLimits,
    ) -> Result<IngestReport, IngestError> {
        ingest_archive(TestReader::new(bytes), store, limits)
    }

    fn xml(id: &str, html: &str) -> String {
        article_xml(CID, id, "1", html)
    }

    fn content_of(store: &MemoryStore, key: &str) -> Option<String> {
        store.get(key).map(|doc| doc.document.as_article().unwrap().content.clone())
    }

    /// Nombre de membres lus avant l'erreur, d'après son message (`archive unreadable after N …`).
    fn entries_in(error: &IngestError) -> u64 {
        let message = error.to_string();
        let tail = message.strip_prefix("archive unreadable after ").unwrap();
        tail.split(' ').next().unwrap().parse().unwrap()
    }

    /// Du texte incompressible, pour que l'archive pèse bien plus que les tampons de lecture.
    fn noise(seed: u64, len: usize) -> String {
        let mut state = seed;
        (0..len)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                char::from_digit(u32::try_from((state >> 33) % 16).unwrap(), 16).unwrap()
            })
            .collect()
    }

    #[test]
    fn ingest_streams_without_extraction() {
        const ARTICLES: usize = 600;
        let mut archive = TestArchive::new();
        for n in 0..ARTICLES {
            let id = format!("LEGIARTI{n:012}");
            let html = format!("<p>{}</p>", noise(u64::try_from(n).unwrap(), 2_000));
            archive.file(
                &article_member("code_en_vigueur", "LEGITEXT000000000001", &id),
                xml(&id, &html).as_bytes(),
            );
        }
        let bytes = archive.gz();
        let total = bytes.len();
        assert!(total > 300_000, "the archive must be far larger than any read buffer: {total}");

        let reader = TestReader::new(bytes);
        let consumed = Rc::clone(&reader.consumed);
        let mut store = TestStore::new();
        store.probe = Some(Rc::clone(&consumed));
        let report = ingest_archive(reader, &mut store, &IngestLimits::default()).unwrap();

        assert_eq!(report.articles, 600);
        assert_eq!(store.inner.len(), 600);
        // le premier document est rangé alors que l'essentiel de l'archive n'est pas encore lu :
        // ni lecture complète d'abord, ni fichier temporaire
        let first = store.first_put_at.unwrap();
        assert!(first < total / 4, "first document stored after {first} of {total} bytes");
        assert_eq!(consumed.get(), total, "the whole archive is read exactly once");
        assert!(report.content_bytes > 1_000_000);
    }

    #[test]
    fn an_oversized_long_name_is_refused_without_being_read_whole() {
        let id = "LEGIARTI000000000009";
        let mut archive = TestArchive::new();
        // un nom long de 100 Kio : seuls les premiers octets sont lus, le reste est sauté
        archive.extension(EntryType::GNULongName, 100_000, &vec![b'a'; 100_000]);
        archive.raw_file(b"short-header-name", b"<x/>");
        archive.file(
            &article_member("code_en_vigueur", "LEGITEXT000000000001", id),
            xml(id, "<p>suite</p>").as_bytes(),
        );
        let mut store = TestStore::new();
        let report = ingest(archive.gz(), &mut store).unwrap();
        assert_eq!(report.rejected, 1);
        assert_eq!(report.rejections[0].reason, RejectReason::Path(PathReject::TooLong));
        // le flux est resté aligné : le membre suivant est rangé
        assert_eq!(report.articles, 1);
        assert_eq!(report.entries, 2);
    }

    #[test]
    fn an_archive_cut_inside_a_long_name_is_an_archive_error() {
        let mut archive = TestArchive::new();
        archive.extension(EntryType::GNULongName, 100_000, &vec![b'a'; 100_000]);
        let mut bytes = archive.gz_stored();
        // l'en-tête (512 octets) puis une partie seulement des 513 octets de nom que l'on lit
        bytes.truncate(900);
        let error = ingest(bytes, &mut TestStore::new()).unwrap_err();
        assert!(error.to_string().starts_with("archive unreadable"), "{error}");
    }

    #[test]
    fn a_long_name_entry_announcing_more_than_the_archive_holds_is_an_archive_error() {
        let mut archive = TestArchive::new();
        archive.extension(EntryType::GNULongName, 1 << 33, &[b'a'; 10]);
        let error = ingest(archive.gz(), &mut TestStore::new()).unwrap_err();
        assert!(error.to_string().starts_with("archive unreadable"), "{error}");
    }

    #[test]
    fn pax_and_long_link_headers_are_skipped_and_counted_not_read() {
        let id = "LEGIARTI000000000008";
        let mut archive = TestArchive::new();
        archive.extension(EntryType::XGlobalHeader, 20, b"20 comment=global\n   ");
        archive.extension(EntryType::XHeader, 12, b"12 path=a/b\n");
        archive.extension(EntryType::GNULongLink, 4, b"cible");
        archive.file(
            &article_member("code_en_vigueur", "LEGITEXT000000000001", id),
            xml(id, "<p>ok</p>").as_bytes(),
        );
        let mut store = TestStore::new();
        let report = ingest(archive.gz(), &mut store).unwrap();
        assert_eq!(report.other_entries, 3);
        assert_eq!(report.entries, 1);
        assert_eq!(report.articles, 1);
        assert_eq!(report.rejected, 0);
    }

    #[test]
    fn a_second_deletion_list_of_the_same_kind_is_refused() {
        let id = "LEGIARTI000000000007";
        let key = article_member("code_en_vigueur", "LEGITEXT000000000001", id);
        let mut archive = TestArchive::new();
        archive.file(&key, xml(id, "<p>reste</p>").as_bytes());
        archive.file("20260101-100000/liste_suppression_legi.dat", b"");
        // la deuxième liste nommerait l'article : elle n'est pas lue
        archive.file("20260101-100000/liste_suppression_legi.dat", key.as_bytes());
        archive.file("20260101-100000/liste_suppression_legi_dossier.dat", b"");
        archive.file("20260101-100000/liste_suppression_legi_dossier.dat", b"legi/x D");
        let mut store = TestStore::new();
        let report = ingest(archive.gz(), &mut store).unwrap();
        assert_eq!(report.rejected, 2);
        assert!(report.rejections.iter().all(|r| r.reason == RejectReason::DuplicateList));
        assert_eq!(report.deleted_files_listed, 0);
        assert_eq!(report.deleted_dirs_listed, 0);
        assert_eq!(report.deleted_files_removed, 0);
        assert_eq!(store.inner.len(), 1, "l'article n'est pas supprimé");
    }

    #[test]
    fn deletions_applied_in_order() {
        let key =
            |sub: &str, id: &str| article_member(sub, CID, id).trim_end_matches(".xml").to_owned();
        let path = article_member("TNC_en_vigueur", CID, ID_A);
        let other = article_member("TNC_en_vigueur", CID, ID_B);
        let first = global_archive(&[
            (&path, xml(ID_A, "first").as_bytes()),
            (&other, xml(ID_B, "kept").as_bytes()),
        ]);
        let deleting =
            increment_archive("20260101-100000", &[&key("TNC_en_vigueur", ID_A)], &[], &[]);
        let readding = increment_archive(
            "20260102-100000",
            &[],
            &[],
            &[(&path, xml(ID_A, "again").as_bytes())],
        );

        // dans l'ordre chronologique : ajout, suppression, nouvel ajout => présent, dernier contenu
        let mut ordered = TestStore::new();
        for archive in [first.clone(), deleting.clone(), readding.clone()] {
            ingest(archive, &mut ordered).unwrap();
        }
        assert_eq!(content_of(&ordered, &key("TNC_en_vigueur", ID_A)).as_deref(), Some("again"));
        assert_eq!(content_of(&ordered, &key("TNC_en_vigueur", ID_B)).as_deref(), Some("kept"));

        // dans le désordre, la suppression passe après le nouvel ajout => absent : l'ordre compte
        let mut shuffled = TestStore::new();
        for archive in [first, readding, deleting] {
            ingest(archive, &mut shuffled).unwrap();
        }
        assert_eq!(content_of(&shuffled, &key("TNC_en_vigueur", ID_A)), None);
        assert_eq!(content_of(&shuffled, &key("TNC_en_vigueur", ID_B)).as_deref(), Some("kept"));
    }

    #[test]
    fn deletions_are_applied_after_the_content_of_their_own_archive() {
        // les listes sont les premiers membres de l'archive, mais le contenu passe avant elles
        let listed = article_member("TNC_en_vigueur", CID, ID_A);
        let unlisted = article_member("TNC_en_vigueur", CID, ID_B);
        let archive = increment_archive(
            "20260101-100000",
            &[listed.trim_end_matches(".xml")],
            &[],
            &[(&listed, xml(ID_A, "x").as_bytes()), (&unlisted, xml(ID_B, "y").as_bytes())],
        );
        let mut store = TestStore::new();
        let report = ingest(archive, &mut store).unwrap();
        assert_eq!(report.documents(), 2);
        assert_eq!(report.deleted_files_listed, 1);
        assert_eq!(report.deleted_files_removed, 1);
        assert_eq!(store.len(), 1);
        assert!(store.get(unlisted.trim_end_matches(".xml")).is_some());
    }

    #[test]
    fn a_deletion_targets_a_path_never_an_id() {
        let old = article_member("TNC_en_vigueur", CID, ID_A);
        let moved = article_member("TNC_non_vigueur", CID, ID_A);
        let dir = text_dir("TNC_en_vigueur", CID);
        let global = global_archive(&[(&old, xml(ID_A, "v1").as_bytes())]);
        let incr = increment_archive(
            "20260101-100000",
            &[old.trim_end_matches(".xml")],
            &[&dir],
            &[(&moved, xml(ID_A, "v2").as_bytes())],
        );
        let mut store = TestStore::new();
        ingest(global, &mut store).unwrap();
        let report = ingest(incr, &mut store).unwrap();
        assert_eq!(report.deleted_files_removed, 1);
        assert_eq!(report.deleted_dir_docs_removed, 0, "the file list already emptied the folder");
        assert_eq!(store.len(), 1, "the moved copy survives its own deletion");
        let survivor = store.preferred(ID_A).unwrap();
        assert_eq!(survivor.path.validity, Validity::NonVigueur);
    }

    #[test]
    fn a_dir_deletion_removes_the_files_the_list_does_not_name() {
        let listed = article_member("TNC_en_vigueur", CID, ID_A);
        let unlisted = article_member("TNC_en_vigueur", CID, ID_B);
        let dir = text_dir("TNC_en_vigueur", CID);
        let global = global_archive(&[
            (&listed, xml(ID_A, "a").as_bytes()),
            (&unlisted, xml(ID_B, "b").as_bytes()),
        ]);
        let incr =
            increment_archive("20260101-100000", &[listed.trim_end_matches(".xml")], &[&dir], &[]);
        let mut store = TestStore::new();
        ingest(global, &mut store).unwrap();
        let report = ingest(incr, &mut store).unwrap();
        assert_eq!((report.deleted_files_removed, report.deleted_dir_docs_removed), (1, 1));
        assert!(store.is_empty());
        assert_eq!(report.deleted_dirs_listed, 1);
    }

    #[test]
    fn the_real_mini_archives_move_a_folder_and_replace_a_file() {
        let mut store = TestStore::new();
        let global = ingest(fixture("archives/legi-global-mini.tar.gz"), &mut store).unwrap();
        assert_eq!(global.entries, 170);
        assert_eq!(global.directories, 143);
        assert_eq!(global.links, 1, "the dangling eli symlink is counted, never followed");
        assert_eq!(global.ignored_eli, 3);
        assert_eq!((global.articles, global.sections), (13, 4));
        assert_eq!((global.text_structs, global.text_versions), (3, 3));
        assert_eq!((global.rejected, global.ignored_other, global.empty_members), (0, 0, 0));
        assert_eq!(store.len(), 23);

        let moved_id = "LEGIARTI000024586531";
        let old_key = "legi/global/code_et_TNC_en_vigueur/TNC_en_vigueur/JORF/TEXT/00/00/24/58/38/JORFTEXT000024583863/article/LEGI/ARTI/00/00/24/58/65/LEGIARTI000024586531";
        let new_key = old_key
            .replace("TNC_en_vigueur", "TNC_non_vigueur")
            .replace("code_et_TNC_en_vigueur", "code_et_TNC_non_vigueur");
        assert!(store.get(old_key).is_some());
        let replaced_key = "legi/global/code_et_TNC_en_vigueur/code_en_vigueur/LEGI/TEXT/00/00/06/07/42/LEGITEXT000006074220/article/LEGI/ARTI/00/00/45/41/85/LEGIARTI000045418514";
        let before = store.get(replaced_key).unwrap().mtime;

        let incr = ingest(fixture("archives/legi-incr-20261028-mini.tar.gz"), &mut store).unwrap();
        assert_eq!((incr.articles, incr.text_structs, incr.text_versions), (5, 2, 2));
        assert_eq!(incr.deleted_files_listed, 58);
        assert_eq!(incr.deleted_dirs_listed, 3);
        assert_eq!(incr.invalid_deletion_lines, 0);
        assert_eq!(
            incr.deleted_files_removed, 5,
            "4 articles and the text version of the old folder"
        );
        assert_eq!(
            incr.deleted_dir_docs_removed, 1,
            "the text structure the file list does not name"
        );

        assert!(store.get(old_key).is_none(), "gone from en_vigueur");
        let copies = store.copies(moved_id);
        assert_eq!(copies.len(), 1);
        assert_eq!(copies[0].path.key, new_key, "re-added under non_vigueur, same id");
        assert_eq!(store.len(), 23, "6 documents left, 6 documents came back");

        let after = store.get(replaced_key).unwrap();
        // les octets diffèrent (rapport 10 §6) mais le modèle n'en garde que l'essentiel :
        // c'est la date du membre qui prouve le remplacement
        assert!(after.mtime > before, "{} <= {before}", after.mtime);
        let expected = xml::parse(
            &fixture("xml/incr-20261028/code_en_vigueur/LEGIARTI000045418514.xml"),
            DocKind::Article,
        )
        .unwrap();
        assert_eq!(after.document, expected);
    }

    #[test]
    fn the_same_real_archive_replayed_changes_nothing() {
        let mut once = TestStore::new();
        let mut twice = TestStore::new();
        for store in [&mut once, &mut twice] {
            ingest(fixture("archives/legi-global-mini.tar.gz"), store).unwrap();
            ingest(fixture("archives/legi-incr-20261028-mini.tar.gz"), store).unwrap();
        }
        ingest(fixture("archives/legi-incr-20261028-mini.tar.gz"), &mut twice).unwrap();
        let keys = |store: &MemoryStore| -> Vec<(String, u64)> {
            store.iter().map(|d| (d.path.key.clone(), d.mtime)).collect()
        };
        assert_eq!(keys(&once), keys(&twice), "an increment is idempotent");
    }

    #[test]
    fn members_that_are_not_documents_are_counted_and_set_aside() {
        let good = article_member("code_en_vigueur", "LEGITEXT000000000001", ID_A);
        let mut archive = TestArchive::new();
        archive
            .dir("legi/")
            .link(&good, "elsewhere", EntryType::Symlink)
            .link("legi/hard", "legi/global", EntryType::Link)
            .fifo("legi/pipe")
            .file("legi/global/code_et_TNC_en_vigueur/empty.xml", b"")
            .file(
                &article_member("code_en_vigueur", "LEGITEXT000000000001", "LEGIARTI000000000009"),
                b"",
            )
            .file("legi/global/eli/jo/2013/10/30/versions.xml", b"<ID/>")
            .file("kali/global/x.xml", b"<ARTICLE/>")
            .file(&good, xml(ID_A, "ok").as_bytes());
        let mut store = TestStore::new();
        let report = ingest(archive.gz(), &mut store).unwrap();
        assert_eq!(report.directories, 1);
        assert_eq!(report.links, 2);
        assert_eq!(report.other_entries, 1);
        assert_eq!(report.empty_members, 1, "a corpus document of 0 bytes");
        assert_eq!(report.ignored_eli, 1);
        assert_eq!(report.ignored_other, 2);
        assert_eq!(report.articles, 1);
        assert_eq!(report.rejected, 0);
        assert_eq!(report.entries, 9);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn defective_members_are_refused_one_by_one_without_stopping_the_archive() {
        let sub = "code_en_vigueur";
        let text = "LEGITEXT000000000001";
        let bad_xml = article_member(sub, text, "LEGIARTI000000000003");
        let mismatch = article_member(sub, text, "LEGIARTI000000000004");
        let big = article_member(sub, text, "LEGIARTI000000000005");
        let good = article_member(sub, text, ID_A);
        let long_path = format!("{}/x.xml", "a".repeat(MAX_PATH_PROBE));
        let mut archive = TestArchive::new();
        archive
            .raw_file(b"../evil.xml", b"x")
            .raw_file(b"legi/global/\xff.xml", b"x")
            .file(&long_path, b"x")
            .file(&bad_xml, b"<ARTICLE><oops></ARTICLE>")
            .file(&mismatch, xml("LEGIARTI000000000099", "x").as_bytes())
            .file(&big, xml("LEGIARTI000000000005", &"x".repeat(5_000)).as_bytes())
            .file(&good, xml(ID_A, "ok").as_bytes());
        let limits = IngestLimits { max_member_bytes: 4_000, max_list_bytes: 10 };
        let mut store = TestStore::new();
        let report = ingest_with(archive.gz(), &mut store, &limits).unwrap();

        let reasons: Vec<RejectReason> = report.rejections.iter().map(|r| r.reason).collect();
        assert_eq!(
            reasons,
            [
                RejectReason::Path(PathReject::Unsafe),
                RejectReason::Path(PathReject::NonUtf8),
                RejectReason::Path(PathReject::TooLong),
                RejectReason::Xml(XmlError::Malformed),
                RejectReason::IdMismatch,
                RejectReason::MemberTooLarge,
            ]
        );
        assert_eq!(report.rejected, 6);
        assert_eq!(report.articles, 1);
        assert_eq!(store.len(), 1, "the good article is stored after the defective ones");
        assert_eq!(report.rejections[0].member, "../evil.xml");
        assert!(report.rejections[2].member.chars().count() <= MAX_REJECTION_PATH_CHARS);
    }

    const MAX_PATH_PROBE: usize = crate::path::MAX_PATH_BYTES + 8;

    #[test]
    fn rejection_details_are_capped_but_the_count_stays_exact() {
        let mut archive = TestArchive::new();
        for n in 0..(MAX_KEPT_REJECTIONS + 8) {
            let id = format!("LEGIARTI{n:012}");
            archive
                .file(&article_member("code_en_vigueur", "LEGITEXT000000000001", &id), b"not xml");
        }
        archive.raw_file(b"a\x01b/../c", b"x");
        let mut store = TestStore::new();
        let report = ingest(archive.gz(), &mut store).unwrap();
        assert_eq!(report.rejected, u64::try_from(MAX_KEPT_REJECTIONS + 9).unwrap());
        assert_eq!(report.rejections.len(), MAX_KEPT_REJECTIONS);
        assert!(report.rejections.iter().all(|r| r.reason == RejectReason::Xml(XmlError::NoRoot)));

        let mut small = TestArchive::new();
        small.raw_file(b"a\x01b/../c", b"x");
        let report = ingest(small.gz(), &mut store).unwrap();
        assert_eq!(report.rejections[0].member, "ab/../c", "control characters are dropped");
    }

    #[test]
    fn oversized_suppression_lists_are_refused_and_junk_lines_counted() {
        let mut archive = TestArchive::new();
        archive
            .file("20260101-100000/liste_suppression_legi.dat", &[b'x'; 64])
            .file("20260101-100000/liste_suppression_legi_dossier.dat", b"nonsense\nlegi/x D\n");
        let mut store = TestStore::new();
        let limits = IngestLimits { max_member_bytes: 1_000, max_list_bytes: 32 };
        let report = ingest_with(archive.gz(), &mut store, &limits).unwrap();
        assert_eq!(report.rejections.len(), 1);
        assert_eq!(report.rejections[0].reason, RejectReason::MemberTooLarge);
        assert_eq!(report.invalid_deletion_lines, 2);
        assert_eq!((report.deleted_files_listed, report.deleted_dirs_listed), (0, 0));
    }

    #[test]
    fn an_empty_input_or_an_empty_tar_is_an_empty_archive() {
        let mut store = TestStore::new();
        assert_eq!(ingest(Vec::new(), &mut store).unwrap(), IngestReport::default());
        let report = ingest(TestArchive::new().gz(), &mut store).unwrap();
        assert_eq!(report, IngestReport::default());
        assert!(store.is_empty());
    }

    #[test]
    fn the_store_is_committed_once_after_the_content_and_the_deletions() {
        let path = article_member("TNC_en_vigueur", CID, ID_A);
        let archive = global_archive(&[(&path, xml(ID_A, "x").as_bytes())]);
        let mut store = TestStore::new();
        ingest(archive, &mut store).unwrap();
        assert_eq!((store.commits, store.len()), (1, 1));
        // une archive vide rend la main avant : rien à rendre durable
        ingest(Vec::new(), &mut store).unwrap();
        assert_eq!(store.commits, 1);
    }

    #[test]
    fn anything_that_is_not_gzip_is_an_archive_error() {
        let mut store = TestStore::new();
        let error = ingest(b"definitely not a tarball".to_vec(), &mut store).unwrap_err();
        assert_eq!(entries_in(&error), 0, "{error}");
        let plain_tar = TestArchive::new().tar();
        let error = ingest(plain_tar, &mut store).unwrap_err();
        assert_eq!(entries_in(&error), 0, "{error}");
    }

    #[test]
    fn a_truncated_archive_is_an_error_wherever_it_is_cut() {
        let mut archive = TestArchive::new();
        for n in 0..4 {
            let id = format!("LEGIARTI{n:012}");
            archive.file(
                &article_member("code_en_vigueur", "LEGITEXT000000000001", &id),
                xml(&id, "x").as_bytes(),
            );
        }
        let full = archive.gz_stored();
        assert_eq!(ingest(full.clone(), &mut TestStore::new()).unwrap().articles, 4);

        // coupée n'importe où, y compris après le dernier membre (fin du tar, somme de contrôle
        // du gzip manquante) : toujours une erreur d'archive, jamais un succès partiel
        let tail = [full.len() - 1, full.len() - 8, full.len() - 600];
        let mut cuts: Vec<usize> = (20..full.len()).step_by(97).collect();
        cuts.extend(tail);
        for cut in cuts {
            let mut store = TestStore::new();
            let truncated = full.get(..cut).unwrap().to_vec();
            let error = ingest(truncated, &mut store).unwrap_err();
            assert!(
                error.to_string().starts_with("archive unreadable after "),
                "cut {cut}: {error}"
            );
        }

        // le nombre de membres déjà lus est porté par l'erreur, leurs documents restent rangés
        let mut store = TestStore::new();
        let cut = full.get(..full.len() - 3_000).unwrap().to_vec();
        let entries = entries_in(&ingest(cut, &mut store).unwrap_err());
        assert!((1..=4).contains(&entries), "entries read before the cut: {entries}");
        let stored = u64::try_from(store.len()).unwrap();
        assert!((entries - 1..=entries).contains(&stored), "{stored} stored of {entries} read");
    }

    #[test]
    fn a_reader_that_fails_at_once_is_an_archive_error() {
        let mut store = TestStore::new();
        let error = ingest_archive(TestReader::failing(), &mut store, &IngestLimits::default())
            .unwrap_err();
        assert_eq!(entries_in(&error), 0);
        assert!(error.to_string().contains("disk gone"));
    }

    #[test]
    fn a_cut_inside_a_suppression_list_is_an_archive_error() {
        let key = article_member("TNC_en_vigueur", CID, ID_A);
        let dir_line = format!("{} D", text_dir("TNC_en_vigueur", CID));
        let stamp = "20260101-100000";
        let mut archive = TestArchive::new();
        archive
            .dir(stamp)
            .file(&format!("{stamp}/liste_suppression_legi.dat"), key.as_bytes())
            .file(&format!("{stamp}/liste_suppression_legi_dossier.dat"), dir_line.as_bytes());
        // gzip sans compression : les décalages du tar se retrouvent dans le gzip (+15 octets) ;
        // le dossier et chaque en-tête font 512 octets, chaque liste tient dans un bloc de 512
        let full = archive.gz_stored();
        assert!(ingest(full.clone(), &mut TestStore::new()).is_ok());
        let in_first_list = full.get(..15 + 512 + 512 + 50).unwrap().to_vec();
        assert_eq!(entries_in(&ingest(in_first_list, &mut TestStore::new()).unwrap_err()), 2);
        let in_second_list = full.get(..15 + 512 + 512 + 512 + 512 + 50).unwrap().to_vec();
        assert_eq!(entries_in(&ingest(in_second_list, &mut TestStore::new()).unwrap_err()), 3);
    }

    #[test]
    fn store_failures_stop_the_ingestion_with_the_store_error() {
        let path = article_member("TNC_en_vigueur", CID, ID_A);
        let dir = text_dir("TNC_en_vigueur", CID);
        let archive = increment_archive(
            "20260101-100000",
            &[path.trim_end_matches(".xml")],
            &[&dir],
            &[(&path, xml(ID_A, "x").as_bytes())],
        );
        for op in ["put", "remove_file", "remove_text_dir", "commit"] {
            let mut store = TestStore::failing(op);
            let error = ingest(archive.clone(), &mut store).unwrap_err();
            assert_eq!(error.to_string(), format!("document store failure: {op} refused"));
        }
    }
}
