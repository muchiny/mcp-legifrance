//! HTML Légifrance vers texte des champs courts : titres, natures, numéros, extraits,
//! entrées de sommaire (03 §7.3).
//!
//! Balises relevées dans les fixtures de la référence
//! (`crates/lf-adapter-piste/tests/fixtures/reference/`) : `<br>`
//! (`search_juri.json`, `search_cetat.json`, `consult_juri.json`, `consult_juri_cetat.json`,
//! `consult_juri_constit.json`), `<BR>` et `</BR>` (`consult_juri_constit.json`), `<mark>`
//! (`search_*.json`), `<p>` (`consult_cnil.json`), `<RECH_ECLI>` (`consult_juri_constit.json`,
//! `consult_juri_cetat.json`), `<a …>` (`consult_kali.json`). Aucune entité HTML (`&amp;`,
//! `&#`, `&nbsp;`) dans aucune fixture ; elles sont tout de même décodées (V-94).
//!
//! Étapes, dans l'ordre, sur un texte déjà réparé du mojibake (03 §7.1) : balises par le lexer
//! partagé (`text::html`), entités, caractères invisibles (retirés et comptés), blancs.

use super::html::{self, Block, Event};

/// Diagnostic d'un nettoyage, pour le compteur journalisé de l'adapter (03 §7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CleanReport {
    /// Caractères invisibles, de contrôle, bidirectionnels ou tags Unicode retirés, qu'ils
    /// soient bruts ou décodés d'une entité numérique.
    pub invisible_removed: u32,
}

/// Nettoie un fragment HTML : voir [`clean_with_report`], dont c'est le texte.
pub fn clean(html: &str) -> String {
    clean_with_report(html).0
}

/// Nettoie un fragment HTML et compte les caractères invisibles retirés.
///
/// 1. Balises retirées (`text::html`, 03 §7.2) : `<br>` donne un saut de ligne, `<p>` et
///    `</p>` un paragraphe (ligne vide), un autre bloc (`li`, `div`, `h1`…, `</br>`) termine
///    la ligne en cours si elle a du texte, une cellule (`td`, `th`) donne une espace, une
///    balise en ligne (`mark`, `a`, `sup`…) disparaît sans espace ; `RECH_*`, `script` et
///    `style` partent avec leur contenu ; commentaires retirés. Le saut « souple » d'un bloc
///    juge la ligne sur le texte **avant** décodage des entités : une ligne qui ne porte que
///    `&nbsp;` compte comme du texte, puis devient blanche (`<li>a</li>&nbsp;<li>b</li>`
///    donne `a\n\nb`).
/// 2. Entités décodées, après les balises : `&lt;b&gt;` reste du texte.
/// 3. Caractères invisibles retirés et comptés (`text::html` : contrôles, et tout point de code
///    `Default_Ignorable_Code_Point`, dont bidi et sélecteurs de variante) ; VT, FF et NEL
///    deviennent une espace (comptés aussi).
/// 4. Blancs : espaces internes réduits à un (NBSP comprise), lignes rognées, au plus une
///    ligne vide ; les sauts de ligne de la source restent des sauts de ligne.
///
/// Non idempotent : une entrée qui contient `<` ou `&` peut donner une sortie qui en contient
/// encore (`<<b>b>` donne `<b>`, `&amp;lt;` donne `&lt;`).
pub fn clean_with_report(html: &str) -> (String, CleanReport) {
    let mut raw = String::with_capacity(html.len());
    // La ligne en cours de `raw` a-t-elle un caractère non blanc ?
    let mut line_has_text = false;
    html::scan(html, |event| match event {
        Event::Text(text) => {
            raw.push_str(text);
            line_has_text = match text.rsplit_once('\n') {
                Some((_, last_line)) => has_text(last_line),
                None => line_has_text || has_text(text),
            };
        }
        Event::Break(Block::Br) => {
            raw.push('\n');
            line_has_text = false;
        }
        Event::Break(Block::P) => {
            raw.push_str("\n\n");
            line_has_text = false;
        }
        Event::Break(Block::Other) => {
            if line_has_text {
                raw.push('\n');
                line_has_text = false;
            }
        }
        Event::Cell => raw.push(' '),
    });
    let decoded = html::decode_entities(&raw);
    let (visible, invisible_removed) = html::strip_invisible(&decoded);
    (normalize_whitespace(&visible), CleanReport { invisible_removed })
}

/// Champ court nettoyé ; absent, ou vide après nettoyage : `None` (le mapping ne produit
/// jamais `Some("")`, 03 §4.0).
pub fn clean_field(raw: Option<&str>) -> Option<String> {
    raw.map(clean).filter(|text| !text.is_empty())
}

/// Tout blanc (saut de ligne, tabulation, espaces répétés) replié en une seule espace, blancs de
/// bord retirés : le texte tient sur une ligne. Sert aux titres et aux extraits de résultats de
/// recherche, qui n'ont pas besoin de paragraphes (03 §7.3).
pub fn flatten_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `s` a un caractère non blanc.
fn has_text(s: &str) -> bool {
    s.chars().any(|c| !c.is_whitespace())
}

/// Espaces internes réduits à un, lignes rognées, au plus une ligne vide.
fn normalize_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_blank = false;
    for line in s.split('\n') {
        let mut words = line.split_whitespace().peekable();
        if words.peek().is_none() {
            pending_blank = true;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if pending_blank { "\n\n" } else { "\n" });
        }
        pending_blank = false;
        for (i, word) in words.enumerate() {
            if i > 0 {
                out.push(' ');
            }
            out.push_str(word);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::encoding::repair_latin1_mojibake;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    #[test]
    fn flatten_whitespace_gives_one_line() {
        assert_eq!(flatten_whitespace(" a\n\nb \t c\r\n d\u{a0}e "), "a b c d e");
        assert_eq!(flatten_whitespace("déjà sur une ligne"), "déjà sur une ligne");
        assert_eq!(flatten_whitespace(" \n\t "), "");
    }

    /// Un seul caractère retiré.
    const ONE: CleanReport = CleanReport { invisible_removed: 1 };

    /// Plages retirées, écrites d'après 03 §7.3 indépendamment de `html::strip_invisible` :
    /// contrôles C0 sauf `\t`, `\n`, `\r` ; U+007F et C1 ; puis les 17 plages de
    /// `Default_Ignorable_Code_Point` (format, bidi, sélecteurs de variante, remplisseurs, tags).
    /// Une table plutôt qu'un `matches!` : une sortie correcte n'en touche aucune alternative,
    /// qui resterait une région jamais exécutée.
    const SPEC_INVISIBLE: [(char, char); 21] = [
        ('\u{0}', '\u{8}'),
        ('\u{b}', '\u{c}'),
        ('\u{e}', '\u{1f}'),
        ('\u{7f}', '\u{9f}'),
        ('\u{ad}', '\u{ad}'),
        ('\u{34f}', '\u{34f}'),
        ('\u{61c}', '\u{61c}'),
        ('\u{115f}', '\u{1160}'),
        ('\u{17b4}', '\u{17b5}'),
        ('\u{180b}', '\u{180f}'),
        ('\u{200b}', '\u{200f}'),
        ('\u{202a}', '\u{202e}'),
        ('\u{2060}', '\u{206f}'),
        ('\u{3164}', '\u{3164}'),
        ('\u{fe00}', '\u{fe0f}'),
        ('\u{feff}', '\u{feff}'),
        ('\u{ffa0}', '\u{ffa0}'),
        ('\u{fff0}', '\u{fff8}'),
        ('\u{1bca0}', '\u{1bca3}'),
        ('\u{1d173}', '\u{1d17a}'),
        ('\u{e0000}', '\u{e0fff}'),
    ];

    fn spec_invisible(c: char) -> bool {
        SPEC_INVISIBLE.iter().any(|&(first, last)| (first..=last).contains(&c))
    }

    /// Fragments HTML hostiles : balises, guillemets, commentaires, entités, invisibles.
    fn hostile() -> impl Strategy<Value = String> {
        proptest::collection::vec(
            prop_oneof![
                "<",
                ">",
                "/",
                "\"",
                "'",
                "=",
                " ",
                "\n",
                "!--",
                "-->",
                "&",
                "#",
                "x",
                ";",
                "173",
                "202E",
                "E0041",
                "br",
                "p",
                "li",
                "td",
                "script",
                "RECH_E",
                "a",
                "é",
                "\u{a0}",
                "\u{200b}",
                "\u{202e}",
                "\u{e0041}",
                "\u{34f}",
                "\u{180e}",
                "\u{2063}",
                "\u{3164}",
                "\u{fe0f}",
                "\u{e0101}",
                "FE0F",
                "&shy;",
                "&lrm;",
                any::<char>().prop_map(String::from),
                (0x80_u8..=0x9f).prop_map(|b| char::from(b).to_string()),
            ],
            0..40,
        )
        .prop_map(|parts| parts.concat())
    }

    #[test]
    fn strips_search_marks() {
        assert_eq!(
            clean("Loi pour une sécurité globale préservant les <mark>libertés</mark>  - réserve"),
            "Loi pour une sécurité globale préservant les libertés - réserve"
        );
    }

    #[test]
    fn clean_strips_mark_tags() {
        assert_eq!(clean("<mark>x</mark>"), "x");
        assert_eq!(clean("a <MARK>libertés</MARK> b"), "a libertés b");
    }

    #[test]
    fn line_breaks_and_paragraphs() {
        assert_eq!(clean("a<br>b<BR></BR>c<br/>d"), "a\nb\nc\nd");
        assert_eq!(clean("<p>un</p><p>deux</p>"), "un\n\ndeux");
        assert_eq!(clean("a<br><br><br><br>b"), "a\n\nb");
    }

    #[test]
    fn keeps_link_text_and_unknown_tags_content() {
        assert_eq!(
            clean("voir <a href=\"/x\">article 9</a> et <RECH>ici</RECH>"),
            "voir article 9 et ici"
        );
    }

    #[test]
    fn keeps_literal_angle_brackets() {
        assert_eq!(clean("a < b et c > d"), "a < b et c > d");
        assert_eq!(clean("a <3"), "a <3");
        assert_eq!(clean("x <a <b>"), "x <a");
        assert_eq!(clean("fin <"), "fin <");
        assert_eq!(clean("<>"), "<>");
        assert_eq!(clean("</>"), "</>");
        assert_eq!(clean("a<b<c>d"), "a<bd");
    }

    #[test]
    fn decodes_entities() {
        assert_eq!(clean("A &amp; B &lt;C&gt; &quot;d&quot; l&apos;e"), "A & B <C> \"d\" l'e");
        assert_eq!(clean("art.&nbsp;9 &#233;t&#xE9; &#XE9;"), "art. 9 été é");
        assert_eq!(
            clean("&laquo;&nbsp;Code&nbsp;&raquo; l&rsquo;art. &ndash; &oelig;"),
            "« Code » l’art. – œ"
        );
    }

    #[test]
    fn keeps_unknown_or_broken_entities() {
        assert_eq!(
            clean("&unknown; &#xZZ; &#; & seul &#1114112;"),
            "&unknown; &#xZZ; &#; & seul &#1114112;"
        );
        // Hexadécimal sans chiffre : ni signe ni chiffre à lire.
        assert_eq!(clean("&#x; &#X;"), "&#x; &#X;");
        assert_eq!(clean("&verylongentityname;"), "&verylongentityname;");
        // Octet 12 au milieu d'un caractère multi-octet : texte conservé, sans panique.
        assert_eq!(clean("&ammmmmmmmmmé;"), "&ammmmmmmmmmé;");
        assert_eq!(clean("&amp;ééééééééé; &lt;"), "&ééééééééé; <");
    }

    #[test]
    fn signed_numeric_refs_stay_literal_and_controls_are_removed() {
        // NUL et signes `+` (acceptés à tort par `parse`) : verbatim.
        assert_eq!(clean("a&#0;b"), "a&#0;b");
        assert_eq!(clean("a&#+65;b"), "a&#+65;b");
        assert_eq!(clean("a&#x+41;b"), "a&#x+41;b");
        // Contrôle C0 (ESC), bidi (RLO), tag Unicode : décodés, puis retirés et comptés.
        assert_eq!(clean_with_report("a&#x1b;b"), ("ab".to_owned(), ONE));
        assert_eq!(clean_with_report("a&#8238;b"), ("ab".to_owned(), ONE));
        assert_eq!(clean_with_report("a&#xE0041;b"), ("ab".to_owned(), ONE));
        // Tabulation, saut de ligne et retour chariot : décodés et gardés.
        assert_eq!(clean("a&#10;b"), "a\nb");
        assert_eq!(clean("a&#9;b&#13;c"), "a b c");
    }

    #[test]
    fn every_bidi_and_c1_control_ref_is_removed_and_counted() {
        // C1 : seuls les cinq codes que Windows-1252 n'attribue pas restent des contrôles.
        for code in
            [0x7f_u32, 0x81, 0x8d, 0x8f, 0x90, 0x9d, 0x61c, 0x200e, 0x200f, 0x202a, 0x202e, 0x2066]
        {
            assert_eq!(clean_with_report(&format!("a&#{code};b")), ("ab".to_owned(), ONE));
        }
        assert_eq!(clean_with_report("a&#x2069;b"), ("ab".to_owned(), ONE));
        for code in [0xE0000_u32, 0xE007F] {
            assert_eq!(clean_with_report(&format!("a&#x{code:X};b")), ("ab".to_owned(), ONE));
        }
        // Bornes voisines : décodées et gardées.
        for (code, decoded) in
            [(0x7e_u32, '~'), (0xa0, ' '), (0x61d, '\u{61d}'), (0x2070, '\u{2070}'), (0x202f, ' ')]
        {
            assert_eq!(clean(&format!("a&#{code};b")), format!("a{decoded}b"));
        }
        assert_eq!(clean("&#xDFFFF;"), "\u{dffff}");
        assert_eq!(clean("&#xE1000;"), "\u{e1000}");
    }

    #[test]
    fn clean_removes_default_ignorable_chars() {
        // Audit L-1 : six sélecteurs de variante après le texte visible portent une charge
        // invisible (un octet par sélecteur) ; ils sont retirés et comptés, bruts ou décodés.
        let smuggled: String =
            "IGNORE".bytes().map(|b| char::from_u32(0xE0100 + u32::from(b)).unwrap()).collect();
        assert_eq!(
            clean_with_report(&format!("<p>Article 1x{smuggled}")),
            ("Article 1x".to_owned(), CleanReport { invisible_removed: 6 })
        );
        for code in [
            0x34f_u32, 0x115f, 0x17b4, 0x180e, 0x2061, 0x2065, 0x206f, 0x3164, 0xfe0f, 0xffa0,
            0xfff0, 0x1bca0, 0x1d173, 0xe0100, 0xe0fff,
        ] {
            assert_eq!(clean_with_report(&format!("a&#x{code:X};b")), ("ab".to_owned(), ONE));
        }
    }

    #[test]
    fn entities_are_decoded_once() {
        assert_eq!(clean("&amp;lt;b&amp;gt;"), "&lt;b&gt;");
        assert_eq!(clean("&lt;script&gt;"), "<script>");
    }

    #[test]
    fn known_tag_limits() {
        // Une seule passe : la sortie peut contenir une balise (non idempotent).
        assert_eq!(clean("<<b>b>"), "<b>");
        // Les guillemets d'un ouvrant littéral (un `<` avant le `>`) restent du texte.
        assert_eq!(clean("<a t=\"<b>\" <i>x"), "<a t=\"<b>\" x");
        // Entités décodées après le retrait des balises : une entité coupée par une balise en
        // ligne est recollée (`&am` puis `p;`), sans pouvoir créer de balise.
        assert_eq!(clean("&am<b></b>p;lt;b&gt;"), "&lt;b>");
        // Le saut souple d'un bloc juge le texte avant décodage : `&nbsp;` seul fait une ligne,
        // devenue blanche après décodage, donc une ligne vide.
        assert_eq!(clean("<li>a</li>&nbsp;<li>b</li>"), "a\n\nb");
    }

    #[test]
    fn normalizes_whitespace() {
        assert_eq!(clean("  a \t b\r\n\n\n\n  c  "), "a b\n\nc");
        assert_eq!(clean("\u{a0}a\u{202f}:"), "a :");
        assert_eq!(clean(""), "");
        assert_eq!(clean("<br><p>"), "");
    }

    #[test]
    fn clean_folds_whitespace_after_repair() {
        // Réparer d'abord (03 §7.1) : la NBSP rendue par la réparation, mêlée d'espaces et
        // de tabulations, devient une seule espace.
        let repaired = repair_latin1_mojibake("a\u{c2}\u{a0} \t\u{c2}\u{a0}b");
        assert_eq!(repaired, "a\u{a0} \t\u{a0}b");
        assert_eq!(clean(&repaired), "a b");
    }

    #[test]
    fn clean_removes_invisible_and_control_chars() {
        assert_eq!(
            clean_with_report("a\u{ad}b\u{200b}c\u{feff}d\u{7}e"),
            ("abcde".to_owned(), CleanReport { invisible_removed: 4 })
        );
        assert_eq!(clean_with_report("abc"), ("abc".to_owned(), CleanReport::default()));
    }

    #[test]
    fn clean_removes_bidi_controls() {
        assert_eq!(clean_with_report("abc\u{202e}def"), ("abcdef".to_owned(), ONE));
        for c in
            ['\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}', '\u{2066}', '\u{2067}', '\u{2069}']
        {
            assert_eq!(clean_with_report(&format!("a{c}b")), ("ab".to_owned(), ONE), "{c:?}");
        }
    }

    #[test]
    fn clean_drops_rech_and_script_with_content() {
        assert_eq!(clean("a<RECH_ECLI>ECLI:FR:CC:2025:2025.1173.QPC</RECH_ECLI>b"), "ab");
        assert_eq!(clean("<script>x</script>y"), "y");
        assert_eq!(clean("<style>p { color: red }</style>y"), "y");
        assert_eq!(clean("<RECH_ECLI>texte sans fermante"), "texte sans fermante");
    }

    #[test]
    fn clean_removes_comments() {
        assert_eq!(clean("a<!-- b -->c"), "ac");
        assert_eq!(clean("a<!-- b"), "a<!-- b");
    }

    #[test]
    fn clean_keeps_gt_inside_quoted_attributes() {
        assert_eq!(clean("<a title=\"a>b\">x</a>"), "x");
        assert_eq!(clean("<a title='a>b'>x</a>"), "x");
    }

    #[test]
    fn clean_unclosed_tag_at_end_is_removed() {
        assert_eq!(clean("abc <a href=\"x"), "abc");
    }

    #[test]
    fn clean_literal_less_than_is_text() {
        assert_eq!(clean("a < b"), "a < b");
        // Ouvrant sans `=` que rien ne ferme : littéral, la fin du texte n'est pas perdue.
        assert_eq!(
            clean("si x <y alors la peine est doublée"),
            "si x <y alors la peine est doublée"
        );
    }

    #[test]
    fn clean_numeric_c1_references_follow_windows_1252() {
        assert_eq!(clean("l&#146;art &#150; s&#156;ur v&#156;u"), "l’art – sœur vœu");
        assert_eq!(clean("&#x92;&#128;"), "’€");
        // Code non attribué : contrôle C1, retiré et compté.
        assert_eq!(clean_with_report("a&#129;b"), ("ab".to_owned(), ONE));
    }

    #[test]
    fn clean_blank_controls_become_a_space() {
        // VT, FF, NEL : une espace (les mots restent séparés), comptée comme invisible.
        assert_eq!(clean_with_report("a\u{c}b"), ("a b".to_owned(), ONE));
        assert_eq!(
            clean_with_report("a\u{b}b\u{85}c"),
            ("a b c".to_owned(), CleanReport { invisible_removed: 2 })
        );
    }

    #[test]
    fn clean_block_tags_separate_lines() {
        assert_eq!(clean("<li>a</li><li>b</li>"), "a\nb");
        assert_eq!(clean("<h1>T</h1>texte<div>x</div>"), "T\ntexte\nx");
        // Un bloc ne termine que la ligne qui a du texte : ni les blancs entre deux blocs,
        // ni un bloc après un saut de ligne n'ajoutent de ligne vide.
        assert_eq!(clean("<ul><li>a</li>\t <li>b</li></ul>"), "a\nb");
        assert_eq!(clean("<li>a<i> </i></li><li>b</li>"), "a\nb");
        assert_eq!(clean("<li>a\n</li><li>b</li>"), "a\nb");
        assert_eq!(clean("a<br></div>b"), "a\nb");
        assert_eq!(clean("a</p><div>b"), "a\n\nb");
    }

    #[test]
    fn clean_cells_become_a_space() {
        assert_eq!(clean("<td>a</td><td>b</td>"), "a b");
        assert_eq!(clean("<tr><th>a</th><th>b</th></tr><tr><td>c</td></tr>"), "a b\nc");
    }

    #[test]
    fn clean_numeric_entity_invisible_is_removed_and_counted() {
        assert_eq!(
            clean_with_report("a&#173;b&#x202E;c"),
            ("abc".to_owned(), CleanReport { invisible_removed: 2 })
        );
        assert_eq!(
            clean_with_report("&shy;x&lrm;"),
            ("x".to_owned(), CleanReport { invisible_removed: 2 })
        );
    }

    #[test]
    fn clean_invalid_numeric_entity_is_kept() {
        assert_eq!(clean("&#x110000; &#0; &#xD800;"), "&#x110000; &#0; &#xD800;");
    }

    #[test]
    fn clean_field_blank_is_none() {
        assert_eq!(clean_field(None), None);
        assert_eq!(clean_field(Some("")), None);
        assert_eq!(clean_field(Some(" <br> \u{200b}<!-- x -->")), None);
        assert_eq!(clean_field(Some(" <b>Titre</b> ")), Some("Titre".to_owned()));
    }

    #[test]
    fn inline_tags_do_not_add_space() {
        assert_eq!(clean("1<sup>er</sup> janvier"), "1er janvier");
        assert_eq!(clean("<span>a</span><font color=red>b</font><em>c</em>"), "abc");
    }

    proptest! {
        #[test]
        fn never_panics_and_is_trimmed(s in "\\PC*") {
            let out = clean(&s);
            prop_assert_eq!(out.trim(), out.as_str());
            prop_assert!(!out.contains("\n\n\n"));
        }

        // Générateur de fragments HTML : `\\PC*` ne tire presque jamais `<` ni `&`.
        // Pas d'idempotence ici (limite connue, voir `known_tag_limits`).
        #[test]
        fn html_fragments_give_trimmed_output(parts in proptest::collection::vec(
            prop_oneof!["<", ">", "&", ";", "#", "x", "br", "/p", "amp", "\n", "[a-z ]{1,3}"],
            0..40,
        )) {
            let out = clean(&parts.concat());
            prop_assert_eq!(out.trim(), out.as_str());
            prop_assert!(!out.contains("\n\n\n"));
        }

        #[test]
        fn output_has_no_decoded_controls(s in "[a-z&#x0-9;<>/ \n]{0,80}") {
            let out = clean(&s);
            prop_assert!(out.chars().all(|c| !c.is_control() || c == '\n'));
        }

        #[test]
        fn plain_text_is_unchanged(words in proptest::collection::vec("[a-zA-Zéèà0-9.,;:]{1,8}", 1..20)) {
            let text = words.join(" ");
            prop_assert_eq!(clean(&text), text);
        }

        #[test]
        fn clean_never_panics(html in hostile()) {
            let (out, report) = clean_with_report(&html);
            prop_assert_eq!(clean(&html), out.clone());
            prop_assert_eq!(out.trim(), out.as_str());
            prop_assert!(!out.contains("\n\n\n"));
            // Chaque caractère retiré vient d'un caractère de la source ou d'une entité.
            let chars = html.chars().count();
            prop_assert!(usize::try_from(report.invisible_removed).is_ok_and(|n| n <= chars));
        }

        #[test]
        fn clean_output_has_no_invisible_chars(html in hostile()) {
            // `\t` et `\r` sont repliés : seul `\n` reste parmi les contrôles.
            let out = clean(&html);
            prop_assert!(out.chars().all(|c| !spec_invisible(c) && c != '\t' && c != '\r'));
        }
    }
}
