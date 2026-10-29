//! Noms des archives DILA du fonds LEGI (annexe C §7.3, rapport 10 §4).
//!
//! - global : `Freemium_legi_global_AAAAMMJJ-HHMMSS.tar.gz` ;
//! - incrément : `LEGI_AAAAMMJJ-HHMMSS.tar.gz`.
//!
//! L'horodatage est en heure de Paris, à champs de largeur fixe : l'ordre lexicographique des
//! noms est l'ordre chronologique, sans conversion de fuseau (rapport 10 §4.2).

use std::{fmt, str::FromStr};

use lf_domain::date::CivilDate;

/// Nombre fait de chiffres ASCII seulement (ni signe, ni espace, que `parse` accepterait).
fn number<T: FromStr>(digits: &str) -> Option<T> {
    digits.bytes().all(|b| b.is_ascii_digit()).then(|| digits.parse().ok()).flatten()
}

/// Type d'une archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArchiveKind {
    /// Dump complet : il remplace tout ce qui précède.
    Global,
    /// Incrément quotidien : il s'applique par-dessus le global.
    Increment,
}

/// Horodatage `AAAAMMJJ-HHMMSS` d'une archive.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArchiveStamp {
    text: String,
    date: CivilDate,
}

impl ArchiveStamp {
    /// Lit un horodatage `AAAAMMJJ-HHMMSS` ; `None` si la forme ou la date est invalide.
    pub fn parse(text: &str) -> Option<Self> {
        // `is_ascii` + les longueurs rendent les découpes ci-dessous toujours valides
        let (day, time) = text.split_once('-').filter(|_| text.is_ascii())?;
        if day.len() != 8 || time.len() != 6 {
            return None;
        }
        let (Some(year), Some(month), Some(day)) =
            (number::<u16>(&day[..4]), number::<u8>(&day[4..6]), number::<u8>(&day[6..]))
        else {
            return None;
        };
        let (Some(hour), Some(minute), Some(second)) =
            (number::<u8>(&time[..2]), number::<u8>(&time[2..4]), number::<u8>(&time[4..]))
        else {
            return None;
        };
        let date = CivilDate::new(year, month, day)?;
        (hour < 24 && minute < 60 && second < 60).then(|| Self { text: text.to_owned(), date })
    }

    /// Le texte de l'horodatage, tel qu'il figure dans le nom de l'archive.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Le jour civil de l'horodatage (heure de Paris), pour mesurer la fraîcheur du corpus.
    pub fn date(&self) -> CivilDate {
        self.date
    }
}

impl fmt::Display for ArchiveStamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// Nom d'une archive LEGI : type, horodatage et nom de fichier d'origine.
///
/// L'ordre est chronologique (horodatage), puis le global avant l'incrément de même horodatage.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArchiveName {
    stamp: ArchiveStamp,
    kind: ArchiveKind,
    file_name: String,
}

impl ArchiveName {
    /// Lit un nom de fichier (casse indifférente, comme l'expression de legi.py) ; `None` si ce
    /// n'est pas une archive LEGI.
    pub fn parse(file_name: &str) -> Option<Self> {
        let lower = file_name.to_ascii_lowercase();
        let stem = lower.strip_suffix(".tar.gz")?;
        let (kind, stamp) = match stem.strip_prefix("freemium_legi_global_") {
            Some(stamp) => (ArchiveKind::Global, stamp),
            None => (ArchiveKind::Increment, stem.strip_prefix("legi_")?),
        };
        Some(Self { stamp: ArchiveStamp::parse(stamp)?, kind, file_name: file_name.to_owned() })
    }

    /// Nom canonique d'une archive du type et de l'horodatage donnés.
    pub fn new(kind: ArchiveKind, stamp: ArchiveStamp) -> Self {
        let file_name = match kind {
            ArchiveKind::Global => format!("Freemium_legi_global_{stamp}.tar.gz"),
            ArchiveKind::Increment => format!("LEGI_{stamp}.tar.gz"),
        };
        Self { stamp, kind, file_name }
    }

    /// Type de l'archive.
    pub fn kind(&self) -> ArchiveKind {
        self.kind
    }

    /// Horodatage de l'archive.
    pub fn stamp(&self) -> &ArchiveStamp {
        &self.stamp
    }

    /// Nom de fichier, tel qu'il figure dans le répertoire des archives.
    pub fn file_name(&self) -> &str {
        &self.file_name
    }
}

impl fmt::Display for ArchiveName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.file_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(text: &str) -> ArchiveStamp {
        ArchiveStamp::parse(text).unwrap()
    }

    #[test]
    fn stamp_parses_the_real_dila_shapes() {
        let s = stamp("20261028-212713");
        assert_eq!(s.as_str(), "20261028-212713");
        assert_eq!(s.to_string(), "20261028-212713");
        assert_eq!(s.date(), CivilDate::new(2026, 10, 28).unwrap());
    }

    #[test]
    fn stamp_rejects_bad_shapes_and_impossible_instants() {
        for bad in [
            "",
            "20261028",
            "20261028-21271",
            "20261028-2127133",
            "2026102-8212713",
            "2026102a-212713",
            "20261028-21a713",
            "20261328-212713",
            "20260230-212713",
            "20261028-242713",
            "20261028-216013",
            "20261028-212760",
            "00001028-212713",
            "2026+028-212713",
            "２０２６1028-212713",
            "20261028-２１2713",
        ] {
            assert_eq!(ArchiveStamp::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn global_and_increment_names_are_recognised() {
        let global = ArchiveName::parse("Freemium_legi_global_20250713-140000.tar.gz").unwrap();
        assert_eq!(global.kind(), ArchiveKind::Global);
        assert_eq!(global.stamp().as_str(), "20250713-140000");
        assert_eq!(global.file_name(), "Freemium_legi_global_20250713-140000.tar.gz");
        assert_eq!(global.to_string(), "Freemium_legi_global_20250713-140000.tar.gz");
        let increment = ArchiveName::parse("LEGI_20261028-212713.tar.gz").unwrap();
        assert_eq!(increment.kind(), ArchiveKind::Increment);
        assert_eq!(increment.stamp().date(), CivilDate::new(2026, 10, 28).unwrap());
    }

    #[test]
    fn names_are_case_insensitive_and_keep_the_original_spelling() {
        let name = ArchiveName::parse("legi_20261028-212713.TAR.GZ").unwrap();
        assert_eq!(name.kind(), ArchiveKind::Increment);
        assert_eq!(name.file_name(), "legi_20261028-212713.TAR.GZ");
    }

    #[test]
    fn other_funds_and_other_files_are_not_legi_archives() {
        for other in [
            "KALI_20261028-212713.tar.gz",
            "Freemium_kali_global_20250713-140000.tar.gz",
            "LEGI_20261028-212713.tar",
            "LEGI_20261028-212713.zip",
            "LEGI_20261028.tar.gz",
            "LEGI_.tar.gz",
            "Freemium_legi_global_.tar.gz",
            "registry.tsv",
            "",
        ] {
            assert_eq!(ArchiveName::parse(other), None, "{other}");
        }
    }

    #[test]
    fn canonical_names_round_trip_through_parse() {
        let global = ArchiveName::new(ArchiveKind::Global, stamp("20250713-140000"));
        assert_eq!(global.file_name(), "Freemium_legi_global_20250713-140000.tar.gz");
        assert_eq!(ArchiveName::parse(global.file_name()), Some(global));
        let increment = ArchiveName::new(ArchiveKind::Increment, stamp("20261028-212713"));
        assert_eq!(increment.file_name(), "LEGI_20261028-212713.tar.gz");
        assert_eq!(ArchiveName::parse(increment.file_name()), Some(increment));
    }

    #[test]
    fn names_sort_chronologically_with_the_global_first_on_a_tie() {
        let mut names: Vec<ArchiveName> = [
            "LEGI_20261028-212713.tar.gz",
            "LEGI_20250713-205013.tar.gz",
            "LEGI_20250713-140000.tar.gz",
            "Freemium_legi_global_20250713-140000.tar.gz",
            "LEGI_20260223-224003.tar.gz",
        ]
        .into_iter()
        .map(|n| ArchiveName::parse(n).unwrap())
        .collect();
        names.sort();
        let sorted: Vec<&str> = names.iter().map(ArchiveName::file_name).collect();
        assert_eq!(
            sorted,
            [
                "Freemium_legi_global_20250713-140000.tar.gz",
                "LEGI_20250713-140000.tar.gz",
                "LEGI_20250713-205013.tar.gz",
                "LEGI_20260223-224003.tar.gz",
                "LEGI_20261028-212713.tar.gz",
            ]
        );
    }
}
