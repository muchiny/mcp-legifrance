//! HTML Légifrance vers texte brut.
//!
//! Règle nouvelle, absente de la référence (qui ne retire que `<mark>`,
//! citations.py:45-52). Balises relevées dans les fixtures de la référence
//! (`tests/fixtures/`) : `<br>` (`search_juri.json`, `search_cetat.json`,
//! `consult_juri.json`, `consult_juri_cetat.json`, `consult_juri_constit.json`),
//! `<BR>` (`consult_juri_constit.json`), `<mark>` (`search_*.json`), `<p>`
//! (`consult_cnil.json`), `<RECH>` (`consult_juri_constit.json`,
//! `consult_juri_cetat.json`), `<a …>` (`consult_kali.json`). Aucune entité HTML
//! (`&amp;`, `&#`, `&nbsp;`) dans aucune fixture ; les entités XML de base et
//! numériques sont tout de même décodées (texte HTML).

/// Nettoie un fragment HTML : balises retirées, `<br>` → saut de ligne,
/// `<p>` → paragraphe, entités décodées, espaces normalisés.
pub fn clean(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(pos) = rest.find(['<', '&']) {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        let consumed = if tail.starts_with('<') {
            tag(tail).map(|(name, closing, len)| {
                out.push_str(tag_text(&name, closing));
                len
            })
        } else {
            entity(tail).map(|(ch, len)| {
                out.push(ch);
                len
            })
        };
        let len = consumed.unwrap_or_else(|| {
            out.push_str(&tail[..1]);
            1
        });
        // Au moins un octet consommé par tour : la boucle termine toujours.
        rest = &tail[len.max(1)..];
    }
    out.push_str(rest);
    normalize_whitespace(&out)
}

/// Balise en tête de `tail`.
///
/// Limites connues, figées par `known_tag_limits` : une seule passe, donc
/// `clean` n'est pas idempotent (`<<b>b>` donne `<b>`) ; un `>` dans un
/// attribut cité coupe la balise (`<a title="a>b">` s'arrête au `>` de
/// l'attribut) ; les commentaires `<!-- … -->` restent littéraux (un nom de
/// balise commence par une lettre). Les blocs (`li`, `div`, `td`…) sont
/// effacés sans séparateur (hors périmètre).
///
/// Résultat : (nom en minuscules, fermante, longueur avec `>`).
fn tag(tail: &str) -> Option<(String, bool, usize)> {
    // Premier `<` ou `>` après l'ouvrant : un `<` d'abord, et l'ouvrant est littéral.
    // Un seul balayage, donc linéaire même sur « <<<<… ».
    let end = 1 + tail[1..].find(['<', '>'])?;
    if !tail[end..].starts_with('>') {
        return None;
    }
    let inner = &tail[1..end];
    let (closing, body) = inner.strip_prefix('/').map_or((false, inner), |b| (true, b));
    let name: String = body
        .chars()
        .take_while(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let starts_with_letter = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
    starts_with_letter.then_some((name, closing, end + 1))
}

/// Texte de remplacement d'une balise reconnue : `<br>` donne un saut de ligne,
/// `<p>` et `</p>` un paragraphe, toute autre balise (ouvrante ou fermante,
/// `</br>` compris) est effacée sans séparateur.
fn tag_text(name: &str, closing: bool) -> &'static str {
    match (name, closing) {
        ("br", false) => "\n",
        ("p", _) => "\n\n",
        _ => "",
    }
}

/// Entité en tête de `tail` : (caractère, longueur avec `;`).
fn entity(tail: &str) -> Option<(char, usize)> {
    // Fenêtre bornée en octets : `;` est ASCII, donc `end` est une frontière de caractère.
    let end = tail.bytes().take(12).position(|b| b == b';')?;
    let name = &tail[1..end];
    let ch = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        _ => numeric(name)?,
    };
    Some((ch, end + 1))
}

/// Entité numérique `#…` (décimale) ou `#x…` (hexadécimale), sans signe.
///
/// Refuse les contrôles (hors tabulation, saut de ligne, retour chariot), les
/// contrôles bidi et les tags Unicode : le texte décodé est destiné à un
/// modèle, et une entité ne doit pas y fabriquer ce que la source n'a pas.
fn numeric(name: &str) -> Option<char> {
    let code = if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
        let digits = hex.bytes().all(|b| b.is_ascii_hexdigit()).then_some(hex)?;
        u32::from_str_radix(digits, 16).ok()?
    } else {
        let digits = name.strip_prefix('#')?;
        let digits = digits.bytes().all(|b| b.is_ascii_digit()).then_some(digits)?;
        digits.parse().ok()?
    };
    let c = char::from_u32(code)?;
    let control = c.is_control() && !matches!(c, '\t' | '\n' | '\r');
    let bidi = matches!(c, '\u{61c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
    let tag = matches!(c, '\u{e0000}'..='\u{e007f}');
    (!control && !bidi && !tag).then_some(c)
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
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    #[test]
    fn strips_search_marks() {
        assert_eq!(
            clean("Loi pour une sécurité globale préservant les <mark>libertés</mark>  - réserve"),
            "Loi pour une sécurité globale préservant les libertés - réserve"
        );
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
    }

    #[test]
    fn keeps_unknown_or_broken_entities() {
        assert_eq!(
            clean("&unknown; &#xZZ; &#; & seul &#1114112;"),
            "&unknown; &#xZZ; &#; & seul &#1114112;"
        );
        assert_eq!(clean("&verylongentityname;"), "&verylongentityname;");
        // Octet 12 au milieu d'un caractère multi-octet : texte conservé, sans panique.
        assert_eq!(clean("&ammmmmmmmmmé;"), "&ammmmmmmmmmé;");
        assert_eq!(clean("&amp;ééééééééé; &lt;"), "&ééééééééé; <");
    }

    #[test]
    fn keeps_control_and_signed_numeric_refs_literal() {
        // Une assertion par classe : contrôle C0, ESC en hexadécimal, contrôle bidi
        // (RLO), tag Unicode, puis signes `+` acceptés à tort par `parse`.
        assert_eq!(clean("a&#0;b"), "a&#0;b");
        assert_eq!(clean("a&#x1b;b"), "a&#x1b;b");
        assert_eq!(clean("a&#8238;b"), "a&#8238;b");
        assert_eq!(clean("a&#xE0041;b"), "a&#xE0041;b");
        assert_eq!(clean("a&#+65;b"), "a&#+65;b");
        assert_eq!(clean("a&#x+41;b"), "a&#x+41;b");
        // Exceptions : tabulation, saut de ligne et retour chariot sont décodés.
        assert_eq!(clean("a&#10;b"), "a\nb");
        assert_eq!(clean("a&#9;b&#13;c"), "a b c");
    }

    #[test]
    fn keeps_every_bidi_and_c1_control_ref_literal() {
        for code in [0x7f_u32, 0x85, 0x9f, 0x61c, 0x200e, 0x200f, 0x202a, 0x202e, 0x2066, 0x2069] {
            let reference = format!("&#{code};");
            assert_eq!(clean(&reference), reference);
        }
        for code in [0xE0000_u32, 0xE007F] {
            let reference = format!("&#x{code:X};");
            assert_eq!(clean(&reference), reference);
        }
        // Bornes voisines : décodées.
        for (code, decoded) in
            [(0x7e_u32, '~'), (0xa0, ' '), (0x61d, '\u{61d}'), (0x2065, '\u{2065}'), (0x202f, ' ')]
        {
            assert_eq!(clean(&format!("a&#{code};b")), format!("a{decoded}b"));
        }
        assert_eq!(clean("&#xDFFFF;"), "\u{dffff}");
        assert_eq!(clean("&#xE0080;"), "\u{e0080}");
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
        // Un `>` dans un attribut cité coupe la balise.
        assert_eq!(clean("<a title=\"a>b\">t</a>"), "b\">t");
        // Les commentaires restent littéraux.
        assert_eq!(clean("a<!-- c -->b"), "a<!-- c -->b");
    }

    #[test]
    fn normalizes_whitespace() {
        assert_eq!(clean("  a \t b\r\n\n\n\n  c  "), "a b\n\nc");
        assert_eq!(clean("\u{a0}a\u{202f}:"), "a :");
        assert_eq!(clean(""), "");
        assert_eq!(clean("<br><p>"), "");
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
    }
}
