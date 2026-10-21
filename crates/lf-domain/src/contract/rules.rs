//! Règles de citation françaises : fonctions pures, une par convention.

use crate::date::CivilDate;

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

/// `Cons. const., décision n° 2025-1173 QPC du 7 novembre 2025`
/// (`citations.py:298-318`, accents rétablis : décision Q16, 00 §4 / ADR-0009).
/// Sans le suffixe « - objet » de la référence (`citations.py:315-317`) :
/// l'objet reste lisible dans `title` (décision C2, 00 §4).
pub fn constit_citation(
    num: Option<&str>,
    nature: Option<&str>,
    date: Option<CivilDate>,
) -> Option<String> {
    let (num, date) = (present(num)?, date?);
    let mut out = format!("Cons. const., décision n° {num}");
    if let Some(nature) = present(nature) {
        out.push(' ');
        out.push_str(&nature.to_uppercase());
    }
    out.push_str(" du ");
    out.push_str(&date.to_french());
    Some(out)
}

/// `CNIL, délibération n° SAN-2026-002 du 8 janvier 2026`
/// (`citations.py:376-380`). Accent rétabli : délibération (décision Q16, 00 §4 / ADR-0009) ; la
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

    fn nov_7() -> Option<CivilDate> {
        CivilDate::new(2025, 11, 7)
    }

    #[test]
    fn article_citation_rules_table() {
        let cases = [
            (Some("Code civil"), Some("9"), Some("Code civil, art. 9")),
            (None, Some("9"), Some("Art. 9")),
            (Some("Code civil"), None, Some("Code civil")),
            (None, None, None),
            // "" vaut absent (citations.py:256-261, vérité Python des chaînes).
            (Some(""), Some("9"), Some("Art. 9")),
            (Some("Code"), Some(""), Some("Code")),
            (Some(""), Some(""), None),
            (Some(""), None, None),
            (None, Some(""), None),
        ];
        for (code_title, num, expected) in cases {
            assert_eq!(
                article_citation(code_title, num).as_deref(),
                expected,
                "{code_title:?} {num:?}"
            );
        }
    }

    #[test]
    fn constit_citation_golden() {
        // consult_juri_constit.json : num 2025-1173, nature QPC, 2025-11-07 ; sans
        // le suffixe « - objet » de la référence (décision C2, 00 §4).
        assert_eq!(
            constit_citation(Some("2025-1173"), Some("qpc"), nov_7()).as_deref(),
            Some("Cons. const., décision n° 2025-1173 QPC du 7 novembre 2025")
        );
    }

    #[test]
    fn constit_optional_parts() {
        assert_eq!(
            constit_citation(Some("2021-817"), None, nov_7()).as_deref(),
            Some("Cons. const., décision n° 2021-817 du 7 novembre 2025")
        );
        assert_eq!(
            constit_citation(Some("2021-817"), Some(""), nov_7()).as_deref(),
            Some("Cons. const., décision n° 2021-817 du 7 novembre 2025")
        );
        assert_eq!(constit_citation(None, Some("DC"), nov_7()), None);
        assert_eq!(constit_citation(Some("2021-817"), Some("DC"), None), None);
        // "" vaut absent (citations.py:303-317).
        assert_eq!(constit_citation(Some(""), Some("DC"), nov_7()), None);
    }

    #[test]
    fn cnil_citation_golden() {
        // consult_cnil.json : num SAN-2026-002, 2026-01-08.
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
}
