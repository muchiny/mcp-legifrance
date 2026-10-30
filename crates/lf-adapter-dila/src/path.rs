//! Classement des chemins de membres d'une archive LEGI (rapport 10 §5.2 et §10).
//!
//! ```text
//! [<AAAAMMJJ-HHMMSS>/]legi/global/code_et_TNC_(en|non)_vigueur/(code|TNC)_(en|non)_vigueur/
//!     (JORF|LEGI)/TEXT/NN/NN/NN/NN/NN/<CID>/
//!         article/LEGI/ARTI/NN/NN/NN/NN/NN/<LEGIARTI…>.xml
//!         section_ta/LEGI/SCTA/NN/NN/NN/NN/NN/<LEGISCTA…>.xml
//!         texte/struct/<LEGITEXT…>.xml
//!         texte/version/<LEGITEXT…>.xml
//! ```
//!
//! L'identité d'un fichier est son chemin sans préfixe d'incrément et sans `.xml` (la « clé »).
//! Une suppression vise une clé, jamais un identifiant : 57 des 58 suppressions de l'incrément du
//! 2026-10-28 sont des déplacements entre `en_vigueur` et `non_vigueur` (rapport 10 §6).
//!
//! Un chemin est refusé s'il est absolu, s'il contient `..`, `.`, un composant vide, une
//! barre oblique inverse ou un octet nul, s'il n'est pas de l'UTF-8 ou s'il dépasse
//! [`MAX_PATH_BYTES`] (le maximum observé est de 159 octets).

use crate::archive::ArchiveStamp;

/// Longueur maximale admise d'un chemin de membre, en octets (159 observés, rapport 10 §3.2).
pub const MAX_PATH_BYTES: usize = 512;

/// Nom de la liste de suppression des fichiers, à la racine de l'archive (hors préfixe).
pub const FILE_LIST_NAME: &str = "liste_suppression_legi.dat";

/// Nom de la liste de suppression des dossiers de texte, à la racine de l'archive.
pub const DIR_LIST_NAME: &str = "liste_suppression_legi_dossier.dat";

/// Nature du document, déduite de son dossier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DocKind {
    /// `ARTICLE` : une version d'article.
    Article,
    /// `SECTION_TA` : une section.
    Section,
    /// `TEXTELR` (`texte/struct`) : la structure d'un texte.
    TextStruct,
    /// `TEXTE_VERSION` (`texte/version`) : une version d'un texte.
    TextVersion,
}

/// Famille de textes du dossier de rang 4 : `code_*` ou `TNC_*` (textes non codifiés).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tree {
    /// Les codes.
    Code,
    /// Les textes non codifiés.
    Tnc,
}

/// Répertoire de vigueur. Il n'est pas l'état de l'article : seuls 47 % des articles du
/// répertoire `en_vigueur` sont `VIGUEUR` (rapport 10 §5.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Validity {
    /// `en_vigueur`.
    EnVigueur,
    /// `non_vigueur`.
    NonVigueur,
}

/// Fonds d'origine du texte, d'après le dossier `JORF` ou `LEGI`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// Texte du Journal officiel consolidé (`JORF/TEXT`).
    Jorf,
    /// Texte propre à LEGI (`LEGI/TEXT`).
    Legi,
}

/// Chemin classé d'un document de l'arbre `legi/global`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DocPath {
    /// Chemin sans préfixe d'incrément et sans `.xml` : l'identité du fichier.
    pub key: String,
    /// Dossier du texte (jusqu'au `<CID>` inclus) : la cible d'une ligne de la liste des dossiers.
    pub text_dir: String,
    /// Identifiant du texte parent (`JORFTEXT…` ou `LEGITEXT…`).
    pub cid: String,
    /// Identifiant du document, tiré du nom de fichier (`LEGIARTI…`, `LEGISCTA…`, `LEGITEXT…`).
    pub id: String,
    /// Nature du document.
    pub kind: DocKind,
    /// Famille `code` ou `TNC`.
    pub tree: Tree,
    /// Répertoire de vigueur.
    pub validity: Validity,
    /// Fonds d'origine du texte.
    pub source: Source,
}

/// Raison pour laquelle un membre est laissé de côté sans être une erreur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ignored {
    /// Arbre `eli/` : index de versions et liens, inutiles au corpus de textes.
    Eli,
    /// Tout autre chemin (autre fonds, profondeur ou nom inattendus).
    Other,
}

/// Classement d'un membre d'archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Member {
    /// Un document XML du corpus.
    Doc(DocPath),
    /// `liste_suppression_legi.dat`.
    FileList,
    /// `liste_suppression_legi_dossier.dat`.
    DirList,
    /// Un membre sans effet sur le corpus.
    Ignored(Ignored),
}

/// Motif de refus d'un chemin de membre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathReject {
    /// Le chemin n'est pas de l'UTF-8.
    NonUtf8,
    /// Le chemin dépasse [`MAX_PATH_BYTES`].
    TooLong,
    /// Le chemin est absolu ou contient `..`, `.`, un composant vide, `\` ou un octet nul.
    Unsafe,
}

/// Découpe un chemin en composants après les contrôles de sécurité.
fn split_safe(raw: &[u8]) -> Result<Vec<&str>, PathReject> {
    if raw.len() > MAX_PATH_BYTES {
        return Err(PathReject::TooLong);
    }
    let text = std::str::from_utf8(raw).map_err(|_| PathReject::NonUtf8)?;
    let text = text.strip_suffix('/').unwrap_or(text);
    let parts: Vec<&str> = text.split('/').collect();
    let safe = parts.iter().all(|p| {
        !p.is_empty() && *p != "." && *p != ".." && !p.contains('\\') && !p.contains('\0')
    });
    if safe { Ok(parts) } else { Err(PathReject::Unsafe) }
}

/// Retire le préfixe d'incrément `AAAAMMJJ-HHMMSS/` s'il y en a un.
fn strip_stamp<'a>(parts: &'a [&'a str]) -> &'a [&'a str] {
    match parts.split_first() {
        Some((first, rest)) if ArchiveStamp::parse(first).is_some() => rest,
        _ => parts,
    }
}

/// Deux chiffres exactement (un niveau `NN` de l'arborescence).
fn is_level(part: &str) -> bool {
    part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit())
}

/// Identifiant à 8 lettres de préfixe imposé suivies de 12 chiffres.
fn has_id_shape(id: &str, prefix: &str) -> bool {
    id.strip_prefix(prefix).is_some_and(|n| n.len() == 12 && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Identifiant de texte (`JORFTEXT…` ou `LEGITEXT…`) : 20 caractères.
fn is_text_id(id: &str) -> bool {
    has_id_shape(id, "JORFTEXT") || has_id_shape(id, "LEGITEXT")
}

/// Début d'un chemin de document : tout jusqu'au `<CID>` inclus.
struct Head<'a> {
    tree: Tree,
    validity: Validity,
    source: Source,
    cid: &'a str,
    rest: &'a [&'a str],
}

/// Reconnaît `legi/global/<groupe>/<sous-dossier>/<JORF|LEGI>/TEXT/NN×5/<CID>/…`.
fn head<'a>(parts: &'a [&'a str]) -> Option<Head<'a>> {
    let ["legi", "global", group, sub, source, "TEXT", n1, n2, n3, n4, n5, cid, rest @ ..] = parts
    else {
        return None;
    };
    let (tree, validity) = match *sub {
        "code_en_vigueur" => (Tree::Code, Validity::EnVigueur),
        "code_non_vigueur" => (Tree::Code, Validity::NonVigueur),
        "TNC_en_vigueur" => (Tree::Tnc, Validity::EnVigueur),
        "TNC_non_vigueur" => (Tree::Tnc, Validity::NonVigueur),
        _ => return None,
    };
    let expected_group = match validity {
        Validity::EnVigueur => "code_et_TNC_en_vigueur",
        Validity::NonVigueur => "code_et_TNC_non_vigueur",
    };
    let source = match *source {
        "JORF" => Source::Jorf,
        "LEGI" => Source::Legi,
        _ => return None,
    };
    let levels = [n1, n2, n3, n4, n5].into_iter().all(|n| is_level(n));
    (*group == expected_group && levels && is_text_id(cid)).then_some(Head {
        tree,
        validity,
        source,
        cid,
        rest,
    })
}

/// Nature, préfixe d'identifiant et nom de fichier d'après la fin du chemin.
fn tail<'a>(rest: &[&'a str]) -> Option<(DocKind, &'static str, &'a str)> {
    match rest {
        ["article", "LEGI", "ARTI", n1, n2, n3, n4, n5, file]
            if [n1, n2, n3, n4, n5].into_iter().all(|n| is_level(n)) =>
        {
            Some((DocKind::Article, "LEGIARTI", *file))
        }
        ["section_ta", "LEGI", "SCTA", n1, n2, n3, n4, n5, file]
            if [n1, n2, n3, n4, n5].into_iter().all(|n| is_level(n)) =>
        {
            Some((DocKind::Section, "LEGISCTA", *file))
        }
        ["texte", "struct", file] => Some((DocKind::TextStruct, "LEGITEXT", *file)),
        ["texte", "version", file] => Some((DocKind::TextVersion, "LEGITEXT", *file)),
        _ => None,
    }
}

/// Joint les `count` premiers composants.
fn join_first(parts: &[&str], count: usize) -> String {
    parts.iter().copied().take(count).collect::<Vec<_>>().join("/")
}

/// Classe un document de l'arbre `legi/global` (composants sans préfixe d'incrément).
fn doc(parts: &[&str]) -> Option<DocPath> {
    let head = head(parts)?;
    let (kind, prefix, file) = tail(head.rest)?;
    let id = file.strip_suffix(".xml").filter(|id| has_id_shape(id, prefix))?;
    // La clé est le chemin dont le dernier composant perd son `.xml`.
    let dirs = join_first(parts, parts.len().saturating_sub(1));
    Some(DocPath {
        key: format!("{dirs}/{id}"),
        text_dir: join_first(parts, parts.len().saturating_sub(head.rest.len())),
        cid: head.cid.to_owned(),
        id: id.to_owned(),
        kind,
        tree: head.tree,
        validity: head.validity,
        source: head.source,
    })
}

/// Classe le chemin d'un membre d'archive tel que le rend le crate `tar` (`path_bytes`).
///
/// # Errors
///
/// [`PathReject`] si le chemin n'est pas de l'UTF-8, est trop long ou est dangereux.
pub fn classify_member(raw: &[u8]) -> Result<Member, PathReject> {
    let parts = split_safe(raw)?;
    let parts = strip_stamp(&parts);
    Ok(match parts {
        [name] if *name == FILE_LIST_NAME => Member::FileList,
        [name] if *name == DIR_LIST_NAME => Member::DirList,
        ["legi", "global", "eli", ..] => Member::Ignored(Ignored::Eli),
        ["legi", "global", ..] => doc(parts).map_or(Member::Ignored(Ignored::Other), Member::Doc),
        _ => Member::Ignored(Ignored::Other),
    })
}

/// Classe une ligne de `liste_suppression_legi.dat` : le chemin sans préfixe ni `.xml`.
/// `None` si la ligne n'est pas le chemin d'un document du corpus.
pub fn doc_for_key(key: &str) -> Option<DocPath> {
    match classify_member(format!("{key}.xml").as_bytes()) {
        Ok(Member::Doc(doc)) => Some(doc),
        _ => None,
    }
}

/// Valide une ligne de `liste_suppression_legi_dossier.dat` : le dossier d'un texte, c'est-à-dire
/// un chemin qui s'arrête au `<CID>`. Rend ce dossier tel qu'il figure dans [`DocPath::text_dir`].
pub fn text_dir_for(dir: &str) -> Option<String> {
    let parts = split_safe(dir.as_bytes()).ok()?;
    let parts = strip_stamp(&parts);
    head(parts).filter(|head| head.rest.is_empty())?;
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODE_DIR: &str = "legi/global/code_et_TNC_en_vigueur/code_en_vigueur/LEGI/TEXT/00/00/06/07/42/LEGITEXT000006074232";
    const ARTICLE: &str = "legi/global/code_et_TNC_en_vigueur/code_en_vigueur/LEGI/TEXT/00/00/06/07/42/LEGITEXT000006074232/article/LEGI/ARTI/00/00/06/84/24/LEGIARTI000006842411";
    const TNC_DIR: &str = "legi/global/code_et_TNC_en_vigueur/TNC_en_vigueur/JORF/TEXT/00/00/24/58/38/JORFTEXT000024583863";

    fn classify(path: &str) -> Member {
        classify_member(path.as_bytes()).unwrap()
    }

    /// Le document classé sous `path` ; sinon `classify` rend autre chose et l'égalité échoue.
    fn doc_of(path: &str) -> DocPath {
        let key = path.trim_end_matches(".xml");
        let doc = doc_for_key(key);
        assert_eq!(classify(path), Member::Doc(doc.clone().unwrap()), "{path}");
        doc.unwrap()
    }

    #[test]
    fn an_article_of_a_code_is_classified_with_its_key_and_text_dir() {
        let doc = doc_of(&format!("{ARTICLE}.xml"));
        assert_eq!(doc.key, ARTICLE);
        assert_eq!(doc.text_dir, CODE_DIR);
        assert_eq!(doc.cid, "LEGITEXT000006074232");
        assert_eq!(doc.id, "LEGIARTI000006842411");
        assert_eq!(doc.kind, DocKind::Article);
        assert_eq!(doc.tree, Tree::Code);
        assert_eq!(doc.validity, Validity::EnVigueur);
        assert_eq!(doc.source, Source::Legi);
    }

    #[test]
    fn the_four_tree_folders_are_told_apart() {
        let cases = [
            ("code_et_TNC_en_vigueur", "code_en_vigueur", Tree::Code, Validity::EnVigueur),
            ("code_et_TNC_non_vigueur", "code_non_vigueur", Tree::Code, Validity::NonVigueur),
            ("code_et_TNC_en_vigueur", "TNC_en_vigueur", Tree::Tnc, Validity::EnVigueur),
            ("code_et_TNC_non_vigueur", "TNC_non_vigueur", Tree::Tnc, Validity::NonVigueur),
        ];
        for (group, sub, tree, validity) in cases {
            let doc = doc_of(&format!(
                "legi/global/{group}/{sub}/LEGI/TEXT/00/00/06/07/42/LEGITEXT000006074232/texte/struct/LEGITEXT000006074232.xml"
            ));
            assert_eq!((doc.tree, doc.validity), (tree, validity), "{sub}");
        }
    }

    #[test]
    fn the_increment_prefix_is_not_part_of_the_key() {
        let with_prefix = doc_of(&format!("20261028-212713/{ARTICLE}.xml"));
        assert_eq!(with_prefix, doc_of(&format!("{ARTICLE}.xml")));
    }

    #[test]
    fn every_kind_of_document_is_recognised() {
        let section =
            format!("{CODE_DIR}/section_ta/LEGI/SCTA/00/00/06/10/86/LEGISCTA000006108694.xml");
        assert_eq!(doc_of(&section).kind, DocKind::Section);
        let text_struct = format!("{CODE_DIR}/texte/struct/LEGITEXT000006074232.xml");
        let doc = doc_of(&text_struct);
        assert_eq!((doc.kind, doc.id.as_str()), (DocKind::TextStruct, "LEGITEXT000006074232"));
        let version = format!("{CODE_DIR}/texte/version/LEGITEXT000006074232.xml");
        assert_eq!(doc_of(&version).kind, DocKind::TextVersion);
    }

    #[test]
    fn struct_and_version_of_one_text_have_distinct_keys() {
        let a = doc_of(&format!("{CODE_DIR}/texte/struct/LEGITEXT000006074232.xml"));
        let b = doc_of(&format!("{CODE_DIR}/texte/version/LEGITEXT000006074232.xml"));
        assert_ne!(a.key, b.key);
        assert_eq!(a.text_dir, b.text_dir);
    }

    #[test]
    fn jorf_texts_under_tnc_and_non_vigueur_are_classified() {
        let tnc = format!("{TNC_DIR}/article/LEGI/ARTI/00/00/24/58/65/LEGIARTI000024586527.xml");
        let doc = doc_of(&tnc);
        assert_eq!(
            (doc.tree, doc.source, doc.validity),
            (Tree::Tnc, Source::Jorf, Validity::EnVigueur)
        );
        let moved = tnc
            .replace("TNC_en_vigueur", "TNC_non_vigueur")
            .replace("code_et_TNC_en_vigueur", "code_et_TNC_non_vigueur");
        let moved = doc_of(&moved);
        assert_eq!(moved.validity, Validity::NonVigueur);
        assert_eq!(moved.id, doc.id);
        assert_ne!(moved.key, doc.key, "a move changes the key, never the id");
    }

    #[test]
    fn the_two_suppression_lists_are_recognised_with_or_without_prefix() {
        assert_eq!(classify("liste_suppression_legi.dat"), Member::FileList);
        assert_eq!(classify("20261028-212713/liste_suppression_legi.dat"), Member::FileList);
        assert_eq!(classify("20261028-212713/liste_suppression_legi_dossier.dat"), Member::DirList);
        // une liste qui ne serait pas à la racine n'est pas une liste
        assert_eq!(classify("legi/liste_suppression_legi.dat"), Member::Ignored(Ignored::Other));
    }

    #[test]
    fn the_eli_tree_and_foreign_paths_are_ignored() {
        assert_eq!(
            classify("legi/global/eli/arrete/2025/4/11/TECM2419022A/jo/article_1/versions.xml"),
            Member::Ignored(Ignored::Eli)
        );
        assert_eq!(
            classify("20261028-212713/legi/global/eli/jo/2013/10/30/versions.xml"),
            Member::Ignored(Ignored::Eli)
        );
        assert_eq!(
            classify("kali/global/article/KALI/ARTI/x.xml"),
            Member::Ignored(Ignored::Other)
        );
        assert_eq!(classify("legi"), Member::Ignored(Ignored::Other));
        assert_eq!(classify("20261028-212713"), Member::Ignored(Ignored::Other));
    }

    #[test]
    fn a_directory_entry_with_a_trailing_slash_is_classified_like_its_path() {
        assert_eq!(classify("legi/global/"), Member::Ignored(Ignored::Other));
    }

    #[test]
    fn malformed_trees_are_ignored_rather_than_misfiled() {
        let bad: Vec<String> = vec![
            // pas de .xml
            ARTICLE.to_string(),
            // identifiant du mauvais type pour le dossier
            format!("{CODE_DIR}/article/LEGI/ARTI/00/00/06/84/24/LEGISCTA000006842411.xml"),
            // identifiant trop court
            format!("{CODE_DIR}/article/LEGI/ARTI/00/00/06/84/24/LEGIARTI0000068424.xml"),
            // niveau qui n'est pas à deux chiffres
            format!("{CODE_DIR}/article/LEGI/ARTI/0/00/06/84/24/LEGIARTI000006842411.xml"),
            format!("{CODE_DIR}/section_ta/LEGI/SCTA/00/00/06/10/8x/LEGISCTA000006108694.xml"),
            // texte/ sans struct ni version
            format!("{CODE_DIR}/texte/autre/LEGITEXT000006074232.xml"),
            // cid mal formé
            ARTICLE.replace("LEGITEXT000006074232", "LEGITEXT0000060742") + ".xml",
            // niveau de l'arbre du texte mal formé
            ARTICLE.replace("TEXT/00/00/06/07/42", "TEXT/00/00/06/07/4") + ".xml",
            // dossiers de rang 3 et 4 en désaccord sur la vigueur
            ARTICLE.replace("code_et_TNC_en_vigueur", "code_et_TNC_non_vigueur") + ".xml",
            // sous-dossier inconnu, fonds inconnu, TEXT absent
            ARTICLE.replace("code_en_vigueur", "autre_en_vigueur") + ".xml",
            ARTICLE.replace("LEGI/TEXT", "KALI/TEXT") + ".xml",
            ARTICLE.replace("LEGI/TEXT", "LEGI/TEXTE") + ".xml",
        ];
        for path in bad {
            assert_eq!(classify(&path), Member::Ignored(Ignored::Other), "{path}");
        }
    }

    #[test]
    fn unsafe_paths_are_rejected() {
        for path in [
            "/legi/global/x.xml",
            "legi/../etc/passwd",
            "legi/./global/x.xml",
            "legi//global/x.xml",
            "legi\\global\\x.xml",
            "legi/gl\0obal/x.xml",
        ] {
            assert_eq!(classify_member(path.as_bytes()), Err(PathReject::Unsafe), "{path:?}");
        }
    }

    #[test]
    fn oversized_and_non_utf8_paths_are_rejected() {
        assert_eq!(classify_member(&vec![b'a'; MAX_PATH_BYTES + 1]), Err(PathReject::TooLong));
        assert!(classify_member(&vec![b'a'; MAX_PATH_BYTES]).is_ok());
        assert_eq!(classify_member(b"legi/global/\xff.xml"), Err(PathReject::NonUtf8));
    }

    #[test]
    fn a_suppression_line_maps_to_the_document_it_names() {
        let doc = doc_for_key(ARTICLE).unwrap();
        assert_eq!(doc.key, ARTICLE);
        assert_eq!(doc.id, "LEGIARTI000006842411");
        assert_eq!(doc_for_key("legi/global/n/importe/quoi"), None);
        assert_eq!(doc_for_key("../etc/passwd"), None);
    }

    #[test]
    fn a_dir_suppression_line_must_stop_at_the_text_folder() {
        assert_eq!(text_dir_for(TNC_DIR).as_deref(), Some(TNC_DIR));
        assert_eq!(text_dir_for(&format!("20261028-212713/{TNC_DIR}")).as_deref(), Some(TNC_DIR));
        // trop court, trop profond, dangereux
        assert_eq!(text_dir_for("legi/global/code_et_TNC_en_vigueur"), None);
        assert_eq!(text_dir_for(&format!("{TNC_DIR}/article")), None);
        assert_eq!(text_dir_for("legi/../x"), None);
    }
}
