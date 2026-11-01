//! Règles de citation françaises : fonctions pures, une par convention.

use crate::{date::CivilDate, entity::ParentNature};

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

/// Citation d'un article selon la nature de son texte parent (03 §5.4, décision Q12).
///
/// Pour un texte non codifié ([`ParentNature::Text`]), le titre officiel perd son appel de
/// note final ` (n)` (« LOI n° 2016-1321 … numérique (1) » devient « … numérique ») avant la
/// règle de [`article_citation`]. Pour un code ([`ParentNature::Code`]), le titre perd son
/// sigle final redondant (« Code général des impôts, CGI. » devient « Code général des
/// impôts », L2-15). Pour les autres natures, c'est exactement [`article_citation`] (parité :
/// le titre garde son éventuel appel de note).
pub fn article_citation_for(
    parent: ParentNature,
    code_title: Option<&str>,
    num: Option<&str>,
) -> Option<String> {
    let title = match parent {
        ParentNature::Text => code_title.map(strip_note_call),
        ParentNature::Code => code_title.map(strip_sigle),
        ParentNature::Kali | ParentNature::Unknown => code_title,
    };
    article_citation(title, num)
}

/// Retire de `title` (bords blancs ôtés) le sigle final redondant d'un titre de code : une
/// virgule, une espace, de 2 à 8 majuscules, un point, en toute fin (« Code général des
/// impôts, CGI. » donne « Code général des impôts »). Tout autre titre est rendu tel quel :
/// sigle sans point final, minuscules, un seul caractère ou plus de huit, texte après le
/// sigle, annexe (« …, annexe II »). Un titre réduit au sigle (« , CGI. ») reste inchangé : le
/// retirer laisserait un titre vide.
fn strip_sigle(title: &str) -> &str {
    let title = title.trim();
    let Some(body) = title.strip_suffix('.') else {
        return title;
    };
    let Some((head, sigle)) = body.rsplit_once(", ") else {
        return title;
    };
    let head = head.trim_end();
    let letters = sigle.chars().count();
    if head.is_empty() || !(2..=8).contains(&letters) || !sigle.chars().all(char::is_uppercase) {
        return title;
    }
    head
}

/// Retire de `title` (bords blancs ôtés) l'appel de note final ` (n)` : une espace, `(`, un ou
/// plusieurs chiffres ASCII, `)`, en toute fin. Tout autre titre est rendu tel quel : `(bis)`,
/// un appel suivi de texte, un appel collé au mot (`Texte(1)`), des chiffres non ASCII. Un
/// titre réduit à l'appel (`(1)`) reste inchangé : le retirer laisserait un titre vide.
fn strip_note_call(title: &str) -> &str {
    let title = title.trim();
    let Some(body) = title.strip_suffix(')') else {
        return title;
    };
    let before_digits = body.trim_end_matches(|c: char| c.is_ascii_digit());
    if before_digits.len() == body.len() {
        return title;
    }
    before_digits
        .strip_suffix('(')
        .and_then(|head| head.strip_suffix(' '))
        .map_or(title, str::trim_end)
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
    fn law_article_citation_drops_the_note_call() {
        let title = "LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique (1)";
        assert_eq!(
            article_citation_for(ParentNature::Text, Some(title), Some("1")).as_deref(),
            Some("LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique, art. 1")
        );
        // Sans numéro ni titre : les replis de `article_citation` s'appliquent au titre nettoyé.
        assert_eq!(
            article_citation_for(ParentNature::Text, Some("Texte (1)"), None).as_deref(),
            Some("Texte")
        );
        assert_eq!(
            article_citation_for(ParentNature::Text, None, Some("1")).as_deref(),
            Some("Art. 1")
        );
    }

    #[test]
    fn code_article_citation_is_unchanged() {
        assert_eq!(
            article_citation_for(ParentNature::Code, Some("Code civil"), Some("9")).as_deref(),
            Some("Code civil, art. 9")
        );
        // Hors texte de loi, l'appel de note reste (parité, aucune nature ne le retire).
        for nature in [ParentNature::Code, ParentNature::Kali, ParentNature::Unknown] {
            assert_eq!(
                article_citation_for(nature, Some("Texte (1)"), Some("2")).as_deref(),
                Some("Texte (1), art. 2"),
                "{nature:?}"
            );
        }
    }

    #[test]
    fn a_code_citation_drops_the_redundant_sigle() {
        // L2-15 (verify_citations-16): le titre LEGI « Code général des impôts, CGI. ».
        assert_eq!(
            article_citation_for(
                ParentNature::Code,
                Some("Code général des impôts, CGI."),
                Some("777")
            )
            .as_deref(),
            Some("Code général des impôts, art. 777")
        );
        // Jumeau : les titres sans sigle final restent tels quels.
        for title in ["Code civil", "Code général des impôts, annexe II"] {
            assert_eq!(
                article_citation_for(ParentNature::Code, Some(title), Some("9")).as_deref(),
                Some(format!("{title}, art. 9").as_str()),
                "{title}"
            );
        }
        // Sans numéro : le titre nettoyé seul. Titre réduit au sigle : jamais de titre vide.
        assert_eq!(
            article_citation_for(ParentNature::Code, Some("Code du travail, C. trav."), None)
                .as_deref(),
            Some("Code du travail, C. trav.")
        );
        assert_eq!(
            article_citation_for(ParentNature::Code, Some("Code de la route, CR."), None)
                .as_deref(),
            Some("Code de la route")
        );
        // Les autres natures gardent le titre brut (parité).
        for nature in [ParentNature::Kali, ParentNature::Unknown] {
            assert_eq!(
                article_citation_for(nature, Some("Code général des impôts, CGI."), Some("777"))
                    .as_deref(),
                Some("Code général des impôts, CGI., art. 777"),
                "{nature:?}"
            );
        }
    }

    #[test]
    fn sigle_only_stripped_when_final_uppercase_two_to_eight_and_dotted() {
        let cases = [
            ("Code général des impôts, CGI.", "Code général des impôts"),
            ("  Code général des impôts, CGI.  ", "Code général des impôts"),
            ("Code, AB.", "Code"),
            ("Code, ABCDEFGH.", "Code"),
            ("Code , CGI.", "Code"),
            ("Code, CÉSEDA.", "Code"),
            // Bornes : 1 ou 9 majuscules, minuscules, mélange, chiffres, vide, sans point.
            ("Code, A.", "Code, A."),
            ("Code, ABCDEFGHI.", "Code, ABCDEFGHI."),
            ("Code, Cgi.", "Code, Cgi."),
            ("Code, CG1.", "Code, CG1."),
            ("Code, .", "Code, ."),
            ("Code, CGI", "Code, CGI"),
            ("Code CGI.", "Code CGI."),
            ("Code, CGI. suite", "Code, CGI. suite"),
            ("Code, C. trav.", "Code, C. trav."),
            // Titre réduit au sigle : rien à retirer, il resterait un titre vide.
            (", CGI.", ", CGI."),
            (" , CGI.", ", CGI."),
            ("", ""),
        ];
        for (title, expected) in cases {
            assert_eq!(strip_sigle(title), expected, "{title:?}");
        }
    }

    #[test]
    fn note_call_only_stripped_at_the_end_and_for_digits() {
        let cases = [
            ("Texte (12)", "Texte"),
            ("Texte (1)", "Texte"),
            ("  Texte  (12)  ", "Texte"),
            ("Titre (bis)", "Titre (bis)"),
            ("Texte (1) suite", "Texte (1) suite"),
            ("Texte(1)", "Texte(1)"),
            ("Texte ()", "Texte ()"),
            ("Texte 12)", "Texte 12)"),
            ("Texte (1", "Texte (1"),
            ("Texte (1a)", "Texte (1a)"),
            // Chiffres non ASCII : inchangé.
            ("Texte (١٢)", "Texte (١٢)"),
            ("Texte (１２)", "Texte (１２)"),
            // Titre réduit à l'appel de note : rien à retirer, il resterait un titre vide.
            ("(1)", "(1)"),
            (" (1) ", "(1)"),
            ("", ""),
        ];
        for (title, expected) in cases {
            assert_eq!(strip_note_call(title), expected, "{title:?}");
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
