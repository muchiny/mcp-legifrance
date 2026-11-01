//! Résultats de recherche (`fr_search`, `citations.py:110-182`).

use std::fmt;

use crate::{
    contract::{Citable, CitationContract, Idcc, cut_at_word, with_company},
    date::CivilDate,
    error::{DomainError, echo},
    fond::{Fond, PaginationMode},
    id::{ArticleId, DocumentId},
    status::LegalStatus,
    text::{clean, flatten_whitespace},
    url::{ResourceKind, legifrance_url},
};

/// Longueur maximale d'un extrait, `…` compris, en caractères (`citations.py:162`).
pub const SNIPPET_MAX_CHARS: usize = 280;

/// Extrait de texte d'un résultat de recherche : nettoyé, non vide, d'au plus
/// [`SNIPPET_MAX_CHARS`] caractères (03 §7.3).
///
/// Les balises `<mark>` sont retirées et les entités décodées par [`clean`] ; au-delà de la
/// borne, le texte est coupé en fin de mot comme une citation (`…` compris dans la borne). Les
/// `[...]` que l'API insère pour marquer les coupes sont conservés.
///
/// ```
/// use lf_domain::entity::Snippet;
///
/// let snippet = Snippet::new("[...] droit au <mark>respect</mark> [...]");
/// assert_eq!(snippet.as_ref().map(Snippet::as_str), Some("[...] droit au respect [...]"));
/// assert_eq!(Snippet::new("<mark> </mark>"), None);
/// ```
///
/// `Debug` n'écrit jamais l'extrait (texte du corps), seulement sa longueur en caractères.
#[derive(Clone, PartialEq, Eq)]
pub struct Snippet(String);

impl fmt::Debug for Snippet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Snippet({} chars)", self.0.chars().count())
    }
}

impl Snippet {
    /// Nettoie `raw` puis le coupe ; `None` s'il ne reste rien.
    ///
    /// Divergence assumée : l'extrait passe par [`clean`] (balises, entités, espaces) au lieu de
    /// `strip_marks` (`citations.py:48-52`, `157`), pour un rendu homogène avec les titres ; la
    /// coupe se fait après normalisation, en fin de mot et avec `…` (la référence coupait
    /// sans le marquer). La logique vit dans `snippet_text` : cargo-mutants ne mute pas
    /// les fonctions nommées `new` (`CONTRIBUTING.md`).
    pub fn new(raw: &str) -> Option<Self> {
        snippet_text(raw).map(Self)
    }

    /// Le texte.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Texte de l'extrait : nettoyé, `None` si vide, coupé à [`SNIPPET_MAX_CHARS`].
fn snippet_text(raw: &str) -> Option<String> {
    // Un extrait tient sur une ligne : les sauts de ligne de la source (textes ACCO) deviennent
    // une espace avant la coupe, qui se fait sur les espaces.
    let text = flatten_whitespace(&clean(raw));
    if text.is_empty() {
        return None;
    }
    Some(cut_at_word(&text, SNIPPET_MAX_CHARS).unwrap_or(text))
}

/// Article trouvé dans un résultat de recherche sur les codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeArticleRef {
    /// Identifiant de l'article.
    pub article_id: ArticleId,
    /// Numéro affiché.
    pub num: Option<String>,
    /// Extrait de l'article, absent s'il est vide après nettoyage.
    pub snippet: Option<Snippet>,
}

impl CodeArticleRef {
    /// Nettoie l'extrait brut puis le coupe : voir [`Snippet::new`].
    pub fn new(article_id: ArticleId, num: Option<String>, raw_snippet: &str) -> Self {
        Self { article_id, num, snippet: Snippet::new(raw_snippet) }
    }
}

/// Un résultat de recherche.
///
/// [`SearchHit::new`] contrôle le couple fond et identifiant ; le mapper remplit ensuite les
/// champs publics, tous vides à la construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// Fond interrogé.
    pub fond: Fond,
    /// Identifiant du texte, suffixe de version retiré.
    pub id: DocumentId,
    /// Date du suffixe `_JJ-MM-AAAA` d'un identifiant `LEGITEXT`, lue par
    /// `parse_versioned` (03 §4.1) ; `None` sans suffixe, ou pour une sentinelle passée par
    /// `open_ended` (date d'entrée en vigueur non déterminée) ; rempli par le mapper ;
    /// ni cité ni rendu dans les hits (`annexes/A-parite.md` §4.6).
    pub version_start: Option<CivilDate>,
    /// Identifiant chronologique, préféré pour l'URL hors LODA/CODE.
    pub cid: Option<DocumentId>,
    /// Titre brut. Le mapper le passe par `text::clean`, car les résultats
    /// contiennent des balises `<mark>`, y compris entre crochets.
    pub title: Option<String>,
    /// Date du texte, quand la source la donne.
    pub date: Option<CivilDate>,
    /// Nature.
    pub nature: Option<String>,
    /// État juridique, déjà résolu par `status_of` (`etat`, sinon `jurisState`).
    pub etat: Option<LegalStatus>,
    /// Extrait du texte trouvé.
    pub snippet: Option<Snippet>,
    /// IDCC (KALI, ACCO).
    pub idcc: Option<Idcc>,
    /// Raison sociale (ACCO).
    pub raison_sociale: Option<String>,
    /// Articles trouvés (CODE).
    pub articles: Vec<CodeArticleRef>,
}

impl SearchHit {
    /// Résultat vide pour `fond` et `id`, dont tous les autres champs sont à compléter.
    ///
    /// # Errors
    ///
    /// [`DomainError::HitKindMismatch`] si le préfixe de `id` n'est pas celui des résultats
    /// de `fond` (`Fond::hit_prefix`).
    ///
    /// ```
    /// use lf_domain::{entity::SearchHit, fond::Fond, id::DocumentId};
    ///
    /// let id = DocumentId::parse("JURITEXT000048465611")?;
    /// assert!(SearchHit::new(Fond::Juri, id.clone(), None).is_ok());
    /// assert_eq!(
    ///     SearchHit::new(Fond::Loda, id, None).map_err(|e| e.to_string()),
    ///     Err("hit identifier \"JURITEXT000048465611\" does not belong to fond LODA".to_owned())
    /// );
    /// # Ok::<(), lf_domain::DomainError>(())
    /// ```
    pub fn new(
        fond: Fond,
        id: DocumentId,
        version_start: Option<CivilDate>,
    ) -> Result<Self, DomainError> {
        check_prefix(fond, &id)?;
        Ok(Self {
            fond,
            id,
            version_start,
            cid: None,
            title: None,
            date: None,
            nature: None,
            etat: None,
            snippet: None,
            idcc: None,
            raison_sociale: None,
            articles: Vec::new(),
        })
    }
}

/// Hors de `SearchHit::new`, que cargo-mutants ne mute jamais (`CONTRIBUTING.md`).
fn check_prefix(fond: Fond, id: &DocumentId) -> Result<(), DomainError> {
    if id.prefix() == fond.hit_prefix() {
        Ok(())
    } else {
        Err(DomainError::HitKindMismatch { id: echo(id.as_str()), fond })
    }
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
    /// Nombre total de résultats annoncé par la source (`totalResultNumber`), sinon le nombre
    /// de résultats reçus.
    pub total: u64,
    /// Nombre total d'articles trouvés (`totalArticleResultNumber`, `CODE`) ; absent s'il est
    /// nul.
    pub total_articles: Option<u64>,
    /// Résultats de la page.
    pub hits: Vec<SearchHit>,
    /// Résultats reçus mais écartés (identifiant invalide pour le fond), comptés pour les
    /// journaux.
    pub dropped: u32,
    /// `typePagination` reçu : alimente le `PaginationMemo` et les journaux, jamais rendu au
    /// modèle.
    pub pagination: PaginationMode,
}

impl SearchPage {
    /// `total` absent : on retient le nombre de résultats reçus (`citations.py:179-181`) ;
    /// `total_articles` nul : absent.
    pub fn new(
        fond: Fond,
        query: &str,
        total: Option<u64>,
        total_articles: Option<u64>,
        hits: Vec<SearchHit>,
        dropped: u32,
        pagination: PaginationMode,
    ) -> Self {
        Self {
            fond,
            query: trimmed_query(query),
            total: page_total(total, &hits),
            total_articles: positive_total(total_articles),
            hits,
            dropped,
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

/// Hors de `SearchPage::new`, que cargo-mutants ne mute jamais (`CONTRIBUTING.md`).
fn positive_total(n: Option<u64>) -> Option<u64> {
    n.filter(|&n| n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::parse_versioned;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn doc(id: &str) -> DocumentId {
        DocumentId::parse(id).unwrap()
    }

    fn hit(fond: Fond, id: &str, cid: Option<&str>) -> SearchHit {
        SearchHit {
            title: Some("Titre".into()),
            raison_sociale: Some("CORSAIR".into()),
            cid: cid.map(doc),
            ..SearchHit::new(fond, doc(id), None).unwrap()
        }
    }

    /// Identifiant valide au préfixe des résultats de `fond`.
    fn id_for(fond: Fond) -> DocumentId {
        doc(&format!("{}000012345678", fond.hit_prefix()))
    }

    #[test]
    fn loda_and_code_use_id() {
        let loda = hit(Fond::Loda, "LEGITEXT000020445850", Some("LEGITEXT000099999999"));
        assert_eq!(
            loda.contract().source_url.as_str(),
            "https://www.legifrance.gouv.fr/loda/id/LEGITEXT000020445850"
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
    fn search_hit_new_accepts_the_right_prefix_per_fond() {
        for fond in Fond::ALL {
            let id = id_for(fond);
            let built = SearchHit::new(fond, id.clone(), None);
            assert_eq!(
                built,
                Ok(SearchHit {
                    fond,
                    id,
                    version_start: None,
                    cid: None,
                    title: None,
                    date: None,
                    nature: None,
                    etat: None,
                    snippet: None,
                    idcc: None,
                    raison_sociale: None,
                    articles: vec![],
                }),
                "{fond}"
            );
        }
    }

    #[test]
    fn invalid_hit_is_rejected_with_kind_mismatch() {
        let cases = [
            (Fond::Juri, "CNILTEXT000053352643"),
            (Fond::Loda, "LEGIARTI000006419288"),
            (Fond::Code, "JORFTEXT000020445850"),
            (Fond::Kali, "ACCOTEXT000051888026"),
        ];
        for (fond, id) in cases {
            assert_eq!(
                SearchHit::new(fond, doc(id), None),
                Err(DomainError::HitKindMismatch { id: id.to_owned(), fond }),
                "{fond}"
            );
            assert_eq!(
                check_prefix(fond, &doc(id)),
                Err(DomainError::HitKindMismatch { id: id.to_owned(), fond })
            );
        }
    }

    #[test]
    fn check_prefix_accepts_each_fond_own_prefix_only() {
        for fond in Fond::ALL {
            for other in Fond::ALL {
                let expected = fond.hit_prefix() == other.hit_prefix();
                assert_eq!(check_prefix(fond, &id_for(other)).is_ok(), expected, "{fond} {other}");
            }
        }
    }

    #[test]
    fn search_hit_new_keeps_version_start() {
        let start = CivilDate::new(2016, 12, 28);
        let id = doc("LEGITEXT000033696785");
        assert_eq!(SearchHit::new(Fond::Loda, id.clone(), start).unwrap().version_start, start);
        assert_eq!(SearchHit::new(Fond::Loda, id, None).unwrap().version_start, None);
    }

    #[test]
    fn snippet_is_truncated_to_280_chars() {
        // 320 caractères : 279 gardés, coupés à la dernière espace (indice 275), puis `…`.
        let long = "mot ".repeat(80);
        let expected = format!("{}mot…", "mot ".repeat(68));
        assert_eq!(snippet_text(&long), Some(expected.clone()));
        assert_eq!(expected.chars().count(), 276);
        // 280 pile : inchangé, sans `…`.
        let exact = format!("{}mots", "mot ".repeat(69));
        assert_eq!(exact.chars().count(), SNIPPET_MAX_CHARS);
        assert_eq!(snippet_text(&exact), Some(exact.clone()));
        // 281 : coupé.
        let over = format!("{exact}s");
        assert_eq!(snippet_text(&over), Some(format!("{}mot…", "mot ".repeat(68))));
    }

    /// L3-07 (search-15) : un extrait de recherche tient sur une ligne ; les sauts de ligne et
    /// les lignes vides de la source (96 textes ACCO sur 100) deviennent une espace.
    #[test]
    fn snippet_is_flattened_to_one_line() {
        assert_eq!(
            snippet_text("définitions liées au télétravail 4\n3.1. Champ\n\n\t d'application\r\n"),
            Some("définitions liées au télétravail 4 3.1. Champ d'application".to_owned())
        );
        let snippet = Snippet::new("a\nb").unwrap();
        assert_eq!(snippet.as_str(), "a b");
        let article =
            CodeArticleRef::new(ArticleId::parse("LEGIARTI000006419288").unwrap(), None, "x\ny");
        assert_eq!(article.snippet.as_ref().map(Snippet::as_str), Some("x y"));
    }

    /// Jumeau : la coupe à 280 s'applique à l'extrait aplati, donc à la dernière espace (et non
    /// à un saut de ligne) ; la longueur reste bornée.
    #[test]
    fn a_multi_line_snippet_is_cut_after_flattening() {
        let long = "mot\n".repeat(80);
        assert_eq!(snippet_text(&long), Some(format!("{}mot…", "mot ".repeat(68))));
        let exact = format!("{}mots", "mot\n".repeat(69));
        assert_eq!(snippet_text(&exact).map(|s| s.chars().count()), Some(SNIPPET_MAX_CHARS));
    }

    #[test]
    fn snippet_cuts_hard_without_a_space_in_the_window() {
        let long = "é".repeat(300);
        let expected = format!("{}…", "é".repeat(SNIPPET_MAX_CHARS - 1));
        assert_eq!(snippet_text(&long), Some(expected));
    }

    #[test]
    fn snippet_keeps_api_ellipsis_brackets() {
        assert_eq!(
            snippet_text("[...] texte <mark>juridique</mark> [...]"),
            Some("[...] texte juridique [...]".to_owned())
        );
    }

    #[test]
    fn snippet_strips_marks_and_blank_is_none() {
        assert_eq!(
            snippet_text("  Chacun a droit au <mark>respect</mark>&nbsp;de sa vie\n "),
            Some("Chacun a droit au respect de sa vie".to_owned())
        );
        for raw in ["", "   ", "<mark></mark>", "<mark> </mark>\n"] {
            assert_eq!(snippet_text(raw), None, "{raw:?}");
            assert_eq!(Snippet::new(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn debug_never_prints_the_snippet() {
        let snippet = Snippet::new("droit au <mark>respect</mark> é").unwrap();
        assert_eq!(format!("{snippet:?}"), "Snippet(18 chars)");
        let mut found = hit(Fond::Juri, "JURITEXT000048465611", None);
        found.snippet = Some(snippet.clone());
        let reference = CodeArticleRef::new(
            ArticleId::parse("LEGIARTI000006419288").unwrap(),
            None,
            "droit au respect é",
        );
        for debug in [format!("{found:?}"), format!("{reference:?}"), format!("{found:#?}")] {
            assert!(!debug.contains("respect"), "{debug}");
            assert!(debug.contains("Snippet(") && debug.contains("chars)"), "{debug}");
        }
    }

    #[test]
    fn snippet_exposes_its_text() {
        assert_eq!(Snippet::new("<mark>a</mark>b").as_ref().map(Snippet::as_str), Some("ab"));
    }

    #[test]
    fn code_article_ref_cleans_and_cuts_the_snippet() {
        let id = ArticleId::parse("LEGIARTI000006419288").unwrap();
        let r = CodeArticleRef::new(
            id.clone(),
            Some("9".into()),
            "Chacun a droit au <mark>respect</mark>",
        );
        assert_eq!(r.snippet.as_ref().map(Snippet::as_str), Some("Chacun a droit au respect"));
        let long = "é".repeat(SNIPPET_MAX_CHARS + 10);
        let cut = CodeArticleRef::new(id.clone(), None, &long).snippet;
        assert_eq!(
            cut.as_ref().map(Snippet::as_str),
            Some(format!("{}…", "é".repeat(SNIPPET_MAX_CHARS - 1)).as_str())
        );
        assert_eq!(CodeArticleRef::new(id, None, "<mark></mark>").snippet, None);
    }

    fn page(total: Option<u64>, total_articles: Option<u64>, dropped: u32) -> SearchPage {
        let hits = vec![hit(Fond::Juri, "JURITEXT000048465611", None)];
        SearchPage::new(
            Fond::Juri,
            "  responsabilité ",
            total,
            total_articles,
            hits,
            dropped,
            PaginationMode::Defaut,
        )
    }

    #[test]
    fn page_total_defaults_to_hit_count() {
        let defaulted = page(None, None, 0);
        assert_eq!((defaulted.total, defaulted.query.as_str()), (1, "responsabilité"));
        assert_eq!(page(Some(1817), None, 0).total, 1817);
    }

    #[test]
    fn search_page_total_articles_keeps_only_positive() {
        assert_eq!(page(None, Some(0), 0).total_articles, None);
        assert_eq!(page(None, Some(3), 0).total_articles, Some(3));
        assert_eq!(page(None, None, 0).total_articles, None);
        assert_eq!(positive_total(Some(1)), Some(1));
        assert_eq!(positive_total(Some(0)), None);
        assert_eq!(positive_total(Some(u64::MAX)), Some(u64::MAX));
        assert_eq!(positive_total(None), None);
    }

    #[test]
    fn search_page_carries_dropped() {
        assert_eq!(page(None, None, 0).dropped, 0);
        assert_eq!(page(None, None, 7).dropped, 7);
    }

    #[test]
    fn page_keeps_received_pagination_mode() {
        for mode in [PaginationMode::Defaut, PaginationMode::Article] {
            let page = SearchPage::new(Fond::Code, "q", Some(0), None, vec![], 0, mode);
            assert_eq!(page.pagination, mode);
        }
    }

    proptest! {
        #[test]
        fn snippet_never_exceeds_cap(s in "\\PC{0,600}") {
            let chars = snippet_text(&s).map_or(0, |t| t.chars().count());
            prop_assert!(chars <= SNIPPET_MAX_CHARS);
        }
    }
}
