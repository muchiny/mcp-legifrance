//! Nettoyage des textes du BOFiP (ADR-0029, décision 4).
//!
//! Le champ `contenu` du jeu `bofip-vigueur` est du texte brut, sans balise, où restent cinq
//! entités et elles seules (mesure du 2026-11-15 sur les 9 173 publications) : `&#13;` (70 038
//! fois), `&nbsp;` (14 532), `&gt;` (492), `&amp;` (328) et `&lt;` (114). S'y ajoutent des retours
//! chariot, des tabulations et des espaces insécables.
//!
//! Dans 5 543 publications, toutes datées de 2012 à 2020, le champ garde des sauts de ligne placés
//! au milieu des phrases (même jeu, mesure du 2026-11-15 : 200 649 sauts). Ils sont remplacés par
//! une espace avant la mise en paragraphes : sans cela, chaque bout de phrase deviendrait un
//! paragraphe, et un passage rendu par une lecture ciblée serait un bout de phrase (AM-95).
//!
//! Les cinq entités sont traitées en **une seule passe** : le résultat d'un décodage n'est jamais
//! relu, donc `&amp;lt;` reste `&lt;` (le texte parlait de l'entité, il n'écrivait pas un
//! chevron). Toute autre entité est laissée telle quelle et comptée : le compte est rendu à
//! l'appelant, qui peut le journaliser. Le nettoyage HTML des champs de Légifrance
//! ([`clean`](crate::text::clean)) ne sert pas ici : il est fait pour du balisage, et un `<` du
//! BOFiP est un chevron.

use crate::{
    error::DomainError,
    text::{LongText, flatten_whitespace, is_invisible, strip_invisible},
};

/// Longueur maximale, en caractères, d'un champ court rendu ([`clean_line`]) ; au-delà, le champ
/// est coupé et suivi de `…`. Le titre le plus long mesuré le 2026-11-15 fait 747 caractères.
pub const LINE_MAX_CHARS: usize = 1_000;

/// Les cinq entités traitées, sans leur `&` initial, et ce qu'elles deviennent.
const KNOWN_ENTITIES: [(&str, &str); 5] =
    [("#13;", ""), ("nbsp;", " "), ("lt;", "<"), ("gt;", ">"), ("amp;", "&")];

/// Longueur maximale du nom d'une entité comptée comme inconnue (`&` et `;` exclus).
const ENTITY_NAME_MAX: usize = 32;

/// Fins de ligne après lesquelles la ligne suivante commence un paragraphe : la phrase est finie,
/// ou elle annonce une suite (liste, exemple).
const SENTENCE_ENDS: [char; 5] = ['.', ':', ';', '!', '?'];

/// Texte canonique du champ `contenu` et nombre d'entités inconnues qui y restent.
///
/// `&#13;` et le retour chariot sont retirés ; `&nbsp;`, l'espace insécable et la tabulation
/// deviennent une espace ; `&lt;`, `&gt;` et `&amp;` sont décodés. Un saut de ligne placé au
/// milieu d'une phrase devient une espace : la ligne d'avant ne finit pas par `.`, `:`, `;`, `!`
/// ou `?`, et la ligne d'après commence par une minuscule, sans être un repère de liste. Le texte
/// passe ensuite par
/// [`LongText::from_plain`] : un paragraphe par ligne non blanche, caractères invisibles
/// retirés, blancs repliés.
///
/// ```
/// use lf_domain::doctrine::clean_text;
///
/// let (text, unknown) = clean_text("1&#13;\nTaux&nbsp;: 5 % &amp; 10 %&#13;\n&eacute;")?;
/// assert_eq!(text.as_str(), "1\n\nTaux : 5 % & 10 %\n\n&eacute;");
/// assert_eq!(unknown, 1);
///
/// let (text, _) = clean_text("10\nLe seuil est prévu\nà l'article 964.\nII. Tarif")?;
/// assert_eq!(text.as_str(), "10\n\nLe seuil est prévu à l'article 964.\n\nII. Tarif");
/// # Ok::<(), lf_domain::DomainError>(())
/// ```
///
/// # Errors
///
/// [`DomainError::TextTooLarge`] si le texte dépasse [`LongText::MAX_CHARS`] caractères.
pub fn clean_text(raw: &str) -> Result<(LongText, u32), DomainError> {
    // Le texte décodé est libéré dès qu'il est recollé : une seule copie reste pendant la mise en
    // paragraphes.
    let (joined, unknown) = {
        let (decoded, unknown) = decode(raw);
        (rejoined(&decoded), unknown)
    };
    LongText::from_plain(&joined).map(|text| (text, unknown))
}

/// Champ court (titre, division) sur une ligne : mêmes entités que [`clean_text`], caractères
/// invisibles retirés, blancs repliés, [`LINE_MAX_CHARS`] caractères au plus (`…` ensuite).
///
/// ```
/// use lf_domain::doctrine::clean_line;
///
/// assert_eq!(clean_line(" ENR -\u{a0}Mutations &amp; partages\r\n"), "ENR - Mutations & partages");
/// ```
pub fn clean_line(raw: &str) -> String {
    let (decoded, _) = decode(raw);
    let (visible, _) = strip_invisible(&decoded);
    clipped(&flatten_whitespace(&visible), LINE_MAX_CHARS)
}

/// `raw` sans retour chariot, ses cinq entités connues décodées en une passe, et le nombre
/// d'entités inconnues laissées telles quelles.
///
/// `raw` est découpé sur `&` : chaque morceau après le premier suit un `&`. S'il commence par
/// une entité connue, elle est remplacée ; sinon le `&` est remis. Rien de ce qui est écrit
/// n'est relu.
fn decode(raw: &str) -> (String, u32) {
    let mut pieces = raw.split('&');
    let mut out = String::with_capacity(raw.len());
    let mut unknown = 0_u32;
    push_without_cr(&mut out, pieces.next().unwrap_or_default());
    for tail in pieces {
        let known =
            KNOWN_ENTITIES.iter().find_map(|(name, to)| Some((*to, tail.strip_prefix(name)?)));
        let rest = if let Some((to, rest)) = known {
            out.push_str(to);
            rest
        } else {
            unknown = unknown.saturating_add(u32::from(is_entity(tail)));
            out.push('&');
            tail
        };
        push_without_cr(&mut out, rest);
    }
    (out, unknown)
}

/// Ajoute `text` à `out`, sans ses retours chariot.
fn push_without_cr(out: &mut String, text: &str) {
    out.extend(text.chars().filter(|c| *c != '\r'));
}

/// `text` où chaque saut de ligne placé au milieu d'une phrase ([`continues`]) est remplacé par
/// une espace. Rien d'autre ne change : ni caractère ajouté, ni caractère retiré.
///
/// Mesure du 2026-11-15 sur les 9 173 publications : 200 649 sauts recollés dans 5 543
/// publications de 2012 à 2020. Dans les 1 405 publications de 2021 à 2023, où chaque ligne est
/// déjà un paragraphe, la règle recolle 47 sauts dans 23 publications : deux cellules de tableau
/// se suivent alors sur une ligne. Les publications depuis 2024 n'ont aucun saut de ligne.
///
/// Limites connues, mesurées sur le même jeu. Un saut placé devant un chiffre, une majuscule ou
/// une parenthèse reste un saut (« L'article », puis « 977 du CGI »), faute de pouvoir le
/// distinguer d'un titre suivi de son numéro de paragraphe. Une ligne finie par le point d'une
/// abréviation reste coupée (529 sauts : « (cf. », puis « art. »). Une ligne faite d'un seul
/// nombre suit la règle commune (314 sauts, le plus souvent un numéro d'article : « 39 », puis
/// « quinquies H du CGI »). Un trait d'union collé au mot d'avant est presque toujours une
/// incise, que l'espace sert (17 sauts sur 18) ; un nom coupé à son trait d'union reçoit une
/// espace de trop (« Savignésous- », puis « le-Lude »).
fn rejoined(text: &str) -> String {
    let mut lines = text.split('\n');
    let mut previous = lines.next().unwrap_or_default();
    let mut out = String::with_capacity(text.len());
    out.push_str(previous);
    for line in lines {
        out.push(if continues(previous, line) { ' ' } else { '\n' });
        out.push_str(line);
        previous = line;
    }
    out
}

/// Vrai si le saut de ligne entre `previous` et `line` tombe au milieu d'une phrase : `previous`
/// ne finit pas par une ponctuation de [`SENTENCE_ENDS`] et `line` commence au milieu d'une
/// phrase ([`starts_mid_sentence`]). Les blancs et les caractères invisibles de fin de `previous`
/// et de début de `line` ne comptent pas ([`is_edge`]). Une ligne blanche, d'un côté ou de
/// l'autre, sépare toujours.
fn continues(previous: &str, line: &str) -> bool {
    let open = previous
        .trim_end_matches(is_edge)
        .chars()
        .next_back()
        .is_some_and(|last| !SENTENCE_ENDS.contains(&last));
    open && starts_mid_sentence(line.trim_start_matches(is_edge))
}

/// Vrai pour un caractère qui ne compte pas au bord d'une ligne : un blanc, ou un caractère
/// invisible que [`LongText::from_plain`] retire ensuite. La décision de recoller se prend ainsi
/// sur ce que le lecteur verra.
fn is_edge(c: char) -> bool {
    c.is_whitespace() || is_invisible(c)
}

/// Vrai si `line` commence par une lettre minuscule, sans être un repère de liste : une seule
/// lettre suivie de `.`, de `)` ou de `°`, puis d'un blanc ou de la fin de la ligne
/// (« a. Principe », « b) », « a° Principe »). « n° 12 » n'est pas un repère : c'est un numéro.
/// « art. 642 » et « à l'article » commencent au milieu d'une phrase. Un caractère invisible qui
/// n'est pas un blanc est sauté : il ne sera pas rendu.
fn starts_mid_sentence(line: &str) -> bool {
    let mut chars = line.chars().filter(|c| c.is_whitespace() || !is_invisible(*c));
    let first = chars.next();
    let sign = match chars.next() {
        Some('.' | ')') => true,
        Some('°') => first != Some('n'),
        _ => false,
    };
    let marker = sign && chars.next().is_none_or(char::is_whitespace);
    first.is_some_and(char::is_lowercase) && !marker
}

/// Vrai si `tail`, le texte qui suit un `&`, commence comme une entité : un `#` facultatif, de
/// 1 à [`ENTITY_NAME_MAX`] lettres ASCII ou chiffres, puis `;`. Un `&` suivi d'autre chose
/// (« R&D », « A & B ») n'est pas une entité.
fn is_entity(tail: &str) -> bool {
    let name = tail.strip_prefix('#').unwrap_or(tail);
    let len = name.bytes().take_while(u8::is_ascii_alphanumeric).count();
    (1..=ENTITY_NAME_MAX).contains(&len) && name.as_bytes().get(len) == Some(&b';')
}

/// `text` coupé à `max` caractères, suivi de `…` s'il était plus long.
fn clipped(text: &str, max: usize) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() { head + "…" } else { head }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn cleaned(raw: &str) -> (String, u32) {
        let (text, unknown) = clean_text(raw).unwrap();
        (text.as_str().to_owned(), unknown)
    }

    #[test]
    fn the_five_measured_entities_are_handled() {
        assert_eq!(cleaned("a&#13;b"), ("ab".to_owned(), 0));
        assert_eq!(cleaned("a&nbsp;b"), ("a b".to_owned(), 0));
        assert_eq!(cleaned("a &lt; b &gt; c"), ("a < b > c".to_owned(), 0));
        assert_eq!(cleaned("R&amp;D"), ("R&D".to_owned(), 0));
        assert_eq!(cleaned("&lt;&gt;&amp;&nbsp;&#13;x"), ("<>& x".to_owned(), 0));
    }

    /// Le résultat d'un décodage n'est jamais relu : une entité écrite en clair dans le texte
    /// (`&amp;lt;`) reste une entité, elle ne devient pas le caractère.
    #[test]
    fn entities_decoded_in_one_pass() {
        for (raw, expected) in [
            ("&amp;lt;", "&lt;"),
            ("&amp;gt;", "&gt;"),
            ("&amp;amp;", "&amp;"),
            ("&amp;#13;", "&#13;"),
            ("&amp;nbsp;", "&nbsp;"),
            ("&amp;amp;lt;", "&amp;lt;"),
            ("&amp;&amp;lt;", "&&lt;"),
            ("&&amp;lt;", "&&lt;"),
        ] {
            assert_eq!(decode(raw), (expected.to_owned(), 0));
            assert_eq!(cleaned(raw), (expected.to_owned(), 0));
        }
        // Un retour chariot retiré ne recolle pas une entité : rien n'est relu.
        assert_eq!(decode("&l\rt;"), ("&lt;".to_owned(), 0));
        assert_eq!(decode("&&#13;lt;"), ("&lt;".to_owned(), 0));
    }

    #[test]
    fn carriage_returns_are_removed_and_lines_become_paragraphs() {
        assert_eq!(cleaned("Un.\r\nDeux.&#13;\nTrois."), ("Un.\n\nDeux.\n\nTrois.".to_owned(), 0));
        assert_eq!(cleaned("a\rb"), ("ab".to_owned(), 0));
        assert_eq!(decode("\r\ra\r\n\r"), ("a\n".to_owned(), 0));
    }

    /// Extrait de `BOI-PAT-IFI-40-10` (2018), tel que le portail le sert : les sauts de ligne
    /// placés au milieu des phrases sont recollés ; le titre, les numéros de paragraphe et la
    /// ligne qui commence par un chiffre restent des paragraphes.
    #[test]
    fn a_line_break_inside_a_sentence_becomes_a_space() {
        let raw = "I. Seuil d'imposition\n1\nLe seuil d’assujettissement à l'impôt sur la fortune \
                   immobilière (IFI) est prévu\nà l'article 964 du code général des impôts (CGI). \
                   Ainsi, sont redevables de l'IFI les personnes physiques dont la valeur \
                   nette\ndes actifs immobiliers imposables au 1er janvier de l'année \
                   d'imposition est supérieure à 1,3 M€.\nII. Tarif\n10\nL'article\n977 du \
                   CGI\u{a0}fixe le tarif de l'impôt";
        let (text, _) = clean_text(raw).unwrap();
        let paragraphs: Vec<&str> = text.paragraphs().map(|(_, paragraph)| paragraph).collect();
        assert_eq!(
            paragraphs,
            [
                "I. Seuil d'imposition",
                "1",
                "Le seuil d’assujettissement à l'impôt sur la fortune immobilière (IFI) est prévu \
                 à l'article 964 du code général des impôts (CGI). Ainsi, sont redevables de \
                 l'IFI les personnes physiques dont la valeur nette des actifs immobiliers \
                 imposables au 1er janvier de l'année d'imposition est supérieure à 1,3 M€.",
                "II. Tarif",
                "10",
                "L'article",
                "977 du CGI fixe le tarif de l'impôt",
            ]
        );
    }

    #[test]
    fn a_finished_line_ends_its_paragraph() {
        for end in SENTENCE_ENDS {
            assert!(!continues(&format!("fin{end}"), "suite"), "{end}");
            assert!(!continues(&format!("fin{end} \t"), "suite"), "{end}");
        }
        for open in ["la valeur nette", "prévu,", "(CGI", "de 10 %", "l'article 964", "fin »"] {
            assert!(continues(open, "suite"), "{open}");
        }
        assert_eq!(cleaned("Phrase finie.\nsuite"), ("Phrase finie.\n\nsuite".to_owned(), 0));
    }

    #[test]
    fn a_line_that_starts_a_paragraph_is_not_joined() {
        for start in
            ["Les dons", "977 du CGI", "10", "- six mois", "(CGI, art. 642)", "« Pinel »", ""]
        {
            assert!(!continues("ligne ouverte", start), "{start}");
            assert!(!starts_mid_sentence(start), "{start}");
        }
        assert_eq!(cleaned("I. Tarif\nLe tarif"), ("I. Tarif\n\nLe tarif".to_owned(), 0));
    }

    /// Un repère de liste est une seule minuscule suivie de `.`, de `)` ou de `°`, puis d'un
    /// blanc ou de la fin de la ligne. Une abréviation, un mot court ou « n° » n'en est pas un.
    #[test]
    fn a_list_marker_is_not_joined() {
        for marker in [
            "a. Obligations des bénéficiaires",
            "b) Cas particuliers",
            "a° Principe",
            "a.",
            "b)",
            "f°",
            "c.\tx",
        ] {
            assert!(!starts_mid_sentence(marker), "{marker}");
        }
        for start in [
            "art. 642).",
            "n° 2018-1317",
            "n°",
            "a°b",
            "à l'article 964",
            "du CGI",
            "de la",
            "a.b",
            "a)b",
            "p",
            "à",
        ] {
            assert!(starts_mid_sentence(start), "{start}");
        }
        assert_eq!(
            cleaned("3. Obligations\na. Des bénéficiaires\nsont tenus"),
            ("3. Obligations\n\na. Des bénéficiaires sont tenus".to_owned(), 0)
        );
        assert_eq!(
            cleaned("1° Réintégrations\na° Principe\n(loi\nn° 2018-1317)"),
            ("1° Réintégrations\n\na° Principe\n\n(loi n° 2018-1317)".to_owned(), 0)
        );
    }

    #[test]
    fn a_blank_line_always_separates() {
        assert!(!continues("", "suite") && !continues(" \t\u{a0}", "suite"));
        assert!(!continues("ligne ouverte", " \t\u{a0}"));
        for raw in
            ["Fraction\n\ntaxable", "Fraction\n \u{a0}\t\ntaxable", "Fraction\n&nbsp;\ntaxable"]
        {
            assert_eq!(cleaned(raw), ("Fraction\n\ntaxable".to_owned(), 0), "{raw:?}");
        }
        // Les blancs autour du saut ne comptent pas.
        assert_eq!(cleaned("la valeur \t\n\u{a0} nette"), ("la valeur nette".to_owned(), 0));
    }

    /// La décision se prend sur ce que le lecteur verra : un caractère invisible ne cache ni une
    /// fin de phrase, ni une minuscule, ni un repère de liste, et une ligne qui n'a que des
    /// caractères invisibles est une ligne blanche.
    #[test]
    fn the_decision_ignores_invisible_characters() {
        assert!(!continues("fin.\u{200b}", "suite") && !continues("fin. \u{feff} ", "suite"));
        assert!(continues("fin\u{200b} ", "suite") && continues("fin", "\u{feff} \u{200b}suite"));
        assert!(!continues("fin", "\u{feff}Suite") && !continues("\u{200b}", "suite"));
        assert!(!continues("fin", "\u{200b}\u{feff}"));
        // Dans un repère de liste : le caractère invisible est sauté, le blanc de contrôle compte.
        for marker in ["a.\u{200b} Des bénéficiaires", "a\u{200b}. Des", "a.\u{b}Des", "a\u{feff})"]
        {
            assert!(!starts_mid_sentence(marker), "{marker:?}");
        }
        assert!(starts_mid_sentence("d\u{200b}u CGI") && starts_mid_sentence("a.\u{200b}b"));
        assert_eq!(cleaned("Fin.\u{200b}\nsuite"), ("Fin.\n\nsuite".to_owned(), 0));
        assert_eq!(cleaned("fin\n\u{feff}suite"), ("fin suite".to_owned(), 0));
        assert_eq!(cleaned("fin\n\u{200b}\nsuite"), ("fin\n\nsuite".to_owned(), 0));
        assert_eq!(
            cleaned("Obligations\na.\u{200b} Des bénéficiaires"),
            ("Obligations\n\na. Des bénéficiaires".to_owned(), 0)
        );
    }

    #[test]
    fn rejoining_changes_line_breaks_only() {
        assert_eq!(rejoined(""), "");
        assert_eq!(rejoined("a"), "a");
        assert_eq!(rejoined("a\nb"), "a b");
        assert_eq!(rejoined("A.\nb"), "A.\nb");
        assert_eq!(rejoined(" a \n b \nC\n\nd\n"), " a   b \nC\n\nd\n");
        assert_eq!(rejoined("a\nb\nfin.\nd"), "a b fin.\nd");
        // Les caractères invisibles ne sont pas touchés ici : ils restent comptés ensuite.
        let (text, _) = clean_text("plus\u{b}\nvalue\u{200b}").unwrap();
        assert_eq!((text.as_str(), text.invisible_removed()), ("plus value", 2));
    }

    #[test]
    fn blanks_become_one_space() {
        assert_eq!(cleaned("a\u{a0}b\tc &nbsp;\u{a0}\t d"), ("a b c d".to_owned(), 0));
        assert_eq!(cleaned("  a  \n\n\n  b  "), ("a\n\nb".to_owned(), 0));
    }

    #[test]
    fn unknown_entities_are_kept_and_counted() {
        assert_eq!(cleaned("&eacute;t&eacute;"), ("&eacute;t&eacute;".to_owned(), 2));
        assert_eq!(cleaned("&#233; &#xE9; &#013;"), ("&#233; &#xE9; &#013;".to_owned(), 3));
        assert_eq!(cleaned("&NBSP; &Lt; &AMP;"), ("&NBSP; &Lt; &AMP;".to_owned(), 3));
        assert_eq!(cleaned("&amp;&copy;"), ("&&copy;".to_owned(), 1));
    }

    #[test]
    fn a_lone_ampersand_is_not_an_entity() {
        for raw in ["R&D", "A & B", "a&", "&", "&;", "&#;", "& lt;", "&l t;", "&é;", "&lt", "&##1;"]
        {
            assert_eq!(cleaned(raw), (raw.to_owned(), 0));
        }
    }

    #[test]
    fn entity_names_are_bounded() {
        let longest = format!("&{};", "a".repeat(ENTITY_NAME_MAX));
        assert_eq!(decode(&longest), (longest.clone(), 1));
        let too_long = format!("&{};", "a".repeat(ENTITY_NAME_MAX + 1));
        assert_eq!(decode(&too_long), (too_long.clone(), 0));
    }

    #[test]
    fn empty_text_is_empty() {
        let (text, unknown) = clean_text("").unwrap();
        assert!(text.is_empty());
        assert_eq!((text.char_len(), unknown), (0, 0));
        assert!(clean_text("&#13;\r\n&nbsp;\t").unwrap().0.is_empty());
    }

    #[test]
    fn invisible_characters_are_removed_from_the_text() {
        let (text, _) = clean_text("plus\u{200b}-value\u{202e}").unwrap();
        assert_eq!((text.as_str(), text.invisible_removed()), ("plus-value", 2));
    }

    #[test]
    fn text_over_the_limit_is_refused() {
        let raw = "é".repeat(16_000_001);
        assert_eq!(clean_text(&raw), Err(DomainError::TextTooLarge { chars: 16_000_001 }));
    }

    #[test]
    fn a_line_is_decoded_flattened_and_stripped() {
        assert_eq!(
            clean_line(" ENR -\u{a0}Mutations &amp; partages\r\n"),
            "ENR - Mutations & partages"
        );
        assert_eq!(clean_line("a\n\nb\tc&nbsp;&nbsp;d"), "a b c d");
        assert_eq!(clean_line("IR\u{200b} - \u{202e}Base&#13;"), "IR - Base");
        assert_eq!(clean_line("&amp;lt; &eacute;"), "&lt; &eacute;");
        assert_eq!(clean_line(" \r\n\t&nbsp;"), "");
    }

    #[test]
    fn a_line_is_clipped_with_an_ellipsis() {
        let longest = "é".repeat(LINE_MAX_CHARS);
        assert_eq!(clean_line(&longest), longest);
        let clipped = clean_line(&"é".repeat(LINE_MAX_CHARS + 1));
        assert_eq!(clipped, format!("{longest}…"));
        assert_eq!(super::clipped("abc", 2), "ab…");
        assert_eq!(super::clipped("abc", 3), "abc");
    }

    proptest! {
        /// Sans `&` ni retour chariot, le décodage ne change rien et ne compte rien.
        #[test]
        fn text_without_ampersand_is_unchanged(raw in "[^&\r]{0,200}") {
            prop_assert_eq!(decode(&raw), (raw.clone(), 0));
        }

        /// Le recollage ne change que des sauts de ligne, chacun en une espace : même longueur,
        /// mêmes caractères partout ailleurs.
        #[test]
        fn rejoining_only_turns_line_breaks_into_spaces(
            raw in "(\\PC|[\n\u{200b}\u{feff}\u{85}\u{b}\u{a0}]){0,60}"
        ) {
            let out = rejoined(&raw);
            prop_assert_eq!(out.len(), raw.len());
            let only_breaks = raw
                .chars()
                .zip(out.chars())
                .all(|(before, after)| before == after || (before == '\n' && after == ' '));
            prop_assert!(only_breaks);
        }

        /// Le décodage ne laisse aucun retour chariot et ne rallonge jamais le texte.
        #[test]
        fn decoding_never_grows_the_text(raw in "(&(amp|lt|gt|nbsp|#13|x);|[a&;#\r ]){0,60}") {
            let (out, unknown) = decode(&raw);
            prop_assert!(!out.contains('\r'));
            prop_assert!(out.len() <= raw.len());
            prop_assert!(usize::try_from(unknown).unwrap() <= raw.matches('&').count());
        }
    }
}
