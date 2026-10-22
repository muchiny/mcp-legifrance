//! Résultats de recherche (`fr_search`, `citations.py:110-182`).

use crate::{
    contract::{Citable, CitationContract, with_company},
    date::CivilDate,
    fond::{Fond, PaginationMode},
    id::{AnyId, ArticleId},
    text::clean,
    url::{ResourceKind, legifrance_url},
};

/// Longueur maximale d'un extrait d'article, en caractères (`citations.py:162`).
pub const SNIPPET_MAX_CHARS: usize = 280;

/// Article trouvé dans un résultat de recherche sur les codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeArticleRef {
    /// Identifiant de l'article.
    pub article_id: ArticleId,
    /// Numéro affiché.
    pub num: Option<String>,
    /// Extrait nettoyé, au plus [`SNIPPET_MAX_CHARS`] caractères.
    pub snippet: String,
}

impl CodeArticleRef {
    /// Nettoie l'extrait brut puis le coupe.
    ///
    /// Divergence assumée : l'extrait passe par [`clean`] (balises, entités,
    /// espaces) au lieu de `strip_marks` (`citations.py:48-52`, `157`), pour un
    /// rendu homogène avec les titres ; la coupe à [`SNIPPET_MAX_CHARS`] se fait
    /// après normalisation.
    pub fn new(article_id: ArticleId, num: Option<String>, raw_snippet: &str) -> Self {
        Self { article_id, num, snippet: cut_snippet(raw_snippet) }
    }
}

/// Hors de `CodeArticleRef::new`, que cargo-mutants ne mute jamais (`CONTRIBUTING.md`).
fn cut_snippet(raw: &str) -> String {
    clean(raw).chars().take(SNIPPET_MAX_CHARS).collect()
}

/// Un résultat de recherche.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// Fond interrogé.
    pub fond: Fond,
    /// Identifiant du texte, suffixe de version retiré.
    pub id: AnyId,
    /// Date du suffixe `_JJ-MM-AAAA` d'un identifiant `LEGITEXT`, lue par
    /// `parse_versioned` (03 §4.1) ; `None` sans suffixe, ou pour une sentinelle passée par
    /// `open_ended` (date d'entrée en vigueur non déterminée) ; rempli par le mapper ;
    /// ni cité ni rendu dans les hits (`annexes/A-parite.md` §4.6).
    pub version_start: Option<CivilDate>,
    /// Identifiant chronologique, préféré pour l'URL hors LODA/CODE.
    pub cid: Option<AnyId>,
    /// Titre brut. Le mapper le passe par `text::clean`, car les résultats
    /// contiennent des balises `<mark>`, y compris entre crochets.
    pub title: Option<String>,
    /// Nature.
    pub nature: Option<String>,
    /// État juridique brut.
    pub etat: Option<String>,
    /// IDCC (KALI, ACCO).
    pub idcc: Option<String>,
    /// Raison sociale (ACCO).
    pub raison_sociale: Option<String>,
    /// Articles trouvés (CODE).
    pub articles: Vec<CodeArticleRef>,
}

impl Citable for SearchHit {
    fn contract(&self) -> CitationContract {
        let key = match self.fond {
            Fond::Loda | Fond::Code => &self.id,
            _ => self.cid.as_ref().unwrap_or(&self.id),
        };
        let citation = match self.fond {
            Fond::Acco => with_company(self.title.as_deref(), self.raison_sociale.as_deref()),
            _ => self.title.clone(),
        };
        CitationContract::from_url(
            legifrance_url(ResourceKind::from(self.fond), key.as_str()),
            citation.as_deref(),
            self.id.as_str(),
        )
    }
}

/// Page de résultats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPage {
    /// Fond interrogé.
    pub fond: Fond,
    /// Requête, espaces retirés.
    pub query: String,
    /// Nombre total de résultats annoncé par la source.
    pub total: u64,
    /// Résultats de la page.
    pub hits: Vec<SearchHit>,
    /// `typePagination` reçu : alimente le `PaginationMemo` et les journaux, jamais rendu au modèle.
    pub pagination: PaginationMode,
}

impl SearchPage {
    /// `total` absent : on retient le nombre de résultats reçus (`citations.py:179-181`).
    pub fn new(
        fond: Fond,
        query: &str,
        total: Option<u64>,
        hits: Vec<SearchHit>,
        pagination: PaginationMode,
    ) -> Self {
        Self {
            fond,
            query: trimmed_query(query),
            total: page_total(total, &hits),
            hits,
            pagination,
        }
    }
}

/// Hors de `SearchPage::new`, que cargo-mutants ne mute jamais (`CONTRIBUTING.md`).
fn trimmed_query(query: &str) -> String {
    query.trim().to_owned()
}

/// Hors de `SearchPage::new`, que cargo-mutants ne mute jamais (`CONTRIBUTING.md`).
fn page_total(total: Option<u64>, hits: &[SearchHit]) -> u64 {
    total.unwrap_or_else(|| u64::try_from(hits.len()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::parse_versioned;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn hit(fond: Fond, id: &str, cid: Option<&str>) -> SearchHit {
        SearchHit {
            fond,
            id: AnyId::parse(id).unwrap(),
            version_start: None,
            cid: cid.map(|c| AnyId::parse(c).unwrap()),
            title: Some("Titre".into()),
            nature: None,
            etat: None,
            idcc: None,
            raison_sociale: Some("CORSAIR".into()),
            articles: vec![],
        }
    }

    #[test]
    fn loda_and_code_use_id() {
        let loda = hit(Fond::Loda, "JORFTEXT000020445850", Some("JORFTEXT000099999999"));
        assert_eq!(
            loda.contract().source_url.as_str(),
            "https://www.legifrance.gouv.fr/loda/id/JORFTEXT000020445850"
        );
        let code = hit(Fond::Code, "LEGITEXT000006070721", Some("LEGITEXT000099999999"));
        assert_eq!(
            code.contract().source_url.as_str(),
            "https://www.legifrance.gouv.fr/codes/texte_lc/LEGITEXT000006070721"
        );
    }

    #[test]
    fn other_fonds_prefer_cid() {
        let constit = hit(Fond::Constit, "CONSTEXT000043596386", Some("CONSTEXT000011111111"));
        assert_eq!(
            constit.contract().source_url.as_str(),
            "https://www.legifrance.gouv.fr/cons/id/CONSTEXT000011111111"
        );
        let juri = hit(Fond::Juri, "JURITEXT000048465611", None);
        assert_eq!(
            juri.contract().source_url.as_str(),
            "https://www.legifrance.gouv.fr/juri/id/JURITEXT000048465611"
        );
        assert_eq!(juri.contract().citation.as_str(), "Titre");
    }

    #[test]
    fn version_start_is_not_cited() {
        // search_loda.json : `titles[0].id` porte un suffixe ; le mapper le
        // lit par `parse_versioned` et le contrat cite la forme nue.
        let versioned = parse_versioned("LEGITEXT000033696785_28-12-2016").unwrap();
        let bare = hit(Fond::Loda, versioned.id.as_str(), None);
        let dated = SearchHit { version_start: versioned.version_start, ..bare.clone() };
        assert_eq!(dated.version_start, CivilDate::new(2016, 12, 28));
        assert_eq!(dated.contract(), bare.contract());
    }

    #[test]
    fn acco_cites_company() {
        let acco = hit(Fond::Acco, "ACCOTEXT000051888026", None);
        assert_eq!(acco.contract().citation.as_str(), "Titre (CORSAIR)");
    }

    #[test]
    fn snippet_is_cleaned_and_cut() {
        let id = ArticleId::parse("LEGIARTI000006419288").unwrap();
        let r = CodeArticleRef::new(
            id.clone(),
            Some("9".into()),
            "Chacun a droit au <mark>respect</mark>",
        );
        assert_eq!(r.snippet, "Chacun a droit au respect");
        let long = "é".repeat(SNIPPET_MAX_CHARS + 10);
        assert_eq!(CodeArticleRef::new(id, None, &long).snippet.chars().count(), SNIPPET_MAX_CHARS);
    }

    #[test]
    fn page_total_defaults_to_hit_count() {
        let hits = vec![hit(Fond::Juri, "JURITEXT000048465611", None)];
        let page = SearchPage::new(
            Fond::Juri,
            "  responsabilité ",
            None,
            hits.clone(),
            PaginationMode::Defaut,
        );
        assert_eq!((page.total, page.query.as_str()), (1, "responsabilité"));
        let page = SearchPage::new(Fond::Juri, "q", Some(1817), hits, PaginationMode::Defaut);
        assert_eq!(page.total, 1817);
    }

    #[test]
    fn page_keeps_received_pagination_mode() {
        for mode in [PaginationMode::Defaut, PaginationMode::Article] {
            assert_eq!(SearchPage::new(Fond::Code, "q", Some(0), vec![], mode).pagination, mode);
        }
    }

    proptest! {
        #[test]
        fn snippet_never_exceeds_cap(s in "\\PC{0,600}") {
            let id = ArticleId::parse("LEGIARTI000006419288").unwrap();
            prop_assert!(
                CodeArticleRef::new(id, None, &s).snippet.chars().count() <= SNIPPET_MAX_CHARS
            );
        }
    }
}
