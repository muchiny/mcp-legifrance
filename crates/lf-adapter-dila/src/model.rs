//! Documents LEGI lus dans les archives : un type par racine XML (annexe C §7.6, rapport 10 §5.3).
//!
//! Ces types sont la sortie de l'ingestion et l'entrée de l'index (V3-08). Ils gardent les
//! valeurs **brutes** de la DILA : `ETAT`, `NATURE`, `TYPE` et `typelien` sont des chaînes
//! (valeurs inconnues conservées, jamais d'échec), les dates sont des dates civiles sans
//! normalisation des sentinelles (`2999-01-01` = fin ouverte, `2222-02-22` = date différée,
//! rapport 10 §5.6 : c'est le mapper de l'index qui applique `lf_domain::date::open_ended`).
//!
//! Un champ vide (`<ETAT/>`) est lu comme absent. Le HTML des champs `CONTENU` est gardé tel
//! quel, références de caractères comprises (`&#13;`, `&lt;`) : la conversion en texte est celle
//! de `lf_domain::text` (`LongText::from_html`), qui décode les entités.

use lf_domain::date::CivilDate;

use crate::path::DocKind;

/// Un document LEGI, d'après sa racine XML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Document {
    /// `ARTICLE` : une version d'article.
    Article(Article),
    /// `SECTION_TA` : une section.
    Section(Section),
    /// `TEXTELR` : la structure d'un texte.
    TextStruct(TextStruct),
    /// `TEXTE_VERSION` : une version d'un texte.
    TextVersion(TextVersion),
}

impl Document {
    /// Identifiant du document (`META_COMMUN/ID`, ou `ID` pour une section).
    pub fn id(&self) -> &str {
        match self {
            Self::Article(doc) => &doc.id,
            Self::Section(doc) => &doc.id,
            Self::TextStruct(doc) => &doc.id,
            Self::TextVersion(doc) => &doc.id,
        }
    }

    /// Nature du document.
    pub fn kind(&self) -> DocKind {
        match self {
            Self::Article(_) => DocKind::Article,
            Self::Section(_) => DocKind::Section,
            Self::TextStruct(_) => DocKind::TextStruct,
            Self::TextVersion(_) => DocKind::TextVersion,
        }
    }

    /// L'article, si le document en est un.
    pub fn as_article(&self) -> Option<&Article> {
        if let Self::Article(doc) = self { Some(doc) } else { None }
    }

    /// La section, si le document en est une.
    pub fn as_section(&self) -> Option<&Section> {
        if let Self::Section(doc) = self { Some(doc) } else { None }
    }

    /// La structure de texte, si le document en est une.
    pub fn as_text_struct(&self) -> Option<&TextStruct> {
        if let Self::TextStruct(doc) = self { Some(doc) } else { None }
    }

    /// La version de texte, si le document en est une.
    pub fn as_text_version(&self) -> Option<&TextVersion> {
        if let Self::TextVersion(doc) = self { Some(doc) } else { None }
    }
}

/// Sections qui précèdent un article ou une section, du plus haut niveau au plus bas
/// (`CONTEXTE/TEXTE`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextContext {
    /// `TEXTE@cid` : identifiant du texte parent.
    pub cid: Option<String>,
    /// `TEXTE@nature` : `CODE`, `LOI`, `ARRETE`…
    pub nature: Option<String>,
    /// `TITRE_TXT` : titre du texte parent.
    pub title: Option<String>,
    /// Chaîne `TM/TITRE_TM`.
    pub sections: Vec<SectionRef>,
}

/// Un maillon de la chaîne de sections (`TITRE_TM`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionRef {
    /// Identifiant `LEGISCTA…` de la section.
    pub id: String,
    /// Intitulé.
    pub title: String,
    /// Début de la version de la section.
    pub date_debut: Option<CivilDate>,
    /// Fin de la version de la section.
    pub date_fin: Option<CivilDate>,
}

/// Cible d'un lien de structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkTarget {
    /// `LIEN_ART` : une version d'article.
    Article,
    /// `LIEN_SECTION_TA` : une section.
    Section,
}

/// Lien de structure (`LIEN_ART`, `LIEN_SECTION_TA`), avec sa fenêtre de vigueur
/// (`debut <= D < fin`, rapport 10 §10.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructLink {
    /// Article ou section visés.
    pub target: LinkTarget,
    /// Identifiant de la cible.
    pub id: String,
    /// `cid` (sections seulement).
    pub cid: Option<String>,
    /// `num` (articles seulement) : numéro de l'article, qui peut être non numérique.
    pub num: Option<String>,
    /// `niv` (sections seulement) : niveau dans l'arbre du texte.
    pub level: Option<u16>,
    /// `etat` brut.
    pub etat: Option<String>,
    /// `debut`.
    pub debut: Option<CivilDate>,
    /// `fin`.
    pub fin: Option<CivilDate>,
    /// Intitulé du lien (sections seulement).
    pub title: Option<String>,
}

/// Lien typé d'un article (`LIENS/LIEN`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// `typelien` brut (31 valeurs observées, rapport 10 §5.5).
    pub link_type: Option<String>,
    /// `sens` : `source` ou `cible`.
    pub direction: Option<String>,
    /// `id` de la cible.
    pub id: Option<String>,
    /// `cidtexte`.
    pub text_cid: Option<String>,
    /// `naturetexte`.
    pub text_nature: Option<String>,
    /// `numtexte`.
    pub text_number: Option<String>,
    /// `nortexte`.
    pub text_nor: Option<String>,
    /// `datesignatexte`.
    pub text_signed: Option<CivilDate>,
    /// Intitulé de la cible.
    pub title: String,
}

/// `ARTICLE` : une version d'article.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Article {
    /// `META_COMMUN/ID` (`LEGIARTI…`).
    pub id: String,
    /// `NUM` : numéro de l'article (vide dans 25 032 articles du global).
    pub num: Option<String>,
    /// `ETAT` brut ; vide dans 229 941 articles (`ENTIEREMENT_MODIF`).
    pub etat: Option<String>,
    /// `TYPE` : `AUTONOME`, `ENTIEREMENT_MODIF`, `PARTIELLEMENT_MODIF`.
    pub kind: Option<String>,
    /// `DATE_DEBUT`.
    pub date_debut: Option<CivilDate>,
    /// `DATE_FIN`.
    pub date_fin: Option<CivilDate>,
    /// Texte et sections parents.
    pub context: TextContext,
    /// `VERSIONS/VERSION/LIEN_ART` : toutes les versions de l'article.
    pub versions: Vec<StructLink>,
    /// `NOTA/CONTENU`, HTML brut.
    pub nota: String,
    /// `BLOC_TEXTUEL/CONTENU`, HTML brut.
    pub content: String,
    /// `LIENS/LIEN`.
    pub links: Vec<Link>,
}

/// `SECTION_TA` : une section et ses enfants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// `ID` (`LEGISCTA…`).
    pub id: String,
    /// `TITRE_TA`.
    pub title: Option<String>,
    /// Texte et sections parents.
    pub context: TextContext,
    /// `STRUCTURE_TA` : sections et articles enfants, dans l'ordre du fichier.
    pub structure: Vec<StructLink>,
}

/// Métadonnées `META_TEXTE_CHRONICLE`, communes à `TEXTELR` et `TEXTE_VERSION`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chronicle {
    /// `CID` : identifiant du texte, commun à toutes ses versions.
    pub cid: Option<String>,
    /// `NUM` : numéro du texte.
    pub num: Option<String>,
    /// `NOR`.
    pub nor: Option<String>,
    /// `DATE_PUBLI`.
    pub date_publi: Option<CivilDate>,
    /// `DATE_TEXTE`.
    pub date_texte: Option<CivilDate>,
    /// `DERNIERE_MODIFICATION`.
    pub last_modified: Option<CivilDate>,
}

/// Version d'un texte, vue depuis `TEXTELR/VERSIONS/VERSION/LIEN_TXT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextVersionLink {
    /// `LIEN_TXT@id`.
    pub id: String,
    /// `VERSION@etat`.
    pub etat: Option<String>,
    /// `debut`.
    pub debut: Option<CivilDate>,
    /// `fin`.
    pub fin: Option<CivilDate>,
    /// `num`.
    pub num: Option<String>,
}

/// `TEXTELR` : la structure d'un texte (sans titre).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextStruct {
    /// `META_COMMUN/ID` (`LEGITEXT…`).
    pub id: String,
    /// `META_COMMUN/NATURE`.
    pub nature: Option<String>,
    /// `META_TEXTE_CHRONICLE`.
    pub chronicle: Chronicle,
    /// `VERSIONS/VERSION/LIEN_TXT`.
    pub versions: Vec<TextVersionLink>,
    /// `STRUCT` : sections et articles de premier niveau.
    pub structure: Vec<StructLink>,
}

/// `TEXTE_VERSION` : une version d'un texte, avec ses métadonnées et ses blocs HTML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextVersion {
    /// `META_COMMUN/ID` (`LEGITEXT…`).
    pub id: String,
    /// `META_COMMUN/NATURE`.
    pub nature: Option<String>,
    /// `META_TEXTE_CHRONICLE`.
    pub chronicle: Chronicle,
    /// `META_TEXTE_VERSION/TITRE`.
    pub title: Option<String>,
    /// `META_TEXTE_VERSION/TITREFULL`.
    pub title_full: Option<String>,
    /// `META_TEXTE_VERSION/ETAT` brut.
    pub etat: Option<String>,
    /// `META_TEXTE_VERSION/DATE_DEBUT`.
    pub date_debut: Option<CivilDate>,
    /// `META_TEXTE_VERSION/DATE_FIN`.
    pub date_fin: Option<CivilDate>,
    /// `META_TEXTE_VERSION/AUTORITE`.
    pub authority: Option<String>,
    /// `META_TEXTE_VERSION/MINISTERE`.
    pub ministry: Option<String>,
    /// `VISAS/CONTENU`, HTML brut.
    pub visas: String,
    /// `SIGNATAIRES/CONTENU`, HTML brut.
    pub signatories: String,
    /// `TP/CONTENU`, HTML brut.
    pub tp: String,
    /// `NOTA/CONTENU`, HTML brut.
    pub nota: String,
    /// `ABRO/CONTENU`, HTML brut.
    pub abro: String,
    /// `RECT/CONTENU`, HTML brut.
    pub rect: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn article(id: &str) -> Article {
        Article {
            id: id.to_owned(),
            num: None,
            etat: None,
            kind: None,
            date_debut: None,
            date_fin: None,
            context: TextContext::default(),
            versions: Vec::new(),
            nota: String::new(),
            content: String::new(),
            links: Vec::new(),
        }
    }

    #[test]
    fn a_document_reports_its_id_and_kind_for_each_root() {
        let section = Section {
            id: "LEGISCTA000006108694".into(),
            title: None,
            context: TextContext::default(),
            structure: Vec::new(),
        };
        let text_struct = TextStruct {
            id: "LEGITEXT000006074232".into(),
            nature: None,
            chronicle: Chronicle::default(),
            versions: Vec::new(),
            structure: Vec::new(),
        };
        let version = TextVersion {
            id: "LEGITEXT000006074233".into(),
            nature: None,
            chronicle: Chronicle::default(),
            title: None,
            title_full: None,
            etat: None,
            date_debut: None,
            date_fin: None,
            authority: None,
            ministry: None,
            visas: String::new(),
            signatories: String::new(),
            tp: String::new(),
            nota: String::new(),
            abro: String::new(),
            rect: String::new(),
        };
        let cases = [
            (
                Document::Article(article("LEGIARTI000006842411")),
                "LEGIARTI000006842411",
                DocKind::Article,
            ),
            (Document::Section(section), "LEGISCTA000006108694", DocKind::Section),
            (Document::TextStruct(text_struct), "LEGITEXT000006074232", DocKind::TextStruct),
            (Document::TextVersion(version), "LEGITEXT000006074233", DocKind::TextVersion),
        ];
        for (document, id, kind) in cases {
            assert_eq!(document.id(), id);
            assert_eq!(document.kind(), kind);
            // un seul accesseur rend le document, les trois autres rendent `None`
            let found = [
                document.as_article().map(|d| d.id.as_str()),
                document.as_section().map(|d| d.id.as_str()),
                document.as_text_struct().map(|d| d.id.as_str()),
                document.as_text_version().map(|d| d.id.as_str()),
            ];
            assert_eq!(found.iter().flatten().collect::<Vec<_>>(), [&id]);
        }
    }
}
