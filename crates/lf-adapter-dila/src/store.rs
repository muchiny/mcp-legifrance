//! Magasin de documents : la sortie de l'ingestion (rapport 10 §10.1).
//!
//! [`DocStore`] est ce que l'ingestion exige d'un magasin. V3-08 l'implémentera pour l'index
//! tantivy ; [`MemoryStore`] en est l'implémentation de référence en mémoire, qui sert aux tests
//! et aux contrôles de contrat sur de petits dumps.
//!
//! Le magasin est indexé par le **chemin** du document ([`DocPath::key`]), jamais par son
//! identifiant : un même identifiant peut avoir deux copies (13 138 articles sont à la fois sous
//! `en_vigueur` et sous `non_vigueur` dans le global, rapport 10 §5.4), et un déplacement entre
//! répertoires est une suppression suivie d'un ajout sous un autre chemin. [`preferred_copy`]
//! applique la règle de lecture de ces doublons.

use std::collections::BTreeMap;

use crate::{
    error::StoreError,
    model::Document,
    path::{DocPath, Validity},
};

/// Un document rangé : son chemin, la date de modification du membre et son contenu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDoc {
    /// Chemin classé du membre.
    pub path: DocPath,
    /// Date de modification du membre, en secondes Unix (0 si l'en-tête tar est illisible).
    pub mtime: u64,
    /// Le document analysé.
    pub document: Document,
}

/// Ce que l'ingestion exige d'un magasin.
///
/// Toutes les opérations sont **idempotentes** : un incrément rejoué donne le même état
/// (rapport 10 §4.3, un incrément remplace des fichiers par chemin).
pub trait DocStore {
    /// Vide le magasin (reconstruction sur un nouveau global).
    ///
    /// # Errors
    ///
    /// [`StoreError`] si le magasin ne peut pas être vidé.
    fn reset(&mut self) -> Result<(), StoreError>;

    /// Range un document sous sa clé, en remplaçant l'ancien contenu de cette clé.
    ///
    /// # Errors
    ///
    /// [`StoreError`] si le document ne peut pas être écrit.
    fn put(&mut self, doc: StoredDoc) -> Result<(), StoreError>;

    /// Retire le document de clé `key` ; rend `true` s'il existait. Une clé absente n'est pas une
    /// erreur (les fixtures ne contiennent qu'un sous-ensemble, rapport 10 §6).
    ///
    /// # Errors
    ///
    /// [`StoreError`] si le retrait échoue.
    fn remove_file(&mut self, key: &str) -> Result<bool, StoreError>;

    /// Retire tous les documents dont [`DocPath::text_dir`] vaut `text_dir` ; rend leur nombre.
    ///
    /// # Errors
    ///
    /// [`StoreError`] si le retrait échoue.
    fn remove_text_dir(&mut self, text_dir: &str) -> Result<u64, StoreError>;

    /// Rend durables les écritures en attente. L'ingestion l'appelle une fois par archive, avant
    /// que le registre ne la déclare appliquée : un magasin qui groupe ses écritures (l'index
    /// tantivy) ne perd ainsi jamais une archive que le registre croit appliquée. Sans effet
    /// pour un magasin qui écrit tout de suite.
    ///
    /// # Errors
    ///
    /// [`StoreError`] si les écritures ne peuvent pas être rendues durables.
    fn commit(&mut self) -> Result<(), StoreError> {
        Ok(())
    }
}

/// Magasin en mémoire, trié par clé.
#[derive(Debug, Default)]
pub struct MemoryStore {
    docs: BTreeMap<String, StoredDoc>,
}

impl MemoryStore {
    /// Magasin vide.
    pub fn new() -> Self {
        Self::default()
    }

    /// Nombre de documents (un par chemin).
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    /// Vrai si le magasin est vide.
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// Le document de clé `key`.
    pub fn get(&self, key: &str) -> Option<&StoredDoc> {
        self.docs.get(key)
    }

    /// Tous les documents, par clé croissante.
    pub fn iter(&self) -> impl Iterator<Item = &StoredDoc> {
        self.docs.values()
    }

    /// Toutes les copies d'un identifiant (une par chemin).
    pub fn copies(&self, id: &str) -> Vec<&StoredDoc> {
        self.docs.values().filter(|doc| doc.path.id == id).collect()
    }

    /// La copie à lire pour un identifiant, d'après [`preferred_copy`].
    pub fn preferred(&self, id: &str) -> Option<&StoredDoc> {
        preferred_copy(self.copies(id))
    }
}

impl DocStore for MemoryStore {
    fn reset(&mut self) -> Result<(), StoreError> {
        self.docs.clear();
        Ok(())
    }

    fn put(&mut self, doc: StoredDoc) -> Result<(), StoreError> {
        self.docs.insert(doc.path.key.clone(), doc);
        Ok(())
    }

    fn remove_file(&mut self, key: &str) -> Result<bool, StoreError> {
        Ok(self.docs.remove(key).is_some())
    }

    fn remove_text_dir(&mut self, text_dir: &str) -> Result<u64, StoreError> {
        let before = self.docs.len();
        self.docs.retain(|_, doc| doc.path.text_dir != text_dir);
        Ok(u64::try_from(before - self.docs.len()).unwrap_or(u64::MAX))
    }
}

/// Choisit la copie à lire parmi celles d'un même identifiant : la plus récente par date de
/// modification du membre, et à égalité celle du répertoire `en_vigueur` (règle de legi.py,
/// rapport 10 §5.4 et §10.1). À égalité complète, la première rencontrée est gardée.
pub fn preferred_copy<'a>(
    copies: impl IntoIterator<Item = &'a StoredDoc>,
) -> Option<&'a StoredDoc> {
    copies.into_iter().reduce(|best, copy| {
        let rank = |doc: &StoredDoc| (doc.mtime, doc.path.validity == Validity::EnVigueur);
        if rank(copy) > rank(best) { copy } else { best }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{Section, TextContext},
        path::doc_for_key,
        testing::article_member,
    };

    const CID: &str = "LEGITEXT000006069445";
    const ID: &str = "LEGIARTI000006284410";

    fn stored(path: &str, mtime: u64) -> StoredDoc {
        let doc_path = doc_for_key(path.trim_end_matches(".xml")).unwrap();
        let document = Document::Section(Section {
            id: doc_path.id.clone(),
            title: Some(path.to_owned()),
            context: TextContext::default(),
            structure: Vec::new(),
        });
        StoredDoc { path: doc_path, mtime, document }
    }

    fn en_vigueur(mtime: u64) -> StoredDoc {
        stored(&article_member("TNC_en_vigueur", CID, ID), mtime)
    }

    fn non_vigueur(mtime: u64) -> StoredDoc {
        stored(&article_member("TNC_non_vigueur", CID, ID), mtime)
    }

    #[test]
    fn put_replaces_by_key_and_get_finds_it() {
        let mut store = MemoryStore::new();
        assert!(store.is_empty());
        store.put(en_vigueur(1)).unwrap();
        store.put(en_vigueur(2)).unwrap();
        assert_eq!(store.len(), 1);
        let key = en_vigueur(0).path.key;
        assert_eq!(store.get(&key).unwrap().mtime, 2);
        assert!(store.get("legi/global/absent").is_none());
        assert_eq!(store.iter().count(), 1);
    }

    #[test]
    fn the_same_id_under_two_directories_is_two_documents() {
        let mut store = MemoryStore::new();
        store.put(en_vigueur(1)).unwrap();
        store.put(non_vigueur(2)).unwrap();
        assert_eq!(store.len(), 2);
        assert_eq!(store.copies(ID).len(), 2);
        assert_eq!(store.copies("LEGIARTI000000000000").len(), 0);
    }

    #[test]
    fn remove_file_reports_whether_the_key_existed() {
        let mut store = MemoryStore::new();
        store.put(en_vigueur(1)).unwrap();
        let key = en_vigueur(0).path.key;
        assert!(store.remove_file(&key).unwrap());
        assert!(!store.remove_file(&key).unwrap());
        assert!(store.is_empty());
    }

    #[test]
    fn remove_text_dir_removes_the_whole_subtree_and_nothing_else() {
        let mut store = MemoryStore::new();
        store.put(en_vigueur(1)).unwrap();
        store.put(non_vigueur(1)).unwrap();
        let other = stored(
            &article_member("TNC_en_vigueur", "LEGITEXT000000000009", "LEGIARTI000000000009"),
            1,
        );
        store.put(other).unwrap();
        let dir = en_vigueur(0).path.text_dir;
        assert_eq!(store.remove_text_dir(&dir).unwrap(), 1);
        assert_eq!(store.len(), 2, "same text under non_vigueur is another folder");
        assert_eq!(store.remove_text_dir(&dir).unwrap(), 0);
    }

    #[test]
    fn reset_empties_the_store() {
        let mut store = MemoryStore::new();
        store.put(en_vigueur(1)).unwrap();
        store.reset().unwrap();
        assert!(store.is_empty());
    }

    #[test]
    fn the_newest_copy_wins_and_en_vigueur_wins_a_tie() {
        // doublon réel LEGIARTI000006284410 : 2023-11-21 sous en_vigueur, 2023-12-31 sous non_vigueur
        let mut store = MemoryStore::new();
        store.put(en_vigueur(1_700_598_840)).unwrap();
        store.put(non_vigueur(1_704_058_080)).unwrap();
        let best = store.preferred(ID).unwrap();
        assert_eq!(best.path.validity, Validity::NonVigueur);

        let mut tie = MemoryStore::new();
        tie.put(non_vigueur(5)).unwrap();
        tie.put(en_vigueur(5)).unwrap();
        assert_eq!(tie.preferred(ID).unwrap().path.validity, Validity::EnVigueur);

        let mut alone = MemoryStore::new();
        alone.put(non_vigueur(5)).unwrap();
        assert_eq!(alone.preferred(ID).unwrap().mtime, 5);
        assert!(alone.preferred("LEGIARTI000000000000").is_none());
    }

    #[test]
    fn preferred_copy_keeps_the_first_of_two_equal_copies() {
        let (a, b) = (non_vigueur(5), non_vigueur(5));
        let first = preferred_copy([&a, &b]).unwrap();
        assert!(std::ptr::eq(first, &raw const a));
    }
}
