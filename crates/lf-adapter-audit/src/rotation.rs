//! Rotation du journal actif (06 §11.5, 6 ; 02 §3.5, point 4).
//!
//! Le fichier actif est `mcp-legifrance.jsonl`. Comme `ts` est la première clé de la ligne, la
//! date UTC de sa première ligne occupe les octets 7 à 16 (`{"ts":"AAAA-MM-JJ…`) : on la relit
//! dans les 17 premiers octets, sans parcourir le fichier. Ces fonctions décident et renomment ;
//! l'appelant (`file.rs`) les exécute **sous le verrou** `.mcp-legifrance.lock`.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Nom du journal actif (annexe A §6.6).
pub(crate) const ACTIVE_FILE: &str = "mcp-legifrance.jsonl";

/// Nombre d'octets à relire au début du fichier actif : `{"ts":"` (7) + `AAAA-MM-JJ` (10).
pub(crate) const PREFIX_LEN: usize = 17;

/// État du fichier actif au moment de la décision.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Active {
    /// Taille en octets.
    pub(crate) len: u64,
    /// Date UTC de sa première ligne, si le début du fichier a la forme attendue.
    pub(crate) date: Option<String>,
}

/// La date `AAAA-MM-JJ` d'un début de fichier `{"ts":"AAAA-MM-JJ…`, ou `None` si ce début n'a
/// pas cette forme (fichier étranger, tronqué ou vide).
pub(crate) fn date_of_prefix(prefix: &[u8]) -> Option<&str> {
    const OPENING: &[u8] = br#"{"ts":""#;
    const DATE_LEN: usize = PREFIX_LEN - OPENING.len();
    let date = prefix.strip_prefix(OPENING)?.get(..DATE_LEN)?;
    let well_formed = date.iter().enumerate().all(|(at, byte)| match at {
        4 | 7 => *byte == b'-',
        _ => byte.is_ascii_digit(),
    });
    // Tout octet de la date est ASCII : la conversion ne peut pas échouer.
    well_formed.then(|| std::str::from_utf8(date).ok()).flatten()
}

/// Faut-il archiver le fichier actif avant d'écrire une ligne de `line_len` octets datée de
/// `line_date` ?
///
/// Oui s'il n'est pas vide et que sa taille plus la ligne dépasse `rotate_bytes`, ou que la date
/// de la ligne est **postérieure** à celle de sa première ligne. Une ligne d'une date antérieure
/// s'ajoute au fichier courant : `ts` est l'instant d'entrée dans la façade, et une longue
/// requête de la veille, écrite après minuit par un processus, ne doit pas renvoyer en arrière le
/// fichier qu'un autre processus vient d'ouvrir (le renommage ferait osciller les archives).
pub(crate) fn must_rotate(
    active: &Active,
    line_date: &str,
    line_len: u64,
    rotate_bytes: u64,
) -> bool {
    if active.len == 0 {
        return false;
    }
    let too_big = active.len.saturating_add(line_len) > rotate_bytes;
    // Les dates ISO `AAAA-MM-JJ` se comparent dans l'ordre chronologique.
    let next_day = active.date.as_deref().is_some_and(|first| line_date > first);
    too_big || next_day
}

/// `mcp-legifrance-AAAA-MM-JJ.jsonl`, puis `-1`, `-2`… au premier nom que `taken` ne retient pas.
pub(crate) fn archive_path(dir: &Path, date: &str, taken: impl Fn(&Path) -> bool) -> PathBuf {
    let mut suffix = 0_u32;
    loop {
        let name = if suffix == 0 {
            format!("mcp-legifrance-{date}.jsonl")
        } else {
            format!("mcp-legifrance-{date}-{suffix}.jsonl")
        };
        let path = dir.join(name);
        if !taken(&path) {
            return path;
        }
        suffix = suffix.saturating_add(1);
    }
}

/// Renomme le fichier actif en archive datée de `date`, puis synchronise le répertoire (Unix).
/// Rien n'est jamais supprimé (02 §3.5).
///
/// # Errors
///
/// L'erreur d'E/S du renommage ou de la synchronisation du répertoire.
pub(crate) fn rotate(dir: &Path, date: &str) -> io::Result<PathBuf> {
    // `symlink_metadata` : un lien pendant occupe aussi le nom, que `rename` écraserait.
    let archive = archive_path(dir, date, |path| fs::symlink_metadata(path).is_ok());
    fs::rename(dir.join(ACTIVE_FILE), &archive).and_then(|()| sync_dir(dir)).map(|()| archive)
}

/// Synchronise le répertoire : le renommage ou la création d'un fichier survit à un arrêt brutal.
/// Sans effet hors Unix (Windows n'ouvre pas un répertoire comme un fichier).
///
/// # Errors
///
/// L'erreur d'E/S de l'ouverture ou de la synchronisation.
#[cfg(unix)]
pub(crate) fn sync_dir(dir: &Path) -> io::Result<()> {
    fs::File::open(dir)?.sync_all()
}

/// Version hors Unix : sans effet.
///
/// # Errors
///
/// Jamais.
#[cfg(not(unix))]
#[expect(clippy::unnecessary_wraps, reason = "même signature que la version Unix")]
pub(crate) fn sync_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn active(len: u64, date: Option<&str>) -> Active {
        Active { len, date: date.map(str::to_owned) }
    }

    #[test]
    fn date_of_prefix_reads_the_first_ts() {
        assert_eq!(
            date_of_prefix(br#"{"ts":"2026-10-15T15:09:12.345+00:00","tool""#),
            Some("2026-10-15")
        );
        // Exactement les 17 octets lus par l'appelant.
        assert_eq!(date_of_prefix(br#"{"ts":"2026-10-15"#), Some("2026-10-15"));
    }

    #[test]
    fn date_of_prefix_rejects_anything_else() {
        let rejected: [&[u8]; 7] = [
            b"",
            br#"{"ts":"2026-10-1"#,
            br#"{"tool":"2026-10-15T"#,
            br#"{"ts":"26-10-2015T00"#,
            br#"{"ts":"2026/10/15T00"#,
            br#"{"ts":"2026-1a-15T00"#,
            "{\"ts\":\"2026-10-1é".as_bytes(),
        ];
        for prefix in rejected {
            let shown = String::from_utf8_lossy(prefix);
            assert_eq!(date_of_prefix(prefix), None, "{shown}");
        }
    }

    #[test]
    fn rotation_creates_new_file_after_limit() {
        let state = active(900, Some("2026-10-15"));
        assert!(!must_rotate(&state, "2026-10-15", 100, 1000), "900 + 100 = 1000 tient");
        assert!(must_rotate(&state, "2026-10-15", 101, 1000), "900 + 101 > 1000");
        // Le total ne déborde pas : saturation, donc rotation.
        assert!(must_rotate(&active(u64::MAX - 5, None), "2026-10-15", 10, u64::MAX - 1));
    }

    #[test]
    fn rotation_on_utc_date_change() {
        let state = active(10, Some("2026-10-15"));
        assert!(must_rotate(&state, "2026-10-16", 1, 1_000_000), "jour suivant");
        assert!(!must_rotate(&state, "2026-10-15", 1, 1_000_000), "même jour");
        // Fin de mois et d'année : l'ordre lexicographique des dates ISO est chronologique.
        assert!(must_rotate(&active(10, Some("2026-12-31")), "2027-01-01", 1, 1_000_000));
    }

    #[test]
    fn earlier_date_appends_to_the_current_file() {
        let state = active(10, Some("2026-10-16"));
        assert!(!must_rotate(&state, "2026-10-15", 1, 1_000_000));
    }

    #[test]
    fn empty_or_foreign_file_never_rotates_by_date_and_empty_never_by_size() {
        // Vide : jamais d'archive vide, même si la ligne seule dépasse la limite.
        assert!(!must_rotate(&active(0, None), "2026-10-15", 5000, 100));
        // Début illisible : la date ne décide rien, la taille décide encore.
        assert!(!must_rotate(&active(10, None), "2026-10-15", 1, 1_000_000));
        assert!(must_rotate(&active(10, None), "2026-10-15", 1000, 100));
    }

    #[test]
    fn rotation_suffix_increments() {
        let dir = tempfile::tempdir().unwrap();
        let first = archive_path(dir.path(), "2026-10-15", Path::exists);
        assert_eq!(first, dir.path().join("mcp-legifrance-2026-10-15.jsonl"));
        fs::write(&first, "x").unwrap();
        let second = archive_path(dir.path(), "2026-10-15", Path::exists);
        assert_eq!(second, dir.path().join("mcp-legifrance-2026-10-15-1.jsonl"));
        fs::write(&second, "x").unwrap();
        let third = archive_path(dir.path(), "2026-10-15", Path::exists);
        assert_eq!(third, dir.path().join("mcp-legifrance-2026-10-15-2.jsonl"));
    }

    #[test]
    fn rotation_never_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let active_path = dir.path().join(ACTIVE_FILE);
        let mut archives = Vec::new();
        for n in 0..3 {
            fs::write(&active_path, format!("contenu {n}")).unwrap();
            archives.push(rotate(dir.path(), "2026-10-15").unwrap());
            assert!(!active_path.exists(), "le fichier actif est renommé, pas copié");
        }
        let contents: Vec<_> = archives.iter().map(|p| fs::read_to_string(p).unwrap()).collect();
        assert_eq!(contents, ["contenu 0", "contenu 1", "contenu 2"]);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
    }

    #[test]
    fn rotate_fails_without_an_active_file() {
        let dir = tempfile::tempdir().unwrap();
        let error = rotate(dir.path(), "2026-10-15").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn sync_dir_accepts_a_directory_and_reports_a_missing_one() {
        let dir = tempfile::tempdir().unwrap();
        sync_dir(dir.path()).unwrap();
        #[cfg(unix)]
        assert_eq!(
            sync_dir(&dir.path().join("absent")).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}
