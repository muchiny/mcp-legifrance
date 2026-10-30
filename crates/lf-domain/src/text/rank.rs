//! Classement BM25 des passages pour le paramètre `focus` (jalon M5).
//!
//! Règle nouvelle, absente de la référence (qui n'a pas de BM25).
//!
//! Okapi BM25, IDF de Lucene `ln(1 + (N - df + 0.5) / (df + 0.5))` (toujours
//! positif), `k1 = 1.2`, `b = 0.75`.
//!
//! Tokenisation (`legal_tokens`, 03 §7.8) : minuscules, accents français repliés, découpe sur
//! les non-alphanumériques, jetons d'au moins 2 caractères, sauf que des chiffres séparés par
//! `.`, `-` ou `/` restent **un seul jeton** (`24-13.470`, `2025-1173`, `761-1`, `85/374`) : un
//! numéro juridique se retrouve en entier ou pas du tout, jamais par ses morceaux.

use std::{collections::BTreeSet, ops::Range};

use super::fold::{fold, tokens as word_tokens};

/// Séparateurs qui laissent deux chiffres dans un même jeton (`24-13.470`, `85/374`).
const NUMBER_JOINERS: [char; 3] = ['.', '-', '/'];

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

/// Étendues, en octets, des numéros juridiques de `s` (déjà replié) : une suite maximale de
/// caractères alphanumériques où au moins un `.`, `-` ou `/` sépare deux chiffres ASCII.
///
/// Un séparateur de bord (`12.`, `-12`), double (`12--34`) ou voisin d'une lettre (`12-ab`,
/// `ab-12`) ne joint rien. Un préfixe de lettres collé aux chiffres reste dans le numéro
/// (`l761-1`).
fn number_spans(s: &str) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    // Début de la suite alphanumérique en cours, et présence d'un séparateur joint dans cette suite.
    let mut run = None;
    let mut joined = false;
    let mut prev = ' ';
    let mut chars = s.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if c.is_alphanumeric() {
            run.get_or_insert(at);
        } else if NUMBER_JOINERS.contains(&c)
            && prev.is_ascii_digit()
            && chars.peek().is_some_and(|&(_, next)| next.is_ascii_digit())
        {
            joined = true;
        } else {
            if let (Some(start), true) = (run.take(), joined) {
                spans.push(start..at);
            }
            joined = false;
        }
        prev = c;
    }
    if let (Some(start), true) = (run, joined) {
        spans.push(start..s.len());
    }
    spans
}

/// Jetons repliés de `s` : ceux de `fold::tokens` (au moins 2 caractères), sauf que les
/// numéros juridiques (`24-13.470`, `2025-1173`, `761-1`, `85/374`) forment chacun un seul
/// jeton. Ils servent aux passages, à la requête et aux termes de `Focus` : redécouper des
/// termes joints par des espaces redonne donc les mêmes termes.
pub(crate) fn legal_tokens(s: &str) -> Vec<String> {
    let folded = fold(s);
    let mut out = Vec::new();
    let mut from = 0;
    for span in number_spans(&folded) {
        out.extend(word_tokens(&folded[from..span.start]));
        out.push(folded[span.clone()].to_owned());
        from = span.end;
    }
    out.extend(word_tokens(&folded[from..]));
    out
}

/// Termes distincts de la requête, en ordre trié : la somme flottante des
/// scores est ainsi reproductible d'une exécution à l'autre.
fn query_terms(query: &str) -> BTreeSet<String> {
    legal_tokens(query).into_iter().collect()
}

/// Compte converti en flottant : les comptes de passages et de jetons sont ≪ 2^52, la conversion est exacte.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "aucune conversion `usize` vers `f64` sans perte n'existe ; comptes ≪ 2^52"
)]
fn count_f64(n: usize) -> f64 {
    n as f64
}

/// Les `k` meilleurs passages pour `query`, score décroissant puis indice
/// croissant. Le score est strictement positif : un passage sans terme commun
/// avec la requête est exclu, et une requête sans jeton donne un résultat vide.
///
/// Coût linéaire en passages × termes : l'idf de chaque terme est calculé une
/// seule fois, hors de la boucle des passages.
pub fn rank(passages: &[&str], query: &str, k: usize) -> Vec<Scored> {
    let docs: Vec<Vec<String>> = passages.iter().map(|p| legal_tokens(p)).collect();
    let total_len: usize = docs.iter().map(Vec::len).sum();
    // Aucun jeton (y compris aucun passage) : rien à classer, et pas de 0/0.
    // Une requête sans jeton donne des scores nuls, filtrés plus bas.
    if total_len == 0 {
        return Vec::new();
    }
    let n = count_f64(docs.len());
    let avg_len = count_f64(total_len) / n;
    let idf: Vec<(String, f64)> = query_terms(query)
        .into_iter()
        .map(|term| {
            let df = docs.iter().filter(|d| d.contains(&term)).count();
            let df = count_f64(df);
            let idf = (1.0 + (n - df + 0.5) / (df + 0.5)).ln();
            (term, idf)
        })
        .collect();
    let mut scored: Vec<Scored> = docs
        .iter()
        .enumerate()
        .filter_map(|(index, doc)| {
            let len_norm = 1.0 - B + B * count_f64(doc.len()) / avg_len;
            let score: f64 = idf
                .iter()
                .map(|(term, idf)| {
                    let tf = count_f64(doc.iter().filter(|t| *t == term).count());
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
    use proptest::sample::select;

    /// Petit vocabulaire de mots et de numéros juridiques : des phrases tirées de ce
    /// vocabulaire se recouvrent souvent, donc le classement rend de vrais résultats.
    fn sentence() -> impl Strategy<Value = String> {
        const WORDS: [&str; 8] =
            ["bail", "loyer", "24-13.470", "2025-1173", "761-1", "85/374", "civil", "12.5"];
        vec(select(WORDS.to_vec()), 1..6).prop_map(|words| words.join(" "))
    }

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

    #[test]
    fn query_terms_are_sorted_and_deduplicated() {
        // Un `HashSet` ne tomberait dans cet ordre qu'une fois sur 720.
        assert_eq!(
            query_terms("ff ee dd cc bb aa bb").into_iter().collect::<Vec<_>>(),
            ["aa", "bb", "cc", "dd", "ee", "ff"]
        );
    }

    #[test]
    fn score_sums_terms_in_sorted_order() {
        // Fixture sensible à l'ordre : la somme dans l'ordre trié (aa, bb, cc) et
        // dans l'ordre (bb, cc, aa) diffèrent d'un bit.
        let passages = ["bb dd ee cc bb aa", "aa ee bb ee dd", "ee ee", "dd"];
        let n = 4.0f64;
        let avg = 14.0 / n;
        let len_norm = 1.0 - B + B * 6.0 / avg;
        let term_score = |df: f64, tf: f64| {
            let idf = (1.0 + (n - df + 0.5) / (df + 0.5)).ln();
            idf * tf * (K1 + 1.0) / (tf + K1 * len_norm)
        };
        // Passage 0 : aa (df 2, tf 1), bb (df 2, tf 2), cc (df 1, tf 1).
        let (aa, bb, cc) = (term_score(2.0, 1.0), term_score(2.0, 2.0), term_score(1.0, 1.0));
        let sorted = aa + bb + cc;
        let unsorted = bb + cc + aa;
        assert_eq!(sorted.to_bits(), 0x4002_1817_8936_d1f1);
        assert_eq!(unsorted.to_bits(), 0x4002_1817_8936_d1f0);
        assert_ne!(sorted.to_bits(), unsorted.to_bits());
        for query in ["cc bb aa", "aa bb cc", "bb aa cc cc"] {
            let ranked = rank(&passages, query, 4);
            let first = ranked.iter().find(|s| s.index == 0).map(|s| s.score.to_bits());
            assert_eq!(first, Some(sorted.to_bits()));
        }
    }

    #[test]
    fn duplicate_query_terms_count_once() {
        let docs = ["loyer loyer", "bail loyer"];
        assert_eq!(rank(&docs, "loyer loyer", 5), rank(&docs, "loyer", 5));
    }

    #[test]
    fn term_in_every_passage_still_scores() {
        // df = N : l'idf de Lucene reste strictement positif.
        assert_eq!(rank(&["loyer", "loyer bail"], "loyer", 5).len(), 2);
    }

    #[test]
    fn empty_passage_counts_in_corpus_stats() {
        // N = 2, df = 1 : idf = ln 2 ; avg = 0.5 ; len_norm = 0.25 + 0.75 * 1 / 0.5 = 1.75.
        let ranked = rank(&["", "loyer"], "loyer", 5);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].index, 1);
        assert_eq!(r12(ranked[0].score), r12(2.0f64.ln() * 2.2 / (1.0 + 1.2 * 1.75)));
    }

    #[test]
    fn legal_numbers_are_single_tokens() {
        // Les quatre formes de 03 §7.8 : décret, loi, article de code, directive.
        for number in ["24-13.470", "2025-1173", "761-1", "85/374"] {
            assert_eq!(legal_tokens(number), [number], "{number}");
        }
        assert_eq!(
            legal_tokens(
                "Décision n° 2025-1173 QPC ; art. 761-1, loi 24-13.470 et directive 85/374."
            ),
            [
                "decision",
                "2025-1173",
                "qpc",
                "art",
                "761-1",
                "loi",
                "24-13.470",
                "et",
                "directive",
                "85/374"
            ]
        );
    }

    #[test]
    fn a_number_is_one_token_wherever_it_stands() {
        // Début, milieu et fin de texte, ponctuation collée, plusieurs séparateurs.
        assert_eq!(legal_tokens("761-1"), ["761-1"]);
        assert_eq!(legal_tokens("(761-1)"), ["761-1"]);
        assert_eq!(legal_tokens("« 85/374 »"), ["85/374"]);
        assert_eq!(legal_tokens("du 01/05/2023."), ["du", "01/05/2023"]);
        assert_eq!(legal_tokens("version 1.2.3"), ["version", "1.2.3"]);
        // Un préfixe de lettres collé au premier chiffre reste dans le jeton.
        assert_eq!(legal_tokens("L761-1"), ["l761-1"]);
        assert_eq!(legal_tokens("L. 761-1"), ["761-1"]);
    }

    #[test]
    fn separators_between_anything_but_digits_still_split() {
        // Apostrophes et tirets entre lettres séparent ; un séparateur de bord n'est pas joint.
        assert_eq!(legal_tokens("l'article"), ["article"]);
        assert_eq!(legal_tokens("bail-mobilité"), ["bail", "mobilite"]);
        assert_eq!(legal_tokens("12."), ["12"]);
        assert_eq!(legal_tokens("12-"), ["12"]);
        assert_eq!(legal_tokens("-12"), ["12"]);
        assert_eq!(legal_tokens("12--34"), ["12", "34"]);
        assert_eq!(legal_tokens("12-ab"), ["12", "ab"]);
        assert_eq!(legal_tokens("ab-12"), ["ab", "12"]);
        assert_eq!(legal_tokens("12 - 34"), ["12", "34"]);
        assert_eq!(legal_tokens("12,5"), ["12"]);
        // Tiret demi-cadratin : ce n'est pas un des trois séparateurs de numéro.
        assert_eq!(legal_tokens("2025–1173"), ["2025", "1173"]);
    }

    #[test]
    fn plain_words_keep_the_legacy_tokenization() {
        assert_eq!(
            legal_tokens("L'article L. 1234-5 du Code « civil »"),
            ["article", "1234-5", "du", "code", "civil"]
        );
        assert_eq!(legal_tokens("Œuvre d'ÉTAT"), ["oeuvre", "etat"]);
        assert_eq!(legal_tokens(""), Vec::<String>::new());
        assert_eq!(legal_tokens(" - . / "), Vec::<String>::new());
    }

    #[test]
    fn a_number_is_searchable_as_a_whole_only() {
        let passages = [
            "Décision n° 2025-1173 QPC du 12 juin",
            "Décision 2025 QPC puis 1173 autres décisions",
            "Article 761-1 du code de procédure civile",
            "Article 761 du code",
            "Directive 85/374/CEE",
        ];
        let hits = |query: &str| -> Vec<usize> {
            rank(&passages, query, 5).iter().map(|s| s.index).collect()
        };
        assert_eq!(hits("2025-1173"), [0]);
        assert_eq!(hits("761-1"), [2]);
        assert_eq!(hits("85/374"), [4]);
        // Un numéro qui n'apparaît nulle part n'accroche rien, ni par ses chiffres isolés.
        assert_eq!(hits("2025-1174"), Vec::<usize>::new());
        assert_eq!(hits("761-2"), Vec::<usize>::new());
    }

    #[test]
    fn rank_is_deterministic_and_within_budget() {
        let passages: Vec<String> = (0..120)
            .map(|i| {
                format!(
                    "paragraphe {i} sur le bail {} et l'article {}-{} ou la loi 2025-{}",
                    ["civil", "commercial", "rural"][i % 3],
                    700 + i % 7,
                    1 + i % 5,
                    1000 + i % 11,
                )
            })
            .collect();
        let refs: Vec<&str> = passages.iter().map(String::as_str).collect();
        for (query, k) in [
            ("bail commercial", 3),
            ("article 703-4", 10),
            ("2025-1003 bail", 1),
            ("la loi 2025-1004", 50),
            ("inconnu", 5),
        ] {
            let first = rank(&refs, query, k);
            // Déterminisme : même sortie, bit à bit, à chaque appel et quel que soit l'ordre
            // ou la répétition des mots de la requête.
            assert_eq!(rank(&refs, query, k), first, "{query}");
            let reversed = query.split(' ').rev().chain(query.split(' ')).collect::<Vec<_>>();
            assert_eq!(rank(&refs, &reversed.join(" "), k), first, "{query}");
            // Budget : au plus `k` résultats, triés, strictement positifs.
            let count = first.len();
            assert!(count <= k, "{query}: {count} > {k}");
            // Ordre strict : score décroissant, puis indice croissant.
            assert!(first.windows(2).all(|w| {
                w[1].score.total_cmp(&w[0].score).then(w[0].index.cmp(&w[1].index)).is_lt()
            }));
            // Un passage sans aucun jeton de la requête n'est jamais rendu.
            let wanted: BTreeSet<String> = legal_tokens(query).into_iter().collect();
            for scored in &first {
                assert!(scored.score > 0.0 && scored.score.is_finite(), "{query}");
                let passage = refs[scored.index];
                let own = legal_tokens(passage);
                assert!(own.iter().any(|t| wanted.contains(t)), "{query}: {passage}");
            }
        }
        // Les bornes se lisent aussi aux extrémités : `k = 0` ne rend rien, `k` grand rend tous
        // les passages qui portent un terme.
        assert_eq!(rank(&refs, "bail", 0), []);
        assert_eq!(rank(&refs, "bail", usize::MAX).len(), 120);
    }

    proptest! {
        /// Idempotence : re-découper un jeton rend ce jeton (un focus joint par des espaces
        /// et re-découpé par `rank` garde donc les mêmes termes).
        #[test]
        fn tokens_are_stable_under_retokenization(s in "\\PC{0,120}") {
            for token in legal_tokens(&s) {
                prop_assert_eq!(legal_tokens(&token), vec![token.clone()]);
            }
        }

        /// Chaque jeton compte au moins 2 caractères, tous alphanumériques ou séparateurs de
        /// numéro, et un séparateur n'ouvre ni ne ferme un jeton et ne touche que des chiffres.
        #[test]
        fn tokens_are_wellformed(s in "[a-zé0-9 .,/'-]{0,80}") {
            for token in legal_tokens(&s) {
                prop_assert!(token.chars().count() >= 2, "token too short: {token:?}");
                let chars: Vec<char> = token.chars().collect();
                for (i, c) in chars.iter().enumerate() {
                    if NUMBER_JOINERS.contains(c) {
                        let around = (i.checked_sub(1).map(|j| chars[j]), chars.get(i + 1));
                        prop_assert!(
                            matches!(around, (Some(a), Some(b)) if a.is_ascii_digit() && b.is_ascii_digit()),
                            "separator outside digits in {token:?}"
                        );
                    } else {
                        prop_assert!(c.is_alphanumeric(), "bad char in {token:?}");
                    }
                }
            }
        }

        /// Même propriété que `ranking_is_bounded_positive_and_sorted`, sur un petit vocabulaire
        /// de mots et de numéros (les recouvrements sont donc fréquents) : au plus `k`
        /// résultats, ordre strict (score décroissant, puis indice croissant), et aucune sortie
        /// sans jeton commun avec la requête.
        #[test]
        fn ranking_over_numbers_is_deterministic_and_bounded(
            docs in vec(sentence(), 1..8),
            query in sentence(),
            k in 0usize..6,
        ) {
            let refs: Vec<&str> = docs.iter().map(String::as_str).collect();
            let ranked = rank(&refs, &query, k);
            prop_assert_eq!(&rank(&refs, &query, k), &ranked);
            prop_assert!(ranked.len() <= k);
            let wanted: BTreeSet<String> = legal_tokens(&query).into_iter().collect();
            for scored in &ranked {
                prop_assert!(scored.score > 0.0 && scored.score.is_finite());
                let passage = refs[scored.index];
                prop_assert!(legal_tokens(passage).iter().any(|t| wanted.contains(t)));
            }
            for pair in ranked.windows(2) {
                let order = pair[1].score.total_cmp(&pair[0].score).then(pair[0].index.cmp(&pair[1].index));
                prop_assert!(order.is_lt());
            }
        }

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
                let Scored { index, score } = *s;
                prop_assert!(score > 0.0 && score.is_finite(), "bad score {score}");
                prop_assert!(index < docs.len(), "index out of range: {index}");
            }
            for pair in ranked.windows(2) {
                prop_assert!(pair[0].score >= pair[1].score, "scores not sorted");
                if pair[0].score.to_bits() == pair[1].score.to_bits() {
                    prop_assert!(pair[0].index < pair[1].index, "tie not broken by index");
                }
            }
            prop_assert_eq!(rank(&refs, &query.to_uppercase(), k), ranked);
        }
    }
}
