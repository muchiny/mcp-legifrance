//! Tokenisation pour la recherche plein texte : minuscules, accents français
//! repliés, découpe sur les non-alphanumériques, jetons d'au moins 2 caractères.

/// Minuscules et accents français repliés (`é` → `e`, `œ` → `oe`).
pub(crate) fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'â' | 'ä' => out.push('a'),
            'ç' => out.push('c'),
            'é' | 'è' | 'ê' | 'ë' => out.push('e'),
            'î' | 'ï' => out.push('i'),
            'ô' | 'ö' => out.push('o'),
            'ù' | 'û' | 'ü' => out.push('u'),
            'ÿ' => out.push('y'),
            'œ' => out.push_str("oe"),
            'æ' => out.push_str("ae"),
            other => out.push(other),
        }
    }
    out
}

/// Jetons repliés d'au moins 2 caractères.
pub(crate) fn tokens(s: &str) -> Vec<String> {
    fold(s)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.chars().nth(1).is_some())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    #[test]
    fn folds_french_accents() {
        assert_eq!(fold("ÀÂÄ Ç ÉÈÊË ÎÏ ÔÖ ÙÛÜ Ÿ Œuvre Æ"), "aaa c eeee ii oo uuu y oeuvre ae");
    }

    #[test]
    fn tokenizes_and_drops_single_chars() {
        assert_eq!(
            tokens("L'article L. 1234-5 du Code « civil »"),
            ["article", "1234", "du", "code", "civil"]
        );
        assert_eq!(tokens(""), Vec::<String>::new());
    }

    proptest! {
        /// Invariant : chaque jeton compte au moins 2 caractères, tous
        /// alphanumériques (la découpe se fait sur les non-alphanumériques).
        #[test]
        fn tokens_are_alphanumeric_and_long_enough(s in "\\PC{0,80}") {
            for token in tokens(&s) {
                prop_assert!(token.chars().count() >= 2, "token too short: {token:?}");
                prop_assert!(
                    token.chars().all(char::is_alphanumeric),
                    "non-alphanumeric char in {token:?}"
                );
            }
        }
    }
}
