//! Sommaire officiel d'une décision (03 §4.4).
//!
//! La répartition par fond (quel champ de l'API va dans `analyses` ou `titrages`) est le
//! travail du mapper (plan 2f) : [`OfficialSummary::new`] reçoit déjà les deux listes et ne
//! fait que les normaliser.

use std::{collections::HashSet, fmt};

use crate::text::{clean, fold::fold};

/// Sommaire officiel d'une décision : analyses rédigées et titrages (mots-clés).
///
/// Chaque liste est normalisée par [`OfficialSummary::new`] : entrées nettoyées sur une
/// seule ligne, code(s) de classement de tête retiré(s), entrées coupées sur les suites de
/// quatre points ou plus, entrées vides écartées, doublons supprimés. Le sens de `analyses` et
/// `titrages` dépend du fond (03 §4.4) ; une liste vide signifie que l'API n'en fournit pas.
///
/// `Debug` n'écrit jamais les entrées (texte du corps d'une décision), seulement leur nombre.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct OfficialSummary {
    /// Analyses rédigées (phrases), sans code de classement, dédoublonnées.
    pub analyses: Vec<String>,
    /// Titrages en mots-clés, sans code de classement, dédoublonnés.
    pub titrages: Vec<String>,
}

impl fmt::Debug for OfficialSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "OfficialSummary {{ analyses: {}, titrages: {} }}",
            self.analyses.len(),
            self.titrages.len()
        )
    }
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
/// espace), perd son code de classement de tête (un ou plusieurs codes, voir
/// `strip_class_code`) puis est coupée en sous-entrées sur chaque suite d'au moins quatre
/// points (`split_dotted`) ; les entrées vides sont écartées et les doublons supprimés sur la
/// vue repliée (casse et accents ignorés), première occurrence gardée, ordre conservé. Aucun
/// dédoublonnage par sous-chaîne : une phrase répétée dans deux analyses de longueur
/// différente est gardée dans les deux.
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
        for fragment in split_dotted(strip_class_code(&flat)) {
            if !fragment.is_empty() && seen.insert(fold(&fragment)) {
                out.push(fragment);
            }
        }
    }
    out
}

/// Retire le code de classement de tête d'une entrée déjà aplatie.
///
/// Grammaire reconnue, au début de l'entrée seulement :
///
/// ```text
/// code    = [ "CETAT" ] D D { "-" D D D? }
/// marque  = "RJ" D { D }
/// liste   = code { "," ( blancs code | blancs? marque ) }
/// entrée  = liste [ blancs lettre ")" ] blancs reste
/// ```
///
/// soit un préfixe `CETAT` facultatif sur le premier code, exactement deux chiffres ASCII,
/// puis zéro, un ou plusieurs groupes `-NN` ou `-NNN` (un tiret suivi de deux ou trois
/// chiffres, jamais un, jamais quatre). Une liste enchaîne d'autres codes après une virgule
/// suivie d'au moins un blanc (`01-01-02-02, 15-03-01-01`) ; chaque code après la virgule porte au
/// moins un groupe `-NN`, de sorte que `12, 15 Euros` reste intact ; et les marques de recueil `RJ1`,
/// `RJ2` des titrages du Conseil d'État (`CETAT01-01-02-02,RJ1,RJ2`). Une subdivision d'une
/// lettre minuscule suivie d'une parenthèse fermante (`01-04-005 a)`) peut suivre. Le tout
/// n'est retiré, avec ses blancs, que si au moins un blanc sépare la liste du reste et que
/// le reste commence par une majuscule (`char::is_uppercase`) : `15 jours après` garde son
/// `15`, `12,50 Euros` reste intact, et un code seul, sans reste, est conservé tel quel. Tout
/// écart à la grammaire (trois chiffres de tête, groupe d'un ou quatre chiffres, élément de
/// liste invalide, pas de blanc) rend l'entrée inchangée. Écart assumé à 03 §4.4, qui retire
/// le code sans condition sur le reste.
fn strip_class_code(entry: &str) -> &str {
    let head = entry.strip_prefix("CETAT").unwrap_or(entry);
    let Some(len) = code_len(head) else { return entry };
    let mut rest = &head[len..];
    while let Some(tail) = rest.strip_prefix(',') {
        let item = tail.trim_start();
        let blank = item.len() != tail.len();
        if let Some(len) = mark_len(item) {
            rest = &item[len..];
        } else if let Some(len) = code_len(item).filter(|&len| blank && len > 2) {
            rest = &item[len..];
        } else {
            return entry;
        }
    }
    let item = rest.trim_start();
    if item.len() != rest.len()
        && let [letter, b')', ..] = item.as_bytes()
        && letter.is_ascii_lowercase()
    {
        rest = &item[2..];
    }
    let after = rest.trim_start();
    if after.len() == rest.len() {
        return entry;
    }
    if after.chars().next().is_some_and(char::is_uppercase) { after } else { entry }
}

/// Longueur du code `D D { "-" D D D? }` en tête de `s`, `None` si `s` n'en commence pas
/// par un, ou si un groupe est mal formé.
fn code_len(s: &str) -> Option<usize> {
    if leading_digits(s) != 2 {
        return None;
    }
    let mut len = 2;
    while let Some(group) = s[len..].strip_prefix('-') {
        let digits = leading_digits(group);
        if !(2..=3).contains(&digits) {
            return None;
        }
        len = len.saturating_add(1).saturating_add(digits);
    }
    Some(len)
}

/// Longueur de la marque `RJ` suivie d'au moins un chiffre en tête de `s`.
fn mark_len(s: &str) -> Option<usize> {
    let digits = leading_digits(s.strip_prefix("RJ")?);
    (digits > 0).then_some(digits.saturating_add(2))
}

/// Coupe une entrée sur chaque suite d'au moins quatre points (séparateur de l'amont DILA,
/// `Cour.......Par suite`). Le point final est restitué au fragment de gauche, sauf s'il se
/// termine déjà par `.`, `!` ou `?`. Une suite sans texte d'un côté (entrée qui commence ou
/// finit par elle, `etc....`) ne coupe rien : l'entrée reste telle quelle. Une suite sans texte à sa
/// gauche, après une coupe (`A.... ....B`), est absorbée : elle ne coupe rien et le fragment
/// suivant commence après elle (`A.` puis `B`).
fn split_dotted(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut from = 0;
    while let Some(offset) = text[from..].find("....") {
        let at = from.saturating_add(offset);
        let end = at.saturating_add(text[at..].bytes().take_while(|&b| b == b'.').count());
        from = end;
        let left = text[start..at].trim();
        if left.is_empty() {
            if start > 0 {
                start = end;
            }
            continue;
        }
        if end == text.len() {
            continue;
        }
        let mut fragment = left.to_owned();
        if !fragment.ends_with(['.', '!', '?']) {
            fragment.push('.');
        }
        out.push(fragment);
        start = end;
    }
    out.push(text[start..].trim().to_owned());
    out
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
    fn debug_never_prints_the_entries() {
        let summary = OfficialSummary::new(
            ["Texte confidentiel de l'analyse", "Seconde analyse secrète"],
            ["TITRAGE SECRET"],
        );
        assert_eq!(format!("{summary:?}"), "OfficialSummary { analyses: 2, titrages: 1 }");
        assert_eq!(format!("{summary:#?}"), "OfficialSummary { analyses: 2, titrages: 1 }");
        assert_eq!(
            format!("{:?}", OfficialSummary::default()),
            "OfficialSummary { analyses: 0, titrages: 0 }"
        );
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

    // Données réelles : sommaires de CETATEXT000007742504 (Nicolo) et CETATEXT000052137642,
    // relevés enregistrés du crate lf-adapter-piste.
    const NICOLO_9AA: &str = "01-01-02-02, 15-03-01-01        En vertu des dispositions de l'article 55 de la Constitution, il appartient au juge administratif de contrôler la compatibilité entre les traités internationaux et les lois françaises même postérieures.";

    #[test]
    fn class_code_list_is_stripped() {
        assert_eq!(
            norm(&[NICOLO_9AA, "17-02-01-01, 54-07-01-04        En vertu des dispositions"]),
            [
                "En vertu des dispositions de l'article 55 de la Constitution, il appartient au juge administratif de contrôler la compatibilité entre les traités internationaux et les lois françaises même postérieures.",
                "En vertu des dispositions"
            ]
        );
        assert_eq!(strip_class_code("44-12, 1, 33-01 Abc"), "44-12, 1, 33-01 Abc");
        assert_eq!(strip_class_code("44-12, 15-03, 33-01 Abc"), "Abc");
    }

    #[test]
    fn class_code_list_with_subdivision() {
        assert_eq!(strip_class_code("01-04-005 a) Abc def"), "Abc def");
        assert_eq!(strip_class_code("01-04-005, 15-03 b) État"), "État");
        assert_eq!(strip_class_code("01-04-005 a)Abc"), "01-04-005 a)Abc");
        assert_eq!(strip_class_code("01-04-005 a) abc"), "01-04-005 a) abc");
        assert_eq!(strip_class_code("01-04-005 A) Abc"), "A) Abc");
        assert_eq!(strip_class_code("01-04-005 ab) Abc"), "01-04-005 ab) Abc");
        assert_eq!(strip_class_code("01-04-005 a Abc"), "01-04-005 a Abc");
    }

    #[test]
    fn class_code_list_keeps_what_is_not_a_code_list() {
        assert_eq!(strip_class_code("12,50 Euros"), "12,50 Euros");
        assert_eq!(strip_class_code("44, Abc"), "44, Abc");
        assert_eq!(strip_class_code("12, 15 Euros"), "12, 15 Euros");
        assert_eq!(strip_class_code("44-12, 15 Abc"), "44-12, 15 Abc");
        assert_eq!(strip_class_code("44-12, 15-1 Abc"), "44-12, 15-1 Abc");
        assert_eq!(strip_class_code("44-12, abc Def"), "44-12, abc Def");
        assert_eq!(strip_class_code("44,"), "44,");
    }

    #[test]
    fn class_code_rj_marks_follow_the_codes() {
        assert_eq!(
            strip_class_code("CETAT01-01-02-02,RJ1,RJ2 ACTES LEGISLATIFS"),
            "ACTES LEGISLATIFS"
        );
        assert_eq!(strip_class_code("CETAT15-03-01-01,RJ1 COMMUNAUTES"), "COMMUNAUTES");
        assert_eq!(strip_class_code("01-01, RJ12 Abc"), "Abc");
        assert_eq!(strip_class_code("01-01,RJ Abc"), "01-01,RJ Abc");
        assert_eq!(strip_class_code("01-01,RJ1x Abc"), "01-01,RJ1x Abc");
        assert_eq!(strip_class_code("RJ1 Abc"), "RJ1 Abc");
    }

    #[test]
    fn dotted_separators_split_the_entry() {
        assert_eq!(
            norm(&[
                "15-07 En vertu de l'article 260, l'exécution de l'arrêt de la Cour.......Par suite l'exécution implique 1) sans exception, et 2) sans recherche.......Application à l'arrêt C-416/17."
            ]),
            [
                "En vertu de l'article 260, l'exécution de l'arrêt de la Cour.",
                "Par suite l'exécution implique 1) sans exception, et 2) sans recherche.",
                "Application à l'arrêt C-416/17."
            ]
        );
        assert_eq!(norm(&["Alpha....1) Beta"]), ["Alpha.", "1) Beta"]);
        assert_eq!(norm(&["Alpha ....Beta"]), ["Alpha.", "Beta"]);
        assert_eq!(norm(&["Alpha. ....Beta"]), ["Alpha.", "Beta"]);
        assert_eq!(norm(&["Vraiment ?....Oui"]), ["Vraiment ?", "Oui"]);
        assert_eq!(norm(&["Non !....Oui"]), ["Non !", "Oui"]);
        assert_eq!(norm(&["A.... ....B"]), ["A.", "B"]);
        assert_eq!(norm(&["A.... ...."]), ["A."]);
        assert_eq!(norm(&["....A.... ....B"]), ["....A.", "B"]);
    }

    #[test]
    fn dotted_separators_keep_neighbours_unchanged() {
        assert_eq!(norm(&["Alpha...Beta"]), ["Alpha...Beta"]);
        assert_eq!(norm(&["etc...."]), ["etc...."]);
        assert_eq!(norm(&["....Beta"]), ["....Beta"]);
        assert_eq!(norm(&["Alpha .... "]), ["Alpha ...."]);
        assert_eq!(norm(&["Alpha...."]), ["Alpha...."]);
    }

    #[test]
    fn dotted_fragments_are_not_deduplicated_by_substring() {
        assert_eq!(
            norm(&["Alpha....Beta gamma", "Beta gamma delta", "Beta"]),
            ["Alpha.", "Beta gamma", "Beta gamma delta", "Beta"]
        );
        assert_eq!(norm(&["Alpha....Beta", "Beta....Alpha"]), ["Alpha.", "Beta", "Beta.", "Alpha"]);
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
