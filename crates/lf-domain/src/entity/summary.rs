//! Sommaire officiel d'une décision (03 §4.4).
//!
//! La répartition par fond (quel champ de l'API va dans `analyses` ou `titrages`) est le
//! travail du mapper (plan 2f) : [`OfficialSummary::new`] reçoit déjà les deux listes et ne
//! fait que les normaliser.

use std::collections::HashSet;

use crate::text::{clean, fold::fold};

/// Sommaire officiel d'une décision : analyses rédigées et titrages (mots-clés).
///
/// Chaque liste est normalisée par [`OfficialSummary::new`] : entrées nettoyées sur une
/// seule ligne, code de classement de tête retiré, entrées vides écartées, doublons
/// supprimés. Le sens de `analyses` et `titrages` dépend du fond (03 §4.4) ; une liste
/// vide signifie que l'API n'en fournit pas.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OfficialSummary {
    /// Analyses rédigées (phrases), sans code de classement, dédoublonnées.
    pub analyses: Vec<String>,
    /// Titrages en mots-clés, sans code de classement, dédoublonnés.
    pub titrages: Vec<String>,
}

impl OfficialSummary {
    /// Construit le sommaire en normalisant chaque liste séparément (le dédoublonnage ne
    /// croise pas `analyses` et `titrages`).
    pub fn new<A, T>(analyses: A, titrages: T) -> Self
    where
        A: IntoIterator,
        A::Item: AsRef<str>,
        T: IntoIterator,
        T::Item: AsRef<str>,
    {
        Self { analyses: normalize_entries(analyses), titrages: normalize_entries(titrages) }
    }
}

/// Normalise une liste d'entrées : chaque entrée passe par [`clean`], est aplatie sur une
/// ligne (tout saut de ligne, donc `<br/>`, et toute suite de blancs deviennent une seule
/// espace), perd son code de classement de tête ; les entrées vides sont écartées et les
/// doublons supprimés sur la vue repliée (casse et accents ignorés), première occurrence
/// gardée, ordre conservé.
fn normalize_entries<I>(entries: I) -> Vec<String>
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for entry in entries {
        let cleaned = clean(entry.as_ref());
        let flat = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
        let text = strip_class_code(&flat);
        if !text.is_empty() && seen.insert(fold(text)) {
            out.push(text.to_owned());
        }
    }
    out
}

/// Retire le code de classement de tête d'une entrée déjà aplatie.
///
/// Grammaire reconnue, au début de l'entrée seulement :
///
/// ```text
/// code  = [ "CETAT" ] DD { "-" DDD? D } blancs reste
/// ```
///
/// soit un préfixe `CETAT` facultatif, exactement deux chiffres ASCII, puis zéro, un ou
/// plusieurs groupes `-NN` ou `-NNN` (un tiret suivi de deux ou trois chiffres, jamais un,
/// jamais quatre), puis au moins un blanc. Le code et ses blancs ne sont retirés que si le
/// reste commence par une majuscule (`char::is_uppercase`) : `15 jours après` garde son
/// `15`, et un code seul, sans reste, est conservé tel quel. Tout écart à la grammaire
/// (trois chiffres de tête, groupe d'un ou quatre chiffres, pas de blanc) rend l'entrée
/// inchangée. Écart assumé à 03 §4.4, qui retire le code sans condition sur le reste.
fn strip_class_code(entry: &str) -> &str {
    let head = entry.strip_prefix("CETAT").unwrap_or(entry);
    if leading_digits(head) != 2 {
        return entry;
    }
    let mut rest = &head[2..];
    while let Some(group) = rest.strip_prefix('-') {
        let digits = leading_digits(group);
        if !(2..=3).contains(&digits) {
            return entry;
        }
        rest = &group[digits..];
    }
    let after = rest.trim_start();
    if after.len() == rest.len() {
        return entry;
    }
    if after.chars().next().is_some_and(char::is_uppercase) { after } else { entry }
}

/// Nombre de chiffres ASCII consécutifs en tête de `s` (chacun fait un octet).
fn leading_digits(s: &str) -> usize {
    s.bytes().take_while(u8::is_ascii_digit).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn norm(entries: &[&str]) -> Vec<String> {
        normalize_entries(entries.iter().copied())
    }

    #[test]
    fn summary_cetat_official_analysis() {
        let entry = "01-03-02-02 Il résulte de l'article 34 que la loi fixe les règles.";
        let summary = OfficialSummary::new([entry, entry, entry], Vec::<String>::new());
        assert_eq!(summary.analyses, ["Il résulte de l'article 34 que la loi fixe les règles."]);
        assert_eq!(summary.titrages, Vec::<String>::new());
    }

    #[test]
    fn summary_strips_cetat_prefixed_titrage() {
        let summary =
            OfficialSummary::new(Vec::<String>::new(), ["CETAT01-03-02-02 ACTES LÉGISLATIFS"]);
        assert_eq!(summary.titrages, ["ACTES LÉGISLATIFS"]);
    }

    #[test]
    fn summary_strips_two_digit_and_dashed_codes() {
        assert_eq!(norm(&["44 Compétence", "68-01-002 Pour"]), ["Compétence", "Pour"]);
    }

    #[test]
    fn summary_keeps_a_leading_number_before_lowercase_text() {
        assert_eq!(norm(&["15 jours après la décision"]), ["15 jours après la décision"]);
    }

    #[test]
    fn summary_br_becomes_a_space() {
        assert_eq!(norm(&["a<br/>b", "c<br>\n<br>d", "e\r\nf"]), ["a b", "c d", "e f"]);
    }

    #[test]
    fn summary_dedupes_on_folded_view() {
        assert_eq!(
            norm(&["Responsabilité DU  fait", "RESPONSABILITE du fait", "responsabilité du fait"]),
            ["Responsabilité DU fait"]
        );
        assert_eq!(norm(&["Œuvre", "oeuvre"]), ["Œuvre"]);
        assert_eq!(norm(&["Abc", "Abd"]), ["Abc", "Abd"]);
    }

    #[test]
    fn summary_dedupes_after_code_removal() {
        assert_eq!(norm(&["44 Compétence", "Compétence", "CETAT44 COMPÉTENCE"]), ["Compétence"]);
    }

    #[test]
    fn summary_dedupes_per_list() {
        let summary = OfficialSummary::new(["Alpha", "Alpha"], ["Alpha", "Alpha"]);
        assert_eq!(summary.analyses, ["Alpha"]);
        assert_eq!(summary.titrages, ["Alpha"]);
    }

    #[test]
    fn summary_drops_blank_entries() {
        assert_eq!(norm(&["", "  \n ", "<br/>", "<p> </p>", "ok", " \u{a0} "]), ["ok"]);
    }

    #[test]
    fn summary_keeps_order() {
        assert_eq!(norm(&["c", "a", "b", "a", "c"]), ["c", "a", "b"]);
    }

    #[test]
    fn summary_new_with_empty_inputs_is_default() {
        let summary = OfficialSummary::new(Vec::<String>::new(), Vec::<&str>::new());
        assert_eq!(summary, OfficialSummary::default());
    }

    #[test]
    fn summary_new_accepts_owned_and_borrowed_items() {
        let summary = OfficialSummary::new(vec![String::from("A")], ["B"]);
        assert_eq!(summary.analyses, ["A"]);
        assert_eq!(summary.titrages, ["B"]);
    }

    #[test]
    fn class_code_strips_leading_digits_of_two_only() {
        assert_eq!(strip_class_code("4 Abc"), "4 Abc");
        assert_eq!(strip_class_code("44 Abc"), "Abc");
        assert_eq!(strip_class_code("123 Abc"), "123 Abc");
        assert_eq!(strip_class_code("1234 Abc"), "1234 Abc");
    }

    #[test]
    fn class_code_dash_groups_take_two_or_three_digits() {
        assert_eq!(strip_class_code("44-1 Abc"), "44-1 Abc");
        assert_eq!(strip_class_code("44-12 Abc"), "Abc");
        assert_eq!(strip_class_code("44-123 Abc"), "Abc");
        assert_eq!(strip_class_code("44-1234 Abc"), "44-1234 Abc");
        assert_eq!(strip_class_code("44-12-123-45 Abc"), "Abc");
        assert_eq!(strip_class_code("44-12-1 Abc"), "44-12-1 Abc");
        assert_eq!(strip_class_code("44-12-1234 Abc"), "44-12-1234 Abc");
        assert_eq!(strip_class_code("44- Abc"), "44- Abc");
        assert_eq!(strip_class_code("44-12-"), "44-12-");
    }

    #[test]
    fn class_code_cetat_prefix_follows_the_same_grammar() {
        assert_eq!(strip_class_code("CETAT44 Abc"), "Abc");
        assert_eq!(strip_class_code("CETAT01-03-02-02 ACTES"), "ACTES");
        assert_eq!(strip_class_code("CETAT4 Abc"), "CETAT4 Abc");
        assert_eq!(strip_class_code("CETAT123 Abc"), "CETAT123 Abc");
        assert_eq!(strip_class_code("CETAT44-1 Abc"), "CETAT44-1 Abc");
        assert_eq!(strip_class_code("CETAT Abc"), "CETAT Abc");
        assert_eq!(strip_class_code("cetat44 Abc"), "cetat44 Abc");
        assert_eq!(strip_class_code("CETATCETAT44 Abc"), "CETATCETAT44 Abc");
    }

    #[test]
    fn class_code_needs_a_blank_and_an_uppercase_rest() {
        assert_eq!(strip_class_code("01-03-02-02"), "01-03-02-02");
        assert_eq!(strip_class_code("44"), "44");
        assert_eq!(strip_class_code("44-12"), "44-12");
        assert_eq!(strip_class_code("44Abc"), "44Abc");
        assert_eq!(strip_class_code("44-12Abc"), "44-12Abc");
        assert_eq!(strip_class_code("44 abc"), "44 abc");
        assert_eq!(strip_class_code("44 é"), "44 é");
        assert_eq!(strip_class_code("44 État"), "État");
        assert_eq!(strip_class_code("44 1 Abc"), "44 1 Abc");
        assert_eq!(strip_class_code("44   Abc"), "Abc");
        assert_eq!(strip_class_code("44\tAbc"), "Abc");
        assert_eq!(strip_class_code("Abc 44 Def"), "Abc 44 Def");
        assert_eq!(strip_class_code(""), "");
    }

    #[test]
    fn leading_digits_counts_ascii_digits_only() {
        assert_eq!(leading_digits(""), 0);
        assert_eq!(leading_digits("a1"), 0);
        assert_eq!(leading_digits("123a4"), 3);
        assert_eq!(leading_digits("12"), 2);
        assert_eq!(leading_digits("\u{661}\u{662}"), 0);
    }
}
