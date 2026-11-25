//! Tests du lecteur d'archive : les mini-archives réelles de T-MS-02, puis des archives écrites
//! ici, dont un champ est faussé à la fois.

use std::io::Write;

use flate2::{Compression, write::DeflateEncoder};
use pretty_assertions::assert_eq;
use proptest::prelude::*;

use super::*;

const PART: &[u8] = include_bytes!("../../tests/fixtures/archives/part-vosdroits-mini.zip");
const PRO: &[u8] = include_bytes!("../../tests/fixtures/archives/pro-vosdroits-mini.zip");
const F1437: &[u8] = include_bytes!("../../tests/fixtures/xml/part/F1437.xml");
const MENU: &[u8] = include_bytes!("../../tests/fixtures/xml/part/menu.xml");

/// Décalages des champs (APPNOTE §4.3.7 et §4.3.12).
mod local {
    pub(super) const FLAGS: usize = 6;
    pub(super) const METHOD: usize = 8;
    pub(super) const CRC: usize = 14;
    pub(super) const COMPRESSED: usize = 18;
    pub(super) const SIZE: usize = 22;
    pub(super) const NAME: usize = 30;
}
mod central {
    pub(super) const FLAGS: usize = 8;
    pub(super) const METHOD: usize = 10;
    pub(super) const CRC: usize = 16;
    pub(super) const COMPRESSED: usize = 20;
    pub(super) const SIZE: usize = 24;
    pub(super) const NAME_LEN: usize = 28;
    pub(super) const EXTRA_LEN: usize = 30;
    pub(super) const COMMENT_LEN: usize = 32;
    pub(super) const HEADER: usize = 42;
}
mod end {
    pub(super) const DISK: usize = 4;
    pub(super) const DIRECTORY_DISK: usize = 6;
    pub(super) const ON_DISK: usize = 8;
    pub(super) const TOTAL: usize = 10;
    pub(super) const SIZE: usize = 12;
    pub(super) const OFFSET: usize = 16;
    pub(super) const COMMENT_LEN: usize = 20;
}

/// Octets d'une archive des entrées `(nom, contenu)`, compressées : pour les tests des autres
/// modules.
pub(crate) fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let entries: Vec<(&[u8], &[u8], Method)> = entries
        .iter()
        .map(|(name, content)| (name.as_bytes(), *content, Method::Deflated))
        .collect();
    Zip::build(&entries).bytes
}

/// Une archive écrite ici, et où sont ses enregistrements.
struct Zip {
    bytes: Vec<u8>,
    locals: Vec<usize>,
    centrals: Vec<usize>,
    end: usize,
}

impl Zip {
    /// Archive des entrées `(nom, contenu, méthode)`, sans extra ni commentaire.
    fn build(entries: &[(&[u8], &[u8], Method)]) -> Self {
        let mut bytes = Vec::new();
        let mut locals = Vec::new();
        let mut records = Vec::new();
        for (name, content, method) in entries {
            let mut crc = Crc::new();
            crc.update(content);
            let data = match method {
                Method::Stored => content.to_vec(),
                Method::Deflated => {
                    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
                    encoder.write_all(content).unwrap();
                    encoder.finish().unwrap()
                }
            };
            let code: u16 = if *method == Method::Stored { 0 } else { 8 };
            let fields = |out: &mut Vec<u8>| {
                for value in [20_u16, 0, code, 0, 0] {
                    out.extend(value.to_le_bytes());
                }
                for value in [crc.sum(), len32(data.len()), len32(content.len())] {
                    out.extend(value.to_le_bytes());
                }
            };
            locals.push(bytes.len());
            bytes.extend(LOCAL_SIGNATURE.to_le_bytes());
            fields(&mut bytes);
            bytes.extend(len16(name.len()).to_le_bytes());
            bytes.extend(0_u16.to_le_bytes());
            bytes.extend(*name);
            bytes.extend(&data);
            let mut record = CENTRAL_SIGNATURE.to_le_bytes().to_vec();
            record.extend(0x031e_u16.to_le_bytes());
            fields(&mut record);
            for value in [len16(name.len()), 0, 0, 0, 0] {
                record.extend(value.to_le_bytes());
            }
            record.extend(0_u32.to_le_bytes());
            record.extend(len32(*locals.last().unwrap()).to_le_bytes());
            record.extend(*name);
            records.push(record);
        }
        let offset = bytes.len();
        let mut centrals = Vec::new();
        for record in records {
            centrals.push(bytes.len());
            bytes.extend(record);
        }
        let end = bytes.len();
        let count = len16(entries.len());
        bytes.extend(END_SIGNATURE.to_le_bytes());
        for value in [0_u16, 0, count, count] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(len32(end.checked_sub(offset).unwrap()).to_le_bytes());
        bytes.extend(len32(offset).to_le_bytes());
        bytes.extend(0_u16.to_le_bytes());
        Self { bytes, locals, centrals, end }
    }

    fn one(name: &str, content: &[u8]) -> Self {
        Self::build(&[(name.as_bytes(), content, Method::Deflated)])
    }

    fn set16(&mut self, at: usize, value: u16) -> &mut Self {
        self.bytes[at..at.checked_add(2).unwrap()].copy_from_slice(&value.to_le_bytes());
        self
    }

    fn set32(&mut self, at: usize, value: u32) -> &mut Self {
        self.bytes[at..at.checked_add(4).unwrap()].copy_from_slice(&value.to_le_bytes());
        self
    }

    /// Le même champ dans l'en-tête local et dans le répertoire central de l'entrée `index`.
    fn both32(&mut self, index: usize, local: usize, central: usize, value: u32) -> &mut Self {
        let (l, c) = (self.locals[index], self.centrals[index]);
        self.set32(l.checked_add(local).unwrap(), value)
            .set32(c.checked_add(central).unwrap(), value)
    }

    fn open(&self) -> Result<Archive, ArchiveError> {
        Archive::open(self.bytes.clone())
    }
}

fn len16(len: usize) -> u16 {
    u16::try_from(len).unwrap()
}

fn len32(len: usize) -> u32 {
    u32::try_from(len).unwrap()
}

#[test]
fn the_real_mini_archives_open_and_read() {
    let part = Archive::open(PART.to_vec()).unwrap();
    assert_eq!((part.len(), part.ignored(), part.byte_len()), (11, 0, PART.len()));
    assert!(!part.is_empty());
    assert_eq!(part.names().next(), Some("F10041.xml"));
    assert_eq!(part.read("F1437.xml", ENTRY_MAX_BYTES).unwrap().unwrap(), F1437);
    // 31 763 octets : l'en-tête d'une entrée courte est l'entrée entière, vérifiée.
    assert_eq!(part.head("menu.xml").unwrap().unwrap(), MENU);
    assert_eq!(part.read("F1.xml", ENTRY_MAX_BYTES), Ok(None));
    assert_eq!(part.head("F1.xml"), Ok(None));
    let pro = Archive::open(PRO.to_vec()).unwrap();
    assert_eq!(pro.names().collect::<Vec<_>>(), ["F2408.xml", "F37954.xml", "menu.xml"]);
}

#[test]
fn stored_and_deflated_entries_read_back() {
    let zip = Zip::build(&[
        (b"F1.xml", b"<Publication/>", Method::Stored),
        (b"F2.xml", b"<Publication>texte</Publication>", Method::Deflated),
        (b"F3.xml", b"", Method::Stored),
    ]);
    let archive = zip.open().unwrap();
    assert_eq!(archive.read("F1.xml", 100).unwrap().unwrap(), b"<Publication/>");
    assert_eq!(archive.head("F2.xml").unwrap().unwrap(), b"<Publication>texte</Publication>");
    assert_eq!(archive.read("F3.xml", 0).unwrap().unwrap(), b"");
    assert!(Archive::open(Zip::build(&[]).bytes).unwrap().is_empty());
}

#[test]
fn odd_names_and_directories_are_ignored_and_counted() {
    let names: [&[u8]; 7] =
        [b"dossier/", b"a.txt", b"../F1.xml", b".xml", "é1.xml".as_bytes(), b"F 1.xml", b"F1.XML"];
    let mut entries: Vec<(&[u8], &[u8], Method)> =
        names.iter().map(|name| (*name, &b"x"[..], Method::Stored)).collect();
    entries.push((b"menu.xml", b"<Menu/>", Method::Deflated));
    let mut zip = Zip::build(&entries);
    // Une entrée ignorée n'est pas contrôlée plus avant : chiffrée, elle ne refuse rien.
    zip.set16(zip.centrals[1] + central::FLAGS, 1);
    let archive = zip.open().unwrap();
    assert_eq!(archive.names().collect::<Vec<_>>(), ["menu.xml"]);
    assert_eq!(archive.ignored(), 7);
}

#[test]
fn an_archive_without_its_end_is_refused() {
    assert_eq!(Archive::open(vec![0; 21]).unwrap_err(), ArchiveError::NoEnd);
    let zip = Zip::one("F1.xml", b"a");
    // Signature présente, mais le commentaire annoncé ne finit pas l'archive.
    let mut long = zip.bytes.clone();
    long.push(0);
    assert_eq!(Archive::open(long).unwrap_err(), ArchiveError::NoEnd);
    let mut commented = Zip::one("F1.xml", b"a");
    let at = commented.end + end::COMMENT_LEN;
    commented.set16(at, 3).bytes.extend(b"abc");
    assert_eq!(commented.open().unwrap().len(), 1);
    // Un commentaire de 65 535 octets est la limite de la recherche.
    let mut longest = Zip::one("F1.xml", b"a");
    let at = longest.end + end::COMMENT_LEN;
    longest.set16(at, 0xFFFF).bytes.extend(vec![b' '; 0xFFFF]);
    assert_eq!(longest.open().unwrap().len(), 1);
}

#[test]
fn zip64_archives_and_entries_are_refused() {
    let base = || Zip::one("F1.xml", b"a");
    let mut cases = Vec::new();
    for (at, wide) in
        [(end::ON_DISK, false), (end::TOTAL, false), (end::SIZE, true), (end::OFFSET, true)]
    {
        let mut zip = base();
        let at = zip.end + at;
        if wide {
            zip.set32(at, ZIP64_SIZE)
        } else {
            zip.set16(at, ZIP64_COUNT)
        };
        cases.push(zip.open().unwrap_err());
    }
    // Localisateur zip64 juste avant la fin de répertoire.
    let mut zip = base();
    let at = zip.end - ZIP64_LOCATOR_LEN;
    zip.set32(at, ZIP64_LOCATOR_SIGNATURE);
    cases.push(zip.open().unwrap_err());
    for field in [central::COMPRESSED, central::SIZE, central::HEADER] {
        let mut zip = base();
        let at = zip.centrals[0] + field;
        zip.set32(at, ZIP64_SIZE);
        cases.push(zip.open().unwrap_err());
    }
    assert_eq!(cases, [ArchiveError::Zip64; 8]);
}

#[test]
fn multi_disk_archives_are_refused() {
    let mut cases = Vec::new();
    for (field, value) in [(end::DISK, 1), (end::DIRECTORY_DISK, 1), (end::ON_DISK, 0)] {
        let mut zip = Zip::one("F1.xml", b"a");
        let at = zip.end + field;
        cases.push(zip.set16(at, value).open().unwrap_err());
    }
    assert_eq!(cases, [ArchiveError::MultiDisk; 3]);
}

#[test]
fn the_number_of_entries_is_bounded() {
    let mut zip = Zip::one("F1.xml", b"a");
    let (on_disk, total) = (zip.end + end::ON_DISK, zip.end + end::TOTAL);
    let over = u16::try_from(ENTRIES_MAX + 1).unwrap();
    zip.set16(on_disk, over).set16(total, over);
    assert_eq!(zip.open().unwrap_err(), ArchiveError::TooManyEntries);
    let names: Vec<Vec<u8>> = (0..ENTRIES_MAX).map(|n| format!("F{n}.xml").into_bytes()).collect();
    let entries: Vec<(&[u8], &[u8], Method)> =
        names.iter().map(|name| (name.as_slice(), &b""[..], Method::Stored)).collect();
    assert_eq!(Zip::build(&entries).open().unwrap().len(), ENTRIES_MAX);
}

#[test]
fn the_central_directory_must_end_at_the_end_record() {
    let mut cases = Vec::new();
    // Répertoire annoncé plus court, ou décalé.
    let mut zip = Zip::one("F1.xml", b"a");
    let (size, offset) = (zip.end + end::SIZE, zip.end + end::OFFSET);
    let declared = len32(zip.end - zip.centrals[0]);
    cases.push(zip.set32(size, declared - 1).open().unwrap_err());
    let mut zip = Zip::one("F1.xml", b"a");
    cases.push(zip.set32(offset, 1).open().unwrap_err());
    // Une entrée annoncée de plus que le répertoire n'en porte.
    let mut zip = Zip::one("F1.xml", b"a");
    let (on_disk, total) = (zip.end + end::ON_DISK, zip.end + end::TOTAL);
    cases.push(zip.set16(on_disk, 2).set16(total, 2).open().unwrap_err());
    // Une de moins : des octets restent dans le répertoire.
    let mut zip =
        Zip::build(&[(b"F1.xml", b"a", Method::Stored), (b"F2.xml", b"b", Method::Stored)]);
    let (on_disk, total) = (zip.end + end::ON_DISK, zip.end + end::TOTAL);
    cases.push(zip.set16(on_disk, 1).set16(total, 1).open().unwrap_err());
    assert_eq!(cases, [ArchiveError::Malformed; 4]);
}

#[test]
fn a_malformed_central_entry_is_refused() {
    let mut cases = Vec::new();
    let mut zip = Zip::one("F1.xml", b"a");
    let at = zip.centrals[0];
    cases.push(zip.set32(at, 0).open().unwrap_err());
    // Nom, puis extra, puis commentaire au-delà du répertoire.
    for field in [central::NAME_LEN, central::EXTRA_LEN, central::COMMENT_LEN] {
        let mut zip = Zip::one("F1.xml", b"a");
        let at = zip.centrals[0] + field;
        cases.push(zip.set16(at, 200).open().unwrap_err());
    }
    assert_eq!(cases, [ArchiveError::Malformed; 4]);
}

#[test]
fn encrypted_descriptor_and_unknown_method_entries_are_refused() {
    let flagged = |flags: u16| {
        let mut zip = Zip::one("F1.xml", b"a");
        let (l, c) = (zip.locals[0] + local::FLAGS, zip.centrals[0] + central::FLAGS);
        zip.set16(l, flags).set16(c, flags).open().unwrap_err()
    };
    assert_eq!(flagged(FLAG_ENCRYPTED), ArchiveError::Encrypted);
    assert_eq!(flagged(FLAG_STRONG_ENCRYPTION), ArchiveError::Encrypted);
    assert_eq!(flagged(FLAG_DESCRIPTOR), ArchiveError::DataDescriptor);
    // Le drapeau UTF-8 (bit 11) est permis.
    let mut zip = Zip::one("F1.xml", b"a");
    let (l, c) = (zip.locals[0] + local::FLAGS, zip.centrals[0] + central::FLAGS);
    assert_eq!(zip.set16(l, 0x0800).set16(c, 0x0800).open().unwrap().len(), 1);
    let mut zip = Zip::one("F1.xml", b"a");
    let (l, c) = (zip.locals[0] + local::METHOD, zip.centrals[0] + central::METHOD);
    assert_eq!(zip.set16(l, 12).set16(c, 12).open().unwrap_err(), ArchiveError::Method);
}

#[test]
fn a_misplaced_or_unsigned_local_header_is_refused() {
    let mut cases = Vec::new();
    let mut zip = Zip::one("F1.xml", b"a");
    let at = zip.centrals[0] + central::HEADER;
    let beyond = len32(zip.bytes.len());
    cases.push(zip.set32(at, beyond).open().unwrap_err());
    let mut zip = Zip::one("F1.xml", b"a");
    let at = zip.locals[0];
    cases.push(zip.set32(at, 0).open().unwrap_err());
    // Données stockées dont la taille compressée n'est pas la taille.
    let mut zip = Zip::build(&[(b"F1.xml", b"abc", Method::Stored)]);
    cases.push(zip.both32(0, local::COMPRESSED, central::COMPRESSED, 2).open().unwrap_err());
    // Données qui débordent sur le répertoire central.
    let mut zip = Zip::one("F1.xml", b"a");
    let into_directory = len32(zip.centrals[0]);
    cases.push(
        zip.both32(0, local::COMPRESSED, central::COMPRESSED, into_directory).open().unwrap_err(),
    );
    assert_eq!(cases, [ArchiveError::Malformed; 4]);
}

#[test]
fn the_local_header_must_repeat_the_central_entry() {
    let mut cases = Vec::new();
    for (field, value) in [
        (local::FLAGS, 0x0800),
        (local::METHOD, 0),
        (local::CRC, 1),
        (local::COMPRESSED, 1),
        (local::SIZE, 2),
    ] {
        let mut zip = Zip::one("F1.xml", b"a");
        let at = zip.locals[0] + field;
        if field == local::FLAGS || field == local::METHOD {
            zip.set16(at, u16::try_from(value).unwrap());
        } else {
            zip.set32(at, value);
        }
        cases.push(zip.open().unwrap_err());
    }
    // Même longueur, autre nom.
    let mut zip = Zip::one("F1.xml", b"a");
    let at = zip.locals[0] + local::NAME;
    zip.bytes[at + 1] = b'2';
    cases.push(zip.open().unwrap_err());
    assert_eq!(cases, [ArchiveError::Mismatch; 6]);
}

#[test]
fn duplicate_names_and_overlapping_entries_are_refused() {
    let twice = Zip::build(&[(b"F1.xml", b"a", Method::Stored), (b"F1.xml", b"b", Method::Stored)]);
    assert_eq!(twice.open().unwrap_err(), ArchiveError::Duplicate);
    // Les données de la première entrée s'étendent sur l'en-tête local de la seconde.
    let mut zip =
        Zip::build(&[(b"F1.xml", b"ab", Method::Stored), (b"F2.xml", b"cd", Method::Stored)]);
    let reach = len32(zip.locals[1] - zip.locals[0] - LOCAL_LEN - 6 + 1);
    let (l, c) = (zip.locals[0] + local::SIZE, zip.centrals[0] + central::SIZE);
    zip.both32(0, local::COMPRESSED, central::COMPRESSED, reach);
    zip.set32(l, reach).set32(c, reach);
    assert_eq!(zip.open().unwrap_err(), ArchiveError::Overlap);
}

#[test]
fn an_entry_larger_than_the_bound_is_refused_before_inflating() {
    let archive = Zip::one("F1.xml", &[b'a'; 100]).open().unwrap();
    assert_eq!(archive.read("F1.xml", 99), Err(EntryError::TooLarge));
    assert_eq!(archive.read("F1.xml", 100).unwrap().unwrap().len(), 100);
}

#[test]
fn a_corrupt_entry_is_refused() {
    let mut cases = Vec::new();
    // Somme CRC-32 fausse, dans les deux en-têtes.
    let mut zip = Zip::one("F1.xml", b"abc");
    cases.push(
        zip.both32(0, local::CRC, central::CRC, 1).open().unwrap().read("F1.xml", 10).unwrap_err(),
    );
    // Taille déclarée trop petite : la décompression s'arrête au premier octet en trop.
    let mut zip = Zip::one("F1.xml", b"abc");
    cases.push(
        zip.both32(0, local::SIZE, central::SIZE, 2)
            .open()
            .unwrap()
            .read("F1.xml", 10)
            .unwrap_err(),
    );
    // Trop grande : les données s'arrêtent avant.
    let mut zip = Zip::one("F1.xml", b"abc");
    cases.push(
        zip.both32(0, local::SIZE, central::SIZE, 4)
            .open()
            .unwrap()
            .read("F1.xml", 10)
            .unwrap_err(),
    );
    // Flux `deflate` illisible.
    let mut zip = Zip::one("F1.xml", b"abc");
    let data = zip.locals[0] + LOCAL_LEN + 6;
    zip.bytes[data] = 0xFF;
    cases.push(zip.open().unwrap().read("F1.xml", 10).unwrap_err());
    assert_eq!(cases, [EntryError::Corrupt; 4]);
}

#[test]
fn the_head_of_a_long_entry_is_its_first_bytes() {
    let content: Vec<u8> =
        (0..HEAD_MAX_BYTES + 10).map(|n| u8::try_from(n % 251).unwrap()).collect();
    for method in [Method::Stored, Method::Deflated] {
        let archive = Zip::build(&[(b"F1.xml", &content, method)]).open().unwrap();
        assert_eq!(archive.head("F1.xml").unwrap().unwrap(), &content[..HEAD_MAX_BYTES]);
    }
    // Des données qui s'arrêtent avant la borne de l'en-tête.
    let mut zip = Zip::one("F1.xml", b"abc");
    let size = len32(HEAD_MAX_BYTES + 1);
    let archive = zip.both32(0, local::SIZE, central::SIZE, size).open().unwrap();
    assert_eq!(archive.head("F1.xml"), Err(EntryError::Corrupt));
    // Un flux `deflate` illisible, dans une entrée longue.
    let mut zip = Zip::one("F1.xml", b"abc");
    let data = zip.locals[0] + LOCAL_LEN + 6;
    zip.bytes[data] = 0xFF;
    let archive = zip.both32(0, local::SIZE, central::SIZE, size).open().unwrap();
    assert_eq!(archive.head("F1.xml"), Err(EntryError::Corrupt));
    // Une entrée courte se lit en entier par l'en-tête, et sa somme est vérifiée.
    let mut zip = Zip::one("F1.xml", b"abc");
    let archive = zip.both32(0, local::CRC, central::CRC, 1).open().unwrap();
    assert_eq!(archive.head("F1.xml"), Err(EntryError::Corrupt));
}

#[test]
fn errors_name_the_refusal_without_any_byte_read() {
    let texts: Vec<String> = [
        ArchiveError::NoEnd,
        ArchiveError::Zip64,
        ArchiveError::MultiDisk,
        ArchiveError::TooManyEntries,
        ArchiveError::Encrypted,
        ArchiveError::DataDescriptor,
        ArchiveError::Method,
        ArchiveError::Malformed,
        ArchiveError::Mismatch,
        ArchiveError::Overlap,
        ArchiveError::Duplicate,
    ]
    .iter()
    .map(ToString::to_string)
    .chain([EntryError::TooLarge, EntryError::Corrupt].iter().map(ToString::to_string))
    .collect();
    assert_eq!(
        texts,
        [
            "no end of central directory",
            "zip64 archives are not read",
            "multi-disk archives are not read",
            "too many entries",
            "encrypted entry",
            "entry with a data descriptor",
            "unsupported compression method",
            "malformed archive",
            "central directory and local header disagree",
            "overlapping entries",
            "duplicate entry name",
            "entry larger than the bound",
            "corrupt entry",
        ]
    );
}

/// Ouvre `bytes` et, si l'archive est retenue, lit chacune de ses entrées.
fn open_and_read_all(bytes: Vec<u8>) {
    if let Ok(archive) = Archive::open(bytes) {
        let names: Vec<String> = archive.names().map(str::to_owned).collect();
        for name in names {
            let _ = archive.read(&name, ENTRY_MAX_BYTES);
            let _ = archive.head(&name);
        }
    }
}

proptest! {
    /// Une vraie archive coupée, allongée d'un octet (tous les décalages qui suivent glissent) ou
    /// dont un octet change ne fait jamais paniquer le lecteur, et toute entrée retenue se lit sans
    /// paniquer. Partir d'une vraie archive atteint les contrôles du répertoire et des en-têtes,
    /// que des octets tirés au hasard n'atteignent presque jamais.
    #[test]
    fn real_archives_cut_lengthened_or_changed_never_panic(
        cut in 0..PRO.len(),
        insert in 0..=PRO.len(),
        at in 0..PRO.len(),
        value in any::<u8>(),
    ) {
        open_and_read_all(PRO[..cut].to_vec());
        let mut longer = PRO.to_vec();
        longer.insert(insert, value);
        open_and_read_all(longer);
        let mut changed = PRO.to_vec();
        changed[at] = value;
        open_and_read_all(changed);
    }
}

/// Les bornes de l'ADR-0033 (décision 5) : 20 000 entrées, 2 Mio par entrée lue, 64 Kio par
/// en-tête.
#[test]
fn the_bounds_are_those_of_the_adr() {
    assert_eq!((ENTRIES_MAX, ENTRY_MAX_BYTES, HEAD_MAX_BYTES), (20_000, 2_097_152, 65_536));
}
