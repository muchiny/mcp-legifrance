//! Table des matières en arbre : sections et articles (décision C4, 03 §4.2).
//!
//! Le mapping de l'adapter remplit un [`TocDraft`] de chaînes brutes (pas un
//! DTO serde) ; [`Toc::build`] applique, dans l'ordre, les règles de 03 §4.2 :
//!
//! 1. filtrage : un article sans identifiant valide est écarté ; une section
//!    dont l'identifiant manque ou est invalide (`id: None`, A10) est gardée
//!    si elle a un titre ou des enfants, évalués après leur propre filtrage
//!    (A12). `title` et `num` passent par [`clean_field`], seul nettoyage de ces
//!    champs (ruling C5) ; vides après nettoyage, ils valent `None` (A13) ;
//! 2. ordre des frères : tri stable par `intOrdre` si tous les frères gardés
//!    le portent (A6) ; sinon l'ordre de l'API, articles avant sous-sections
//!    (A7), comme la référence à l'intérieur d'une section
//!    (`citations.py:190-201`), et ici aussi à la racine ;
//! 3. dédoublonnage en préordre, après le tri : un article déjà placé est
//!    écarté ; une section déjà placée part avec tout son sous-arbre (A8) ;
//! 4. profondeur : `DomainError::TocTooDeep` dès qu'un nœud du brouillon,
//!    gardé ou non, serait au niveau `TOC_MAX_DEPTH + 1` (A5).
//!
//! [`Toc::dropped`] compte les articles du brouillon absents de l'arbre (A9).
//! [`Toc::page`] aplatit en préordre : une section sans titre n'est pas une
//! entrée, mais ses enfants gardent leur profondeur (05 §5.0.4) ; [`Toc::len`],
//! `total` et `next_offset` comptent donc le même ensemble d'entrées. Le texte
//! en ligne des articles KALI est un [`LongText`] (`content_html` ; absent, blanc ou vide
//! après conversion : `None`), construit seulement pour l'article retenu par le
//! dédoublonnage (règle 3) ; `etat` passe par [`LegalStatus::parse`].

use std::{collections::HashSet, fmt, num::NonZeroU32};

use crate::{
    error::DomainError,
    id::{ArticleId, SectionId},
    status::LegalStatus,
    text::{LongText, clean_field},
};

/// Profondeur maximale d'une table des matières (03 §1.3, MEN-10).
pub const TOC_MAX_DEPTH: u16 = 64;

/// Brouillon de table des matières, rempli par le mapping de l'adapter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TocDraft {
    /// Sections de premier niveau, dans l'ordre de l'API.
    pub sections: Vec<SectionDraft>,
    /// Articles de premier niveau, dans l'ordre de l'API.
    pub articles: Vec<ArticleDraft>,
}

/// Section brute d'un [`TocDraft`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SectionDraft {
    /// Identifiant brut, ex. `LEGISCTA000033205040`.
    pub id: Option<String>,
    /// Titre brut, HTML compris.
    pub title: Option<String>,
    /// Rang `intOrdre` de l'API.
    pub int_ordre: Option<i64>,
    /// État juridique brut.
    pub etat: Option<String>,
    /// Sous-sections, dans l'ordre de l'API.
    pub sections: Vec<SectionDraft>,
    /// Articles de la section, dans l'ordre de l'API.
    pub articles: Vec<ArticleDraft>,
}

/// Article brut d'un [`TocDraft`].
///
/// `Debug` n'écrit jamais `content_html` (HTML du corps, plusieurs Mio possibles), seulement sa
/// longueur en octets (`content_html: 25 bytes`).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ArticleDraft {
    /// Identifiant brut, ex. `LEGIARTI000033205126`.
    pub id: Option<String>,
    /// Numéro brut, HTML compris.
    pub num: Option<String>,
    /// Rang `intOrdre` de l'API.
    pub int_ordre: Option<i64>,
    /// État juridique brut, ex. `VIGUEUR`.
    pub etat: Option<String>,
    /// Contenu HTML en ligne (`content`, attesté pour KALI) ; `None` : absent.
    pub content_html: Option<String>,
}

impl fmt::Debug for ArticleDraft {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArticleDraft")
            .field("id", &self.id)
            .field("num", &self.num)
            .field("int_ordre", &self.int_ordre)
            .field("etat", &self.etat)
            .field("content_html", &HtmlLen(self.content_html.as_deref().map(str::len)))
            .finish()
    }
}

/// Longueur d'un HTML pour `Debug` : `None`, ou `<n> bytes`.
struct HtmlLen(Option<usize>);

impl fmt::Debug for HtmlLen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(bytes) => write!(f, "{bytes} bytes"),
            None => f.write_str("None"),
        }
    }
}

/// Nœud de la table des matières.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TocNode {
    /// Section et ses enfants.
    Section(TocSection),
    /// Article.
    Article(TocArticle),
}

/// Section de la table des matières.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocSection {
    /// Identifiant de la section, s'il est valide.
    pub id: Option<SectionId>,
    /// Titre nettoyé ; `None` : la section n'est pas une entrée de [`Toc::page`].
    pub title: Option<String>,
    /// État juridique, non rendu (05 §5.0.4).
    pub etat: Option<LegalStatus>,
    /// Enfants, dans l'ordre de la table.
    pub children: Vec<TocNode>,
}

/// Entrée de table des matières : un article résoluble par `get_article`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocArticle {
    /// Identifiant de l'article.
    pub article_id: ArticleId,
    /// Numéro affiché, ex. `L. 1234-5`.
    pub num: Option<String>,
    /// État juridique, ex. `Vigueur`.
    pub etat: Option<LegalStatus>,
    /// Texte en ligne, disponible sans appel supplémentaire ; `None` : contenu absent ou vide.
    pub inline_text: Option<LongText>,
}

/// Table des matières validée : arbre ordonné, dédoublonné, d'au plus
/// [`TOC_MAX_DEPTH`] niveaux. `Default` : table vide.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Toc {
    roots: Vec<TocNode>,
    dropped: u32,
}

/// Entrée aplatie de [`Toc::page`], en préordre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TocEntry<'a> {
    /// Profondeur, 1 au premier niveau.
    pub depth: u16,
    /// Section titrée ou article.
    pub node: TocEntryRef<'a>,
}

/// Contenu d'une [`TocEntry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TocEntryRef<'a> {
    /// Section titrée : une section sans titre n'est pas une entrée.
    Section {
        /// Titre nettoyé, non vide.
        title: &'a str,
    },
    /// Article.
    Article(&'a TocArticle),
}

/// Page de [`Toc::page`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocPage<'a> {
    /// Au plus `limit` entrées, à partir de `offset`.
    pub entries: Vec<TocEntry<'a>>,
    /// Nombre total d'entrées (`toc_total`), égal à [`Toc::len`].
    pub total: u32,
    /// Début de la page suivante (`next_toc_offset`), `None` en fin de table.
    pub next_offset: Option<u32>,
}

impl Toc {
    /// Construit l'arbre depuis le brouillon (règles du module).
    ///
    /// # Errors
    ///
    /// [`DomainError::TocTooDeep`] si un nœud du brouillon dépasse
    /// [`TOC_MAX_DEPTH`] niveaux ; [`DomainError::TextTooLarge`] si le texte en ligne d'un
    /// article retenu (identifiant valide, première occurrence) dépasse
    /// [`LongText::MAX_CHARS`] : celui d'un article écarté n'est jamais construit.
    pub fn build(draft: TocDraft) -> Result<Self, DomainError> {
        let total = weigh(&draft.sections, &draft.articles, TOC_MAX_DEPTH)?;
        let mut builder = Builder::default();
        let roots = builder.children(draft.sections, draft.articles)?;
        Ok(Self { roots, dropped: to_u32(total - builder.kept) })
    }

    /// Nœuds de premier niveau.
    pub fn roots(&self) -> &[TocNode] {
        &self.roots
    }

    /// Articles du brouillon absents de l'arbre : identifiant invalide,
    /// doublon, ou sous-arbre d'une section en double (A9).
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    /// Nombre d'entrées aplaties (`toc_total`) : sections titrées et articles.
    pub fn len(&self) -> u32 {
        to_u32(self.entries().len())
    }

    /// Vrai si la table n'a aucune entrée.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Au plus `limit` entrées en préordre, à partir de `offset`.
    ///
    /// # Errors
    ///
    /// [`DomainError::TocOffsetPastEnd`] si `offset >= len()`, sauf pour une
    /// table vide lue à `offset = 0` (page vide).
    pub fn page(&self, offset: u32, limit: NonZeroU32) -> Result<TocPage<'_>, DomainError> {
        let all = self.entries();
        let total = to_u32(all.len());
        if offset > 0 && offset >= total {
            return Err(DomainError::TocOffsetPastEnd { offset, total });
        }
        let entries: Vec<_> =
            all.into_iter().skip(to_usize(offset)).take(to_usize(limit.get())).collect();
        // Ici offset < total ou offset = 0 = total : la somme ne dépasse pas total.
        let next = offset + to_u32(entries.len());
        Ok(TocPage { entries, total, next_offset: (next < total).then_some(next) })
    }

    fn entries(&self) -> Vec<TocEntry<'_>> {
        let mut out = Vec::new();
        flatten(&self.roots, 1, &mut out);
        out
    }
}

/// Nombre d'articles du brouillon, profondeur contrôlée : `budget` est le
/// nombre de niveaux encore permis à `sections` et `articles` (A5).
fn weigh(
    sections: &[SectionDraft],
    articles: &[ArticleDraft],
    budget: u16,
) -> Result<usize, DomainError> {
    if budget == 0 && !(sections.is_empty() && articles.is_empty()) {
        return Err(DomainError::TocTooDeep { max: TOC_MAX_DEPTH });
    }
    // Un niveau de moins par appel : au plus TOC_MAX_DEPTH + 1 appels imbriqués.
    sections.iter().try_fold(articles.len(), |total, section| {
        Ok(total + weigh(&section.sections, &section.articles, budget - 1)?)
    })
}

/// État de la construction : identifiants déjà placés (clé `as_str`, A14) et
/// nombre d'articles gardés.
#[derive(Default)]
struct Builder {
    articles: HashSet<String>,
    sections: HashSet<String>,
    kept: usize,
}

/// Frère en attente de tri : article validé, dont le texte en ligne n'est construit que s'il est
/// retenu par le dédoublonnage (son `content_html` attend à côté), ou section encore brute.
enum Sibling {
    Article(TocArticle, Option<String>),
    Section(SectionDraft),
}

impl Builder {
    /// Enfants d'un parent, ou racines : règles 1 à 3.
    fn children(
        &mut self,
        sections: Vec<SectionDraft>,
        articles: Vec<ArticleDraft>,
    ) -> Result<Vec<TocNode>, DomainError> {
        // Règle 1 ; ordre de départ du tri stable : articles, puis sections (A7).
        let mut siblings: Vec<(Option<i64>, Sibling)> = Vec::new();
        for draft in articles {
            let int_ordre = draft.int_ordre;
            if let Some((article, content_html)) = article_of(draft) {
                siblings.push((int_ordre, Sibling::Article(article, content_html)));
            }
        }
        siblings
            .extend(sections.into_iter().map(|draft| (draft.int_ordre, Sibling::Section(draft))));
        // Règle 2.
        if siblings.iter().all(|(int_ordre, _)| int_ordre.is_some()) {
            siblings.sort_by_key(|(int_ordre, _)| *int_ordre);
        }
        // Règle 3 : chaque frère est placé avant ses descendants (préordre).
        let mut nodes = Vec::new();
        for (_, sibling) in siblings {
            let node = match sibling {
                Sibling::Article(article, content_html) => {
                    self.article(article, content_html.as_deref())?
                }
                Sibling::Section(draft) => self.section(draft)?,
            };
            nodes.extend(node);
        }
        Ok(nodes)
    }

    /// Article placé s'il n'est pas un doublon ; son texte en ligne n'est construit qu'alors
    /// (03 §4.2 : dédoublonnage, puis texte en ligne de l'article retenu).
    fn article(
        &mut self,
        mut article: TocArticle,
        content_html: Option<&str>,
    ) -> Result<Option<TocNode>, DomainError> {
        if !self.articles.insert(article.article_id.as_str().to_owned()) {
            return Ok(None);
        }
        self.kept += 1;
        article.inline_text = inline_text(content_html)?;
        Ok(Some(TocNode::Article(article)))
    }

    fn section(&mut self, draft: SectionDraft) -> Result<Option<TocNode>, DomainError> {
        let id = draft.id.as_deref().and_then(|raw| SectionId::parse(raw).ok());
        if id.as_ref().is_some_and(|id| !self.sections.insert(id.as_str().to_owned())) {
            return Ok(None);
        }
        let children = self.children(draft.sections, draft.articles)?;
        let title = clean_field(draft.title.as_deref());
        let etat = draft.etat.as_deref().and_then(LegalStatus::parse);
        let kept = id.is_some() || title.is_some() || !children.is_empty();
        Ok(kept.then_some(TocNode::Section(TocSection { id, title, etat, children })))
    }
}

/// Article validé du brouillon (règle 1), sans texte en ligne, et son `content_html` ; `None`
/// si son identifiant manque ou est invalide.
fn article_of(draft: ArticleDraft) -> Option<(TocArticle, Option<String>)> {
    let article_id = draft.id.as_deref().and_then(|raw| ArticleId::parse(raw).ok())?;
    let article = TocArticle {
        article_id,
        num: clean_field(draft.num.as_deref()),
        etat: draft.etat.as_deref().and_then(LegalStatus::parse),
        inline_text: None,
    };
    Some((article, draft.content_html))
}

/// Texte en ligne d'un article : `None` si le contenu est absent, blanc ou vide après
/// conversion.
fn inline_text(content_html: Option<&str>) -> Result<Option<LongText>, DomainError> {
    let Some(html) = content_html.filter(|html| !html.trim().is_empty()) else {
        return Ok(None);
    };
    let text = LongText::from_html(html)?;
    Ok((!text.is_empty()).then_some(text))
}

/// Aplatit `nodes` en préordre : chaque article est une entrée, chaque section
/// titrée aussi ; les enfants d'une section sont à `depth + 1` dans tous les cas.
fn flatten<'a>(nodes: &'a [TocNode], depth: u16, out: &mut Vec<TocEntry<'a>>) {
    for node in nodes {
        match node {
            TocNode::Article(article) => {
                out.push(TocEntry { depth, node: TocEntryRef::Article(article) });
            }
            TocNode::Section(section) => {
                if let Some(title) = section.title.as_deref() {
                    out.push(TocEntry { depth, node: TocEntryRef::Section { title } });
                }
                // Profondeur bornée par `Toc::build` : au plus TOC_MAX_DEPTH.
                flatten(&section.children, depth + 1, out);
            }
        }
    }
}

/// Compte en `u32`, saturé (une table réelle en est très loin).
fn to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Position en `usize`, saturée (sans perte sur les cibles 32 et 64 bits).
fn to_usize(n: u32) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn limit(n: u32) -> NonZeroU32 {
        NonZeroU32::new(n).unwrap()
    }

    fn article(id: &str, num: &str) -> ArticleDraft {
        ArticleDraft { id: Some(id.into()), num: Some(num.into()), ..ArticleDraft::default() }
    }

    fn section(
        id: &str,
        title: &str,
        sections: Vec<SectionDraft>,
        articles: Vec<ArticleDraft>,
    ) -> SectionDraft {
        SectionDraft {
            id: Some(id.into()),
            title: Some(title.into()),
            sections,
            articles,
            ..SectionDraft::default()
        }
    }

    fn roots(sections: Vec<SectionDraft>) -> TocDraft {
        TocDraft { sections, articles: vec![] }
    }

    /// `levels` sections imbriquées (niveaux 1 à `levels`), la plus profonde
    /// portant `articles`.
    fn chain(levels: u16, articles: Vec<ArticleDraft>) -> TocDraft {
        let mut draft = TocDraft { sections: vec![], articles };
        for level in (1..=levels).rev() {
            let title = Some(format!("S{level}"));
            let inner = SectionDraft {
                title,
                sections: draft.sections,
                articles: draft.articles,
                ..SectionDraft::default()
            };
            draft = roots(vec![inner]);
        }
        draft
    }

    fn node_section(id: Option<&str>, title: Option<&str>, children: Vec<TocNode>) -> TocNode {
        TocNode::Section(TocSection {
            id: id.map(|id| SectionId::parse(id).unwrap()),
            title: title.map(str::to_owned),
            etat: None,
            children,
        })
    }

    fn node_article(id: &str, num: Option<&str>) -> TocNode {
        TocNode::Article(TocArticle {
            article_id: ArticleId::parse(id).unwrap(),
            num: num.map(str::to_owned),
            etat: None,
            inline_text: None,
        })
    }

    /// Entrée lisible : `profondeur § titre` ou `profondeur id num`.
    fn show(entry: &TocEntry<'_>) -> String {
        match entry.node {
            TocEntryRef::Section { title } => format!("{} § {title}", entry.depth),
            TocEntryRef::Article(article) => format!(
                "{} {} {}",
                entry.depth,
                article.article_id.as_str(),
                article.num.as_deref().unwrap_or("-")
            ),
        }
    }

    fn texts(page: &TocPage<'_>) -> Vec<String> {
        page.entries.iter().map(show).collect()
    }

    /// Toutes les entrées, en une page.
    fn shown(toc: &Toc) -> Vec<String> {
        texts(&toc.page(0, NonZeroU32::MAX).unwrap())
    }

    #[test]
    fn toc_preserves_section_titles() {
        // Première branche de consult_loda.json (titres réparés), jusqu'au
        // deuxième article de la section 2 ; aucun nœud n'a d'intOrdre. Les huit
        // premières entrées sont celles de l'exemple fr_get_act (05 §5.2).
        let section_1 = section(
            "LEGISCTA000033205022",
            "Section 1 : Ouverture de l'accès aux données publiques",
            vec![],
            vec![
                article("LEGIARTI000033205126", "1"),
                article("LEGIARTI000033205128", "2"),
                article("LEGIARTI000033205130", "3"),
                article("LEGIARTI000033205132", "4"),
                article("LEGIARTI000033205062", "5"),
            ],
        );
        let section_2 = section(
            "LEGISCTA000033205023",
            "Section 2 : Données d'intérêt général",
            vec![],
            vec![article("LEGIARTI000033205150", "17"), article("LEGIARTI000033205152", "18")],
        );
        let chapitre = section(
            "LEGISCTA000033205015",
            "Chapitre Ier : Economie de la donnée",
            vec![section_1, section_2],
            vec![],
        );
        let titre = section(
            "LEGISCTA000033205040",
            "Titre Ier : LA CIRCULATION DES DONNÉES ET DU SAVOIR",
            vec![chapitre],
            vec![],
        );
        let toc = Toc::build(roots(vec![titre])).unwrap();
        let first = toc.page(0, limit(8)).unwrap();
        assert_eq!(
            texts(&first),
            [
                "1 § Titre Ier : LA CIRCULATION DES DONNÉES ET DU SAVOIR",
                "2 § Chapitre Ier : Economie de la donnée",
                "3 § Section 1 : Ouverture de l'accès aux données publiques",
                "4 LEGIARTI000033205126 1",
                "4 LEGIARTI000033205128 2",
                "4 LEGIARTI000033205130 3",
                "4 LEGIARTI000033205132 4",
                "4 LEGIARTI000033205062 5",
            ]
        );
        assert_eq!((first.total, first.next_offset), (11, Some(8)));
        let last = toc.page(8, limit(8)).unwrap();
        assert_eq!(
            texts(&last),
            [
                "3 § Section 2 : Données d'intérêt général",
                "4 LEGIARTI000033205150 17",
                "4 LEGIARTI000033205152 18",
            ]
        );
        assert_eq!((last.total, last.next_offset), (11, None));
        assert_eq!((toc.len(), toc.is_empty(), toc.dropped()), (11, false, 0));
    }

    #[test]
    fn untitled_section_is_not_an_entry() {
        // 05 §5.0.4 : une section sans titre n'est pas une entrée, mais ses
        // enfants gardent leur profondeur ; len et total comptent les mêmes
        // entrées que page.
        let inner = section(
            "LEGISCTA000000000002",
            "Sous-section",
            vec![],
            vec![article("LEGIARTI000000000002", "2")],
        );
        let untitled = SectionDraft {
            title: None,
            ..section(
                "LEGISCTA000000000001",
                "",
                vec![inner],
                vec![article("LEGIARTI000000000001", "1")],
            )
        };
        let toc = Toc::build(roots(vec![untitled])).unwrap();
        assert_eq!(
            shown(&toc),
            ["2 LEGIARTI000000000001 1", "2 § Sous-section", "3 LEGIARTI000000000002 2"]
        );
        assert_eq!((toc.len(), toc.page(1, limit(1)).unwrap().total), (3, 3));
    }

    #[test]
    fn articles_without_a_valid_id_are_dropped() {
        // Règle 1 (03 §4.2) : écartés et comptés dans dropped.
        let draft = TocDraft {
            sections: vec![],
            articles: vec![
                ArticleDraft::default(),
                article("", "a"),
                article("LEGITEXT000033205014", "b"),
                article("nope", "c"),
                article("KALIARTI000049886059", "19"),
            ],
        };
        let toc = Toc::build(draft).unwrap();
        assert_eq!(shown(&toc), ["1 KALIARTI000049886059 19"]);
        assert_eq!(toc.dropped(), 4);
    }

    #[test]
    fn titles_and_nums_are_cleaned() {
        // consult_kali.json : l'espace final du titre disparaît (règle 1) ; un
        // titre ou un numéro vide après nettoyage devient None (A13) ; etat
        // est converti en statut.
        let kali = SectionDraft {
            id: Some("KALISCTA000049886029".into()),
            title: Some("Chapitre 1er : Recours au CDD d'usage ".into()),
            int_ordre: Some(1_074_003_967),
            etat: Some("VIGUEUR".into()),
            sections: vec![],
            articles: vec![ArticleDraft {
                id: Some("KALIARTI000049886059".into()),
                num: Some(" 19  (des dispositions <b>communes</b>) ".into()),
                int_ordre: Some(1_073_741_823),
                etat: Some("VIGUEUR".into()),
                content_html: None,
            }],
        };
        let blank = SectionDraft {
            title: Some(" <br> ".into()),
            ..section(
                "LEGISCTA000000000001",
                "",
                vec![],
                vec![article("LEGIARTI000000000001", " ")],
            )
        };
        let toc = Toc::build(roots(vec![kali, blank])).unwrap();
        assert_eq!(
            toc.roots(),
            [
                TocNode::Section(TocSection {
                    id: Some(SectionId::parse("KALISCTA000049886029").unwrap()),
                    title: Some("Chapitre 1er : Recours au CDD d'usage".into()),
                    etat: Some(LegalStatus::Vigueur),
                    children: vec![TocNode::Article(TocArticle {
                        article_id: ArticleId::parse("KALIARTI000049886059").unwrap(),
                        num: Some("19 (des dispositions communes)".into()),
                        etat: Some(LegalStatus::Vigueur),
                        inline_text: None,
                    })],
                }),
                node_section(
                    Some("LEGISCTA000000000001"),
                    None,
                    vec![node_article("LEGIARTI000000000001", None)],
                ),
            ]
        );
    }

    /// Brouillon d'un article portant `content_html`, sous une section titrée.
    fn with_content(content: Option<&str>) -> TocDraft {
        let article = ArticleDraft {
            content_html: content.map(str::to_owned),
            ..article("KALIARTI000049886059", "19")
        };
        roots(vec![section("KALISCTA000049886029", "Chapitre 1er", vec![], vec![article])])
    }

    /// Texte en ligne du seul article d'une table.
    fn inline(toc: &Toc) -> Option<LongText> {
        let page = toc.page(0, NonZeroU32::MAX).unwrap();
        let articles: Vec<_> = page
            .entries
            .iter()
            .filter_map(|entry| match entry.node {
                TocEntryRef::Article(article) => Some(article),
                TocEntryRef::Section { .. } => None,
            })
            .collect();
        assert_eq!(articles.len(), 1);
        articles.first().and_then(|article| article.inline_text.clone())
    }

    #[test]
    fn toc_article_keeps_inline_text_from_content_html() {
        // kali_inline_content_is_available_without_extra_call, côté domaine.
        let toc = Toc::build(with_content(Some("<p>Art. 1</p><p>Alinéa</p>"))).unwrap();
        let text = inline(&toc).unwrap();
        let paragraphs: Vec<_> = text.paragraphs().collect();
        assert_eq!(paragraphs, [(0, "Art. 1"), (8, "Alinéa")]);
        assert_eq!(text.as_str(), "Art. 1\n\nAlinéa");
    }

    #[test]
    fn toc_article_without_content_has_no_inline_text() {
        let toc = Toc::build(with_content(None)).unwrap();
        assert_eq!(inline(&toc), None);
    }

    #[test]
    fn toc_blank_content_gives_no_inline_text() {
        // Absent, blanc, ou vide après nettoyage : aucun texte en ligne.
        for content in ["", "  \n\t ", "<p> </p>", "<br>"] {
            let toc = Toc::build(with_content(Some(content))).unwrap();
            assert_eq!(inline(&toc), None, "{content:?}");
        }
    }

    #[test]
    fn toc_status_is_parsed() {
        let status = |raw: Option<&str>, expected: Option<LegalStatus>| {
            let draft = TocDraft {
                sections: vec![SectionDraft {
                    etat: raw.map(str::to_owned),
                    articles: vec![ArticleDraft {
                        etat: raw.map(str::to_owned),
                        ..article("LEGIARTI000000000001", "1")
                    }],
                    ..section("LEGISCTA000000000001", "S1", vec![], vec![])
                }],
                articles: vec![],
            };
            let want = TocNode::Section(TocSection {
                id: Some(SectionId::parse("LEGISCTA000000000001").unwrap()),
                title: Some("S1".into()),
                etat: expected.clone(),
                children: vec![TocNode::Article(TocArticle {
                    article_id: ArticleId::parse("LEGIARTI000000000001").unwrap(),
                    num: Some("1".into()),
                    etat: expected,
                    inline_text: None,
                })],
            });
            assert_eq!(Toc::build(draft).unwrap().roots(), [want], "{raw:?}");
        };
        status(Some("VIGUEUR"), Some(LegalStatus::Vigueur));
        status(Some(""), None);
        status(None, None);
        status(Some("INCONNU"), Some(LegalStatus::Other("INCONNU".into())));
    }

    #[test]
    fn toc_propagates_text_too_large() {
        let huge = format!("<p>{}</p>", "a".repeat(16_000_001));
        assert_eq!(
            Toc::build(with_content(Some(&huge))),
            Err(DomainError::TextTooLarge { chars: 16_000_001 })
        );
        // Un article écarté (identifiant invalide) ne construit pas son texte.
        let dropped = TocDraft {
            sections: vec![],
            articles: vec![ArticleDraft { content_html: Some(huge), ..article("nope", "1") }],
        };
        assert_eq!(Toc::build(dropped).unwrap().dropped(), 1);
    }

    #[test]
    fn article_draft_debug_never_prints_the_html() {
        // Audit L-4 : le HTML brut (plusieurs Mio possibles) ne fuit pas dans un `{:?}` ; seule
        // sa longueur en octets est écrite.
        let draft = ArticleDraft {
            content_html: Some("<p>Texte confidentiel</p>".into()),
            ..article("KALIARTI000049886059", "19")
        };
        assert_eq!(
            format!("{draft:?}"),
            "ArticleDraft { id: Some(\"KALIARTI000049886059\"), num: Some(\"19\"), \
             int_ordre: None, etat: None, content_html: 25 bytes }"
        );
        assert_eq!(
            format!("{:?}", ArticleDraft::default()),
            "ArticleDraft { id: None, num: None, int_ordre: None, etat: None, content_html: None }"
        );
        let toc = TocDraft { sections: vec![], articles: vec![draft] };
        for debug in [format!("{toc:?}"), format!("{toc:#?}")] {
            assert!(!debug.contains("confidentiel"), "{debug}");
            assert!(debug.contains("content_html: 25 bytes"), "{debug}");
        }
    }

    #[test]
    fn duplicate_article_does_not_build_its_inline_text() {
        // Audit L-3 : le texte en ligne n'est construit que pour l'article retenu par le
        // dédoublonnage (première occurrence) ; un doublon écarté ne peut pas faire échouer la
        // table par `TextTooLarge`.
        let first = ArticleDraft {
            content_html: Some("<p>Premier texte</p>".into()),
            ..article("KALIARTI000049886059", "19")
        };
        let huge = format!("<p>{}</p>", "a".repeat(16_000_001));
        let second =
            ArticleDraft { content_html: Some(huge), ..article("KALIARTI000049886059", "19 bis") };
        let draft = roots(vec![section(
            "KALISCTA000049886029",
            "Chapitre 1er",
            vec![],
            vec![first, second],
        )]);
        let toc = Toc::build(draft).unwrap();
        assert_eq!(shown(&toc), ["1 § Chapitre 1er", "2 KALIARTI000049886059 19"]);
        assert_eq!(toc.dropped(), 1);
        assert_eq!(inline(&toc).as_ref().map(LongText::as_str), Some("Premier texte"));
    }

    #[test]
    fn section_without_id_is_kept_with_a_title_or_children() {
        // Règle 1 : une section sans id est gardée si elle a un titre ou des
        // enfants (évalué après le filtrage des enfants, A12) ; un id de section
        // invalide devient None (A10).
        let bare = |title: Option<&str>, articles: Vec<ArticleDraft>| SectionDraft {
            title: title.map(str::to_owned),
            articles,
            ..SectionDraft::default()
        };
        let draft = roots(vec![
            bare(None, vec![]),
            bare(Some("Titre"), vec![]),
            bare(None, vec![article("LEGIARTI000000000001", "1")]),
            bare(None, vec![ArticleDraft::default()]),
            SectionDraft {
                id: Some("LEGIARTI000000000002".into()),
                ..bare(Some("Id invalide"), vec![])
            },
            SectionDraft { id: Some("LEGISCTA000000000003".into()), ..SectionDraft::default() },
        ]);
        let toc = Toc::build(draft).unwrap();
        assert_eq!(
            toc.roots(),
            [
                node_section(None, Some("Titre"), vec![]),
                node_section(None, None, vec![node_article("LEGIARTI000000000001", Some("1"))]),
                node_section(None, Some("Id invalide"), vec![]),
                node_section(Some("LEGISCTA000000000003"), None, vec![]),
            ]
        );
        assert_eq!(toc.dropped(), 1);
    }

    #[test]
    fn toc_sorted_by_int_ordre() {
        // Règle 2 : tous les frères gardés (articles et sections) portent
        // intOrdre, d'où un tri stable ; à égalité, l'article reste devant (A7).
        // L'article écarté, sans intOrdre, ne compte pas (A6).
        let draft = TocDraft {
            articles: vec![
                ArticleDraft { int_ordre: Some(30), ..article("LEGIARTI000000000003", "3") },
                ArticleDraft::default(),
                ArticleDraft { int_ordre: Some(10), ..article("LEGIARTI000000000001", "1") },
                ArticleDraft { int_ordre: Some(20), ..article("LEGIARTI000000000002", "2") },
            ],
            sections: vec![
                SectionDraft {
                    int_ordre: Some(20),
                    ..section("LEGISCTA000000000002", "S2", vec![], vec![])
                },
                SectionDraft {
                    int_ordre: Some(5),
                    ..section("LEGISCTA000000000001", "S1", vec![], vec![])
                },
            ],
        };
        assert_eq!(
            shown(&Toc::build(draft).unwrap()),
            [
                "1 § S1",
                "1 LEGIARTI000000000001 1",
                "1 LEGIARTI000000000002 2",
                "1 § S2",
                "1 LEGIARTI000000000003 3",
            ]
        );
    }

    #[test]
    fn api_order_unless_every_sibling_has_int_ordre() {
        // Règle 2 : un frère sans intOrdre suffit à garder l'ordre de l'API,
        // articles avant sous-sections, à la racine comme dans une section.
        let inner = SectionDraft {
            int_ordre: Some(1),
            ..section("LEGISCTA000000000002", "S2", vec![], vec![])
        };
        let outer = section(
            "LEGISCTA000000000001",
            "S1",
            vec![inner],
            vec![
                ArticleDraft { int_ordre: Some(3), ..article("LEGIARTI000000000003", "3") },
                article("LEGIARTI000000000004", "4"),
            ],
        );
        let draft = TocDraft {
            sections: vec![SectionDraft { int_ordre: Some(1), ..outer }],
            articles: vec![
                ArticleDraft { int_ordre: Some(9), ..article("LEGIARTI000000000001", "1") },
                article("LEGIARTI000000000002", "2"),
            ],
        };
        assert_eq!(
            shown(&Toc::build(draft).unwrap()),
            [
                "1 LEGIARTI000000000001 1",
                "1 LEGIARTI000000000002 2",
                "1 § S1",
                "2 LEGIARTI000000000003 3",
                "2 LEGIARTI000000000004 4",
                "2 § S2",
            ]
        );
    }

    #[test]
    fn toc_dedups_articles() {
        // Règle 3 : sur tout l'arbre, la première occurrence en préordre, après
        // le tri, est gardée ; les doublons sont comptés dans dropped (A9).
        let draft = roots(vec![
            SectionDraft {
                int_ordre: Some(2),
                ..section(
                    "LEGISCTA000000000001",
                    "S1",
                    vec![],
                    vec![
                        article("LEGIARTI000000000001", "1"),
                        article("LEGIARTI000000000002", "2"),
                    ],
                )
            },
            SectionDraft {
                int_ordre: Some(1),
                ..section(
                    "LEGISCTA000000000002",
                    "S2",
                    vec![],
                    vec![
                        article("LEGIARTI000000000002", "2 bis"),
                        article("legiarti000000000002", "2 ter"),
                    ],
                )
            },
        ]);
        let toc = Toc::build(draft).unwrap();
        assert_eq!(
            shown(&toc),
            ["1 § S2", "2 LEGIARTI000000000002 2 bis", "1 § S1", "2 LEGIARTI000000000001 1"]
        );
        assert_eq!(toc.dropped(), 2);
    }

    #[test]
    fn duplicate_section_leaves_with_its_subtree() {
        // Règle 3 (A8, A9) : la seconde occurrence d'une section part avec tout
        // son sous-arbre, dont les articles sont comptés dans dropped ; un
        // article vu seulement dans ce sous-arbre reste disponible ensuite. Les
        // sections sans id ne sont pas dédoublonnées.
        let duplicate = section(
            "LEGISCTA000000000001",
            "S1 bis",
            vec![section(
                "LEGISCTA000000000002",
                "S2",
                vec![],
                vec![article("LEGIARTI000000000002", "2"), ArticleDraft::default()],
            )],
            vec![article("LEGIARTI000000000001", "1")],
        );
        let draft = roots(vec![
            section(
                "LEGISCTA000000000001",
                "S1",
                vec![],
                vec![article("LEGIARTI000000000001", "1")],
            ),
            duplicate,
            section(
                "LEGISCTA000000000003",
                "S3",
                vec![],
                vec![article("LEGIARTI000000000002", "2 bis")],
            ),
            SectionDraft { id: None, ..section("", "Sans id", vec![], vec![]) },
            SectionDraft { id: None, ..section("", "Sans id", vec![], vec![]) },
        ]);
        let toc = Toc::build(draft).unwrap();
        assert_eq!(
            shown(&toc),
            [
                "1 § S1",
                "2 LEGIARTI000000000001 1",
                "1 § S3",
                "2 LEGIARTI000000000002 2 bis",
                "1 § Sans id",
                "1 § Sans id",
            ]
        );
        assert_eq!(toc.dropped(), 3);
    }

    #[test]
    fn toc_depth_64_is_accepted() {
        let toc = Toc::build(chain(63, vec![article("LEGIARTI000000000064", "64")])).unwrap();
        assert_eq!(
            texts(&toc.page(62, limit(10)).unwrap()),
            ["63 § S63", "64 LEGIARTI000000000064 64"]
        );
        assert_eq!(Toc::build(chain(TOC_MAX_DEPTH, vec![])).unwrap().len(), 64);
    }

    #[test]
    fn toc_depth_65_is_too_deep() {
        // MEN-10 : un nœud au niveau 65, article ou section, suffit, même s'il
        // aurait été écarté (A5) ; la récursion s'arrête là (profondeur 200).
        for draft in [
            chain(64, vec![article("LEGIARTI000000000065", "65")]),
            chain(64, vec![ArticleDraft::default()]),
            chain(65, vec![]),
            chain(200, vec![]),
        ] {
            assert_eq!(Toc::build(draft), Err(DomainError::TocTooDeep { max: 64 }));
        }
    }

    #[test]
    fn offset_past_the_end_is_rejected() {
        let toc = Toc::build(chain(2, vec![article("LEGIARTI000000000001", "1")])).unwrap();
        let last = toc.page(2, limit(5)).unwrap();
        assert_eq!(texts(&last), ["3 LEGIARTI000000000001 1"]);
        assert_eq!((last.total, last.next_offset), (3, None));
        let rest = toc.page(1, NonZeroU32::MAX).unwrap();
        assert_eq!(texts(&rest), ["2 § S2", "3 LEGIARTI000000000001 1"]);
        assert_eq!(rest.next_offset, None);
        for offset in [3, 4, u32::MAX] {
            assert_eq!(
                toc.page(offset, limit(1)),
                Err(DomainError::TocOffsetPastEnd { offset, total: 3 })
            );
        }
    }

    #[test]
    fn empty_toc_pages_from_zero_only() {
        let toc = Toc::build(TocDraft::default()).unwrap();
        assert_eq!(toc, Toc::default());
        assert_eq!((toc.len(), toc.is_empty(), toc.dropped()), (0, true, 0));
        assert_eq!(
            toc.page(0, limit(100)),
            Ok(TocPage { entries: vec![], total: 0, next_offset: None })
        );
        assert_eq!(
            toc.page(1, limit(100)),
            Err(DomainError::TocOffsetPastEnd { offset: 1, total: 0 })
        );
        // Une section sans titre ni enfant, gardée pour son id : aucune entrée.
        let hollow = Toc::build(roots(vec![SectionDraft {
            id: Some("LEGISCTA000000000001".into()),
            ..SectionDraft::default()
        }]))
        .unwrap();
        assert_eq!((hollow.roots().len(), hollow.len(), hollow.is_empty()), (1, 0, true));
    }

    const ARTICLE_IDS: [&str; 4] =
        ["LEGIARTI000000000001", "LEGIARTI000000000002", "KALIARTI000000000003", "nope"];
    const SECTION_IDS: [&str; 3] =
        ["LEGISCTA000000000001", "KALISCTA000000000002", "LEGIARTI000000000001"];

    /// Peu d'identifiants : doublons fréquents ; certains sont invalides.
    fn article_draft() -> impl Strategy<Value = ArticleDraft> {
        (
            proptest::option::of(proptest::sample::select(ARTICLE_IDS.to_vec())),
            proptest::option::of(0_i64..3),
        )
            .prop_map(|(id, int_ordre)| ArticleDraft {
                id: id.map(str::to_owned),
                num: None,
                int_ordre,
                etat: None,
                content_html: None,
            })
    }

    fn section_with(
        sections: impl Strategy<Value = Vec<SectionDraft>>,
    ) -> impl Strategy<Value = SectionDraft> {
        (
            proptest::option::of(proptest::sample::select(SECTION_IDS.to_vec())),
            proptest::option::of("[ab ]{0,2}"),
            proptest::option::of(0_i64..3),
            proptest::collection::vec(article_draft(), 0..3),
            sections,
        )
            .prop_map(|(id, title, int_ordre, articles, sections)| SectionDraft {
                id: id.map(str::to_owned),
                title,
                int_ordre,
                etat: None,
                sections,
                articles,
            })
    }

    fn section_draft() -> impl Strategy<Value = SectionDraft> {
        section_with(Just(vec![]))
            .prop_recursive(4, 48, 3, |inner| section_with(proptest::collection::vec(inner, 0..3)))
    }

    proptest! {
        #[test]
        fn pages_partition_the_toc(
            sections in proptest::collection::vec(section_draft(), 0..4),
            articles in proptest::collection::vec(article_draft(), 0..3),
            size in 1_u32..6,
        ) {
            let toc = Toc::build(TocDraft { sections, articles }).unwrap();
            let all = toc.page(0, NonZeroU32::MAX).unwrap();
            prop_assert_eq!((all.total, all.next_offset), (toc.len(), None));
            prop_assert_eq!(toc.is_empty(), all.entries.is_empty());
            // Pages successives : au plus all.total + 1 tours, même muté.
            let mut joined = Vec::new();
            let mut next = Some(0);
            let mut turns = 0;
            while let Some(offset) = next && turns <= all.total {
                let page = toc.page(offset, limit(size)).unwrap();
                let full = page.entries.len() == usize::try_from(size).unwrap();
                prop_assert_eq!(page.total, all.total);
                prop_assert!(full || page.next_offset.is_none());
                prop_assert!(page.entries.len() <= usize::try_from(size).unwrap());
                joined.extend(page.entries);
                next = page.next_offset;
                turns += 1;
            }
            prop_assert_eq!(next, None);
            prop_assert_eq!(&joined, &all.entries);
            prop_assert!(joined.iter().all(|e| (1..=TOC_MAX_DEPTH).contains(&e.depth)));
            let ids: Vec<&str> = joined
                .iter()
                .filter_map(|entry| match entry.node {
                    TocEntryRef::Article(article) => Some(article.article_id.as_str()),
                    TocEntryRef::Section { .. } => None,
                })
                .collect();
            let unique: HashSet<&str> = ids.iter().copied().collect();
            prop_assert_eq!(unique.len(), ids.len());
        }
    }
}
