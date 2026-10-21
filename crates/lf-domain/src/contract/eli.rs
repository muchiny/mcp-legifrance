//! `Eli` : identifiant ELI natif validé, jamais fabriqué (ADR-0009, décision C2).
//!
//! Règle nouvelle : la référence ne renvoie aucun ELI. Un ELI vient de la
//! source ou n'existe pas, il n'est déduit ni du NOR ni de la date (03 §1.4).
//! [`Eli::parse`] rogne (`str::trim`), retire un préfixe [`WEB_BASE`] éventuel,
//! exige un chemin `/eli/` suivi d'au moins un octet de `[A-Za-z0-9_./-]` et
//! rend la forme absolue `{WEB_BASE}{chemin}` si elle tient en [`ELI_MAX_CHARS`]
//! octets (ASCII). Tout le reste donne `None` (autre hôte, `http://`, blanc
//! interne, `?`, `#`, `%`, non-ASCII) : pas de variante d'erreur, le mapper
//! écarte un ELI invalide avec un diagnostic. La grammaire complète
//! (`annexes/B-citations.md` §4.13) arrive avec le parseur de M6.

use std::fmt;

use crate::url::WEB_BASE;

/// Longueur maximale d'un ELI absolu, en octets.
pub const ELI_MAX_CHARS: usize = 256;

/// ELI natif en forme absolue.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Eli(String);

impl Eli {
    /// Valide `input` (chemin ou URL absolue) ; `None` si ce n'est pas un ELI.
    pub fn parse(input: &str) -> Option<Self> {
        let trimmed = input.trim();
        let path = trimmed.strip_prefix(WEB_BASE).unwrap_or(trimmed);
        let name = path.strip_prefix("/eli/")?;
        let valid = !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'/' | b'-'));
        let absolute = format!("{WEB_BASE}{path}");
        (valid && absolute.len() <= ELI_MAX_CHARS).then_some(Self(absolute))
    }

    /// Forme absolue.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Eli {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn eli(input: &str) -> Option<String> {
        Eli::parse(input).map(|e| e.as_str().to_owned())
    }

    #[test]
    fn path_and_absolute_url_give_the_absolute_url() {
        // Exemples de B-citations:683.
        for path in ["/eli/decret/2021/7/13/PRMD2117108D/jo/article_1", "/eli/jo/2019/1/25/0021"] {
            let absolute = format!("https://www.legifrance.gouv.fr{path}");
            assert_eq!(eli(path), Some(absolute.clone()));
            assert_eq!(eli(&absolute), Some(absolute.clone()));
            assert_eq!(eli(&format!(" {path}\n")), Some(absolute));
        }
    }

    #[test]
    fn display_is_the_absolute_url() {
        assert_eq!(
            Eli::parse("/eli/jo/2019/1/25/0021").unwrap().to_string(),
            "https://www.legifrance.gouv.fr/eli/jo/2019/1/25/0021"
        );
    }

    #[test]
    fn accepts_each_allowed_char() {
        for path in ["/eli/a", "/eli/Z", "/eli/9", "/eli/a_b", "/eli/a.b", "/eli/a/b", "/eli/a-b"] {
            assert_eq!(eli(path), Some(format!("{WEB_BASE}{path}")), "{path}");
        }
    }

    #[test]
    fn rejects_other_shapes() {
        for bad in [
            "",
            "   ",
            "/eli",
            "/eli/",
            "eli/jo/2019",
            "/ELI/jo/2019",
            "/loda/id/LEGITEXT000006070721",
            "http://www.legifrance.gouv.fr/eli/jo/2019",
            "https://legifrance.gouv.fr/eli/jo/2019",
            "https://www.example.org/eli/jo/2019",
            "https://www.legifrance.gouv.fr.example.org/eli/jo/2019",
            "https://www.legifrance.gouv.fr//eli/jo/2019",
            "https://www.legifrance.gouv.fr/eli/",
            "/eli/jo/2019 1",
            "/eli/jo/2019\n1",
            "/eli/jo/2019?x=1",
            "/eli/jo/2019#art",
            "/eli/jo/%20",
            "/eli/jo/é",
        ] {
            assert_eq!(Eli::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn length_is_bounded() {
        // 30 (`WEB_BASE`) + 5 (`/eli/`) + 221 = 256 caractères.
        let longest = format!("/eli/{}", "a".repeat(221));
        assert_eq!(eli(&longest).as_deref().map(str::len), Some(ELI_MAX_CHARS));
        assert_eq!(eli(&format!("{longest}a")), None);
        assert_eq!(eli(&format!("{WEB_BASE}{longest}a")), None);
    }

    proptest! {
        #[test]
        fn parse_never_panics(s in "(?s).{0,300}") {
            let _ = Eli::parse(&s);
        }

        #[test]
        fn valid_paths_roundtrip(path in "/eli/[A-Za-z0-9_./-]{1,200}") {
            let eli = Eli::parse(&path).unwrap();
            prop_assert_eq!(eli.as_str(), format!("{WEB_BASE}{path}"));
            prop_assert_eq!(Eli::parse(eli.as_str()), Some(eli.clone()));
        }

        #[test]
        fn parsed_eli_is_a_fixed_point(
            s in "(https://www\\.legifrance\\.gouv\\.fr)?/eli/[A-Za-z0-9_./-]{0,240}[ -~]?",
        ) {
            let parsed = Eli::parse(&s);
            prop_assert_eq!(parsed.as_ref().and_then(|e| Eli::parse(e.as_str())), parsed);
        }
    }
}
