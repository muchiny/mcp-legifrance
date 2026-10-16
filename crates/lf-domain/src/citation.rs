//! Contrat de citation et conventions de citation françaises.
//!
//! CONSTITUTION art. 4 de la référence : toute entité citable porte `eli_uri`,
//! `human_readable_citation` et `source_url`. PISTE renvoie un ELI nul pour la
//! législation : `eli_uri` porte donc l'URL Légifrance stable, jamais un ELI
//! fabriqué (`citations.py:8-23`).

use crate::date::CivilDate;

/// Note jointe aux résultats de législation (`models.py:14-17`).
pub const ELI_NOTE: &str = "Legifrance lf-engine-app returns the ELI field null for legislation; \
'eli_uri' is the stable CID-keyed legifrance.gouv.fr resource URL, not a synthesized /eli/ identifier.";

/// Contrat de citation porté par toute entité citable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitationContract {
    /// URL Légifrance stable (pas d'ELI fabriqué).
    pub eli_uri: String,
    /// Citation selon l'usage français, absente si la source ne la permet pas.
    pub human_readable_citation: Option<String>,
    /// Page legifrance.gouv.fr de la ressource.
    pub source_url: String,
}

impl CitationContract {
    /// `eli_uri` et `source_url` reçoivent tous deux `url`.
    pub fn from_url(url: String, human_readable_citation: Option<String>) -> Self {
        Self { eli_uri: url.clone(), human_readable_citation, source_url: url }
    }
}

/// Entité qui sait produire son contrat de citation.
pub trait Citable {
    /// Contrat de citation de l'entité.
    fn citation(&self) -> CitationContract;
}

/// Parité avec la vérité Python des chaînes : `""` vaut absent
/// (`citations.py:256-261`, `303-317`, `377-378`, `458`).
fn present(s: Option<&str>) -> Option<&str> {
    s.filter(|s| !s.is_empty())
}

/// `Code civil, art. 9` ; `Art. 9` sans titre de code (`citations.py:256-261`).
pub fn article_citation(code_title: Option<&str>, num: Option<&str>) -> Option<String> {
    match (present(code_title), present(num)) {
        (Some(code), Some(num)) => Some(format!("{code}, art. {num}")),
        (None, Some(num)) => Some(format!("Art. {num}")),
        (code, None) => code.map(str::to_owned),
    }
}

/// Premier contenu entre crochets, non vide et sur une seule ligne.
///
/// Divergence avec `re.search(r"\[(.+?)\]", titre)` (`citations.py:315-317`) :
/// ici, premier `[` puis premier `]` qui suit, sinon `None`. Sur `a [] [b]`, la
/// référence capture `] [b` (artefact de la regex) et on renvoie `None` ; sur
/// `a [b\nc] [d]`, elle capture `d` et on renvoie `None`. On ne fabrique pas
/// d'objet hors du premier segment.
fn bracketed(title: &str) -> Option<&str> {
    let (_, after) = title.split_once('[')?;
    let (inner, _) = after.split_once(']')?;
    (!inner.is_empty() && !inner.contains('\n')).then_some(inner)
}

/// `Cons. const., décision n° 2025-1173 QPC du 7 novembre 2025 - <objet>`
/// (`citations.py:298-318`, accents rétablis : décision Q11, 00 §4 / ADR-0009).
pub fn constit_citation(
    num: Option<&str>,
    nature: Option<&str>,
    date: Option<CivilDate>,
    title: Option<&str>,
) -> Option<String> {
    let (num, date) = (present(num)?, date?);
    let mut out = format!("Cons. const., décision n° {num}");
    if let Some(nature) = present(nature) {
        out.push(' ');
        out.push_str(&nature.to_uppercase());
    }
    out.push_str(" du ");
    out.push_str(&date.to_french());
    if let Some(subject) = title.and_then(bracketed) {
        out.push_str(" - ");
        out.push_str(subject);
    }
    Some(out)
}

/// `CNIL, délibération n° SAN-2026-002 du 8 janvier 2026`
/// (`citations.py:376-380`). Accent rétabli : délibération (décision Q11, 00 §4 / ADR-0009) ; la
/// référence écrit `deliberation` (`citations.py:377-378`).
pub fn cnil_citation(num: Option<&str>, date: Option<CivilDate>) -> Option<String> {
    Some(format!("CNIL, délibération n° {} du {}", present(num)?, date?.to_french()))
}

/// `<titre> (<raison sociale>)` pour un accord d'entreprise (`citations.py:458`).
pub fn with_company(title: Option<&str>, company: Option<&str>) -> Option<String> {
    match (present(title), present(company)) {
        (Some(title), Some(company)) => Some(format!("{title} ({company})")),
        (title, _) => title.map(str::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn nov_7() -> Option<CivilDate> {
        CivilDate::new(2025, 11, 7)
    }

    #[test]
    fn contract_uses_url_twice() {
        let c = CitationContract::from_url("https://u".into(), Some("Cit".into()));
        assert_eq!(c.eli_uri, "https://u");
        assert_eq!(c.source_url, "https://u");
        assert_eq!(c.human_readable_citation.as_deref(), Some("Cit"));
    }

    #[test]
    fn article_rules() {
        assert_eq!(
            article_citation(Some("Code civil"), Some("9")).as_deref(),
            Some("Code civil, art. 9")
        );
        assert_eq!(article_citation(None, Some("9")).as_deref(), Some("Art. 9"));
        assert_eq!(article_citation(Some("Code civil"), None).as_deref(), Some("Code civil"));
        assert_eq!(article_citation(None, None), None);
        // "" vaut absent (citations.py:256-261, vérité Python des chaînes).
        assert_eq!(article_citation(Some(""), Some("9")).as_deref(), Some("Art. 9"));
        assert_eq!(article_citation(Some("Code"), Some("")).as_deref(), Some("Code"));
        assert_eq!(article_citation(Some(""), Some("")), None);
        assert_eq!(article_citation(Some(""), None), None);
        assert_eq!(article_citation(None, Some("")), None);
    }

    #[test]
    fn constit_full_citation_from_fixture() {
        // consult_juri_constit.json : num 2025-1173, nature QPC, 2025-11-07.
        let title = "Décision 2025-1173 QPC - 07 novembre 2025 - M. Amine G. [Absence de publicité des débats]";
        assert_eq!(
            constit_citation(Some("2025-1173"), Some("qpc"), nov_7(), Some(title)).as_deref(),
            Some(
                "Cons. const., décision n° 2025-1173 QPC du 7 novembre 2025 - Absence de publicité des débats"
            )
        );
    }

    #[test]
    fn constit_optional_parts() {
        assert_eq!(
            constit_citation(Some("2021-817"), None, nov_7(), Some("pas de crochets")).as_deref(),
            Some("Cons. const., décision n° 2021-817 du 7 novembre 2025")
        );
        assert_eq!(
            constit_citation(Some("2021-817"), Some(""), nov_7(), None).as_deref(),
            Some("Cons. const., décision n° 2021-817 du 7 novembre 2025")
        );
        assert_eq!(constit_citation(None, Some("DC"), nov_7(), None), None);
        assert_eq!(constit_citation(Some("2021-817"), Some("DC"), None, None), None);
        // "" vaut absent (citations.py:303-317).
        assert_eq!(constit_citation(Some(""), Some("DC"), nov_7(), None), None);
    }

    #[test]
    fn bracket_edges() {
        assert_eq!(bracketed("a [b] [c]"), Some("b"));
        assert_eq!(bracketed("a []"), None);
        assert_eq!(bracketed("a [b\nc]"), None);
        assert_eq!(bracketed("a [b"), None);
        // Divergence assumée avec la regex de la référence (voir `bracketed`).
        assert_eq!(bracketed("a [] [b]"), None);
        assert_eq!(bracketed("a [b\nc] [d]"), None);
        assert_eq!(bracketed("a b]"), None);
    }

    #[test]
    fn cnil_rules() {
        let jan_8 = CivilDate::new(2026, 1, 8);
        assert_eq!(
            cnil_citation(Some("SAN-2026-002"), jan_8).as_deref(),
            Some("CNIL, délibération n° SAN-2026-002 du 8 janvier 2026")
        );
        assert_eq!(cnil_citation(None, jan_8), None);
        assert_eq!(cnil_citation(Some("SAN-2026-002"), None), None);
        // "" vaut absent (citations.py:377-378).
        assert_eq!(cnil_citation(Some(""), jan_8), None);
    }

    #[test]
    fn company_rules() {
        assert_eq!(
            with_company(Some("Télétravail"), Some("CORSAIR")).as_deref(),
            Some("Télétravail (CORSAIR)")
        );
        assert_eq!(with_company(Some("Télétravail"), None).as_deref(), Some("Télétravail"));
        assert_eq!(with_company(None, Some("CORSAIR")), None);
        // "" vaut absent (citations.py:458).
        assert_eq!(with_company(Some("T"), Some("")).as_deref(), Some("T"));
        assert_eq!(with_company(Some(""), Some("X")), None);
        assert_eq!(with_company(Some(""), None), None);
    }

    proptest! {
        #[test]
        fn bracketed_never_panics(s in "\\PC{0,80}") {
            let _ = bracketed(&s);
        }

        #[test]
        fn bracketed_never_panics_on_brackets(s in "[a-z\\[\\] \n]{0,40}") {
            let _ = bracketed(&s);
        }

        #[test]
        fn bracketed_finds_inner(
            pre in "[a-z ]{0,5}",
            inner in "[a-z ]{1,5}",
            post in "[a-z \\[\\]\n]{0,8}",
        ) {
            let title = format!("{pre}[{inner}]{post}");
            prop_assert_eq!(bracketed(&title), Some(inner.as_str()));
        }
    }
}
