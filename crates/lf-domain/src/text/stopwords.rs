//! Mots vides du français juridique, écartés de `focus` (03 §7.8).
//!
//! La liste est figée par un test contre le texte de la spécification. Elle ne contient aucun
//! mot juridiquement porteur (`article`, `loi`, `code`, `non`, `conformité`) : l'IDF de BM25 les
//! pondère déjà.

/// Mots vides, sous leur forme repliée (minuscules, accents français retirés), en ordre
/// strictement croissant des octets : la recherche est dichotomique.
const STOPWORDS: &[&str] = &[
    "a", "ai", "aient", "ait", "as", "au", "aucun", "aucune", "aupres", "aussi", "autre", "autres",
    "aux", "avaient", "avait", "avant", "avec", "avoir", "ayant", "c", "ca", "ce", "ceci", "cela",
    "celle", "celles", "celui", "ces", "cet", "cette", "ceux", "chaque", "ci", "comme", "d",
    "dans", "de", "deja", "des", "donc", "dont", "du", "elle", "elles", "en", "encore", "entre",
    "est", "et", "etaient", "etait", "ete", "etre", "eu", "eux", "il", "ils", "j", "je", "l", "la",
    "le", "les", "leur", "leurs", "lors", "lui", "m", "ma", "mais", "me", "meme", "memes", "mes",
    "moi", "mon", "n", "ne", "ni", "nos", "notre", "nous", "on", "ont", "ou", "par", "parmi",
    "pas", "peu", "peut", "plus", "pour", "qu", "quand", "que", "quel", "quelle", "quelles",
    "quels", "qui", "quoi", "s", "sa", "sans", "se", "selon", "ses", "si", "sinon", "soit", "son",
    "sont", "sous", "sur", "t", "ta", "te", "tel", "telle", "telles", "tels", "tes", "toi", "ton",
    "tous", "tout", "toute", "toutes", "tres", "tu", "un", "une", "vos", "votre", "vous", "y",
];

/// Le mot, déjà replié (minuscules, accents français retirés), est-il un mot vide ?
pub(crate) fn is_stopword(folded: &str) -> bool {
    STOPWORDS.binary_search(&folded).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::fold::fold;
    use pretty_assertions::assert_eq;

    /// Liste de 03 §7.8, recopiée telle quelle (ordre de la spécification, sans tri).
    const SPECIFICATION: &str = "\
a ai aient ait as au aucun aucune aupres aussi autre autres aux avaient avait avant avec avoir ayant c ca ce ceci
cela celle celles celui ces cet cette ceux chaque ci comme d dans de des deja donc dont du elle elles en encore entre
est et etaient etait ete etre eu eux il ils j je l la le les leur leurs lors lui m ma mais me meme memes mes moi mon
n ne ni nos notre nous on ont ou par parmi pas peu peut plus pour qu quand que quel quelle quelles quels qui quoi s
sa sans se selon ses si sinon soit son sont sous sur t ta te tel telle telles tels tes toi ton tous tout toute
toutes tres tu un une vos votre vous y";

    #[test]
    fn stopword_list_is_the_frozen_specification_list() {
        let mut expected: Vec<&str> = SPECIFICATION.split_whitespace().collect();
        assert_eq!(expected.len(), 136);
        expected.sort_unstable();
        expected.dedup();
        assert_eq!(expected.len(), 136, "la spécification ne répète aucun mot");
        assert_eq!(STOPWORDS, expected);
    }

    #[test]
    fn stopwords_are_sorted_for_binary_search() {
        assert!(STOPWORDS.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn stopwords_are_already_folded() {
        for word in STOPWORDS {
            assert_eq!(fold(word), *word, "{word} doit être sous forme repliée");
        }
    }

    #[test]
    fn every_listed_word_is_a_stopword() {
        for word in STOPWORDS {
            assert!(is_stopword(word), "{word}");
        }
        // Les deux extrémités de la recherche dichotomique.
        assert!(is_stopword("a"));
        assert!(is_stopword("y"));
    }

    #[test]
    fn legal_words_are_not_stopwords() {
        for word in ["article", "loi", "code", "non", "conformite", "bail", "decide", "juge"] {
            assert!(!is_stopword(word), "{word}");
        }
    }

    #[test]
    fn lookup_is_exact_on_the_folded_form() {
        // Ni préfixe, ni suffixe, ni casse, ni accent : l'appelant fournit la forme repliée.
        assert!(!is_stopword(""));
        assert!(!is_stopword("lesion"));
        assert!(!is_stopword("ale"));
        assert!(!is_stopword("LE"));
        assert!(!is_stopword("déjà"));
        assert!(is_stopword("deja"));
    }
}
