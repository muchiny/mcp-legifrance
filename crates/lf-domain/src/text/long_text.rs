//! Texte long structuré en paragraphes (03 §7.5).
//!
//! Le texte canonique est défini **après** `clean` (03 §7.3) : paragraphes non vides, sans saut
//! de ligne ni blanc de bord, joints par une ligne vide. Fenêtres, passages et classement le
//! consomment tels quels.

use std::{fmt, sync::Arc};

use super::{clean_with_report, html};
use crate::error::DomainError;

/// Séparateur des paragraphes dans le texte canonique.
const SEPARATOR: &str = "\n\n";

/// Nombre de caractères du séparateur de paragraphes.
const SEPARATOR_CHARS: u64 = 2;

/// Texte long : paragraphes joints par `"\n\n"` (forme exposée au modèle et base des offsets),
/// et table des paragraphes. Le texte et la table sont partagés par `Arc` : cloner une entité
/// servie par le cache ne copie pas le texte, qui peut atteindre [`LongText::MAX_CHARS`].
///
/// `Debug` n'écrit jamais le texte, seulement sa longueur.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct LongText {
    /// Paragraphes joints par `"\n\n"`.
    canonical: Arc<str>,
    /// Une entrée par paragraphe.
    paras: Arc<[Para]>,
    /// Longueur de `canonical` en caractères Unicode (scalaires).
    char_len: u32,
    /// Invisibles et bidirectionnels retirés à la construction (diagnostic, 00 §4).
    invisible_removed: u32,
}

/// Position d'un paragraphe dans le texte canonique.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Para {
    /// Décalage en octets de son premier octet.
    byte_start: u32,
    /// Position de son premier caractère, en caractères Unicode.
    char_start: u32,
    /// Longueur en caractères.
    char_len: u32,
}

impl LongText {
    /// Plus grand nombre de caractères d'un texte canonique, séparateurs compris. Au-delà, la
    /// construction échoue (`DomainError::TextTooLarge`).
    pub const MAX_CHARS: u32 = 16_000_000;

    /// Texte canonique d'un fragment HTML : un paragraphe par bloc (03 §7.2).
    ///
    /// # Errors
    ///
    /// `DomainError::TextTooLarge` si le texte dépasse [`LongText::MAX_CHARS`] caractères.
    pub fn from_html(html: &str) -> Result<Self, DomainError> {
        let (paragraphs, invisible_removed) = html::paragraphs(html);
        Self::assemble(&paragraphs, invisible_removed)
    }

    /// Texte canonique d'un texte brut : coupure sur les suites de sauts de ligne (`\r\n`
    /// normalisé en `\n`), chaque morceau passant par `clean`. Les morceaux vides sont
    /// supprimés. Le `texte` de l'API n'a aucun saut de ligne (D-26) : il donne un seul
    /// paragraphe.
    ///
    /// # Errors
    ///
    /// `DomainError::TextTooLarge` si le texte dépasse [`LongText::MAX_CHARS`] caractères.
    pub fn from_plain(text: &str) -> Result<Self, DomainError> {
        let normalized = text.replace("\r\n", "\n");
        let mut paragraphs = Vec::new();
        let mut invisible_removed = 0_u32;
        for chunk in normalized.split('\n') {
            let (cleaned, report) = clean_with_report(chunk);
            invisible_removed = invisible_removed.saturating_add(report.invisible_removed);
            // `clean` peut produire des sauts de ligne (un `<br>` littéral du texte brut) :
            // chaque ligne non vide est alors un paragraphe.
            paragraphs
                .extend(cleaned.split('\n').filter(|line| !line.is_empty()).map(str::to_owned));
        }
        Self::assemble(&paragraphs, invisible_removed)
    }

    /// Choisit la meilleure source, `texteHtml` ou `texte` (03 §7.4).
    ///
    /// 1. `html` présent et non blanc : son texte, sauf garde-fou.
    /// 2. Garde-fou : si le texte issu du HTML fait moins de 90 % de la longueur repliée de
    ///    `plain`, le HTML est probablement incomplet et `plain` est préféré.
    /// 3. Sinon `plain` si le HTML est absent ou blanc.
    /// 4. Aucun des deux : texte vide.
    ///
    /// # Errors
    ///
    /// `DomainError::TextTooLarge` si le texte retenu dépasse [`LongText::MAX_CHARS`] caractères.
    pub fn best_of(html: Option<&str>, plain: Option<&str>) -> Result<Self, DomainError> {
        let html = html.filter(|text| !text.trim().is_empty());
        let plain = plain.filter(|text| !text.trim().is_empty());
        match (html, plain) {
            (Some(html), Some(plain)) => {
                let (paragraphs, invisible_removed) = html::paragraphs(html);
                if html_is_complete(joined_chars(&paragraphs), folded_chars(plain)) {
                    Self::assemble(&paragraphs, invisible_removed)
                } else {
                    Self::from_plain(plain)
                }
            }
            (Some(html), None) => Self::from_html(html),
            (None, Some(plain)) => Self::from_plain(plain),
            (None, None) => Ok(Self::default()),
        }
    }

    /// Texte canonique : paragraphes joints par `"\n\n"`.
    pub fn as_str(&self) -> &str {
        &self.canonical
    }

    /// Longueur du texte canonique en caractères Unicode, séparateurs compris (`text_length`).
    pub fn char_len(&self) -> u32 {
        self.char_len
    }

    /// Le texte est-il vide ?
    pub fn is_empty(&self) -> bool {
        self.char_len == 0
    }

    /// Invisibles, contrôles et bidirectionnels retirés à la construction (diagnostic).
    pub fn invisible_removed(&self) -> u32 {
        self.invisible_removed
    }

    /// Paragraphes, avec la position de leur premier caractère dans le texte canonique, en
    /// caractères Unicode : le paragraphe `i + 1` commence à `char_start(i) + char_len(i) + 2`.
    pub fn paragraphs(&self) -> impl Iterator<Item = (u32, &str)> {
        self.paras.iter().enumerate().map(|(i, para)| {
            let start = index(para.byte_start);
            let end = self
                .paras
                .get(i + 1)
                .map_or(self.canonical.len(), |next| index(next.byte_start) - SEPARATOR.len());
            (para.char_start, &self.canonical[start..end])
        })
    }

    /// Construit le texte canonique de paragraphes déjà nettoyés (non vides, sans `\n`) et
    /// applique la limite : les longueurs se comptent en `u64`, comparées à
    /// [`LongText::MAX_CHARS`] avant toute conversion en `u32`.
    fn assemble(paragraphs: &[String], invisible_removed: u32) -> Result<Self, DomainError> {
        let lengths: Vec<u64> = paragraphs.iter().map(|p| wide(p.chars().count())).collect();
        let separators = wide(paragraphs.len().saturating_sub(1)) * SEPARATOR_CHARS;
        let total = lengths.iter().sum::<u64>() + separators;
        if total > u64::from(Self::MAX_CHARS) {
            return Err(DomainError::TextTooLarge { chars: total });
        }
        let mut byte_start = 0_usize;
        let mut char_start = 0_u64;
        let mut paras = Vec::with_capacity(paragraphs.len());
        for (paragraph, &chars) in paragraphs.iter().zip(&lengths) {
            paras.push(Para {
                byte_start: narrow(byte_start),
                char_start: narrow_wide(char_start),
                char_len: narrow_wide(chars),
            });
            byte_start += paragraph.len() + SEPARATOR.len();
            char_start += chars + SEPARATOR_CHARS;
        }
        Ok(Self {
            canonical: paragraphs.join(SEPARATOR).into(),
            paras: paras.into(),
            char_len: narrow_wide(total),
            invisible_removed,
        })
    }
}

impl fmt::Debug for LongText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LongText({} chars)", self.char_len)
    }
}

/// Le texte issu du HTML est-il assez complet face à `plain` ? Garde-fou de 03 §7.4 : il l'est
/// à 90 % de la longueur repliée de `plain` ou plus (9 contre 10 le garde). Les produits ne
/// saturent qu'au-delà de 10^18 caractères, hors de portée d'une mémoire réelle.
fn html_is_complete(html_chars: usize, plain_chars: usize) -> bool {
    html_chars.saturating_mul(10) >= plain_chars.saturating_mul(9)
}

/// Caractères des paragraphes joints par une espace.
fn joined_chars(paragraphs: &[String]) -> usize {
    let chars: usize = paragraphs.iter().map(|p| p.chars().count()).sum();
    chars + paragraphs.len().saturating_sub(1)
}

/// Caractères de `text` une fois ses blancs repliés en une espace et ses bords rognés.
fn folded_chars(text: &str) -> usize {
    let (mut words, mut chars) = (0_usize, 0_usize);
    for word in text.split_whitespace() {
        words += 1;
        chars += word.chars().count();
    }
    chars + words.saturating_sub(1)
}

/// `usize` en `u64`. Sans perte sur toute cible usuelle (`usize` d'au plus 64 bits).
fn wide(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// Décalage ou position d'un texte dont la limite est déjà vérifiée (au plus 16 000 000
/// caractères, donc au plus 64 000 000 octets) : tient en `u32`.
fn narrow(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Comme [`narrow`], depuis un `u64`.
fn narrow_wide(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// `u32` en indice. Sans perte sur toute cible usuelle (`usize` d'au moins 32 bits).
fn index(n: u32) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::DomainError;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    const CONSTIT: &str = include_str!(
        "../../../lf-adapter-piste/tests/fixtures/reference/consult_juri_constit.json"
    );
    const CETAT: &str =
        include_str!("../../../lf-adapter-piste/tests/fixtures/reference/consult_juri_cetat.json");
    const CNIL: &str =
        include_str!("../../../lf-adapter-piste/tests/fixtures/reference/consult_cnil.json");

    /// Champ texte d'une fixture (`text.<name>`), `None` s'il est `null`.
    fn text_field(fixture_json: &str, name: &str) -> Option<String> {
        let value: serde_json::Value = serde_json::from_str(fixture_json).unwrap();
        value["text"][name].as_str().map(str::to_owned)
    }

    /// Suite de blancs repliée en une espace, bords rognés.
    fn fold_ws(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn html(s: &str) -> LongText {
        LongText::from_html(s).unwrap()
    }

    fn plain(s: &str) -> LongText {
        LongText::from_plain(s).unwrap()
    }

    fn parts(text: &LongText) -> Vec<&str> {
        text.paragraphs().map(|(_, p)| p).collect()
    }

    // ---- invariants ----

    #[test]
    fn canonical_is_paragraphs_joined_by_blank_line() {
        let text = html("<p>a</p><p>b c</p><p>d</p>");
        assert_eq!(text.as_str(), "a\n\nb c\n\nd");
        assert_eq!(parts(&text), ["a", "b c", "d"]);
        assert_eq!(html("seul").as_str(), "seul");
    }

    #[test]
    fn no_paragraph_is_empty_or_contains_newline() {
        let inputs = ["a<br>b<p></p><p> </p>c\nd", "<p>a<br></br>b</p>", "&#10;a&#13;&#10;b", ""];
        for input in inputs {
            check_invariants(&html(input));
            check_invariants(&plain(input));
        }
        // Un `<br>` littéral du texte brut ne doit pas laisser de saut de ligne.
        let text = plain("a<br>b\n\nc<p>d</p>");
        assert_eq!(parts(&text), ["a", "b", "c", "d"]);
        for fixture in [CONSTIT, CETAT, CNIL] {
            check_invariants(&html(&text_field(fixture, "texteHtml").unwrap()));
            check_invariants(&plain(&text_field(fixture, "texte").unwrap()));
        }
    }

    /// Invariants de 03 §7.5 : paragraphes non vides, sans saut de ligne ni blanc de bord.
    fn check_invariants(text: &LongText) {
        for (_, paragraph) in text.paragraphs() {
            assert_ne!(paragraph, "");
            assert!(!paragraph.contains('\n'), "{paragraph:?}");
            assert_eq!(paragraph.trim(), paragraph);
        }
        assert_eq!(text.as_str(), parts(text).join("\n\n"));
        assert_eq!(text.is_empty(), text.as_str().is_empty());
    }

    #[test]
    fn char_len_counts_chars_not_bytes() {
        // « é » : 2 octets, « 😀 » : 4 octets ; deux paragraphes séparés par 2 caractères.
        let text = html("<p>é😀</p><p>ça</p>");
        assert_eq!(text.as_str().len(), (2 + 4) + 2 + (2 + 1));
        assert_eq!(text.char_len(), 2 + 2 + 2);
        assert_eq!(plain("").char_len(), 0);
        assert!(plain("").is_empty());
        assert!(!plain("a").is_empty());
    }

    #[test]
    fn paragraphs_report_char_starts() {
        let text = html("<p>é😀</p><p>ça va</p><p>x</p>");
        let starts: Vec<(u32, &str)> = text.paragraphs().collect();
        assert_eq!(starts, [(0, "é😀"), (4, "ça va"), (11, "x")]);
        // `char_start(i + 1) = char_start(i) + char_len(i) + 2`, positions en caractères.
        let canonical: Vec<char> = text.as_str().chars().collect();
        for (start, paragraph) in &starts {
            let from = usize::try_from(*start).unwrap();
            let slice: String = canonical[from..from + paragraph.chars().count()].iter().collect();
            assert_eq!(&slice, paragraph);
        }
        assert_eq!(text.paragraphs().count(), 3);
        assert_eq!(LongText::default().paragraphs().count(), 0);
    }

    #[test]
    fn text_length_counts_chars_of_canonical_text() {
        // 03 §7.5 : 14 236 caractères de paragraphes + 2 × 48 séparateurs.
        let constit = html(&text_field(CONSTIT, "texteHtml").unwrap());
        assert_eq!(constit.char_len(), 14_332);
        assert_eq!(constit.paragraphs().count(), 49);
        assert_eq!(usize::try_from(constit.char_len()).unwrap(), constit.as_str().chars().count());
        let last = constit.paragraphs().last().unwrap();
        assert_eq!(last.0 + u32::try_from(last.1.chars().count()).unwrap(), 14_332);
    }

    // ---- from_plain ----

    #[test]
    fn from_plain_splits_on_newline_runs() {
        assert_eq!(parts(&plain("a\nb\n\n\nc")), ["a", "b", "c"]);
        assert_eq!(parts(&plain("a\r\nb\r\n\r\nc")), ["a", "b", "c"]);
        assert_eq!(parts(&plain("\n  \na  b \n\t\n")), ["a b"]);
        assert_eq!(parts(&plain("a <b>gras</b>\nc &amp; d")), ["a gras", "c & d"]);
    }

    #[test]
    fn from_plain_without_newline_is_one_paragraph() {
        // Le `texte` de l'API n'a aucun saut de ligne (D-26).
        let text = text_field(CONSTIT, "texte").unwrap();
        assert!(!text.contains('\n'));
        let built = plain(&text);
        assert_eq!(built.paragraphs().count(), 1);
        assert_eq!(built.as_str(), fold_ws(&text));
        assert_eq!(parts(&plain("un seul bloc")), ["un seul bloc"]);
    }

    // ---- best_of et garde-fou ----

    #[test]
    fn best_of_prefers_html_when_present() {
        let text = LongText::best_of(Some("<p>a</p><p>b</p>"), Some("a b")).unwrap();
        assert_eq!(parts(&text), ["a", "b"]);
        let text = LongText::best_of(Some("<p>a</p><p>b</p>"), None).unwrap();
        assert_eq!(parts(&text), ["a", "b"]);
        let text = LongText::best_of(Some("a"), Some("  \n ")).unwrap();
        assert_eq!(parts(&text), ["a"]);
        let text = LongText::best_of(None, Some("a\nb")).unwrap();
        assert_eq!(parts(&text), ["a", "b"]);
    }

    #[test]
    fn best_of_falls_back_to_plain_below_90_percent() {
        // HTML : 3 caractères ; plain replié : 10 caractères. 30 < 90.
        let text = LongText::best_of(Some("<p>abc</p>"), Some("abcdefghij")).unwrap();
        assert_eq!(parts(&text), ["abcdefghij"]);
        // Les longueurs se comptent en caractères, pas en octets : « é » en vaut un.
        let text = LongText::best_of(Some("éééééééé"), Some("abcdefghij")).unwrap();
        assert_eq!(parts(&text), ["abcdefghij"]);
        // Le HTML sans aucun texte est incomplet face à un texte brut non vide.
        let text = LongText::best_of(Some("<p></p>"), Some("abc")).unwrap();
        assert_eq!(parts(&text), ["abc"]);
    }

    #[test]
    fn best_of_keeps_html_at_exactly_90_percent() {
        // HTML : 9 caractères ; plain : 10. 90 contre 90 : le HTML est gardé.
        let text = LongText::best_of(Some("<p>abcdefghi</p>"), Some("abcdefghij")).unwrap();
        assert_eq!(parts(&text), ["abcdefghi"]);
        // La longueur du HTML joint les paragraphes par une espace (4 + 1 + 4 = 9)...
        let text = LongText::best_of(Some("<p>abcd</p><p>efgh</p>"), Some("abcdefghij")).unwrap();
        assert_eq!(parts(&text), ["abcd", "efgh"]);
        // ... et celle de `plain` replie les blancs (4 + 1 + 5 = 10, pas 14).
        let text =
            LongText::best_of(Some("<p>abcd</p><p>efgh</p>"), Some("abcd \n\t  efghi")).unwrap();
        assert_eq!(parts(&text), ["abcd", "efgh"]);
    }

    #[test]
    fn best_of_counts_the_spaces_of_the_folded_plain_text() {
        // `plain` replié : « abcde fghij » = 5 + 1 + 5 = 11 caractères. 9 × 10 < 9 × 11.
        let text = LongText::best_of(Some("<p>abcdefghi</p>"), Some("abcde \t  fghij")).unwrap();
        assert_eq!(parts(&text), ["abcde fghij"]);
        // Dix mots d'une lettre : 19 caractères repliés.
        let text = LongText::best_of(Some("<p>abcdefgh</p>"), Some("a b c d e f g h i j")).unwrap();
        assert_eq!(parts(&text), ["a b c d e f g h i j"]);
        assert_eq!(folded_chars(""), 0);
        assert_eq!(folded_chars(" \n "), 0);
        assert_eq!(folded_chars(" ab\t\u{a0}cde \n"), 6);
        assert_eq!(joined_chars(&[]), 0);
        assert_eq!(joined_chars(&["ab".to_owned(), "é".to_owned()]), 4);
    }

    #[test]
    fn html_is_complete_at_the_boundary() {
        assert!(html_is_complete(9, 10));
        assert!(!html_is_complete(8, 10));
        assert!(html_is_complete(10, 10));
        assert!(html_is_complete(90, 100));
        assert!(!html_is_complete(89, 100));
        assert!(html_is_complete(0, 0));
        assert!(!html_is_complete(0, 1));
        assert!(html_is_complete(1, 0));
        // Aucun dépassement aux valeurs extrêmes.
        assert!(html_is_complete(usize::MAX, usize::MAX));
        assert!(html_is_complete(usize::MAX, 0));
    }

    #[test]
    fn best_of_constit_keeps_html() {
        // 99,8 % : seul l'ECLI final manque au HTML.
        let html_text = text_field(CONSTIT, "texteHtml").unwrap();
        let plain_text = text_field(CONSTIT, "texte").unwrap();
        let text = LongText::best_of(Some(&html_text), Some(&plain_text)).unwrap();
        assert_eq!(text, html(&html_text));
        assert_eq!(text.paragraphs().count(), 49);
    }

    #[test]
    fn best_of_empty_inputs_give_empty() {
        for (h, p) in [(None, None), (Some(" "), Some("")), (Some("\n"), None), (None, Some("\t "))]
        {
            let text = LongText::best_of(h, p).unwrap();
            assert_eq!(text, LongText::default());
            assert!(text.is_empty());
            assert_eq!(text.char_len(), 0);
            assert_eq!(text.invisible_removed(), 0);
        }
    }

    #[test]
    fn best_of_reports_too_large() {
        let big = "a".repeat(16_000_001);
        assert_eq!(
            LongText::best_of(Some(&big), None),
            Err(DomainError::TextTooLarge { chars: 16_000_001 })
        );
        assert_eq!(
            LongText::best_of(None, Some(&big)),
            Err(DomainError::TextTooLarge { chars: 16_000_001 })
        );
        // Le garde-fou préfère `plain`, trop long : l'erreur vient de `plain`.
        assert_eq!(
            LongText::best_of(Some("a"), Some(&big)),
            Err(DomainError::TextTooLarge { chars: 16_000_001 })
        );
    }

    // ---- limite ----

    #[test]
    fn long_text_over_the_limit_is_text_too_large() {
        assert_eq!(
            LongText::from_plain(&"a".repeat(16_000_001)),
            Err(DomainError::TextTooLarge { chars: 16_000_001 })
        );
        assert_eq!(
            LongText::from_html(&"a".repeat(16_000_001)),
            Err(DomainError::TextTooLarge { chars: 16_000_001 })
        );
        let limit = LongText::from_plain(&"a".repeat(16_000_000)).unwrap();
        assert_eq!(limit.char_len(), LongText::MAX_CHARS);
        assert_eq!(LongText::MAX_CHARS, 16_000_000);
    }

    #[test]
    fn the_limit_counts_paragraph_separators() {
        // 8 000 000 + 2 + 7 999 999 = 16 000 001 caractères une fois joints.
        let over = format!("{}\n{}", "a".repeat(8_000_000), "b".repeat(7_999_999));
        assert_eq!(
            LongText::from_plain(&over),
            Err(DomainError::TextTooLarge { chars: 16_000_001 })
        );
        let exact = format!("{}\n{}", "a".repeat(8_000_000), "b".repeat(7_999_998));
        assert_eq!(LongText::from_plain(&exact).unwrap().char_len(), LongText::MAX_CHARS);
    }

    #[test]
    fn the_error_message_names_the_constant() {
        let error = DomainError::TextTooLarge { chars: 17 };
        assert_eq!(
            error.to_string(),
            format!("text of 17 characters exceeds the {} limit", LongText::MAX_CHARS)
        );
    }

    // ---- invisibles, Debug, partage ----

    #[test]
    fn long_text_counts_removed_invisibles() {
        let text = html("<p>a\u{202E}b</p>");
        assert_eq!(text.invisible_removed(), 1);
        assert_eq!(text.as_str(), "ab");
        // Somme sur tous les paragraphes, y compris ceux qui disparaissent.
        assert_eq!(html("a\u{200b}<p>\u{202e}\u{200b}<p>b&#173;").invisible_removed(), 4);
        // `from_plain` : somme sur les morceaux.
        let text = plain("a\u{200b}\n\u{202e}\nb\u{feff}");
        assert_eq!(text.invisible_removed(), 3);
        assert_eq!(parts(&text), ["a", "b"]);
        assert_eq!(html("a<p>b").invisible_removed(), 0);
        // Le compteur de `best_of` est celui de la source retenue.
        let kept = LongText::best_of(Some("<p>a\u{202e}b</p>"), Some("ab")).unwrap();
        assert_eq!(kept.invisible_removed(), 1);
        let fallback = LongText::best_of(Some("<p>a</p>"), Some("abcd\u{202e}efgh")).unwrap();
        assert_eq!(fallback.invisible_removed(), 1);
    }

    #[test]
    fn debug_never_prints_the_text() {
        let secret = html("<p>texte confidentiel</p><p>é</p>");
        assert_eq!(format!("{secret:?}"), "LongText(21 chars)");
        assert_eq!(format!("{:?}", LongText::default()), "LongText(0 chars)");
        assert_eq!(format!("{secret:#?}"), "LongText(21 chars)");
    }

    #[test]
    fn clone_shares_the_buffer() {
        let original = plain("un texte\npartagé");
        let clone = original.clone();
        assert_eq!(original, clone);
        assert_eq!(original.as_str().as_ptr(), clone.as_str().as_ptr());
        let rebuilt = plain("un texte\npartagé");
        assert_eq!(original, rebuilt);
        assert_ne!(original.as_str().as_ptr(), rebuilt.as_str().as_ptr());
    }

    #[test]
    fn equality_compares_content() {
        assert_eq!(html("<p>a</p><p>b</p>"), plain("a\nb"));
        assert_ne!(html("a"), html("b"));
        assert_ne!(html("a\u{200b}"), html("a"));
    }

    // ---- fixtures ----

    #[test]
    fn verbatim_text_is_unchanged_except_markup() {
        // `texte` est `texteHtml` sans balises, suivi de l'ECLI que `RECH_ECLI` supprime.
        for fixture in [CONSTIT, CETAT] {
            let built = html(&text_field(fixture, "texteHtml").unwrap());
            let ecli = text_field(fixture, "ecli").unwrap();
            let verbatim = fold_ws(&text_field(fixture, "texte").unwrap());
            assert_eq!(verbatim, format!("{} {ecli}", fold_ws(built.as_str())));
        }
    }

    #[test]
    fn truncated_cnil_text_is_unchanged_except_markup() {
        // Pas d'ECLI. La fixture CNIL est un extrait tronqué (` [...]`) à deux endroits
        // différents pour `texte` et `texteHtml` : les deux textes concordent jusqu'à la
        // coupure de `texteHtml`.
        assert_eq!(text_field(CNIL, "ecli"), None);
        let built = html(&text_field(CNIL, "texteHtml").unwrap());
        let verbatim = fold_ws(&text_field(CNIL, "texte").unwrap());
        let kept = built.as_str().strip_suffix(" [...]").unwrap();
        assert!(verbatim.starts_with(&fold_ws(kept)), "{verbatim}");
        assert_eq!(built.paragraphs().count(), 3);
    }

    proptest! {
        #[test]
        fn constructors_keep_the_invariants(
            pieces in proptest::collection::vec(
                prop_oneof!["<", ">", "p", "br", "/", "&", "#10;", "\n", "\r\n", " ", "é",
                    "😀", "\u{200b}", "\u{a0}", "a", "td", any::<char>().prop_map(String::from)],
                0..40,
            ),
        ) {
            let source = pieces.concat();
            for text in [html(&source), plain(&source)] {
                for (_, paragraph) in text.paragraphs() {
                    prop_assert_ne!(paragraph, "");
                    prop_assert!(!paragraph.contains('\n'));
                    prop_assert_eq!(paragraph.trim(), paragraph);
                }
                prop_assert_eq!(text.as_str(), parts(&text).join("\n\n"));
                prop_assert_eq!(
                    usize::try_from(text.char_len()).unwrap(),
                    text.as_str().chars().count()
                );
            }
        }
    }
}
