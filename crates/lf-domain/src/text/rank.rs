//! Classement BM25 des passages pour le paramètre `focus` (jalon M5).
//!
//! Okapi BM25, IDF de Lucene `ln(1 + (N - df + 0.5) / (df + 0.5))` (toujours
//! positif), `k1 = 1.2`, `b = 0.75`.

use std::collections::HashSet;

use super::fold::tokens;

/// Saturation de la fréquence des termes.
pub const K1: f64 = 1.2;
/// Normalisation par la longueur.
pub const B: f64 = 0.75;

/// Passage classé.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scored {
    /// Indice du passage dans l'entrée.
    pub index: usize,
    /// Score BM25, strictement positif.
    pub score: f64,
}

/// Les `k` meilleurs passages pour `query`, score décroissant puis indice
/// croissant ; les passages sans terme commun sont exclus.
#[allow(clippy::cast_precision_loss)] // comptes de passages et de jetons ≪ 2^52
pub fn rank(passages: &[&str], query: &str, k: usize) -> Vec<Scored> {
    let docs: Vec<Vec<String>> = passages.iter().map(|p| tokens(p)).collect();
    let terms: HashSet<String> = tokens(query).into_iter().collect();
    let total_len: usize = docs.iter().map(Vec::len).sum();
    // Aucun jeton (y compris aucun passage) : rien à classer, et pas de 0/0.
    // Une requête sans jeton donne des scores nuls, filtrés plus bas.
    if total_len == 0 {
        return Vec::new();
    }
    let n = docs.len() as f64;
    let avg_len = total_len as f64 / n;
    let mut scored: Vec<Scored> = docs
        .iter()
        .enumerate()
        .filter_map(|(index, doc)| {
            let len_norm = 1.0 - B + B * doc.len() as f64 / avg_len;
            let score: f64 = terms
                .iter()
                .map(|term| {
                    let tf = doc.iter().filter(|t| *t == term).count() as f64;
                    let df = docs.iter().filter(|d| d.contains(term)).count() as f64;
                    let idf = (1.0 + (n - df + 0.5) / (df + 0.5)).ln();
                    idf * tf * (K1 + 1.0) / (tf + K1 * len_norm)
                })
                .sum();
            (score > 0.0).then_some(Scored { index, score })
        })
        .collect();
    scored.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.index.cmp(&b.index)));
    scored.truncate(k);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::collection::vec;
    use proptest::prelude::*;

    /// Score arrondi à 12 décimales, comparable avec `assert_eq!`.
    fn r12(score: f64) -> String {
        format!("{score:.12}")
    }

    #[test]
    fn single_term_score_matches_hand_computation() {
        // N = 2, df = 1 : idf = ln(1 + 1.5 / 1.5) = ln 2.
        // doc 0 : 2 jetons, avg = 2.5 : len_norm = 0.25 + 0.75 * 0.8 = 0.85.
        // score = ln 2 * 1 * 2.2 / (1 + 1.2 * 0.85) = ln 2 * 2.2 / 2.02.
        let ranked = rank(&["droit civil", "code pénal article"], "civil", 5);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].index, 0);
        assert_eq!(r12(ranked[0].score), r12(2.0f64.ln() * 2.2 / 2.02));
    }

    #[test]
    fn term_frequency_and_length_matter() {
        // N = 3, df("bail") = 2 : idf = ln(1 + 1.5 / 2.5) = ln 1.6 ; avg = 8 / 3.
        let ranked = rank(&["bail bail commercial", "bail rural de location", "vente"], "bail", 5);
        let idf = 1.6f64.ln();
        let avg = 8.0 / 3.0;
        let s0 = idf * 2.0 * 2.2 / (2.0 + 1.2 * (0.25 + 0.75 * 3.0 / avg));
        let s1 = idf * 1.0 * 2.2 / (1.0 + 1.2 * (0.25 + 0.75 * 4.0 / avg));
        assert_eq!(ranked.iter().map(|s| s.index).collect::<Vec<_>>(), [0, 1]);
        assert_eq!(r12(ranked[0].score), r12(s0));
        assert_eq!(r12(ranked[1].score), r12(s1));
    }

    #[test]
    fn multiple_terms_add_up_and_fold_accents() {
        let ranked = rank(
            &["responsabilité civile délictuelle", "responsabilite", "autre chose"],
            "Responsabilité CIVILE",
            5,
        );
        assert_eq!(ranked.iter().map(|s| s.index).collect::<Vec<_>>(), [0, 1]);
        assert!(ranked[0].score > ranked[1].score);
    }

    #[test]
    fn ties_keep_input_order_and_k_truncates() {
        let ranked = rank(&["x loyer", "loyer x", "rien"], "loyer", 1);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].index, 0);
    }

    #[test]
    fn degenerate_inputs_give_nothing() {
        assert_eq!(rank(&[], "loyer", 5), []);
        assert_eq!(rank(&["", "a"], "loyer", 5), []);
        assert_eq!(rank(&["loyer"], "a", 5), []);
        assert_eq!(rank(&["loyer"], "bail", 5), []);
        assert_eq!(rank(&["loyer"], "loyer", 0), []);
    }

    proptest! {
        /// Le premier passage est dupliqué en dernier et la requête est son
        /// premier mot : au moins deux résultats (k >= 2), donc les boucles de
        /// vérification s'exécutent à coup sûr.
        #[test]
        fn ranking_is_bounded_positive_and_sorted(
            mut docs in vec("[a-zé]{2,6}( [a-zé]{2,6}){0,4}", 1..8),
            k in 2usize..10,
        ) {
            docs.push(docs[0].clone());
            let query = docs[0].split(' ').next().unwrap_or_default().to_owned();
            let refs: Vec<&str> = docs.iter().map(String::as_str).collect();
            let ranked = rank(&refs, &query, k);
            prop_assert!(ranked.len() >= 2, "expected at least two results");
            prop_assert!(ranked.len() <= k);
            for s in &ranked {
                prop_assert!(s.score > 0.0 && s.score.is_finite(), "bad score {}", s.score);
                prop_assert!(s.index < docs.len(), "index out of range: {}", s.index);
            }
            for pair in ranked.windows(2) {
                prop_assert!(pair[0].score >= pair[1].score, "scores not sorted");
            }
        }
    }
}
