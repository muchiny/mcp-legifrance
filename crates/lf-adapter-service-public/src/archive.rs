//! Lecteur d'archive zip, écrit pour le format que la DILA sert (ADR-0033 décision 5, T-MS-26).
//!
//! Sous-ensemble lu : un seul disque, ni zip64, ni chiffrement, ni descripteur de données ;
//! méthodes `deflate` (8) ou aucune compression (0) ; [`ENTRIES_MAX`] entrées au plus. Le
//! répertoire central doit finir là où commence la fin de répertoire ; chaque entrée retenue est
//! recoupée avec son en-tête local (nom, drapeaux, méthode, CRC-32 et tailles), ses données
//! tiennent avant le répertoire central, et les données de deux entrées ne se chevauchent jamais :
//! la décompression ne lit donc jamais deux fois les mêmes octets. Une entrée au nom hors de la
//! forme `[A-Za-z0-9]+.xml`, ou un répertoire, est ignorée et comptée, sans autre contrôle.
//!
//! Une entrée se lit décompressée en mémoire, bornée par sa taille déclarée : la décompression
//! s'arrête au premier octet en trop. Lue en entier, sa taille et sa somme CRC-32 sont vérifiées ;
//! une entrée plus grande que la borne demandée est refusée avant toute décompression. L'en-tête
//! d'une entrée ([`Archive::head`]) en lit les [`HEAD_MAX_BYTES`] premiers octets. Rien n'est écrit
//! sur disque, et aucune erreur ne cite un octet lu.
//!
//! Format (APPNOTE.TXT de PKWARE, §4.3) : en-têtes locaux et données, répertoire central, fin de
//! répertoire (22 octets, plus un commentaire de 65 535 octets au plus). Archives réelles du
//! 2026-11-23 : 5 555 et 2 665 entrées, toutes en `deflate`, drapeaux à zéro, sans commentaire.
//!
//! L'arithmétique sur les décalages lus est saturante : un décalage démesuré sature, puis échoue
//! au contrôle de bornes qui suit (`get`), sans branche qu'aucun test ne pourrait prendre.

use std::{collections::BTreeMap, io::Read};

use flate2::{Crc, read::DeflateDecoder};
use thiserror::Error;

/// Nombre d'entrées au plus (5 555 mesurées chez Particuliers).
pub(crate) const ENTRIES_MAX: usize = 20_000;

/// Taille d'une entrée lue en entière, en octets, au plus (une seule au-delà le 2026-11-23 :
/// `F37096`, une recherche guidée de 18 Mo).
pub(crate) const ENTRY_MAX_BYTES: usize = 2 * 1024 * 1024;

/// Octets lus pour l'en-tête d'une entrée (l'index n'y cherche que les métadonnées, dans les 6
/// premiers Ko mesurés).
pub(crate) const HEAD_MAX_BYTES: usize = 64 * 1024;

/// Fin de répertoire central.
const END_SIGNATURE: u32 = 0x0605_4b50;
const END_LEN: usize = 22;
/// Localisateur de la fin de répertoire zip64, juste avant la fin de répertoire.
const ZIP64_LOCATOR_SIGNATURE: u32 = 0x0706_4b50;
const ZIP64_LOCATOR_LEN: usize = 20;
/// Entrée du répertoire central.
const CENTRAL_SIGNATURE: u32 = 0x0201_4b50;
const CENTRAL_LEN: usize = 46;
/// En-tête local.
const LOCAL_SIGNATURE: u32 = 0x0403_4b50;
const LOCAL_LEN: usize = 30;
/// Longueur maximale du commentaire de la fin de répertoire.
const COMMENT_MAX: usize = 0xFFFF;
/// Drapeaux : chiffrement (bit 0), descripteur de données (bit 3), chiffrement fort (bit 6).
const FLAG_ENCRYPTED: u16 = 0x0001;
const FLAG_DESCRIPTOR: u16 = 0x0008;
const FLAG_STRONG_ENCRYPTION: u16 = 0x0040;
/// Valeurs qui renvoient aux champs zip64.
const ZIP64_COUNT: u16 = 0xFFFF;
const ZIP64_SIZE: u32 = 0xFFFF_FFFF;

/// Refus d'une archive entière. Aucun ne porte d'octet lu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum ArchiveError {
    /// Pas de fin de répertoire central.
    #[error("no end of central directory")]
    NoEnd,
    /// Archive ou entrée zip64.
    #[error("zip64 archives are not read")]
    Zip64,
    /// Archive sur plusieurs disques.
    #[error("multi-disk archives are not read")]
    MultiDisk,
    /// Plus de [`ENTRIES_MAX`] entrées.
    #[error("too many entries")]
    TooManyEntries,
    /// Entrée chiffrée.
    #[error("encrypted entry")]
    Encrypted,
    /// Entrée suivie d'un descripteur de données.
    #[error("entry with a data descriptor")]
    DataDescriptor,
    /// Méthode de compression autre que `deflate` ou aucune.
    #[error("unsupported compression method")]
    Method,
    /// Structure hors des bornes ou signature absente.
    #[error("malformed archive")]
    Malformed,
    /// Le répertoire central et l'en-tête local d'une entrée diffèrent.
    #[error("central directory and local header disagree")]
    Mismatch,
    /// Les données de deux entrées se chevauchent.
    #[error("overlapping entries")]
    Overlap,
    /// Deux entrées de même nom.
    #[error("duplicate entry name")]
    Duplicate,
}

/// Refus de la lecture d'une entrée. Aucun ne porte d'octet lu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum EntryError {
    /// Entrée plus grande que la borne demandée.
    #[error("entry larger than the bound")]
    TooLarge,
    /// Données illisibles, ou dont la taille ou la somme CRC-32 n'est pas celle déclarée.
    #[error("corrupt entry")]
    Corrupt,
}

/// Méthode de compression d'une entrée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    Stored,
    Deflated,
}

/// Une entrée retenue.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    method: Method,
    crc32: u32,
    /// Taille décompressée déclarée.
    size: usize,
    /// Début de l'en-tête local.
    header: usize,
    /// Données compressées : début et fin dans l'archive.
    start: usize,
    end: usize,
}

/// Une archive ouverte : ses octets, et ses entrées retenues par nom.
#[derive(Debug)]
pub(crate) struct Archive {
    bytes: Vec<u8>,
    entries: BTreeMap<String, Entry>,
    ignored: usize,
}

impl Archive {
    /// Ouvre une archive : lit et contrôle la fin de répertoire, le répertoire central et
    /// l'en-tête local de chaque entrée retenue. Rien n'est décompressé.
    ///
    /// # Errors
    ///
    /// [`ArchiveError`] au premier écart avec le sous-ensemble lu.
    pub(crate) fn open(bytes: Vec<u8>) -> Result<Self, ArchiveError> {
        let end = end_of_directory(&bytes)?;
        // `end_of_directory` a vérifié que le répertoire finit à la fin de répertoire : la tranche
        // vide ne sert jamais.
        let directory = bytes.get(end.offset..end.at).unwrap_or_default();
        let mut entries = BTreeMap::new();
        let mut ignored = 0_usize;
        let mut cursor = 0_usize;
        for _ in 0..end.count {
            let (name, central, next) = central_entry(directory, cursor)?;
            cursor = next;
            if !retained(name) {
                ignored = ignored.saturating_add(1);
                continue;
            }
            let entry = checked_entry(&bytes, name, &central, end.offset)?;
            let name = String::from_utf8_lossy(name).into_owned();
            if entries.insert(name, entry).is_some() {
                return Err(ArchiveError::Duplicate);
            }
        }
        if cursor != directory.len() {
            return Err(ArchiveError::Malformed);
        }
        no_overlap(entries.values())?;
        Ok(Self { bytes, entries, ignored })
    }

    /// Noms des entrées retenues, dans l'ordre alphabétique.
    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// Nombre d'entrées retenues.
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Vrai si l'archive ne retient aucune entrée.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Nombre d'entrées ignorées : nom hors de la forme `[A-Za-z0-9]+.xml`, ou répertoire.
    #[must_use]
    pub(crate) const fn ignored(&self) -> usize {
        self.ignored
    }

    /// Taille de l'archive, en octets.
    #[must_use]
    pub(crate) fn byte_len(&self) -> usize {
        self.bytes.len()
    }

    /// L'entrée `name` lue en entier, `None` si l'archive ne la retient pas.
    ///
    /// # Errors
    ///
    /// [`EntryError::TooLarge`] si sa taille déclarée dépasse `bound` (rien n'est décompressé) ;
    /// [`EntryError::Corrupt`] si ses données ne se décompressent pas, ou pas en sa taille et sa
    /// somme CRC-32 déclarées.
    pub(crate) fn read(&self, name: &str, bound: usize) -> Result<Option<Vec<u8>>, EntryError> {
        let Some(entry) = self.entries.get(name) else { return Ok(None) };
        if entry.size > bound {
            return Err(EntryError::TooLarge);
        }
        let out = self.inflate(entry, entry.size.saturating_add(1))?;
        let mut crc = Crc::new();
        crc.update(&out);
        if out.len() != entry.size || crc.sum() != entry.crc32 {
            return Err(EntryError::Corrupt);
        }
        Ok(Some(out))
    }

    /// Les [`HEAD_MAX_BYTES`] premiers octets de l'entrée `name`, `None` si l'archive ne la
    /// retient pas. Une entrée plus courte est lue en entier, et vérifiée comme par
    /// [`Archive::read`].
    ///
    /// # Errors
    ///
    /// [`EntryError::Corrupt`] si ses données ne se décompressent pas, ou s'arrêtent avant la
    /// borne ; et les erreurs d'[`Archive::read`] pour une entrée courte.
    pub(crate) fn head(&self, name: &str) -> Result<Option<Vec<u8>>, EntryError> {
        let Some(entry) = self.entries.get(name) else { return Ok(None) };
        if entry.size <= HEAD_MAX_BYTES {
            return self.read(name, HEAD_MAX_BYTES);
        }
        let out = self.inflate(entry, HEAD_MAX_BYTES)?;
        if out.len() != HEAD_MAX_BYTES {
            return Err(EntryError::Corrupt);
        }
        Ok(Some(out))
    }

    /// Les `limit` premiers octets décompressés de `entry`, moins si ses données s'arrêtent avant.
    fn inflate(&self, entry: &Entry, limit: usize) -> Result<Vec<u8>, EntryError> {
        // Bornes vérifiées à l'ouverture : la tranche vide ne sert jamais.
        let data = self.bytes.get(entry.start..entry.end).unwrap_or_default();
        match entry.method {
            Method::Stored => Ok(data.get(..limit).unwrap_or(data).to_vec()),
            Method::Deflated => {
                let mut out = Vec::new();
                let limit = u64::try_from(limit).unwrap_or(u64::MAX);
                DeflateDecoder::new(data)
                    .take(limit)
                    .read_to_end(&mut out)
                    .map_err(|_| EntryError::Corrupt)?;
                Ok(out)
            }
        }
    }
}

/// Fin de répertoire : où elle commence, où commence le répertoire central, combien d'entrées.
struct EndOfDirectory {
    at: usize,
    offset: usize,
    count: usize,
}

/// Cherche la fin de répertoire en partant de la fin : la dernière signature dont le commentaire
/// finit exactement l'archive.
fn end_of_directory(bytes: &[u8]) -> Result<EndOfDirectory, ArchiveError> {
    let last = bytes.len().checked_sub(END_LEN).ok_or(ArchiveError::NoEnd)?;
    let first = last.saturating_sub(COMMENT_MAX);
    let at = (first..=last)
        .rev()
        .find(|&at| {
            le32(bytes, at) == END_SIGNATURE
                && at
                    .saturating_add(END_LEN)
                    .saturating_add(usize::from(le16(bytes, at.saturating_add(20))))
                    == bytes.len()
        })
        .ok_or(ArchiveError::NoEnd)?;
    let (disk, directory_disk) =
        (le16(bytes, at.saturating_add(4)), le16(bytes, at.saturating_add(6)));
    let (on_disk, total) = (le16(bytes, at.saturating_add(8)), le16(bytes, at.saturating_add(10)));
    let size = le32(bytes, at.saturating_add(12));
    let offset = le32(bytes, at.saturating_add(16));
    let locator = at.checked_sub(ZIP64_LOCATOR_LEN).map(|locator| le32(bytes, locator));
    if [on_disk, total].contains(&ZIP64_COUNT)
        || [size, offset].contains(&ZIP64_SIZE)
        || locator == Some(ZIP64_LOCATOR_SIGNATURE)
    {
        return Err(ArchiveError::Zip64);
    }
    if disk != 0 || directory_disk != 0 || on_disk != total {
        return Err(ArchiveError::MultiDisk);
    }
    let count = usize::from(total);
    if count > ENTRIES_MAX {
        return Err(ArchiveError::TooManyEntries);
    }
    let offset = to_usize(offset);
    if offset.saturating_add(to_usize(size)) != at {
        return Err(ArchiveError::Malformed);
    }
    Ok(EndOfDirectory { at, offset, count })
}

/// Champs d'une entrée du répertoire central que l'en-tête local doit répéter.
struct Central {
    flags: u16,
    method: u16,
    crc32: u32,
    compressed: u32,
    size: u32,
    header: u32,
}

/// Lit l'entrée du répertoire central à `cursor` : son nom, ses champs, et le début de la
/// suivante.
fn central_entry(directory: &[u8], cursor: usize) -> Result<(&[u8], Central, usize), ArchiveError> {
    let record =
        directory.get(cursor..cursor.saturating_add(CENTRAL_LEN)).ok_or(ArchiveError::Malformed)?;
    if le32(record, 0) != CENTRAL_SIGNATURE {
        return Err(ArchiveError::Malformed);
    }
    let name_start = cursor.saturating_add(CENTRAL_LEN);
    let name_end = name_start.saturating_add(usize::from(le16(record, 28)));
    let next = name_end
        .saturating_add(usize::from(le16(record, 30)))
        .saturating_add(usize::from(le16(record, 32)));
    let name = directory.get(name_start..name_end).ok_or(ArchiveError::Malformed)?;
    if next > directory.len() {
        return Err(ArchiveError::Malformed);
    }
    let central = Central {
        flags: le16(record, 8),
        method: le16(record, 10),
        crc32: le32(record, 16),
        compressed: le32(record, 20),
        size: le32(record, 24),
        header: le32(record, 42),
    };
    Ok((name, central, next))
}

/// Vrai pour un nom de la forme `[A-Za-z0-9]+.xml`.
fn retained(name: &[u8]) -> bool {
    name.strip_suffix(b".xml")
        .is_some_and(|stem| !stem.is_empty() && stem.iter().all(u8::is_ascii_alphanumeric))
}

/// Contrôle une entrée retenue : drapeaux, méthode, champs zip64, puis son en-tête local et la
/// place de ses données, qui doivent tenir avant le répertoire central (`directory`).
fn checked_entry(
    bytes: &[u8],
    name: &[u8],
    central: &Central,
    directory: usize,
) -> Result<Entry, ArchiveError> {
    if central.flags & (FLAG_ENCRYPTED | FLAG_STRONG_ENCRYPTION) != 0 {
        return Err(ArchiveError::Encrypted);
    }
    if central.flags & FLAG_DESCRIPTOR != 0 {
        return Err(ArchiveError::DataDescriptor);
    }
    let method = match central.method {
        0 => Method::Stored,
        8 => Method::Deflated,
        _ => return Err(ArchiveError::Method),
    };
    if [central.compressed, central.size, central.header].contains(&ZIP64_SIZE) {
        return Err(ArchiveError::Zip64);
    }
    if method == Method::Stored && central.compressed != central.size {
        return Err(ArchiveError::Malformed);
    }
    let header = to_usize(central.header);
    let record =
        bytes.get(header..header.saturating_add(LOCAL_LEN)).ok_or(ArchiveError::Malformed)?;
    if le32(record, 0) != LOCAL_SIGNATURE {
        return Err(ArchiveError::Malformed);
    }
    let name_start = header.saturating_add(LOCAL_LEN);
    let name_end = name_start.saturating_add(usize::from(le16(record, 26)));
    let start = name_end.saturating_add(usize::from(le16(record, 28)));
    let end = start.saturating_add(to_usize(central.compressed));
    let repeated = le16(record, 6) == central.flags
        && le16(record, 8) == central.method
        && le32(record, 14) == central.crc32
        && le32(record, 18) == central.compressed
        && le32(record, 22) == central.size
        && bytes.get(name_start..name_end) == Some(name);
    if !repeated {
        return Err(ArchiveError::Mismatch);
    }
    if end > directory {
        return Err(ArchiveError::Malformed);
    }
    Ok(Entry { method, crc32: central.crc32, size: to_usize(central.size), header, start, end })
}

/// Refuse deux entrées dont l'en-tête et les données se chevauchent.
fn no_overlap<'a>(entries: impl Iterator<Item = &'a Entry>) -> Result<(), ArchiveError> {
    let mut spans: Vec<(usize, usize)> = entries.map(|entry| (entry.header, entry.end)).collect();
    spans.sort_unstable();
    if spans.iter().zip(spans.iter().skip(1)).any(|((_, end), (next, _))| end > next) {
        return Err(ArchiveError::Overlap);
    }
    Ok(())
}

/// Entier de 16 bits petit-boutiste à `at` ; 0 hors de `bytes` (jamais pour un enregistrement
/// dont la longueur a été vérifiée).
fn le16(bytes: &[u8], at: usize) -> u16 {
    bytes.get(at..).and_then(<[u8]>::first_chunk).map_or(0, |pair| u16::from_le_bytes(*pair))
}

/// Entier de 32 bits petit-boutiste à `at` ; 0 hors de `bytes`.
fn le32(bytes: &[u8], at: usize) -> u32 {
    bytes.get(at..).and_then(<[u8]>::first_chunk).map_or(0, |quad| u32::from_le_bytes(*quad))
}

/// Décalage lu, en `usize` (saturé sur une cible de moins de 32 bits, puis refusé par les bornes).
fn to_usize(value: u32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

#[cfg(test)]
pub(crate) mod tests;
