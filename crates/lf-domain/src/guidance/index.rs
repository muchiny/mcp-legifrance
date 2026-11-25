//! Index de recherche d'un public et classement des pages (05 §5.16 ; ADR-0033, décision 3).
//!
//! L'index d'un public tient le titre et le résumé (`dc:description`) de chaque fiche et page de
//! navigation de son archive. La recherche est la règle mesurée à T-MS-03 (V-229 : 17 succès sur
//! 20, la fiche attendue dans les cinq premiers), confirmée à T-MS-46 sur vingt questions
//! nouvelles (18 sur 20) :
//!
//! - mots de la page : ceux du titre, comptés [`TITLE_WEIGHT`] fois, puis ceux du résumé, sous
//!   leur forme de comparaison ([`fold_word`]) ;
//! - mots de la requête : ceux de [`GuidanceTerms::folded`], sans les mots vides de
//!   [`STOP_WORDS`], sauf s'il ne reste rien, et chacun une fois ;
//! - un mot de la requête correspond à tous les mots de l'index qu'il commence (« loyer » trouve
//!   « loyers », pas « déployer ») ;
//! - score BM25 ([`K1`], [`B`], IDF de Lucene) : pour chaque mot de la requête, la fréquence est
//!   la somme des fréquences des mots qu'il commence, et une page compte dans la fréquence
//!   documentaire si elle porte l'un d'eux ;
//! - ordre : score décroissant, puis numéro croissant ([`SheetId::number`]), puis numéro écrit.
//!
//! Avec plusieurs publics (`all`), un numéro trouvé sous Particuliers n'est rendu qu'une fois,
//! sous Particuliers : relevé du 2026-11-24, les 109 numéros communs aux deux archives y ont le
//! même titre et le même résumé.
//!
//! **Budget** : l'archive vient du réseau, et ses titres, résumés et chemins font la taille de
//! l'index. Au plus [`INDEX_WORDS_MAX`] mots distincts, [`INDEX_POSTINGS_MAX`] couples (mot, page)
//! et [`INDEX_TEXT_BYTES_MAX`] octets de texte gardés ; au-delà, la construction s'arrête
//! ([`IndexTooLarge`]) avant de lire la page suivante. Un mot de l'index ou de la requête est coupé
//! à [`INDEX_WORD_CHARS_MAX`] caractères. Relevé du 2026-11-24 : 6 334 mots, 76 955 couples et
//! 0,86 Mo de texte pour Particuliers, 3 623 mots, 23 184 couples et 0,25 Mo pour Entreprendre ;
//! aucun mot de plus de 20 caractères.

use std::{
    collections::{BTreeMap, BTreeSet, btree_map::Entry},
    num::NonZeroU32,
};

use crate::{
    date::CivilDate,
    doctrine::plain_words,
    search::PageSize,
    text::{B, K1, count_f64},
};

use super::{
    audience::{Audience, AudienceFilter},
    doc::GuidanceKind,
    id::SheetId,
    terms::{GuidanceTerms, fold_word},
    url::ServicePublicUrl,
};

/// Poids du titre : ses mots comptent trois fois.
pub const TITLE_WEIGHT: usize = 3;

/// Mots distincts d'un index, au plus : seize fois l'archive de Particuliers du 2026-11-24.
pub const INDEX_WORDS_MAX: usize = 100_000;

/// Couples (mot, page) d'un index, au plus : six fois et demie l'archive de Particuliers du
/// 2026-11-24.
pub const INDEX_POSTINGS_MAX: usize = 500_000;

/// Octets de texte gardés par un index, au plus (titres, résumés et intitulés des chemins de ses
/// pages) : près de dix fois l'archive de Particuliers du 2026-11-24.
pub const INDEX_TEXT_BYTES_MAX: usize = 8 * 1024 * 1024;

/// Caractères d'un mot de l'index ou de la requête, après repli ; au-delà, le mot est coupé, des
/// deux côtés de la même façon : un mot coupé trouve encore les mots qu'il commence.
pub const INDEX_WORD_CHARS_MAX: usize = 40;

/// Index refusé : plus de mots, de couples ou de texte que son budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "search index over budget ({words} words, {postings} postings, {text_bytes} bytes of text at \
     most)"
)]
pub struct IndexTooLarge {
    words: usize,
    postings: usize,
    text_bytes: usize,
}

/// Bornes d'un index.
#[derive(Debug, Clone, Copy)]
struct Budget {
    words: usize,
    postings: usize,
    text_bytes: usize,
}

/// Le budget de production.
const BUDGET: Budget = Budget {
    words: INDEX_WORDS_MAX,
    postings: INDEX_POSTINGS_MAX,
    text_bytes: INDEX_TEXT_BYTES_MAX,
};

/// Mots vides retirés de la requête, triés : la liste fermée de la mesure de T-MS-03, sous leur
/// forme de comparaison (« à » s'y écrit `a`).
pub const STOP_WORDS: [&str; 18] = [
    "a", "au", "aux", "d", "de", "des", "du", "en", "et", "l", "la", "le", "les", "par", "pour",
    "sur", "un", "une",
];

/// Une page telle que l'index la garde.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedPage {
    /// Numéro.
    pub id: SheetId,
    /// Nature.
    pub kind: GuidanceKind,
    /// Titre, sur une ligne.
    pub title: String,
    /// Chemin dans le site, tiré du menu ([`GuidanceMenu::path`](super::GuidanceMenu::path)) :
    /// intitulés du thème, du sous-thème et du dossier, du plus large au plus étroit ; vide hors du
    /// menu (Q41).
    pub path: Vec<String>,
    /// Résumé (`dc:description`), sur une ligne ; vide s'il manque.
    pub summary: String,
    /// Date de dernière modification importante (fiches).
    pub updated: Option<CivilDate>,
    /// Adresse publique.
    pub source_url: ServicePublicUrl,
}

impl IndexedPage {
    /// Octets de texte que l'index garde pour la page : titre, résumé et intitulés du chemin.
    fn text_bytes(&self) -> usize {
        self.path
            .iter()
            .map(String::len)
            .fold(self.title.len().saturating_add(self.summary.len()), usize::saturating_add)
    }
}

/// Une page qui porte un mot de l'index, et combien de fois.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Posting {
    page: u32,
    count: u32,
}

/// Index d'un public : ses pages, et pour chaque mot les pages qui le portent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GuidanceIndex {
    pages: Vec<IndexedPage>,
    /// Mots distincts, triés.
    words: Vec<String>,
    /// Pages de chaque mot, au même rang que `words`, par page croissante.
    postings: Vec<Vec<Posting>>,
    /// Nombre de mots de chaque page, titre compté [`TITLE_WEIGHT`] fois.
    lengths: Vec<usize>,
    /// Somme de `lengths`.
    total_length: usize,
}

impl GuidanceIndex {
    /// Construit l'index de `pages`, dans l'ordre donné. Les pages sont lues une à une : un
    /// itérateur paresseux n'est pas lu au-delà du budget.
    ///
    /// # Errors
    ///
    /// [`IndexTooLarge`] au-delà de [`INDEX_WORDS_MAX`] mots distincts, de [`INDEX_POSTINGS_MAX`]
    /// couples (mot, page) ou de [`INDEX_TEXT_BYTES_MAX`] octets de texte.
    pub fn build(pages: impl IntoIterator<Item = IndexedPage>) -> Result<Self, IndexTooLarge> {
        Self::build_within(pages, BUDGET)
    }

    /// Construit l'index de `pages` sous `budget`, arrêté au premier dépassement. La boucle seule
    /// est générique : le travail est dans [`Builder`].
    fn build_within(
        pages: impl IntoIterator<Item = IndexedPage>,
        budget: Budget,
    ) -> Result<Self, IndexTooLarge> {
        let mut builder = Builder::new(budget);
        for page in pages {
            builder.add(page)?;
        }
        Ok(builder.finish())
    }

    /// Nombre de pages.
    pub fn len(&self) -> usize {
        self.pages.len()
    }

    /// Vrai si l'index n'a aucune page.
    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    /// Les pages, dans l'ordre de construction.
    pub fn pages(&self) -> &[IndexedPage] {
        &self.pages
    }

    /// Pages qui portent au moins un mot de la requête, avec leur score strictement positif,
    /// dans l'ordre du classement.
    fn ranked(&self, terms: &GuidanceTerms) -> Vec<(&IndexedPage, f64)> {
        // Score des seules pages trouvées : le coût suit les couples lus, pas les pages.
        let mut scores: BTreeMap<u32, f64> = BTreeMap::new();
        let pages = count_f64(self.pages.len());
        let average = count_f64(self.total_length) / pages;
        for word in query_words(terms) {
            let mut frequencies: BTreeMap<u32, u32> = BTreeMap::new();
            for posting in
                self.postings.get(self.prefix_range(word)).unwrap_or_default().iter().flatten()
            {
                let frequency = frequencies.entry(posting.page).or_default();
                *frequency = frequency.saturating_add(posting.count);
            }
            let found = count_f64(frequencies.len());
            let idf = (1.0 + (pages - found + 0.5) / (found + 0.5)).ln();
            for (rank, frequency) in frequencies {
                let length = usize::try_from(rank)
                    .ok()
                    .and_then(|rank| self.lengths.get(rank))
                    .copied()
                    .unwrap_or_default();
                let norm = 1.0 - B + B * count_f64(length) / average;
                let frequency = f64::from(frequency);
                *scores.entry(rank).or_default() +=
                    idf * frequency * (K1 + 1.0) / (frequency + K1 * norm);
            }
        }
        let mut ranked: Vec<(&IndexedPage, f64)> = self
            .pages
            .iter()
            .zip(0_u32..)
            .filter_map(|(page, rank)| scores.get(&rank).map(|score| (page, *score)))
            .collect();
        ranked.sort_by(|(left, left_score), (right, right_score)| {
            right_score.total_cmp(left_score).then_with(|| by_number(&left.id, &right.id))
        });
        ranked
    }

    /// Rangs des mots de l'index que `prefix` commence : ils se suivent dans l'ordre trié.
    fn prefix_range(&self, prefix: &str) -> std::ops::Range<usize> {
        let start = self.words.partition_point(|word| word.as_str() < prefix);
        let tail = self.words.get(start..).unwrap_or_default();
        start..start.saturating_add(tail.partition_point(|word| word.starts_with(prefix)))
    }
}

/// Un index en construction, sous son budget.
struct Builder {
    budget: Budget,
    pages: Vec<IndexedPage>,
    by_word: BTreeMap<String, Vec<Posting>>,
    /// Couples (mot, page) posés.
    postings: usize,
    /// Octets de texte gardés.
    text_bytes: usize,
    lengths: Vec<usize>,
    total_length: usize,
}

impl Builder {
    fn new(budget: Budget) -> Self {
        Self {
            budget,
            pages: Vec::new(),
            by_word: BTreeMap::new(),
            postings: 0,
            text_bytes: 0,
            lengths: Vec::new(),
            total_length: 0,
        }
    }

    /// Ajoute `page`, au rang suivant ; refusé au premier dépassement du budget.
    fn add(&mut self, page: IndexedPage) -> Result<(), IndexTooLarge> {
        let budget = self.budget;
        let over = IndexTooLarge {
            words: budget.words,
            postings: budget.postings,
            text_bytes: budget.text_bytes,
        };
        self.text_bytes = self.text_bytes.saturating_add(page.text_bytes());
        if self.text_bytes > budget.text_bytes {
            return Err(over);
        }
        // Le budget de texte borne le nombre de pages bien en deçà de `u32::MAX`.
        let rank = u32::try_from(self.pages.len()).unwrap_or(u32::MAX);
        let mut counts: BTreeMap<String, u32> = BTreeMap::new();
        let mut length = 0_usize;
        let title = index_words(&page.title);
        let summary = index_words(&page.summary);
        let titles = std::iter::repeat_n(title.iter(), TITLE_WEIGHT).flatten();
        for word in titles.chain(summary.iter()) {
            let count = counts.entry(word.clone()).or_default();
            *count = count.saturating_add(1);
            length = length.saturating_add(1);
        }
        for (word, count) in counts {
            self.postings = self.postings.saturating_add(1);
            if self.postings > budget.postings {
                return Err(over);
            }
            let distinct = self.by_word.len();
            let posting = Posting { page: rank, count };
            match self.by_word.entry(word) {
                Entry::Occupied(mut known) => known.get_mut().push(posting),
                Entry::Vacant(_) if distinct >= budget.words => return Err(over),
                Entry::Vacant(new) => {
                    new.insert(vec![posting]);
                }
            }
        }
        self.lengths.push(length);
        self.total_length = self.total_length.saturating_add(length);
        self.pages.push(page);
        Ok(())
    }

    fn finish(self) -> GuidanceIndex {
        let (words, postings) = self.by_word.into_iter().unzip();
        GuidanceIndex {
            pages: self.pages,
            words,
            postings,
            lengths: self.lengths,
            total_length: self.total_length,
        }
    }
}

/// Mots de `text` sous leur forme de comparaison, dans l'ordre : la règle de la requête
/// ([`GuidanceTerms`]), chacun coupé à [`INDEX_WORD_CHARS_MAX`] caractères.
fn index_words(text: &str) -> Vec<String> {
    plain_words(text)
        .split(' ')
        .map(|word| capped(&fold_word(word)).to_owned())
        .filter(|word| !word.is_empty())
        .collect()
}

/// Les [`INDEX_WORD_CHARS_MAX`] premiers caractères de `word`.
fn capped(word: &str) -> &str {
    word.char_indices()
        .nth(INDEX_WORD_CHARS_MAX)
        .and_then(|(end, _)| word.get(..end))
        .unwrap_or(word)
}

/// Mots de la requête qui comptent : sans les mots vides, sauf s'il ne reste rien ; chacun une
/// fois, à sa première place.
fn query_words(terms: &GuidanceTerms) -> Vec<&str> {
    let mut seen = BTreeSet::new();
    let all: Vec<&str> =
        terms.folded().iter().map(|word| capped(word)).filter(|word| seen.insert(*word)).collect();
    let kept: Vec<&str> =
        all.iter().copied().filter(|word| STOP_WORDS.binary_search(word).is_err()).collect();
    if kept.is_empty() { all } else { kept }
}

/// Ordre de deux numéros à score égal : nombre croissant, puis numéro écrit (`F` avant `N`).
fn by_number(left: &SheetId, right: &SheetId) -> std::cmp::Ordering {
    left.number().cmp(&right.number()).then_with(|| left.cmp(right))
}

/// Recherche validée : des mots, les publics cherchés, une page.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GuidanceQuery {
    /// Mots cherchés.
    pub terms: GuidanceTerms,
    /// Publics cherchés.
    pub audience: AudienceFilter,
    /// Numéro de page, à partir de 1.
    pub page: NonZeroU32,
    /// Nombre de résultats par page.
    pub page_size: PageSize,
}

/// Un résultat de recherche.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuidanceHit {
    /// Public de l'archive où la page a été trouvée.
    pub audience: Audience,
    /// Numéro.
    pub id: SheetId,
    /// Nature.
    pub kind: GuidanceKind,
    /// Titre.
    pub title: String,
    /// Chemin dans le site, tiré du menu ([`GuidanceMenu::path`](super::GuidanceMenu::path)) :
    /// intitulés du thème, du sous-thème et du dossier, du plus large au plus étroit ; vide hors du
    /// menu (Q41).
    pub path: Vec<String>,
    /// Date de dernière modification importante (fiches).
    pub updated: Option<CivilDate>,
    /// Adresse publique.
    pub source_url: ServicePublicUrl,
}

/// Une page de résultats, et le nombre de résultats en tout.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GuidancePage {
    /// Nombre de résultats en tout, toutes pages confondues.
    pub total: u64,
    /// Résultats de la page demandée.
    pub hits: Vec<GuidanceHit>,
}

/// Cherche `query` dans l'index de chaque public donné, publics dans l'ordre de
/// [`AudienceFilter::audiences`], et rend la page demandée.
///
/// Un numéro déjà trouvé sous un public précédent n'est pas rendu une seconde fois. Les
/// résultats de plusieurs publics sont fusionnés dans l'ordre du classement.
pub fn search<'a>(
    indexes: impl IntoIterator<Item = (Audience, &'a GuidanceIndex)>,
    query: &GuidanceQuery,
) -> GuidancePage {
    let mut found: Vec<(Audience, &IndexedPage, f64)> = Vec::new();
    let mut seen: BTreeSet<&SheetId> = BTreeSet::new();
    for (audience, index) in indexes {
        let fresh: Vec<(&IndexedPage, f64)> = index
            .ranked(&query.terms)
            .into_iter()
            .filter(|(page, _)| !seen.contains(&page.id))
            .collect();
        seen.extend(fresh.iter().map(|(page, _)| &page.id));
        found.extend(fresh.into_iter().map(|(page, score)| (audience, page, score)));
    }
    found.sort_by(|(_, left, left_score), (_, right, right_score)| {
        right_score.total_cmp(left_score).then_with(|| by_number(&left.id, &right.id))
    });
    let total = u64::try_from(found.len()).unwrap_or(u64::MAX);
    let size = usize::try_from(query.page_size.get()).unwrap_or(usize::MAX);
    let offset = usize::try_from(query.page.get().saturating_sub(1))
        .unwrap_or(usize::MAX)
        .saturating_mul(size);
    let hits = found
        .into_iter()
        .skip(offset)
        .take(size)
        .map(|(audience, page, _)| GuidanceHit {
            audience,
            id: page.id.clone(),
            kind: page.kind,
            title: page.title.clone(),
            path: page.path.clone(),
            updated: page.updated,
            source_url: page.source_url.clone(),
        })
        .collect();
    GuidancePage { total, hits }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use regex::Regex;

    fn page(audience: Audience, id: &str, title: &str, summary: &str) -> IndexedPage {
        let id = SheetId::parse(id).unwrap();
        IndexedPage {
            source_url: ServicePublicUrl::read(None, audience, &id),
            kind: if id.is_navigation() { GuidanceKind::Folder } else { GuidanceKind::Sheet },
            id,
            title: title.to_owned(),
            path: Vec::new(),
            summary: summary.to_owned(),
            updated: None,
        }
    }

    fn index(pages: &[(&str, &str, &str)]) -> GuidanceIndex {
        GuidanceIndex::build(
            pages
                .iter()
                .map(|(id, title, summary)| page(Audience::Particuliers, id, title, summary)),
        )
        .unwrap()
    }

    fn query(words: &str, audience: AudienceFilter, page: u32, page_size: u32) -> GuidanceQuery {
        GuidanceQuery {
            terms: GuidanceTerms::parse(words).unwrap(),
            audience,
            page: NonZeroU32::new(page).unwrap(),
            page_size: PageSize::new(page_size).unwrap(),
        }
    }

    /// Numéros trouvés dans un seul index, dans l'ordre.
    fn found(index: &GuidanceIndex, words: &str) -> Vec<String> {
        let terms = GuidanceTerms::parse(words).unwrap();
        index.ranked(&terms).into_iter().map(|(page, _)| page.id.to_string()).collect()
    }

    /// `(public, numéro)` de chaque résultat, et le total.
    fn listed(page: &GuidancePage) -> (Vec<(&'static str, String)>, u64) {
        let hits = page.hits.iter().map(|hit| (hit.audience.name(), hit.id.to_string())).collect();
        (hits, page.total)
    }

    #[test]
    fn stop_words_are_sorted_folded_and_closed() {
        assert!(STOP_WORDS.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(STOP_WORDS.iter().all(|word| fold_word(word) == *word));
    }

    #[test]
    fn a_word_matches_the_words_it_starts() {
        let index = index(&[
            ("F1", "Loyer impayé", ""),
            ("F2", "Déployer un service", ""),
            ("F3", "Les LOYERS", ""),
            ("F4", "Aide", "loyer"),
        ]);
        assert_eq!(found(&index, "loyer"), ["F1", "F3", "F4"]);
        assert_eq!(found(&index, "Loyers"), ["F3"]);
        assert_eq!(found(&index, "dépl"), ["F2"]);
        assert_eq!(found(&index, "impaye"), ["F1"]);
        assert_eq!(found(&index, "absent"), Vec::<String>::new());
    }

    #[test]
    fn title_weighs_more_than_summary() {
        let index = index(&[
            ("F1", "Allocation", "Aide au logement"),
            ("F2", "Aide au logement", "Allocation"),
        ]);
        assert_eq!(found(&index, "logement"), ["F2", "F1"]);
        assert_eq!(found(&index, "allocation"), ["F1", "F2"]);
    }

    /// Scores calculés à la main. Deux pages : F1 « Allocation » / « Aide au logement » (3 + 3 =
    /// 6 mots), F2 « Aide au logement » / « Allocation » (9 + 1 = 10 mots), longueur moyenne 8.
    /// « logement » est dans les deux pages : idf = ln(1 + 0,5 / 2,5) = ln 1,2.
    /// F2 : fréquence 3, norme 0,25 + 0,75 × 10 / 8 = 1,1875 ;
    /// score = ln 1,2 × 3 × 2,2 / (3 + 1,2 × 1,1875) = ln 1,2 × 6,6 / 4,425.
    /// F1 : fréquence 1, norme 0,25 + 0,75 × 6 / 8 = 0,8125 ;
    /// score = ln 1,2 × 2,2 / (1 + 1,2 × 0,8125) = ln 1,2 × 2,2 / 1,975.
    #[test]
    fn scores_follow_bm25_computed_by_hand() {
        let index = index(&[
            ("F1", "Allocation", "Aide au logement"),
            ("F2", "Aide au logement", "Allocation"),
        ]);
        let terms = GuidanceTerms::parse("logement").unwrap();
        let scores: Vec<f64> = index.ranked(&terms).into_iter().map(|(_, score)| score).collect();
        let idf = 1.2_f64.ln();
        let expected = [idf * 6.6 / 4.425, idf * 2.2 / 1.975];
        assert_eq!(scores.len(), 2);
        for (score, expected) in scores.iter().zip(expected) {
            assert!((score - expected).abs() < 1e-12, "{score} != {expected}");
        }
    }

    #[test]
    fn stop_words_are_dropped_unless_nothing_remains() {
        let index = index(&[
            ("F1", "La carte grise", ""),
            ("F2", "La vie de famille", ""),
            ("F3", "Permis", ""),
        ]);
        // « la » est retiré : seule « carte » compte.
        assert_eq!(found(&index, "la carte"), ["F1"]);
        // Rien que des mots vides : ils comptent tous.
        assert_eq!(found(&index, "de la"), ["F2", "F1"]);
        assert_eq!(found(&index, "À"), Vec::<String>::new());
    }

    #[test]
    fn a_repeated_query_word_counts_once() {
        let index = index(&[
            ("F1", "Loyer et charges", "Le loyer du logement"),
            ("F2", "Charges de copropriété", ""),
            ("F3", "Loyer", ""),
        ]);
        let scored = |words: &str| -> Vec<(String, f64)> {
            let terms = GuidanceTerms::parse(words).unwrap();
            index
                .ranked(&terms)
                .into_iter()
                .map(|(page, score)| (page.id.to_string(), score))
                .collect()
        };
        assert_eq!(scored("loyer loyer charges"), scored("loyer charges"));
        assert_eq!(scored("de de"), scored("de"));
    }

    fn pages(titles: &[(&str, &str)]) -> Vec<IndexedPage> {
        titles.iter().map(|(id, title)| page(Audience::Particuliers, id, title, "")).collect()
    }

    #[test]
    fn the_index_stops_at_its_budget_of_distinct_words() {
        // Trois mots distincts, trois couples.
        let three = pages(&[("F1", "alpha beta"), ("F2", "gamma beta")]);
        let fits = Budget { words: 3, postings: 4, text_bytes: 100 };
        assert_eq!(
            GuidanceIndex::build_within(three.clone(), fits).map(|index| index.words.len()),
            Ok(3)
        );
        let tight = Budget { words: 2, postings: 4, text_bytes: 100 };
        assert_eq!(
            GuidanceIndex::build_within(three, tight),
            Err(IndexTooLarge { words: 2, postings: 4, text_bytes: 100 })
        );
    }

    #[test]
    fn the_index_stops_at_its_budget_of_postings() {
        // Un mot, sur deux pages : deux couples.
        let two = pages(&[("F1", "alpha"), ("F2", "alpha alpha")]);
        let fits = Budget { words: 1, postings: 2, text_bytes: 100 };
        assert_eq!(GuidanceIndex::build_within(two.clone(), fits).map(|index| index.len()), Ok(2));
        let tight = Budget { words: 1, postings: 1, text_bytes: 100 };
        let refused = GuidanceIndex::build_within(two, tight);
        assert_eq!(refused, Err(IndexTooLarge { words: 1, postings: 1, text_bytes: 100 }));
        assert_eq!(
            refused.unwrap_err().to_string(),
            "search index over budget (1 words, 1 postings, 100 bytes of text at most)"
        );
    }

    /// Le texte compté : titre, résumé et chaque intitulé du chemin, en octets (« é » en vaut deux).
    #[test]
    fn the_index_stops_at_its_budget_of_text() {
        let mut first = page(Audience::Particuliers, "F1", "Été", "abc");
        first.path = vec!["de".to_owned(), "fgh".to_owned()];
        let second = page(Audience::Particuliers, "F2", "i", "");
        assert_eq!(first.text_bytes(), 5 + 3 + 2 + 3);
        let fits = Budget { words: 10, postings: 10, text_bytes: 14 };
        let both = [first.clone(), second.clone()];
        assert_eq!(GuidanceIndex::build_within(both.clone(), fits).map(|index| index.len()), Ok(2));
        let tight = Budget { words: 10, postings: 10, text_bytes: 13 };
        assert_eq!(
            GuidanceIndex::build_within(both, tight),
            Err(IndexTooLarge { words: 10, postings: 10, text_bytes: 13 })
        );
    }

    /// Les pages d'un itérateur paresseux sont lues une à une : rien n'est lu après le dépassement.
    #[test]
    fn the_index_reads_no_page_past_its_budget() {
        let mut read = 0_usize;
        let lazy = ["F1", "F2", "F3"].into_iter().map(|id| {
            read = read.saturating_add(1);
            page(Audience::Particuliers, id, "abcd", "")
        });
        let tight = Budget { words: 10, postings: 10, text_bytes: 6 };
        assert!(GuidanceIndex::build_within(lazy, tight).is_err());
        assert_eq!(read, 2);
    }

    #[test]
    fn an_index_knows_whether_it_has_pages() {
        assert!(GuidanceIndex::default().is_empty());
        let one = index(&[("F1", "Aide", "")]);
        assert!(!one.is_empty());
        assert_eq!(one.len(), 1);
    }

    #[test]
    fn the_production_budget_is_the_measured_archive_with_its_margin() {
        assert_eq!(
            (INDEX_WORDS_MAX, INDEX_POSTINGS_MAX, INDEX_TEXT_BYTES_MAX, INDEX_WORD_CHARS_MAX),
            (100_000, 500_000, 8_388_608, 40)
        );
        assert_eq!(GuidanceIndex::build(pages(&[("F1", "alpha")])).map(|index| index.len()), Ok(1));
    }

    /// Un mot long est coupé à 40 caractères dans l'index et dans la requête : il trouve encore
    /// les mots qu'il commence, coupés ou non. Un mot hangûl se replie en jamos (trois pour « 각 »)
    /// avant d'être coupé : 14 syllabes donnent 42 jamos, et 40 restent.
    #[test]
    fn long_words_are_cut_on_both_sides() {
        let long = "a".repeat(45);
        let index = index(&[("F1", &long, ""), ("F2", "abc", ""), ("F3", &"각".repeat(14), "")]);
        let lengths: Vec<usize> = index.words.iter().map(|word| word.chars().count()).collect();
        assert_eq!(lengths, [40, 3, 40]);
        assert_eq!(fold_word(&"각".repeat(14)).chars().count(), 42);
        assert_eq!(found(&index, &"a".repeat(39)), ["F1"]);
        assert_eq!(found(&index, &"a".repeat(40)), ["F1"]);
        assert_eq!(found(&index, &"a".repeat(41)), ["F1"]);
        assert_eq!(found(&index, &format!("{}b", "a".repeat(40))), ["F1"]);
        assert_eq!(found(&index, &format!("{}b", "a".repeat(39))), Vec::<String>::new());
        assert_eq!(found(&index, "ab"), ["F2"]);
        assert_eq!(capped(&"é".repeat(41)), "é".repeat(40));
        assert_eq!(capped("abc"), "abc");
    }

    #[test]
    fn equal_scores_follow_the_number() {
        let index = index(&[
            ("N20", "Impôts", ""),
            ("F100", "Impôts", ""),
            ("F20", "Impôts", ""),
            ("F3", "Impôts", ""),
        ]);
        assert_eq!(found(&index, "impots"), ["F3", "F20", "N20", "F100"]);
    }

    #[test]
    fn an_empty_index_finds_nothing() {
        let empty = GuidanceIndex::build(Vec::new()).unwrap();
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        assert_eq!(found(&empty, "loyer"), Vec::<String>::new());
        let page = search(
            [(Audience::Particuliers, &empty)],
            &query("loyer", AudienceFilter::Particuliers, 1, 10),
        );
        assert_eq!(page, GuidancePage::default());
    }

    #[test]
    fn shared_numbers_are_listed_once_under_particuliers() {
        let part = index(&[("F2408", "Avantages en nature", ""), ("F1", "Nature", "")]);
        let pro = GuidanceIndex::build(vec![
            page(Audience::Entreprendre, "F2408", "Avantages en nature", ""),
            page(Audience::Entreprendre, "F5", "Nature et avantages", "avantages en nature"),
        ])
        .unwrap();
        let all = query("avantages nature", AudienceFilter::All, 1, 10);
        let both = [(Audience::Particuliers, &part), (Audience::Entreprendre, &pro)];
        let page = search(both, &all);
        assert_eq!(
            listed(&page),
            (
                vec![
                    ("particuliers", "F2408".to_owned()),
                    ("entreprendre", "F5".to_owned()),
                    ("particuliers", "F1".to_owned())
                ],
                3
            )
        );
        assert_eq!(
            page.hits.first().map(|hit| hit.source_url.as_str()),
            Some("https://www.service-public.gouv.fr/particuliers/vosdroits/F2408")
        );
        // Entreprendre seul : sa propre fiche F2408, rendue sous Entreprendre.
        let alone = search(
            [(Audience::Entreprendre, &pro)],
            &query("avantages nature", AudienceFilter::Entreprendre, 1, 10),
        );
        assert_eq!(
            listed(&alone),
            (vec![("entreprendre", "F5".to_owned()), ("entreprendre", "F2408".to_owned())], 2)
        );
    }

    #[test]
    fn pages_follow_the_order() {
        let pages: Vec<(String, String)> =
            (1..=25).map(|n| (format!("F{n}"), "Démarche".to_owned())).collect();
        let pairs: Vec<(&str, &str, &str)> =
            pages.iter().map(|(id, title)| (id.as_str(), title.as_str(), "")).collect();
        let index = index(&pairs);
        let numbers = |page: u32, size: u32| {
            let result = search(
                [(Audience::Particuliers, &index)],
                &query("demarche", AudienceFilter::Particuliers, page, size),
            );
            (result.hits.iter().map(|hit| hit.id.number()).collect::<Vec<u32>>(), result.total)
        };
        assert_eq!(numbers(1, 10), ((1..=10).collect(), 25));
        assert_eq!(numbers(3, 10), ((21..=25).collect(), 25));
        assert_eq!(numbers(4, 10), (Vec::new(), 25));
        assert_eq!(numbers(2, 24), (vec![25], 25));
        assert_eq!(numbers(u32::MAX, 100), (Vec::new(), 25));
    }

    #[test]
    fn hits_carry_what_the_index_holds() {
        let mut sheet =
            page(Audience::Particuliers, "F1437", "Solde bancaire insaisissable (SBI)", "Saisie");
        sheet.updated = CivilDate::new(2026, 4, 1);
        let index = GuidanceIndex::build(vec![sheet.clone()]).unwrap();
        assert_eq!(index.pages(), [sheet.clone()]);
        let result = search(
            [(Audience::Particuliers, &index)],
            &query("solde", AudienceFilter::Particuliers, 1, 10),
        );
        assert_eq!(
            result.hits,
            [GuidanceHit {
                audience: Audience::Particuliers,
                id: sheet.id,
                kind: GuidanceKind::Sheet,
                title: sheet.title,
                path: sheet.path,
                updated: sheet.updated,
                source_url: sheet.source_url,
            }]
        );
    }

    /// Pages d'un public tirées des fixtures de T-MS-02 : titre, résumé et nature lus dans le
    /// XML, entités décodées.
    fn fixture_index(audience: Audience, files: &[&str]) -> GuidanceIndex {
        let field = |xml: &str, name: &str| {
            Regex::new(&format!("(?s)<{name}>(.*?)</{name}>")).unwrap().captures(xml).map(|found| {
                found[1]
                    .replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&quot;", "\"")
                    .replace("&apos;", "'")
                    .replace("&amp;", "&")
            })
        };
        GuidanceIndex::build(files.iter().map(|xml| {
            let id = field(xml, "dc:identifier").unwrap();
            let mut page = page(
                audience,
                &id,
                &field(xml, "dc:title").unwrap(),
                &field(xml, "dc:description").unwrap(),
            );
            if xml.contains(r#" type="Theme""#) {
                page.kind = GuidanceKind::Theme;
            }
            page
        }))
        .unwrap()
    }

    /// Un petit index tiré des fixtures : ordres attendus calculés hors du dépôt par une copie
    /// indépendante de la formule de la mesure (`python3 -I`), scores arrondis notés à côté.
    #[test]
    fn fixture_index_orders_as_computed_by_hand() {
        let part = fixture_index(
            Audience::Particuliers,
            &[
                include_str!(
                    "../../../lf-adapter-service-public/tests/fixtures/xml/part/F10041.xml"
                ),
                include_str!(
                    "../../../lf-adapter-service-public/tests/fixtures/xml/part/F1239.xml"
                ),
                include_str!(
                    "../../../lf-adapter-service-public/tests/fixtures/xml/part/F1437.xml"
                ),
                include_str!(
                    "../../../lf-adapter-service-public/tests/fixtures/xml/part/F1903.xml"
                ),
                include_str!(
                    "../../../lf-adapter-service-public/tests/fixtures/xml/part/F2408.xml"
                ),
                include_str!(
                    "../../../lf-adapter-service-public/tests/fixtures/xml/part/F31613.xml"
                ),
                include_str!(
                    "../../../lf-adapter-service-public/tests/fixtures/xml/part/F34680.xml"
                ),
                include_str!("../../../lf-adapter-service-public/tests/fixtures/xml/part/F468.xml"),
                include_str!(
                    "../../../lf-adapter-service-public/tests/fixtures/xml/part/N19804.xml"
                ),
                include_str!(
                    "../../../lf-adapter-service-public/tests/fixtures/xml/part/N31675.xml"
                ),
            ],
        );
        let pro = fixture_index(
            Audience::Entreprendre,
            &[
                include_str!("../../../lf-adapter-service-public/tests/fixtures/xml/pro/F2408.xml"),
                include_str!(
                    "../../../lf-adapter-service-public/tests/fixtures/xml/pro/F37954.xml"
                ),
            ],
        );
        assert_eq!(part.len(), 10);
        // 3,8876 ; 2,8786 ; 1,1562.
        assert_eq!(found(&part, "compte bancaire"), ["F1437", "N31675", "F1903"]);
        // Mots vides seuls : 1,1507 ; 0,9892 ; 0,9227 ; 0,6975.
        assert_eq!(found(&part, "la"), ["F1437", "F468", "F10041", "F1903"]);
        assert_eq!(found(&part, "saisie sur salaire"), ["F1437"]);
        assert_eq!(found(&part, "temps de travail"), ["F1903"]);
        assert_eq!(found(&part, "salaire"), Vec::<String>::new());
        assert!(
            part.pages()
                .iter()
                .any(|page| page.kind == GuidanceKind::Theme && page.id.as_str() == "N19804")
        );
        // Les deux publics : 2,6581 et 2,2629 sous Particuliers ; F2408 (1,1690) écartée
        // d'Entreprendre ; puis 1,0939 sous Entreprendre.
        let both = [(Audience::Particuliers, &part), (Audience::Entreprendre, &pro)];
        let page = search(both, &query("professionnel entrepreneur", AudienceFilter::All, 1, 10));
        assert_eq!(
            listed(&page).0,
            [
                ("particuliers", "F10041".to_owned()),
                ("particuliers", "F2408".to_owned()),
                ("entreprendre", "F37954".to_owned()),
            ]
        );
    }

    proptest! {
        /// Quel que soit l'index, chaque page trouvée porte un mot qui commence par un mot de la
        /// requête, l'ordre suit les scores, et la pagination découpe la même liste.
        #[test]
        fn search_finds_only_pages_with_a_matching_word(
            titles in prop::collection::vec("[a-zé ]{0,20}", 0..12),
            words in "[a-zé]{1,4}( [a-zé]{1,4}){0,2}",
            size in 1_u32..5,
        ) {
            let pages: Vec<(String, String)> =
                titles.iter().enumerate().map(|(n, title)| (format!("F{n}"), title.clone())).collect();
            let pairs: Vec<(&str, &str, &str)> =
                pages.iter().map(|(id, title)| (id.as_str(), title.as_str(), "")).collect();
            let index = index(&pairs);
            let terms = GuidanceTerms::parse(&words).unwrap();
            let wanted = query_words(&terms);
            let ranked = index.ranked(&terms);
            for (page, score) in &ranked {
                prop_assert!(*score > 0.0);
                let title = index_words(&page.title);
                prop_assert!(wanted.iter().any(|word| title.iter().any(|found| found.starts_with(word))));
            }
            prop_assert!(ranked.windows(2).all(|pair| pair[0].1 >= pair[1].1));
            let all = search([(Audience::Particuliers, &index)], &query(&words, AudienceFilter::Particuliers, 1, 100));
            let mut paged = Vec::new();
            for number in 1..=12_u32 {
                paged.extend(search([(Audience::Particuliers, &index)], &query(&words, AudienceFilter::Particuliers, number, size)).hits);
            }
            prop_assert_eq!(paged, all.hits.clone());
            prop_assert_eq!(all.total, u64::try_from(ranked.len()).unwrap());
        }
    }
}
