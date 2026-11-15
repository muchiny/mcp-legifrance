//! Lecture des textes du BOFiP (ADR-0029, décision 4 ; ADR-0030).
//!
//! Le jeu `bofip-vigueur` sert le texte d'une publication sous deux formes. [`publication_text`]
//! lit le champ HTML (`contenu_html`) et garde le champ brut (`contenu`) en secours.
//!
//! # Le champ HTML (ADR-0030)
//!
//! Depuis 2024, le champ brut n'a plus aucun saut de ligne : 1 737 publications y tiennent en un
//! seul paragraphe, numéros de paragraphe et cellules de tableau collés au texte (mesure du
//! 2026-11-15 sur les 9 173 publications). Le champ HTML garde les blocs. Il est lu par le
//! lecteur HTML du domaine ([`LongText::from_html`], 03 §7.2), sans changement de sa grammaire,
//! après deux réparations propres au portail (`prepared`) :
//!
//! - **guillemets** : le portail double chaque guillemet double du champ (`class=""x""`,
//!   1 704 693 suites de deux et 63 de quatre, aucune suite impaire). Si toutes les suites sont
//!   paires, chacune est réduite de moitié ; sinon le champ est lu tel quel ;
//! - **restes de document** : trois publications portent un document entier, coupé aux deux
//!   bouts. Le champ commence par un reste d'en-tête (`YPE html PUBLIC …>`) et finit par le
//!   `<` d'une balise coupée (`</head><`), qui sortiraient en texte. Si la première balise du
//!   champ est une balise ouvrante `html`, ce qui la précède est écarté, ainsi que le `<` qui
//!   finit le champ. Aucune autre publication du jeu n'a de balise `html`, et aucune autre ne
//!   finit par un `<`.
//!
//! La lecture du champ HTML n'est servie que si elle est complète face à celle du champ brut
//! (`is_complete` : au moins neuf dixièmes de ses lettres et de ses chiffres) ; sinon le champ
//! brut est servi et le repli est signalé à l'appelant.
//!
//! # Le champ brut (ADR-0029, AM-95)
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
//! l'appelant, qui peut le journaliser. Le lecteur HTML ne sert pas pour ce champ : un `<` y est
//! un chevron.

use std::borrow::Cow;

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

/// Texte d'une publication, et ce que sa lecture a rencontré ([`publication_text`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationText {
    /// Texte canonique.
    pub text: LongText,
    /// Entités inconnues laissées dans le texte, quand il vient du champ brut ; zéro quand il
    /// vient du champ HTML, dont le lecteur ne les compte pas.
    pub unknown_entities: u32,
    /// Vrai si la garde a écarté la lecture du champ HTML : le texte vient du champ brut alors
    /// que le champ HTML était là.
    pub fell_back: bool,
}

/// Texte canonique d'une publication, lu dans son champ HTML (`contenu_html`), avec son champ
/// brut (`contenu`) en secours (ADR-0030).
///
/// 1. `html` absent ou blanc : le champ brut, lu par [`clean_text`].
/// 2. Sinon `html` est préparé (guillemets doublés par le portail réduits, restes de document
///    écartés) puis lu par [`LongText::from_html`] : un paragraphe par bloc, une ligne de tableau
///    par paragraphe (cellules jointes par ` | `), aucune balise rendue.
/// 3. Garde : si ce texte compte moins de 90 % des lettres et des chiffres de la lecture du
///    champ brut, le champ HTML est tenu pour incomplet. Le champ brut est servi et `fell_back`
///    est vrai. Un champ brut absent ou blanc ne retient jamais le champ HTML.
///
/// ```
/// use lf_domain::doctrine::publication_text;
///
/// let html = r#"<p class=""numero"">10</p><p>Le seuil est de 1,3&nbsp;M€.</p>"#;
/// let read = publication_text(Some(html), Some("10Le seuil est de 1,3 M€."))?;
/// assert_eq!(read.text.as_str(), "10\n\nLe seuil est de 1,3 M€.");
/// assert!(!read.fell_back);
///
/// // Un champ HTML qui a perdu le texte : le champ brut est servi.
/// let read = publication_text(Some("<p>10</p>"), Some("10\nLe seuil est de 1,3 M€."))?;
/// assert_eq!(read.text.as_str(), "10\n\nLe seuil est de 1,3 M€.");
/// assert!(read.fell_back);
/// # Ok::<(), lf_domain::DomainError>(())
/// ```
///
/// # Errors
///
/// [`DomainError::TextTooLarge`] si la lecture de l'un des deux champs dépasse
/// [`LongText::MAX_CHARS`] caractères.
pub fn publication_text(
    html: Option<&str>,
    plain: Option<&str>,
) -> Result<PublicationText, DomainError> {
    let (plain, unknown_entities) = clean_text(plain.unwrap_or_default())?;
    let Some(html) = html.filter(|field| !field.trim().is_empty()) else {
        return Ok(PublicationText { text: plain, unknown_entities, fell_back: false });
    };
    let html = LongText::from_html(&prepared(html))?;
    Ok(if is_complete(&html, &plain) {
        PublicationText { text: html, unknown_entities: 0, fell_back: false }
    } else {
        PublicationText { text: plain, unknown_entities, fell_back: true }
    })
}

/// Champ HTML du portail prêt pour le lecteur du domaine : sans ses restes de document
/// ([`without_wrapper`]), et ses suites de guillemets doubles réduites de moitié si toutes sont
/// paires ([`quote_runs_are_even`]). Avec une seule suite impaire, le portail a cessé de
/// doubler : les guillemets sont lus tels quels.
///
/// La parité se juge sur le champ entier, comme la mesure : un reste d'en-tête aux guillemets
/// simples dit, lui aussi, que le portail a cessé de doubler. Les deux coupes de
/// [`without_wrapper`] tombent sur un `<` : elles ne coupent aucune suite.
fn prepared(field: &str) -> Cow<'_, str> {
    let body = without_wrapper(field);
    if quote_runs_are_even(field) {
        Cow::Owned(body.replace("\"\"", "\""))
    } else {
        Cow::Borrowed(body)
    }
}

/// Vrai si chaque suite de guillemets doubles de `field` a une longueur paire ; vrai aussi sans
/// aucun guillemet.
fn quote_runs_are_even(field: &str) -> bool {
    field.split(|c| c != '"').all(|run| run.len().is_multiple_of(2))
}

/// `field` sans les restes d'un document entier. Si sa première balise est une balise ouvrante
/// `html` ([`opens_html`]), il commence à cette balise, et le `<` qui finit le champ, début
/// d'une balise coupée, est écarté. Rien d'autre ne l'est : un `<` suivi de texte reste un
/// chevron, que le lecteur rend. Sinon le champ est rendu entier : une balise `html` placée
/// plus loin (dans un commentaire, après une autre balise) ne coupe rien.
fn without_wrapper(field: &str) -> &str {
    let document = field.find('<').and_then(|at| field.get(at..)).filter(|tail| opens_html(tail));
    document.map_or(field, |document| document.strip_suffix('<').unwrap_or(document))
}

/// Vrai si `tail` commence par une balise ouvrante `html` : `<html`, casse indifférente, suivi
/// de `>`, de `/` ou d'un blanc ASCII. `<htmlx>` et `</html>` n'en sont pas.
fn opens_html(tail: &str) -> bool {
    let bytes = tail.as_bytes();
    bytes.get(..5).is_some_and(|open| open.eq_ignore_ascii_case(b"<html"))
        && bytes.get(5).is_some_and(|end| matches!(end, b'>' | b'/') || end.is_ascii_whitespace())
}

/// La lecture du champ HTML est-elle assez complète face à celle du champ brut ? Elle l'est si
/// elle porte au moins 90 % de ses lettres et de ses chiffres (9 contre 10 la garde). Une
/// lecture vide du champ brut ne retient rien.
///
/// C'est la garde de 03 §7.4, celle des champs `texteHtml` et `texte` de Légifrance, à une
/// différence près : seuls les lettres et les chiffres comptent. Les deux lectures n'ont pas la
/// même mise en forme (le champ HTML sépare les cellules d'un tableau par ` | `, le champ brut
/// les colle) : avec tous les caractères comptés, les séparateurs d'un tableau cacheraient la
/// perte d'une phrase.
///
/// Mesure du 2026-11-15 sur les 9 173 publications : le rapport va de 0,987 à 1,126.
fn is_complete(html: &LongText, plain: &LongText) -> bool {
    letters_and_digits(html).saturating_mul(10) >= letters_and_digits(plain).saturating_mul(9)
}

/// Nombre de lettres et de chiffres de `text`.
fn letters_and_digits(text: &LongText) -> usize {
    text.as_str().chars().filter(|c| c.is_alphanumeric()).count()
}

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

    // ---- lecture du champ HTML (ADR-0030) ------------------------------------------------------

    /// Texte, entités inconnues comptées et repli de la lecture d'une publication.
    fn read(html: Option<&str>, plain: Option<&str>) -> (String, u32, bool) {
        let read = publication_text(html, plain).unwrap();
        (read.text.as_str().to_owned(), read.unknown_entities, read.fell_back)
    }

    fn served(text: &str, unknown: u32, fell_back: bool) -> (String, u32, bool) {
        (text.to_owned(), unknown, fell_back)
    }

    /// Extrait de `BOI-TVA-CHAMP-30-10-20-50` (2024), tel que le portail le sert : dans le champ
    /// brut, tout est collé ; dans le champ HTML, chaque guillemet est doublé et le numéro de
    /// paragraphe a son bloc.
    #[test]
    fn a_publication_is_read_from_its_html_field() {
        let html = r#"<h1 id=""Activites_denseignement_10"">I. Activités d'enseignement</h1><p class=""numero-de-paragraphe-western"" id=""10_01"">10</p><p class=""paragraphe-western"" id=""Les_etablissements_02"">Les établissements d'enseignement&nbsp;&amp; de formation.</p>"#;
        let plain =
            "I. Activités d'enseignement10Les établissements d'enseignement &amp; de formation.";
        assert_eq!(
            read(Some(html), Some(plain)),
            served(
                "I. Activités d'enseignement\n\n10\n\nLes établissements d'enseignement & de \
                 formation.",
                0,
                false
            )
        );
        // Le champ brut seul donne un paragraphe : c'est le défaut corrigé.
        let (plain_text, _) = clean_text(plain).unwrap();
        assert_eq!(plain_text.paragraphs().count(), 1);
    }

    #[test]
    fn a_table_row_is_one_paragraph_and_no_tag_is_rendered() {
        let html = r#"<table><tbody><tr><th>Fraction</th><th>Tarif</th></tr><tr><td><p class=""paragraphe-western"">N'excédant pas 800&nbsp;000&nbsp;€</p></td><td>0 %</td></tr></tbody></table><p>Fin <strong>du</strong> <a href=""https://exemple.invalid/x"">barème</a>.</p>"#;
        let (text, unknown, fell_back) = read(Some(html), Some("FractionTarifN'excédant pas"));
        assert_eq!(text, "Fraction | Tarif\n\nN'excédant pas 800 000 € | 0 %\n\nFin du barème.");
        assert_eq!((unknown, fell_back), (0, false));
        assert!(!text.contains('<') && !text.contains("href") && !text.contains("exemple"));
    }

    /// Le champ HTML porte la date là où le champ brut garde un jeton du portail. La date a
    /// moins de lettres et de chiffres que le jeton (8 contre 16) : sur un texte de quelques
    /// mots, la garde prend cet écart pour une perte et sert le champ brut. C'est une limite
    /// connue (ADR-0030, « À surveiller »), que la fin de ce test fixe ; aucune publication du
    /// jeu n'est aussi courte (plus petit rapport mesuré le 2026-11-15 : 0,987).
    #[test]
    fn the_html_field_carries_the_date_the_plain_field_lacks() {
        let html = "<p>Actualité liée : 03/07/2024 : TVA - Champ d'application et territorialité \
                    - Exonérations - Opérations exonérées en régime intérieur</p>";
        let plain = "Actualité liée : [node:date:14246-PGP] : TVA - Champ d'application et \
                     territorialité - Exonérations - Opérations exonérées en régime intérieur";
        let (text, unknown, fell_back) = read(Some(html), Some(plain));
        assert!(text.starts_with("Actualité liée : 03/07/2024 : TVA - Champ"), "{text}");
        assert_eq!((text.contains("[node:"), unknown, fell_back), (false, 0, false));
        let short = read(Some("<p>Voir : 03/07/2024</p>"), Some("Voir : [node:date:14246-PGP]"));
        assert_eq!(short, served("Voir : [node:date:14246-PGP]", 0, true));
    }

    /// Les entités inconnues du champ brut ne sont comptées que si c'est lui qui est servi :
    /// le champ HTML les décode, il n'en reste aucune dans le texte.
    #[test]
    fn unknown_entities_of_the_plain_field_are_not_counted_when_the_html_is_served() {
        let html = "<p>Le seuil prévu par cet article est relev&eacute; chaque année, comme le \
                    barème de l'impôt sur le revenu.</p>";
        let plain = "Le seuil prévu par cet article est relev&eacute; chaque année, comme le \
                     barème de l'impôt sur le revenu.";
        let text = "Le seuil prévu par cet article est relevé chaque année, comme le barème de \
                    l'impôt sur le revenu.";
        assert_eq!(read(Some(html), Some(plain)), served(text, 0, false));
        assert_eq!(read(None, Some(plain)), served(plain, 1, false));
        // Le même champ brut, servi parce que le champ HTML a perdu son texte : compté.
        assert_eq!(read(Some("<p>Le seuil.</p>"), Some(plain)), served(plain, 1, true));
    }

    #[test]
    fn without_an_html_field_the_plain_field_is_read() {
        let plain = "10\nLe seuil est prévu\nà l'article 964.&#13;\n&eacute;";
        let expected = served("10\n\nLe seuil est prévu à l'article 964.\n\n&eacute;", 1, false);
        for html in [None, Some(""), Some(" \n\t\u{a0}")] {
            assert_eq!(read(html, Some(plain)), expected, "{html:?}");
        }
        // Aucun des deux : un texte vide, rien de compté.
        for (html, plain) in [(None, None), (Some(" "), Some("")), (None, Some("&#13;\n"))] {
            assert_eq!(read(html, plain), served("", 0, false), "{html:?} {plain:?}");
        }
    }

    #[test]
    fn without_a_plain_field_the_html_field_is_read_unguarded() {
        for plain in [None, Some(""), Some(" \n&#13;\n")] {
            assert_eq!(
                read(Some("<p>Un.</p><p>Deux.</p>"), plain),
                served("Un.\n\nDeux.", 0, false)
            );
        }
        // Un champ HTML sans texte et un champ brut vide : un texte vide, sans repli.
        assert_eq!(read(Some("<p></p><br>"), None), served("", 0, false));
    }

    /// Un champ HTML qui a perdu plus du dixième du texte est écarté : le champ brut est servi,
    /// avec son compte d'entités, et le repli est signalé.
    #[test]
    fn an_incomplete_html_field_falls_back_to_the_plain_field() {
        let plain = "Le seuil est prévu\nà l'article 964.\n&eacute;";
        let expected = served("Le seuil est prévu à l'article 964.\n\n&eacute;", 1, true);
        for html in ["<p>Le seuil", "<p></p>", "<br>", "<p>Le seuil est prévu à l'article 96"] {
            assert_eq!(read(Some(html), Some(plain)), expected, "{html}");
        }
        // Une zone `script` fermée qui avale la suite : son contenu n'est jamais rendu, la
        // garde voit la perte.
        let swallowed = "<p>Le seuil</p><script>est prévu à l'article 964. &eacute;</script>";
        assert_eq!(LongText::from_html(swallowed).unwrap().as_str(), "Le seuil");
        assert_eq!(read(Some(swallowed), Some(plain)), expected);
    }

    /// La garde compte les lettres et les chiffres : 9 contre 10 la passe, 8 contre 10
    /// l'arrête. Ni les séparateurs de paragraphes ou de cellules, ni la ponctuation, ni les
    /// espaces ne comptent, d'un côté comme de l'autre.
    #[test]
    fn the_guard_passes_at_ninety_percent_of_the_letters_and_digits() {
        let text = |html: &str| LongText::from_html(html).unwrap();
        let ten = text("abcdefghij");
        assert!(is_complete(&text("abcdefghi"), &ten));
        assert!(!is_complete(&text("abcdefgh"), &ten));
        assert!(is_complete(&ten, &ten) && is_complete(&text("abcdefghijk"), &ten));
        // Huit lettres en huit cellules : 29 caractères, dont 8 comptent.
        let cells = text(
            "<table><tr><td>a</td><td>b</td><td>c</td><td>d</td><td>e</td><td>f</td><td>g</td>\
             <td>h</td></tr></table>",
        );
        assert_eq!(cells.as_str(), "a | b | c | d | e | f | g | h");
        assert_eq!((cells.char_len(), letters_and_digits(&cells)), (29, 8));
        assert!(!is_complete(&cells, &ten));
        // Neuf lettres et chiffres en neuf paragraphes, face à dix d'un seul tenant.
        let nine = text("<p>a<p>b<p>c<p>1<p>2<p>é<p>É<p>ß<p>9");
        assert_eq!((nine.char_len(), letters_and_digits(&nine)), (25, 9));
        assert!(is_complete(&nine, &ten) && is_complete(&ten, &nine));
        // Ponctuation, espaces et symboles : rien à compter.
        assert_eq!(letters_and_digits(&text("« … » - % € | ; ! ? ( )")), 0);
        // Un texte vide : rien à retenir d'un côté, tout de l'autre.
        let empty = LongText::default();
        assert_eq!((letters_and_digits(&empty), letters_and_digits(&text("a"))), (0, 1));
        assert!(is_complete(&empty, &empty) && is_complete(&cells, &empty));
        assert!(!is_complete(&empty, &text("a")));
    }

    /// Un tableau ne cache pas la perte d'une phrase : ses séparateurs de cellule ne comptent
    /// pas. Avec tous les caractères comptés, cette lecture passerait (797 contre 253).
    #[test]
    fn a_table_does_not_hide_a_lost_sentence() {
        let sentence = "Ces montants ne s'appliquent pas aux couples mariés.";
        let table = format!("<table><tr>{}</tr></table>", "<td>1</td>".repeat(200));
        let plain = format!("{}\n{sentence}", "1".repeat(200));
        let read = publication_text(Some(&table), Some(&plain)).unwrap();
        assert!(read.fell_back && read.text.as_str().ends_with(sentence));
        assert_eq!(LongText::from_html(&table).unwrap().char_len(), 797);
        // Le même tableau, suivi de sa phrase : le champ HTML est servi.
        let whole = format!("{table}<p>{sentence}</p>");
        let read = publication_text(Some(&whole), Some(&plain)).unwrap();
        assert!(!read.fell_back && read.text.as_str().ends_with(sentence));
        assert_eq!(read.text.paragraphs().count(), 2);
    }

    #[test]
    fn doubled_quotes_are_halved_when_every_run_is_even() {
        for (field, expected) in [
            (r#"<p class=""a"">x</p>"#, r#"<p class="a">x</p>"#),
            (r#"<p title="""">x</p>"#, r#"<p title="">x</p>"#),
            (r#"a """"""b"" c"#, r#"a """b" c"#),
            ("sans guillemet", "sans guillemet"),
            ("", ""),
            ("\"\"", "\""),
        ] {
            assert_eq!(prepared(field), expected, "{field}");
            assert!(quote_runs_are_even(field), "{field}");
        }
    }

    /// Une seule suite impaire : le portail a cessé de doubler, rien n'est réduit.
    #[test]
    fn quotes_are_left_alone_when_one_run_is_odd() {
        for field in [
            r#"<p class="a">x</p>"#,
            r#"<p class=""a"" id="b">x</p>"#,
            r#"a """ b"#,
            "\"",
            r#""" """ """#,
        ] {
            assert_eq!(prepared(field), field, "{field}");
            assert!(!quote_runs_are_even(field), "{field}");
        }
        // Avec ses guillemets simples, l'attribut reste lu comme un attribut.
        assert_eq!(read(Some(r#"<p class="a" id="b">x</p>"#), None), served("x", 0, false));
    }

    /// Sans la réduction, le lecteur prendrait la suite du champ pour la valeur d'un attribut.
    #[test]
    fn halving_the_quotes_keeps_the_text_after_an_attribute() {
        let html = r#"<p class="""">Un.</p><p id=""a"">Deux.</p>"#;
        assert_eq!(read(Some(html), None), served("Un.\n\nDeux.", 0, false));
    }

    /// Début et fin du champ de `ACTU-2020-00272`, tels que le portail les sert : le champ
    /// commence au milieu d'une déclaration de type de document et finit par le `<` d'une balise
    /// coupée.
    #[test]
    fn the_remnants_of_a_whole_document_are_dropped() {
        let field = r#"YPE html PUBLIC ""-//W3C//DTD HTML 4.0 Transitional//EN"" ""http://www.w3.org/TR/REC-html40/loose.dtd""> <html><head><meta><article><p class=""paragraphe-western"">Texte.</p> </article> </head><"#;
        assert_eq!(
            prepared(field),
            r#"<html><head><meta><article><p class="paragraphe-western">Texte.</p> </article> </head>"#
        );
        assert_eq!(read(Some(field), Some("Texte.")), served("Texte.", 0, false));
        for (field, expected) in [
            ("avant<html>après", "<html>après"),
            ("avant<HTML lang=fr>après", "<HTML lang=fr>après"),
            ("avant<hTmL\n>après", "<hTmL\n>après"),
            ("avant<html/>après", "<html/>après"),
            ("a<html>d<html>e<b>", "<html>d<html>e<b>"),
            ("<html>", "<html>"),
            ("é<html>", "<html>"),
            // Le `<` qui finit le champ est écarté, et lui seul : ni un second, ni un `<` que
            // suit du texte ou un blanc, ni une balise finie.
            ("avant<html><p>a</p><", "<html><p>a</p>"),
            ("avant<html><", "<html>"),
            ("avant<html><p>a</p><<", "<html><p>a</p><"),
            ("avant<html><p>a</p>< ", "<html><p>a</p>< "),
            ("avant<html><p>a</p>x < y", "<html><p>a</p>x < y"),
            ("avant<html><p>a</p></hea", "<html><p>a</p></hea"),
            ("avant<html><p>a</p></head>", "<html><p>a</p></head>"),
            ("avant<html><p>a</p>b > c", "<html><p>a</p>b > c"),
            ("avant<html a", "<html a"),
        ] {
            assert_eq!(without_wrapper(field), expected, "{field}");
        }
        // Un chevron suivi de texte, dans un document entier, reste du texte.
        assert_eq!(read(Some("<html><p>Si R < P</p>"), None), served("Si R < P", 0, false));
    }

    /// Le reste d'en-tête vote aussi : si ses guillemets sont simples, le portail a cessé de
    /// doubler, et ceux du corps sont lus tels quels, même tous pairs.
    #[test]
    fn the_dropped_header_still_tells_whether_quotes_are_doubled() {
        let single = r#"YPE html PUBLIC "x"> <html><p title="">Texte.</p>"#;
        assert_eq!(prepared(single), r#"<html><p title="">Texte.</p>"#);
        let doubled = r#"YPE html PUBLIC ""x""> <html><p title="""">Texte.</p>"#;
        assert_eq!(prepared(doubled), r#"<html><p title="">Texte.</p>"#);
    }

    /// Si la première balise du champ n'est pas une balise ouvrante `html`, rien n'est écarté :
    /// ni début, ni chevron final, ni ce qui précède une balise `html` placée plus loin.
    #[test]
    fn nothing_is_dropped_without_an_html_tag() {
        for field in [
            "",
            "texte",
            "YPE html PUBLIC> <p>Texte.</p>",
            "a<htmlx>b",
            "a</html>b",
            "a<htm>b",
            "a<html",
            "a<abcd>b",
            "a<p>b< html>c",
            "a&lt;html&gt;b",
            "<",
            "é<é>",
            "<p>a</p><",
            "<p>R < P",
            "a<b>c<html>d",
            "<p>a</p><html>b<",
            "a < b<html>c",
            "<!-- <html> --><p>a</p>",
        ] {
            assert_eq!(without_wrapper(field), field, "{field}");
        }
        // Une balise `html` dans un commentaire, après le début du texte : rien n'est perdu.
        let commented = "<p>Ce régime est abrogé.</p><!-- <html> --><p>La suite s'applique.</p>";
        let expected = served("Ce régime est abrogé.\n\nLa suite s'applique.", 0, false);
        assert_eq!(read(Some(commented), None), expected);
        assert!(opens_html("<html>") && opens_html("<html\t") && opens_html("<HtMl/"));
        assert!(!opens_html("<html") && !opens_html("<htmlx>") && !opens_html("<abcd>"));
        assert!(!opens_html("") && !opens_html("<") && !opens_html("xhtml>"));
        assert_eq!(read(Some("<p>R < P"), None), served("R < P", 0, false));
    }

    #[test]
    fn invisible_characters_are_removed_from_the_html_reading() {
        let read = publication_text(Some("<p>plus\u{200b}-value&#x202E;</p>"), None).unwrap();
        assert_eq!((read.text.as_str(), read.text.invisible_removed()), ("plus-value", 2));
    }

    #[test]
    fn a_field_over_the_limit_is_refused() {
        let huge = "é".repeat(16_000_001);
        let too_large = Err(DomainError::TextTooLarge { chars: 16_000_001 });
        assert_eq!(publication_text(Some(&huge), Some("x")), too_large);
        assert_eq!(publication_text(Some("<p>x</p>"), Some(&huge)), too_large);
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

        /// Doubler chaque guillemet puis préparer le champ rend l'entrée : la réduction défait
        /// exactement ce que le portail fait.
        #[test]
        fn doubling_then_halving_gives_the_input_back(raw in "(\\PC|[\"\n<>=]){0,80}") {
            prop_assume!(without_wrapper(&raw) == raw);
            let doubled = raw.replace('"', "\"\"");
            prop_assert!(quote_runs_are_even(&doubled));
            let halved = prepared(&doubled);
            prop_assert_eq!(halved.as_ref(), raw.as_str());
        }

        /// La préparation ne rallonge jamais le champ et n'y ajoute rien : ce qui reste est un
        /// morceau du champ, à ses guillemets près.
        #[test]
        fn preparing_never_grows_the_field(raw in "(\\PC|[\"\n<>]|<html>){0,80}") {
            let out = prepared(&raw);
            prop_assert!(out.len() <= raw.len());
            let body = without_wrapper(&raw);
            prop_assert!(raw.contains(body));
            prop_assert_eq!(out.replace('"', ""), body.replace('"', ""));
        }

        /// Le texte servi est toujours l'une des deux lectures, et celle du champ HTML n'est
        /// servie que si elle tient la garde face à celle du champ brut.
        #[test]
        fn the_served_text_is_one_of_the_two_readings(
            html in "(\\PC|[\"\n]|<p>|</p>|<td>|<tr>|<script>|&amp;){0,60}",
            plain in "(\\PC|[\n&]){0,60}"
        ) {
            let read = publication_text(Some(&html), Some(&plain)).unwrap();
            let (plain_text, unknown) = clean_text(&plain).unwrap();
            if read.fell_back {
                prop_assert_eq!((&read.text, read.unknown_entities), (&plain_text, unknown));
                prop_assert!(!html.trim().is_empty());
            } else if html.trim().is_empty() {
                prop_assert_eq!((&read.text, read.unknown_entities), (&plain_text, unknown));
            } else {
                let html_text = LongText::from_html(&prepared(&html)).unwrap();
                prop_assert_eq!((&read.text, read.unknown_entities), (&html_text, 0));
                prop_assert!(is_complete(&read.text, &plain_text));
            }
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
