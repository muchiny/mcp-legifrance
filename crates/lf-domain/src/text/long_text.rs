//! Texte long structuré en paragraphes (03 §7.5).
//!
//! Le texte canonique est défini **après** `clean` (03 §7.3) : paragraphes non vides, sans saut
//! de ligne ni blanc de bord, joints par une ligne vide. Fenêtres, passages et classement le
//! consomment tels quels.
//!
//! Construction en flux (audit M-2) : chaque segment, issu du HTML ou d'une ligne du texte
//! brut, est replié directement dans le texte canonique, sans tableau intermédiaire de
//! paragraphes ni de mots. Au-delà de [`LongText::MAX_CHARS`] caractères, le texte n'est plus
//! construit et sa mémoire est libérée ; seuls les caractères restants sont comptés, pour
//! l'erreur. Au-delà de [`LongText::MAX_PARAGRAPHS`] paragraphes, la suite rejoint le dernier.

use std::{fmt, mem, sync::Arc};

use super::html;
use crate::error::DomainError;

/// Séparateur des paragraphes dans le texte canonique.
const SEPARATOR: &str = "\n\n";

/// Texte long : paragraphes joints par `"\n\n"` (forme exposée au modèle et base des offsets),
/// et table des paragraphes. Le texte et la table sont partagés par `Arc` : cloner une entité
/// servie par le cache ne copie pas le texte, qui peut atteindre [`LongText::MAX_CHARS`]. La
/// table a au plus [`LongText::MAX_PARAGRAPHS`] entrées ; [`LongText::heap_bytes`] donne leur
/// poids, pour le `weigher` du cache (02 §9).
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

    /// Plus grand nombre de paragraphes. Au-delà, le texte restant rejoint le dernier
    /// paragraphe, séparé par une seule espace : rien n'est perdu, les invariants tiennent, et la
    /// table des paragraphes reste bornée (audit M-2 : 16 000 000 caractères permettaient
    /// 5,3 millions de paragraphes d'un caractère, soit 64 Mio d'index). La plus longue fixture
    /// en compte 49 ; une longue décision, quelques milliers.
    pub const MAX_PARAGRAPHS: u32 = 100_000;

    /// Texte canonique d'un fragment HTML : un paragraphe par bloc (03 §7.2).
    ///
    /// # Errors
    ///
    /// `DomainError::TextTooLarge` si le texte dépasse [`LongText::MAX_CHARS`] caractères.
    pub fn from_html(html: &str) -> Result<Self, DomainError> {
        Canonical::from_html(html).finish()
    }

    /// Texte canonique d'un texte brut (Ruling 12) : coupure sur les suites de sauts de ligne,
    /// caractères invisibles retirés et comptés, blancs repliés en une espace. `\r` est un
    /// blanc : `\r\n` coupe comme `\n`. Aucun lexing : ni balise, ni commentaire, ni entité
    /// (le `texte` de l'API n'est pas du HTML, un `<` y est un chevron : `R<P` reste `R<P`). Les
    /// lignes blanches sont supprimées. Le `texte` de l'API n'a aucun saut de ligne (D-26) : il
    /// donne un seul paragraphe.
    ///
    /// # Errors
    ///
    /// `DomainError::TextTooLarge` si le texte dépasse [`LongText::MAX_CHARS`] caractères.
    pub fn from_plain(text: &str) -> Result<Self, DomainError> {
        let mut canonical = Canonical::with_capacity(text.len());
        for line in text.split('\n') {
            let (visible, removed) = html::strip_invisible(line);
            canonical.invisible_removed = canonical.invisible_removed.saturating_add(removed);
            canonical.push_segment(&visible);
        }
        canonical.finish()
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
                let built = Canonical::from_html(html);
                if html_is_complete(built.joined_chars(), wide(folded_chars(plain))) {
                    built.finish()
                } else {
                    // Le texte issu du HTML est libéré avant de construire `plain` (audit M-2).
                    drop(built);
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

    /// Octets occupés sur le tas : texte canonique et table des paragraphes (en-têtes des `Arc`
    /// exclus). Poids d'une entité pour le `weigher` du cache (02 §9).
    pub fn heap_bytes(&self) -> usize {
        self.canonical.len() + mem::size_of_val(&*self.paras)
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
}

impl fmt::Debug for LongText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LongText({} chars)", self.char_len)
    }
}

/// Texte canonique en construction, en flux (audit M-2).
struct Canonical {
    /// Texte canonique et table des paragraphes ; `None` dès que `chars` dépasse
    /// [`LongText::MAX_CHARS`] : le texte ne sera jamais rendu, sa mémoire est libérée aussitôt.
    built: Option<(String, Vec<Para>)>,
    /// Segments non blancs reçus, y compris ceux fusionnés dans le dernier paragraphe.
    segments: u64,
    /// Caractères du texte canonique, séparateurs compris ; comptés aussi après le dépassement.
    chars: u64,
    /// Caractères invisibles retirés.
    invisible_removed: u32,
}

impl Canonical {
    /// Construction vide ; `capacity` est la taille de l'entrée.
    fn with_capacity(capacity: usize) -> Self {
        Self {
            built: Some((String::with_capacity(capacity), Vec::new())),
            segments: 0,
            chars: 0,
            invisible_removed: 0,
        }
    }

    /// Texte de `html`, segment par segment (03 §7.2).
    fn from_html(html: &str) -> Self {
        let mut canonical = Self::with_capacity(html.len());
        let invisible = html::paragraphs(html, &mut |visible| canonical.push_segment(visible));
        canonical.invisible_removed = invisible;
        canonical
    }

    /// Ajoute un segment déjà débarrassé de ses invisibles : ses blancs deviennent une espace et
    /// ses bords sont rognés ; un segment blanc ne fait rien.
    fn push_segment(&mut self, visible: &str) {
        let mut words = visible.split_whitespace();
        let Some(first) = words.next() else {
            return;
        };
        self.open_paragraph();
        self.push(first);
        for word in words {
            self.push(" ");
            self.push(word);
        }
    }

    /// Début d'un segment non blanc : un nouveau paragraphe, ou, au-delà de
    /// [`LongText::MAX_PARAGRAPHS`], la suite du dernier, après une seule espace.
    fn open_paragraph(&mut self) {
        let fresh = self.segments < u64::from(LongText::MAX_PARAGRAPHS);
        if self.segments > 0 {
            self.seal();
            self.push(if fresh { SEPARATOR } else { " " });
        }
        self.segments += 1;
        if fresh && let Some((text, paras)) = &mut self.built {
            let (byte_start, char_start) = (narrow(text.len()), narrow_wide(self.chars));
            paras.push(Para { byte_start, char_start, char_len: 0 });
        }
    }

    /// Fixe la longueur du dernier paragraphe : jusqu'à la position courante.
    fn seal(&mut self) {
        if let Some((_, paras)) = &mut self.built
            && let Some(last) = paras.last_mut()
        {
            last.char_len = narrow_wide(self.chars - u64::from(last.char_start));
        }
    }

    /// Ajoute `piece` au texte et compte ses caractères ; au dépassement de
    /// [`LongText::MAX_CHARS`], le texte est abandonné.
    fn push(&mut self, piece: &str) {
        self.chars += wide(piece.chars().count());
        if self.chars > u64::from(LongText::MAX_CHARS) {
            self.built = None;
        }
        if let Some((text, _)) = &mut self.built {
            text.push_str(piece);
        }
    }

    /// Caractères du texte si ses paragraphes étaient joints par une espace (garde-fou de
    /// [`LongText::best_of`]) : chaque séparateur `"\n\n"` y compte pour un.
    fn joined_chars(&self) -> u64 {
        let paragraphs = self.segments.min(u64::from(LongText::MAX_PARAGRAPHS));
        self.chars - paragraphs.saturating_sub(1)
    }

    /// Texte construit, ou `TextTooLarge` avec la longueur du texte entier.
    fn finish(mut self) -> Result<LongText, DomainError> {
        self.seal();
        let Some((text, paras)) = self.built else {
            return Err(DomainError::TextTooLarge { chars: self.chars });
        };
        Ok(LongText {
            canonical: text.into(),
            paras: paras.into(),
            char_len: narrow_wide(self.chars),
            invisible_removed: self.invisible_removed,
        })
    }
}

/// Le texte issu du HTML est-il assez complet face à `plain` ? Garde-fou de 03 §7.4 : il l'est
/// à 90 % de la longueur repliée de `plain` ou plus (9 contre 10 le garde). Les produits ne
/// saturent qu'au-delà de 10^18 caractères, hors de portée d'une mémoire réelle.
fn html_is_complete(html_chars: u64, plain_chars: u64) -> bool {
    html_chars.saturating_mul(10) >= plain_chars.saturating_mul(9)
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
        // Le texte brut n'est pas lexé (Ruling 12) : `<br>` et `<p>` y restent du texte.
        let text = plain("a<br>b\n\nc<p>d</p>");
        assert_eq!(parts(&text), ["a<br>b", "c<p>d</p>"]);
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
        // Ni balise, ni commentaire, ni entité : le texte brut reste tel quel, blancs repliés.
        assert_eq!(
            parts(&plain("a <b>gras</b>\nc &amp; d <!-- e -->")),
            ["a <b>gras</b>", "c &amp; d <!-- e -->"]
        );
    }

    #[test]
    fn from_plain_keeps_raw_chevrons() {
        // Ruling 12 : un `<` du texte brut est un chevron (formule, comparaison), pas une balise.
        for sentence in [
            "Lorsque R<P, le taux T = 5 % s'applique ; sinon…",
            "si A<B et C>D alors la somme est due.",
        ] {
            assert_eq!(parts(&plain(sentence)), [sentence]);
            // Le chemin `plain` de `best_of` aussi : seul, ou en repli du garde-fou.
            assert_eq!(parts(&LongText::best_of(None, Some(sentence)).unwrap()), [sentence]);
            let fallback = LongText::best_of(Some("<p>si</p>"), Some(sentence)).unwrap();
            assert_eq!(parts(&fallback), [sentence]);
        }
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
        // Le texte du HTML, paragraphes joints par une espace : « ab é ».
        assert_eq!(Canonical::from_html("").joined_chars(), 0);
        assert_eq!(Canonical::from_html("<p>ab</p><p>é</p>").joined_chars(), 4);
        // Fusion au-delà de MAX_PARAGRAPHS : l'espace de fusion compte déjà pour un.
        let merged = Canonical::from_html(&"a<p>".repeat(100_002));
        assert_eq!(merged.joined_chars(), 100_002 + 100_001);
        // Deux paragraphes (4 + 1 + 4 = 9, et non 10) contre 11 : 90 < 99, repli sur `plain`.
        let text = LongText::best_of(Some("<p>abcd</p><p>efgh</p>"), Some("abcd efghij")).unwrap();
        assert_eq!(parts(&text), ["abcd efghij"]);
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
        assert!(html_is_complete(u64::MAX, u64::MAX));
        assert!(html_is_complete(u64::MAX, 0));
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
    fn the_limit_error_counts_the_whole_text() {
        // Au dépassement, le texte n'est plus construit (sa mémoire est libérée), mais les
        // caractères restants sont comptés : l'erreur donne la longueur du texte entier.
        let over = format!("{}\nb\n\nc d", "a".repeat(16_000_001));
        assert_eq!(
            LongText::from_plain(&over),
            Err(DomainError::TextTooLarge { chars: 16_000_001 + 2 + 1 + 2 + 3 })
        );
        let over = format!("<p>{}<p>b<p>c d", "a".repeat(16_000_001));
        assert_eq!(
            LongText::from_html(&over),
            Err(DomainError::TextTooLarge { chars: 16_000_001 + 2 + 1 + 2 + 3 })
        );
    }

    // ---- borne du nombre de paragraphes (audit M-2) ----

    /// `n` lignes `p0` à `p<n-1>`, séparées par `sep`.
    fn numbered(n: usize, sep: &str) -> String {
        (0..n).map(|i| format!("p{i}")).collect::<Vec<_>>().join(sep)
    }

    #[test]
    fn paragraphs_past_the_maximum_are_merged_into_the_last_one() {
        // Au-delà de 100 000 paragraphes, le texte restant rejoint le dernier, après une seule
        // espace : rien n'est perdu et les invariants tiennent.
        for (lines, last) in [
            (100_000, "p99999"),
            (100_001, "p99999 p100000"),
            (100_003, "p99999 p100000 p100001 p100002"),
        ] {
            for text in [plain(&numbered(lines, "\n")), html(&numbered(lines, "<p>"))] {
                let paragraphs = parts(&text);
                assert_eq!(paragraphs.len(), 100_000);
                assert_eq!(paragraphs.last().copied(), Some(last));
                assert_eq!(paragraphs[99_998], "p99998");
                check_invariants(&text);
                let words: Vec<&str> = text.as_str().split_whitespace().collect();
                assert_eq!(words.len(), lines);
                assert_eq!(words.last().copied(), Some(format!("p{}", lines - 1).as_str()));
                assert_eq!(
                    usize::try_from(text.char_len()).unwrap(),
                    text.as_str().chars().count()
                );
            }
        }
    }

    #[test]
    fn each_paragraph_entry_has_its_offsets_and_length() {
        // `Para` (03 §7.5) n'est lu que par `Eq` et `paragraphs` : ses champs sont vérifiés ici.
        let text = html("<p>é😀</p><p>ça va</p>");
        assert_eq!(
            text.paras[..],
            [
                Para { byte_start: 0, char_start: 0, char_len: 2 },
                Para { byte_start: 8, char_start: 4, char_len: 5 },
            ]
        );
        // Le dernier paragraphe, fusionné au-delà de MAX_PARAGRAPHS, compte toute sa longueur.
        for text in [plain("a\n\nbc d"), plain(&numbered(100_002, "\n")), LongText::default()] {
            let lengths: Vec<u32> = text.paras.iter().map(|para| para.char_len).collect();
            let expected: Vec<u32> = text
                .paragraphs()
                .map(|(_, paragraph)| u32::try_from(paragraph.chars().count()).unwrap())
                .collect();
            assert_eq!(lengths, expected);
        }
    }

    #[test]
    fn heap_bytes_counts_the_text_and_the_paragraph_table() {
        // « a\n\nbc » : 5 octets de texte, 2 paragraphes de 12 octets d'index.
        assert_eq!(html("<p>a</p><p>bc</p>").heap_bytes(), 5 + 2 * 12);
        assert_eq!(plain("é").heap_bytes(), 2 + 12);
        assert_eq!(LongText::default().heap_bytes(), 0);
        // Croît avec le texte et avec le nombre de paragraphes.
        let short = plain("un texte");
        let longer = plain("un texte\nplus long");
        assert!(longer.heap_bytes() > short.heap_bytes());
        assert!(plain(&"a ".repeat(1000)).heap_bytes() > plain(&"a ".repeat(10)).heap_bytes());
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
        // Sélecteurs de variante et autres `Default_Ignorable_Code_Point` (audit L-1).
        for text in [html("x\u{fe0f}\u{e0101}&#x2063;y"), plain("x\u{fe0f}\u{e0101}\u{2063}y")] {
            assert_eq!((text.as_str(), text.invisible_removed()), ("xy", 3));
        }
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
