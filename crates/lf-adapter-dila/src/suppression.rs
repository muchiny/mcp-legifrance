//! Listes de suppression d'un incrément (rapport 10 §6, annexe C §7.4-4).
//!
//! - `liste_suppression_legi.dat` : un chemin de fichier par ligne (LF), sans le préfixe
//!   d'incrément et sans `.xml`. L'annexe C parle de chemins « séparés par des espaces » : on
//!   accepte les deux séparateurs.
//! - `liste_suppression_legi_dossier.dat` : une ligne `<dossier de texte> D`.
//!
//! Les deux fichiers sont les premiers de l'archive mais **s'appliquent après son contenu**. Une
//! suppression vise un **chemin**, jamais un identifiant seul : 57 des 58 suppressions du
//! 2026-10-28 sont des déplacements, ré-ajoutés sous un autre répertoire avec le même identifiant.
//!
//! Une ligne illisible ou qui ne désigne pas un document du corpus est comptée, jamais fatale. Une
//! suppression d'un chemin absent du magasin est sans effet (rapport 10 §6, point 4).

use crate::path::{DocPath, doc_for_key, text_dir_for};

/// Suppressions lues dans un incrément, dans l'ordre des fichiers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Deletions {
    /// Fichiers à retirer, par clé.
    pub files: Vec<DocPath>,
    /// Dossiers de texte à retirer en entier (`text_dir`).
    pub dirs: Vec<String>,
    /// Lignes ou jetons ignorés faute d'être un chemin du corpus.
    pub invalid: u64,
}

impl Deletions {
    /// Ajoute les chemins de `liste_suppression_legi.dat`.
    pub fn read_file_list(&mut self, data: &[u8]) {
        for line in data.split(|b| *b == b'\n') {
            let Ok(line) = std::str::from_utf8(line) else {
                self.invalid = self.invalid.saturating_add(1);
                continue;
            };
            for token in line.split_ascii_whitespace() {
                let key = token.strip_suffix(".xml").unwrap_or(token);
                match doc_for_key(key) {
                    Some(doc) => self.files.push(doc),
                    None => self.invalid = self.invalid.saturating_add(1),
                }
            }
        }
    }

    /// Ajoute les lignes `<dossier> D` de `liste_suppression_legi_dossier.dat`.
    pub fn read_dir_list(&mut self, data: &[u8]) {
        for line in data.split(|b| *b == b'\n') {
            let Ok(line) = std::str::from_utf8(line) else {
                self.invalid = self.invalid.saturating_add(1);
                continue;
            };
            let mut tokens = line.split_ascii_whitespace();
            match (tokens.next(), tokens.next(), tokens.next()) {
                (None, _, _) => {}
                (Some(dir), Some("D"), None) => match text_dir_for(dir) {
                    Some(dir) => self.dirs.push(dir),
                    None => self.invalid = self.invalid.saturating_add(1),
                },
                _ => self.invalid = self.invalid.saturating_add(1),
            }
        }
    }

    /// Vrai si l'incrément ne supprime rien.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.dirs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIR: &str = "legi/global/code_et_TNC_en_vigueur/TNC_en_vigueur/JORF/TEXT/00/00/24/58/38/JORFTEXT000024583863";
    const FILE: &str = "legi/global/code_et_TNC_en_vigueur/TNC_en_vigueur/JORF/TEXT/00/00/24/58/38/JORFTEXT000024583863/article/LEGI/ARTI/00/00/30/86/06/LEGIARTI000030860648";
    const OTHER: &str = "legi/global/code_et_TNC_en_vigueur/TNC_en_vigueur/JORF/TEXT/00/00/24/58/38/JORFTEXT000024583863/texte/version/LEGITEXT000024586520";

    #[test]
    fn file_list_is_read_line_by_line_in_order() {
        let mut deletions = Deletions::default();
        deletions.read_file_list(format!("{FILE}\n{OTHER}\n").as_bytes());
        let keys: Vec<&str> = deletions.files.iter().map(|d| d.key.as_str()).collect();
        assert_eq!(keys, [FILE, OTHER]);
        assert_eq!(deletions.files[0].id, "LEGIARTI000030860648");
        assert_eq!(deletions.invalid, 0);
        assert!(!deletions.is_empty());
    }

    #[test]
    fn file_list_tolerates_crlf_space_separators_and_a_xml_suffix() {
        let mut deletions = Deletions::default();
        deletions.read_file_list(format!("{FILE}\r\n\r\n  {OTHER}.xml {FILE}").as_bytes());
        assert_eq!(deletions.files.len(), 3);
        assert_eq!(deletions.invalid, 0);
    }

    #[test]
    fn file_list_counts_what_is_not_a_corpus_path() {
        let mut deletions = Deletions::default();
        deletions.read_file_list(b"legi/global/n/importe/quoi\n../etc/passwd\n\xff\xfe\n");
        assert_eq!(deletions.files.len(), 0);
        assert_eq!(deletions.invalid, 3);
        assert!(deletions.is_empty());
    }

    #[test]
    fn dir_list_reads_the_text_folder_before_the_d_marker() {
        let mut deletions = Deletions::default();
        deletions.read_dir_list(format!("{DIR} D\n").as_bytes());
        assert_eq!(deletions.dirs, [DIR]);
        assert_eq!(deletions.invalid, 0);
        assert!(!deletions.is_empty());
    }

    #[test]
    fn dir_list_counts_malformed_lines_and_skips_blank_ones() {
        let mut deletions = Deletions::default();
        let data = format!(
            "\n{DIR}\n{DIR} X\n{DIR} D extra\nlegi/global/code_et_TNC_en_vigueur D\n\r\n{DIR} D\r\n"
        );
        deletions.read_dir_list(data.as_bytes());
        assert_eq!(deletions.dirs, [DIR]);
        assert_eq!(deletions.invalid, 4);
        deletions.read_dir_list(b"\xff D\n");
        assert_eq!(deletions.invalid, 5);
    }

    #[test]
    fn a_fresh_value_deletes_nothing() {
        assert!(Deletions::default().is_empty());
    }
}
