//! Politique de lecture des textes longs (05 §6, 02 §6.1) : trois modes (résumé, texte plein,
//! passages), `offset`, `max_chars` et `next_offset`.
//!
//! [`ReadingRequest::parse`] valide les cinq paramètres de 05 §5.0.2 et en déduit le mode ;
//! [`read`] lit un [`LongText`] selon ce mode :
//!
//! - **résumé** (défaut) : le texte entier s'il tient dans `min(max_chars, 1 200)` caractères
//!   (`min(max_chars, 2 000)` pour un article, [`ReadingRequest::for_article`]), accompagné du
//!   sommaire officiel s'il existe, sinon le résumé du domaine ([`lf_domain::text::summary`]),
//!   toujours d'au plus `min(max_chars, 1 200)` caractères ; [`Reading::summary_truncated`] dit
//!   que ce résumé s'arrête avant la fin de sa source ;
//! - **texte plein** (`detail="full"`, ou `offset > 0` sans `detail`) : la fenêtre
//!   [`lf_domain::text::window`] de `max_chars` caractères à partir de `offset`, avec le
//!   `next_offset` à repasser tel quel ; les pages successives se concatènent en le texte exact ;
//! - **passages** (`focus`) : les meilleurs segments au sens BM25 ([`lf_domain::text::rank`]),
//!   dont la somme des longueurs ne dépasse pas `max_chars`.
//!
//! La politique ne dépend d'aucun algorithme de résumé ou de classement : elle consomme les
//! signatures du domaine.

use lf_domain::{
    DomainError,
    entity::OfficialSummary,
    text::{
        DispositifRule, Focus, LongText, SUMMARY_CAP_CHARS, Summary, SummarySource, rank, summary,
        window,
    },
};

use crate::{
    budget::{self, ResponseBudget},
    error::AppError,
    input::ReadingInput,
    messages,
    tools::ToolName,
};

/// Remarque d'un texte vide (05 §5.0.3).
const NO_TEXT_NOTE: &str = "No text is available from Légifrance for this record.";

/// Remarque d'un `focus` dont aucun passage ne correspond (05 §5.0.3).
const NO_PASSAGE_NOTE: &str = "No passage matches focus; read the text with detail=\"full\".";

/// Refus de `detail="summary"` explicite combiné à un `offset` positif (05 §7.3).
const OFFSET_NEEDS_FULL: &str = "offset requires detail=\"full\" (or omit detail).";

/// Plafond, en caractères, du texte qu'un article rend entier en mode résumé (Q20, 05 §5.0.3) :
/// plus haut que [`SUMMARY_CAP_CHARS`], qui reste celui du résumé d'une décision, d'une
/// délibération ou d'un article plus long.
pub const ARTICLE_WHOLE_CHARS: usize = 2_000;

/// Taille maximale, en caractères, d'un segment de texte classé en mode passages (03 §7.7).
const SEGMENT_MAX_CHARS: usize = 1_200;

/// Place minimale, en caractères, qu'il doit rester pour couper un passage qui dépasserait le
/// budget restant ; en deçà, le passage est écarté (03 §7.8).
const MIN_CUT_CHARS: usize = 200;

/// Outil nommé par les messages du domaine. Il n'influe que sur les messages d'identifiant, que
/// ni `focus` ni `offset` n'emploient : les trois lectures longues partagent ces textes.
const TOOL: ToolName = ToolName::FR_GET_TEXT;

/// Lecture demandée, déjà validée : mode, `offset`, `max_chars`, `focus` et `passages`.
///
/// Elle ne se construit que par [`ReadingRequest::parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingRequest {
    mode: Mode,
    max_chars: usize,
    /// Plafond, en caractères, sous lequel le mode résumé rend le texte entier (avant `max_chars`).
    whole_cap: usize,
}

/// Mode de lecture déduit des paramètres.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    /// Résumé, ou texte entier d'un texte court.
    Summary,
    /// Fenêtre de texte à partir de `offset`.
    Full { offset: usize },
    /// Les `count` meilleurs passages pour `focus`.
    Passages { focus: Focus, count: usize },
}

/// Valeur de `detail` (insensible à la casse).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Detail {
    Summary,
    Full,
}

impl Detail {
    /// `detail` sans casse ; toute autre valeur est refusée (`detail='long' must be one of
    /// ['full', 'summary'].`).
    fn parse(raw: &str) -> Result<Self, AppError> {
        if raw.eq_ignore_ascii_case("summary") {
            Ok(Self::Summary)
        } else if raw.eq_ignore_ascii_case("full") {
            Ok(Self::Full)
        } else {
            Err(messages::not_one_of("detail", raw, &["full", "summary"]))
        }
    }
}

impl ReadingRequest {
    /// Valide les paramètres de lecture dans l'ordre de la référence (`detail`, `offset`,
    /// `max_chars`, `focus`, `passages`) et en déduit le mode.
    ///
    /// - `detail` vaut `full` quand `offset > 0` est fourni sans `detail` ; `detail="summary"`
    ///   explicite avec `offset > 0` est refusé.
    /// - `focus` l'emporte sur `detail` : `offset` est alors ignoré (non validé), de même que
    ///   `passages` est ignoré sans `focus` (05 §5.0.2).
    /// - `max_chars` vaut 4 000 par défaut, de 500 à 40 000 ; `passages` vaut 3 par défaut, de 1
    ///   à 10.
    ///
    /// # Errors
    ///
    /// [`AppError::InvalidArgument`] pour une valeur hors bornes, un `detail` inconnu, un
    /// `offset` combiné à `detail="summary"` ou un `focus` sans mot recherchable.
    pub fn parse(input: &ReadingInput) -> Result<Self, AppError> {
        let detail = input.detail.as_deref().map(Detail::parse).transpose()?;
        let offset = if input.focus.is_some() { 0 } else { parse_offset(input.offset, detail)? };
        let max_chars = budget::bounded(
            "max_chars",
            input.max_chars,
            ResponseBudget::MAX_CHARS_DEFAULT,
            ResponseBudget::MAX_CHARS_MIN,
            ResponseBudget::MAX_CHARS_MAX,
        )?;
        let mode = match input.focus.as_deref() {
            Some(raw) => {
                let focus = parse_focus(raw)?;
                let count = budget::bounded(
                    "passages",
                    input.passages,
                    ResponseBudget::PASSAGES_DEFAULT,
                    1,
                    ResponseBudget::PASSAGES_MAX,
                )?;
                Mode::Passages { focus, count }
            }
            None if detail == Some(Detail::Full) || offset > 0 => Mode::Full { offset },
            None => Mode::Summary,
        };
        Ok(Self { mode, max_chars, whole_cap: SUMMARY_CAP_CHARS })
    }

    /// Cette lecture pour un article (`fr_get_text`) : en mode résumé, un article de
    /// `min(max_chars, ARTICLE_WHOLE_CHARS)` caractères au plus est rendu entier, au lieu de
    /// `min(max_chars, 1 200)`. Le plafond du résumé d'un texte plus long reste 1 200.
    #[must_use]
    pub fn for_article(self) -> Self {
        Self { whole_cap: ARTICLE_WHOLE_CHARS, ..self }
    }
}

/// `focus` après `trim` (1 à 500 caractères, au moins un mot recherchable, 05 §5.0.2) : un `focus`
/// blanc n'a aucun mot, quelle que soit l'analyse du domaine ; le reste est jugé par [`Focus::parse`].
fn parse_focus(raw: &str) -> Result<Focus, AppError> {
    let raw = raw.trim();
    let parsed =
        if raw.is_empty() { Err(DomainError::FocusWithoutTerms) } else { Focus::parse(raw) };
    parsed.map_err(|error| messages::domain(TOOL, &error))
}

/// `offset` demandé : 0 par défaut ; négatif, supérieur à `u32::MAX` (la borne annoncée, les
/// positions rendues tiennent dans un `u32`), ou positif avec `detail="summary"`, il est refusé.
fn parse_offset(raw: Option<i64>, detail: Option<Detail>) -> Result<usize, AppError> {
    let Some(raw) = raw else { return Ok(0) };
    let Some(offset) = u32::try_from(raw).ok().and_then(|offset| usize::try_from(offset).ok())
    else {
        return Err(messages::out_of_range("offset", raw, 0, i64::from(u32::MAX)));
    };
    if offset > 0 && detail == Some(Detail::Summary) {
        return Err(messages::invalid_argument(OFFSET_NEEDS_FULL));
    }
    Ok(offset)
}

/// Un passage rendu en mode `focus` : sa position, son texte et son score (05 §5.0.3).
///
/// Le score est un `f64` : le type ne dérive pas `Eq`.
#[derive(Debug, Clone, PartialEq)]
pub struct PassageOut {
    /// Position du premier caractère du passage dans le texte canonique, en caractères.
    pub offset: u32,
    /// Texte du passage.
    pub text: String,
    /// Score BM25 du segment dont le passage est tiré, arrondi à 2 décimales (`round_score`) ;
    /// strictement positif avant l'arrondi. Un passage coupé au budget garde le score du segment.
    pub score: f64,
}

/// Arrondit un score BM25 à 2 décimales (05 §5.0.3 : « score arrondi à 2 décimales »).
fn round_score(score: f64) -> f64 {
    (score * 100.0).round() / 100.0
}

/// Résultat d'une lecture : les champs communs aux trois lectures longues (05 §5.0.3). Chaque
/// mode ne remplit que les siens.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    /// Résumé, en mode résumé.
    pub summary: Option<Summary>,
    /// Texte, en mode plein texte ou pour un texte court.
    pub text: Option<String>,
    /// Longueur du texte canonique entier, en caractères.
    pub text_length: u32,
    /// Position de la fenêtre suivante, absente en fin de texte.
    pub next_offset: Option<u32>,
    /// Passages, en mode `focus`.
    pub passages: Option<Vec<PassageOut>>,
    /// Remarque exceptionnelle (texte vide, par exemple).
    pub note: Option<String>,
    /// Le résumé s'arrête avant la fin de sa source (vrai pour un résumé coupé, ou pour un début de
    /// texte plus court que le texte) ; faux sans résumé.
    pub summary_truncated: bool,
}

/// Lit `text` selon `req`.
///
/// Un texte vide donne la remarque « No text is available… » dans les trois modes, sans erreur
/// pour `offset = 0`. Le sommaire officiel `official` et la règle de dispositif `rule` ne servent
/// qu'au mode résumé.
///
/// # Errors
///
/// [`AppError::InvalidArgument`] quand `offset` dépasse la fin du texte (mode texte plein).
pub fn read(
    text: &LongText,
    official: Option<&OfficialSummary>,
    rule: DispositifRule,
    req: &ReadingRequest,
) -> Result<Reading, AppError> {
    match &req.mode {
        Mode::Summary => Ok(read_summary(text, official, rule, req.max_chars, req.whole_cap)),
        Mode::Full { offset } => read_window(text, *offset, req.max_chars),
        Mode::Passages { focus, count } => Ok(read_passages(text, focus, *count, req.max_chars)),
    }
}

/// Le `Reading` d'un texte sans contenu propre : seuls `text_length` et la remarque varient.
fn bare(text: &LongText) -> Reading {
    Reading {
        summary: None,
        text: None,
        text_length: text.char_len(),
        next_offset: None,
        passages: None,
        note: text.is_empty().then(|| NO_TEXT_NOTE.to_owned()),
        summary_truncated: false,
    }
}

/// Mode résumé : le texte entier s'il tient dans `min(max_chars, whole_cap)` caractères (avec le
/// sommaire officiel s'il existe, jamais un résumé tiré du texte lui-même), sinon le résumé, d'au
/// plus `min(max_chars, 1 200)` caractères. `whole_cap` vaut 1 200, ou 2 000 pour un article.
fn read_summary(
    text: &LongText,
    official: Option<&OfficialSummary>,
    rule: DispositifRule,
    max_chars: usize,
    whole_cap: usize,
) -> Reading {
    let built = summary(text, official, rule, max_chars.min(SUMMARY_CAP_CHARS));
    let partial = summary_is_partial(&built, length(text));
    if length(text) <= max_chars.min(whole_cap) {
        let official = built.source == SummarySource::Official;
        Reading {
            summary: official.then_some(built),
            text: (!text.is_empty()).then(|| text.as_str().to_owned()),
            summary_truncated: official && partial,
            ..bare(text)
        }
    } else {
        Reading { summary: Some(built), summary_truncated: partial, ..bare(text) }
    }
}

/// Vrai quand `summary` s'arrête avant la fin de sa source : le domaine l'a coupé au plafond, ou
/// c'est un début de texte (`lead`) plus court que le texte entier de `text_length` caractères.
/// `Summary::truncated` seul ne suffit pas : un début de ~800 caractères sur un texte de 1 307
/// n'est pas coupé par le plafond, mais s'arrête bien avant la fin. Un sommaire officiel ou un
/// dispositif a sa propre fin : sa longueur ne dit rien du texte.
fn summary_is_partial(summary: &Summary, text_length: usize) -> bool {
    summary.truncated
        || (summary.source == SummarySource::Lead && chars(&summary.text) < text_length)
}

/// Mode texte plein : la fenêtre de `max_chars` caractères à partir de `offset`. Un texte vide
/// accepte `offset = 0` (remarque) ; tout autre `offset` en deçà de la fin est lu tel quel.
fn read_window(text: &LongText, offset: usize, max_chars: usize) -> Result<Reading, AppError> {
    if text.is_empty() && offset == 0 {
        return Ok(bare(text));
    }
    let text_length = length(text);
    if offset >= text_length {
        let error = DomainError::OffsetPastEnd { offset, text_length };
        return Err(messages::domain(TOOL, &error));
    }
    let page = window(text.as_str(), offset, max_chars);
    Ok(Reading {
        text: Some(page.text.to_owned()),
        next_offset: page.next_offset.map(to_u32),
        ..bare(text)
    })
}

/// Mode passages : au plus `count` segments classés par BM25 sur les termes de `focus`, dont la
/// somme des longueurs ne dépasse pas `max_chars` (03 §7.8). Sans correspondance, `passages` est
/// vide et une remarque propose la lecture en texte plein.
#[expect(
    clippy::indexing_slicing,
    reason = "`scored.index` vient du classement de ces mêmes `segments`"
)]
fn read_passages(text: &LongText, focus: &Focus, count: usize, max_chars: usize) -> Reading {
    let segments = segments(text);
    let texts: Vec<&str> = segments.iter().map(|(_, segment)| *segment).collect();
    let ranked = rank(&texts, &focus.terms().join(" "), texts.len());
    let mut left = max_chars;
    let mut passages = Vec::new();
    for scored in ranked {
        if passages.len() == count {
            break;
        }
        let (offset, segment) = segments[scored.index];
        let kept = if chars(segment) <= left {
            segment
        } else if left >= MIN_CUT_CHARS {
            window(segment, 0, left).text
        } else {
            continue;
        };
        let kept = kept.trim_end();
        left = left.saturating_sub(chars(kept));
        passages.push(PassageOut {
            offset,
            text: kept.to_owned(),
            score: round_score(scored.score),
        });
    }
    let note = if text.is_empty() {
        Some(NO_TEXT_NOTE)
    } else if passages.is_empty() {
        Some(NO_PASSAGE_NOTE)
    } else {
        None
    };
    Reading { passages: Some(passages), note: note.map(str::to_owned), ..bare(text) }
}

/// Segments classables du texte : un par paragraphe, avec sa position en caractères ; un
/// paragraphe de plus de 1 200 caractères est découpé en fenêtres de 1 200 au plus, coupées en
/// fin de phrase (03 §7.7). La découpe avance sur le reste du paragraphe : linéaire, alors qu'un
/// `texte` brut de l'API est un seul paragraphe (D-26).
fn segments(text: &LongText) -> Vec<(u32, &str)> {
    let mut out = Vec::new();
    for (start, paragraph) in text.paragraphs() {
        let (mut at, mut rest) = (start, paragraph);
        while !rest.is_empty() {
            let page = window(rest, 0, SEGMENT_MAX_CHARS);
            out.push((at, page.text));
            at = at.saturating_add(to_u32(chars(page.text)));
            rest = &rest[page.text.len()..];
        }
    }
    out
}

/// Nombre de caractères de `text`.
fn chars(text: &str) -> usize {
    text.chars().count()
}

/// Longueur du texte canonique, en caractères.
fn length(text: &LongText) -> usize {
    usize::try_from(text.char_len()).unwrap_or(usize::MAX)
}

/// `usize` vers `u32`, saturant : les positions d'un texte (au plus 16 000 000) y tiennent.
fn to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "tests : valeurs bornées par les données du test"
)]
#[cfg(test)]
#[expect(
    clippy::as_conversions,
    reason = "tests : tailles et indices bornés par les données du test"
)]
mod tests {
    use lf_domain::text::{SUMMARY_CAP_CHARS, SummarySource, rank};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::budget::ResponseBudget;

    const NO_TEXT: &str = "No text is available from Légifrance for this record.";
    const NO_PASSAGE: &str = "No passage matches focus; read the text with detail=\"full\".";
    const SUMMARY_WITH_OFFSET: &str = "offset requires detail=\"full\" (or omit detail).";

    fn long(text: &str) -> LongText {
        LongText::from_plain(text).unwrap()
    }

    fn input(edit: impl FnOnce(&mut ReadingInput)) -> ReadingInput {
        let mut input = ReadingInput::default();
        edit(&mut input);
        input
    }

    fn request(edit: impl FnOnce(&mut ReadingInput)) -> ReadingRequest {
        ReadingRequest::parse(&input(edit)).unwrap()
    }

    fn refusal(edit: impl FnOnce(&mut ReadingInput)) -> AppError {
        ReadingRequest::parse(&input(edit)).unwrap_err()
    }

    fn invalid(message: &str) -> AppError {
        AppError::invalid_argument(message)
    }

    fn run(text: &LongText, req: &ReadingRequest) -> Result<Reading, AppError> {
        read(text, None, DispositifRule::None, req)
    }

    fn chars(text: &str) -> usize {
        text.chars().count()
    }

    /// Une phrase de 130 caractères environ, avec une abréviation juridique et des accents.
    fn sentence(i: usize) -> String {
        format!(
            "Considérant que l'article L. {i}-1 du code civil prévoit une règle précise : la \
             solution s'impose à l'égard des justiciables. "
        )
    }

    /// `count` paragraphes de `sentences` phrases chacun, séparés par une ligne vide.
    fn paragraphs(count: usize, sentences: usize) -> String {
        (0..count)
            .map(|p| {
                (0..sentences)
                    .map(|s| sentence(p * 100 + s))
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Texte d'exactement `n` caractères : `n` lettres `a`.
    fn flat(n: usize) -> LongText {
        long(&"a".repeat(n))
    }

    /// Lecture d'un article en mode résumé avec `max_chars` (4 000 par défaut).
    fn article_read(text: &LongText, max_chars: Option<i64>) -> Reading {
        let req = request(|i| i.max_chars = max_chars).for_article();
        run(text, &req).unwrap()
    }

    /// Texte SYNTHÉTIQUE (aucune donnée réelle) fait de paragraphes de 400 caractères, au total
    /// `total` caractères au plus : sa longueur est celle de l'article 757 B du CGI (1 307).
    fn blocks(total: usize) -> LongText {
        let paragraph = format!("{} {}", "Lorem", "a".repeat(394));
        let mut parts = Vec::new();
        let mut used = 0;
        while used < total {
            let room = total - used;
            let part: String = paragraph.chars().take(room).collect();
            used += chars(&part) + 2;
            parts.push(part);
        }
        long(&parts.join("\n\n"))
    }

    #[test]
    fn an_article_is_whole_up_to_2000_characters_and_summarised_beyond() {
        // 757 B du CGI : 1 307 caractères (texte synthétique de même longueur).
        let b757 = blocks(1_307);
        assert_eq!(b757.char_len(), 1_307);
        let reading = article_read(&b757, None);
        assert_eq!(reading.text.as_deref(), Some(b757.as_str()));
        assert_eq!((reading.summary, reading.summary_truncated), (None, false));
        // Les bornes : 2 000 entier, 2 001 résumé coupé.
        let edge = article_read(&flat(ARTICLE_WHOLE_CHARS), None);
        assert_eq!(edge.text.as_deref(), Some("a".repeat(2_000).as_str()));
        assert_eq!((edge.summary, edge.summary_truncated), (None, false));
        let over = article_read(&flat(ARTICLE_WHOLE_CHARS + 1), None);
        assert_eq!(over.text, None);
        assert!(over.summary.is_some() && over.summary_truncated);
        // Résumé d'un article de 2 001 caractères : toujours plafonné à 1 200.
        let size = chars(&over.summary.unwrap().text);
        assert_eq!(size, SUMMARY_CAP_CHARS);
    }

    #[test]
    fn a_small_max_chars_keeps_the_min_rule_for_articles() {
        // max_chars < 2 000 : le plafond est max_chars (règle min(max_chars, 2 000)).
        let whole = article_read(&flat(1_500), Some(1_500));
        assert_eq!(whole.text.as_deref(), Some("a".repeat(1_500).as_str()));
        assert!(!whole.summary_truncated);
        let cut = article_read(&flat(1_501), Some(1_500));
        assert_eq!(cut.text, None);
        assert!(cut.summary_truncated);
        assert_eq!(chars(&cut.summary.unwrap().text), SUMMARY_CAP_CHARS);
        let min = article_read(&flat(501), Some(500));
        assert_eq!(min.text, None);
        assert_eq!(chars(&min.summary.unwrap().text), 500);
        // max_chars plus grand que 2 000 : 2 000 reste le plafond.
        assert_eq!(article_read(&flat(2_001), Some(40_000)).text, None);
        assert!(article_read(&flat(2_000), Some(40_000)).text.is_some());
    }

    #[test]
    fn the_whole_cap_applies_to_articles_only() {
        let text = flat(1_500);
        let plain = request(|_| {});
        assert_eq!(run(&text, &plain).unwrap().text, None);
        assert!(run(&text, &plain.clone().for_article()).unwrap().text.is_some());
        // Les autres modes ignorent le plafond de l'article.
        let full = request(|i| i.detail = Some("full".to_owned())).for_article();
        assert!(!run(&text, &full).unwrap().summary_truncated);
    }

    #[test]
    fn a_lead_that_stops_short_of_its_source_is_flagged_even_when_the_cut_is_clean() {
        // 757 B : 1 307 caractères, lus comme une décision (plafond 1 200) : le début s'arrête
        // après 2 paragraphes (~800), `cut()` ne coupe rien, mais le texte continue.
        let text = blocks(1_307);
        let reading = run(&text, &request(|_| {})).unwrap();
        let summary = reading.summary.as_ref().unwrap();
        assert_eq!(summary.source, SummarySource::Lead);
        assert!(!summary.truncated, "le domaine ne dit rien : c'est le cas du piège");
        assert!(chars(&summary.text) < usize::try_from(text.char_len()).unwrap());
        assert!(reading.summary_truncated);
        // Même chose pour un article de 2 400 caractères.
        let article = article_read(&blocks(2_400), None);
        assert!(!article.summary.as_ref().unwrap().truncated);
        assert!(article.summary_truncated);
    }

    #[test]
    fn summary_partiality_follows_the_source() {
        let make =
            |source, truncated, text: &str| Summary { source, text: text.to_owned(), truncated };
        // Début de texte : partiel dès qu'il est plus court que le texte ; complet sinon.
        assert!(summary_is_partial(&make(SummarySource::Lead, false, "abc"), 4));
        assert!(!summary_is_partial(&make(SummarySource::Lead, false, "abc"), 3));
        assert!(summary_is_partial(&make(SummarySource::Lead, true, "abc"), 3));
        // Sommaire officiel et dispositif : leur longueur ne dit rien, seule la coupe compte.
        for source in [SummarySource::Official, SummarySource::Dispositif] {
            assert!(!summary_is_partial(&make(source, false, "abc"), 9_999));
            assert!(summary_is_partial(&make(source, true, "abc"), 9_999));
        }
    }

    #[test]
    fn official_and_dispositif_summaries_are_flagged_only_when_cut() {
        let text = long(&paragraphs(30, 4));
        let cut_official = OfficialSummary::new(["Mot ".repeat(400)], Vec::<String>::new());
        let flagged =
            read(&text, Some(&cut_official), DispositifRule::None, &request(|_| {})).unwrap();
        assert_eq!(flagged.summary.as_ref().map(|s| s.source), Some(SummarySource::Official));
        assert!(flagged.summary_truncated);
        let short_official =
            OfficialSummary::new(["Analyse officielle courte."], Vec::<String>::new());
        let kept =
            read(&text, Some(&short_official), DispositifRule::None, &request(|_| {})).unwrap();
        assert_eq!(kept.summary.as_ref().map(|s| s.source), Some(SummarySource::Official));
        assert!(!kept.summary_truncated);
        // Dispositif non coupé : un texte court après l'en-tête, la clôture arrête le dispositif.
        let decision = long(&format!(
            "{}\n\nDÉCIDE :\n\nArticle 1er.- Le texte est conforme.\n\nDélibéré par le Conseil.",
            paragraphs(20, 4)
        ));
        let dispositif =
            read(&decision, None, DispositifRule::ConseilConstitutionnel, &request(|_| {}))
                .unwrap();
        assert_eq!(dispositif.summary.as_ref().map(|s| s.source), Some(SummarySource::Dispositif));
        assert!(!dispositif.summary_truncated);
        // Dispositif coupé au plafond.
        let long_dispositif = long(&format!(
            "{}\n\nDÉCIDE :\n\n{}\n\nDélibéré par le Conseil.",
            paragraphs(20, 4),
            paragraphs(10, 4)
        ));
        let cut =
            read(&long_dispositif, None, DispositifRule::ConseilConstitutionnel, &request(|_| {}))
                .unwrap();
        assert_eq!(cut.summary.as_ref().map(|s| s.source), Some(SummarySource::Dispositif));
        assert!(cut.summary_truncated);
    }

    #[test]
    fn modes_without_a_summary_never_flag_one() {
        let text = long(&paragraphs(30, 4));
        let full = run(&text, &request(|i| i.detail = Some("full".to_owned()))).unwrap();
        assert!(!full.summary_truncated);
        let focused = run(&text, &request(|i| i.focus = Some("solution".to_owned()))).unwrap();
        assert!(!focused.summary_truncated);
        assert!(!run(&long(""), &request(|_| {})).unwrap().summary_truncated);
    }

    #[test]
    fn reading_compares_by_value() {
        let reading = Reading {
            summary: None,
            text: Some("texte".to_owned()),
            text_length: 5,
            next_offset: None,
            passages: Some(vec![PassageOut { offset: 0, text: "texte".to_owned(), score: 1.0 }]),
            note: None,
            summary_truncated: false,
        };
        assert_eq!(reading.clone(), reading);
        assert_ne!(Reading { note: Some("vide".to_owned()), ..reading.clone() }, reading);
    }

    #[test]
    fn reading_request_modes() {
        let default = request(|_| {});
        assert_eq!(
            default,
            ReadingRequest { mode: Mode::Summary, max_chars: 4_000, whole_cap: SUMMARY_CAP_CHARS }
        );
        assert_eq!(default.max_chars, ResponseBudget::MAX_CHARS_DEFAULT);

        let summary = request(|i| i.detail = Some("summary".to_owned()));
        assert_eq!(summary.mode, Mode::Summary);

        for spelling in ["full", "FULL", "Full"] {
            let full = request(|i| i.detail = Some(spelling.to_owned()));
            assert_eq!(full.mode, Mode::Full { offset: 0 }, "detail={spelling}");
        }
        for spelling in ["summary", "SUMMARY", "Summary"] {
            let summary = request(|i| i.detail = Some(spelling.to_owned()));
            assert_eq!(summary.mode, Mode::Summary, "detail={spelling}");
        }

        let paged = request(|i| {
            i.detail = Some("full".to_owned());
            i.offset = Some(120);
            i.max_chars = Some(800);
        });
        assert_eq!(
            paged,
            ReadingRequest {
                mode: Mode::Full { offset: 120 },
                max_chars: 800,
                whole_cap: SUMMARY_CAP_CHARS
            }
        );

        let focused = request(|i| i.focus = Some("  zibeline ".to_owned()));
        let focus = Focus::parse("zibeline").unwrap();
        assert_eq!(focused.mode, Mode::Passages { focus, count: 3 });
        assert_eq!(focused.max_chars, 4_000);
    }

    #[test]
    fn unknown_detail_is_invalid_arg() {
        assert_eq!(
            refusal(|i| i.detail = Some("long".to_owned())),
            invalid("detail='long' must be one of ['full', 'summary'].")
        );
        assert_eq!(
            refusal(|i| i.detail = Some(String::new())),
            invalid("detail='' must be one of ['full', 'summary'].")
        );
    }

    #[test]
    fn offset_without_detail_means_full() {
        assert_eq!(request(|i| i.offset = Some(1)).mode, Mode::Full { offset: 1 });
        assert_eq!(request(|i| i.offset = Some(9_999)).mode, Mode::Full { offset: 9_999 });
        // offset = 0 n'est pas un offset « fourni » : le résumé reste le défaut.
        assert_eq!(request(|i| i.offset = Some(0)).mode, Mode::Summary);
        assert_eq!(request(|i| i.offset = None).mode, Mode::Summary);
        // detail="full" explicite garde son offset, nul ou non.
        let explicit = request(|i| {
            i.detail = Some("full".to_owned());
            i.offset = Some(0);
        });
        assert_eq!(explicit.mode, Mode::Full { offset: 0 });
    }

    #[test]
    fn detail_summary_with_offset_is_invalid_arg() {
        for spelling in ["summary", "SUMMARY"] {
            assert_eq!(
                refusal(|i| {
                    i.detail = Some(spelling.to_owned());
                    i.offset = Some(1);
                }),
                invalid(SUMMARY_WITH_OFFSET)
            );
        }
        // Un offset nul est compatible avec le résumé.
        let zero = request(|i| {
            i.detail = Some("summary".to_owned());
            i.offset = Some(0);
        });
        assert_eq!(zero.mode, Mode::Summary);
        // Un offset négatif est refusé avant la combinaison (ordre de validation de 05 §5.0.2).
        assert_eq!(
            refusal(|i| {
                i.detail = Some("summary".to_owned());
                i.offset = Some(-1);
            }),
            invalid("offset=-1 out of range (0..4294967295).")
        );
        assert_eq!(
            refusal(|i| i.offset = Some(i64::MIN)),
            invalid(&format!("offset={} out of range (0..4294967295).", i64::MIN))
        );
        // La borne annoncée est réelle : u32::MAX est accepté, au-delà l'offset est refusé.
        assert_eq!(
            request(|i| i.offset = Some(i64::from(u32::MAX))).mode,
            Mode::Full { offset: usize::try_from(u32::MAX).unwrap() }
        );
        for over in [i64::from(u32::MAX) + 1, i64::MAX] {
            assert_eq!(
                refusal(|i| i.offset = Some(over)),
                invalid(&format!("offset={over} out of range (0..4294967295)."))
            );
        }
        // `focus` l'emporte : l'offset est ignoré, donc sans conflit avec le résumé.
        let focused = request(|i| {
            i.detail = Some("summary".to_owned());
            i.offset = Some(50);
            i.focus = Some("zibeline".to_owned());
        });
        let focus = Focus::parse("zibeline").unwrap();
        assert_eq!(focused.mode, Mode::Passages { focus, count: 3 });
    }

    #[test]
    fn validation_follows_the_reference_order() {
        // detail, puis offset, puis max_chars, puis focus, puis passages.
        let all_wrong = |edit: fn(&mut ReadingInput)| {
            refusal(|i| {
                i.detail = Some("long".to_owned());
                i.offset = Some(-1);
                i.max_chars = Some(1);
                i.focus = Some(" ".to_owned());
                i.passages = Some(0);
                edit(i);
            })
        };
        assert_eq!(all_wrong(|_| {}), invalid("detail='long' must be one of ['full', 'summary']."));
        assert_eq!(
            all_wrong(|i| i.detail = None),
            invalid("max_chars=1 out of range (500..40000).")
        );
        assert_eq!(
            all_wrong(|i| {
                i.detail = None;
                i.max_chars = None;
            }),
            invalid(
                "focus contains no searchable word (only stop words or punctuation); give a few \
                 meaningful words or omit focus."
            )
        );
        assert_eq!(
            all_wrong(|i| {
                i.detail = None;
                i.max_chars = None;
                i.focus = Some("zibeline".to_owned());
            }),
            invalid("passages=0 out of range (1..10).")
        );
        // Sans focus, l'offset négatif est lu avant max_chars.
        assert_eq!(
            refusal(|i| {
                i.offset = Some(-3);
                i.max_chars = Some(1);
            }),
            invalid("offset=-3 out of range (0..4294967295).")
        );
    }

    #[test]
    fn max_chars_bounds_500_40000() {
        for accepted in [500, 501, 4_000, 39_999, 40_000] {
            let req = request(|i| i.max_chars = Some(accepted));
            assert_eq!(req.max_chars, usize::try_from(accepted).unwrap());
        }
        for (rejected, shown) in [
            (499, "499"),
            (0, "0"),
            (-1, "-1"),
            (40_001, "40001"),
            (i64::MAX, "9223372036854775807"),
        ] {
            assert_eq!(
                refusal(|i| i.max_chars = Some(rejected)),
                invalid(&format!("max_chars={shown} out of range (500..40000)."))
            );
        }
        // Les bornes valent pour les trois modes.
        let full = request(|i| {
            i.detail = Some("full".to_owned());
            i.max_chars = Some(40_000);
        });
        assert_eq!(full.max_chars, 40_000);
        let focused = request(|i| {
            i.focus = Some("zibeline".to_owned());
            i.max_chars = Some(500);
        });
        assert_eq!(focused.max_chars, 500);
        assert_eq!(
            refusal(|i| {
                i.focus = Some("zibeline".to_owned());
                i.max_chars = Some(499);
            }),
            invalid("max_chars=499 out of range (500..40000).")
        );
    }

    #[test]
    fn focus_without_a_word_is_invalid_arg() {
        for blank in ["", " ", " \t\n "] {
            assert_eq!(
                refusal(|i| i.focus = Some(blank.to_owned())),
                invalid(
                    "focus contains no searchable word (only stop words or punctuation); give a \
                     few meaningful words or omit focus."
                )
            );
        }
    }

    #[test]
    fn a_focus_over_500_characters_gives_its_length_not_a_value() {
        // L4-09 (protocol-13) : « focus is 501 characters long », pas « focus=501 out of range ».
        for length in [501, 1_396] {
            assert_eq!(
                refusal(|i| i.focus = Some("a".repeat(length))),
                invalid(&format!("focus is {length} characters long (max 500); shorten it."))
            );
        }
        // Jumeau : 500 caractères exactement sont acceptés.
        let accepted = request(|i| i.focus = Some("a".repeat(500)));
        assert_eq!(
            accepted.mode,
            Mode::Passages { focus: Focus::parse(&"a".repeat(500)).unwrap(), count: 3 }
        );
    }

    #[test]
    fn passages_ignored_without_focus() {
        let default = request(|_| {});
        for count in [-4, 0, 3, 11, 99] {
            assert_eq!(request(|i| i.passages = Some(count)), default, "passages={count}");
        }
        let full = request(|i| {
            i.detail = Some("full".to_owned());
            i.passages = Some(99);
        });
        assert_eq!(full.mode, Mode::Full { offset: 0 });
        // Avec focus, le nombre de passages est borné de 1 à 10, 3 par défaut.
        let mode = |value: Option<i64>| {
            request(|i| {
                i.focus = Some("zibeline".to_owned());
                i.passages = value;
            })
            .mode
        };
        let passages =
            |count: usize| Mode::Passages { focus: Focus::parse("zibeline").unwrap(), count };
        assert_eq!(mode(None), passages(3));
        assert_eq!(mode(Some(1)), passages(1));
        assert_eq!(mode(Some(10)), passages(10));
        for (rejected, shown) in [(0, "0"), (11, "11"), (-1, "-1")] {
            assert_eq!(
                refusal(|i| {
                    i.focus = Some("zibeline".to_owned());
                    i.passages = Some(rejected);
                }),
                invalid(&format!("passages={shown} out of range (1..10)."))
            );
        }
    }

    #[test]
    fn focus_ignores_offset_and_detail() {
        let plain = request(|i| i.focus = Some("zibeline".to_owned()));
        let loaded = request(|i| {
            i.focus = Some("zibeline".to_owned());
            i.offset = Some(50);
            i.detail = Some("full".to_owned());
        });
        assert_eq!(loaded, plain);
        // Même un offset négatif ou démesuré est ignoré avec un focus.
        for offset in [i64::MIN, -1, i64::MAX] {
            let ignored = request(|i| {
                i.focus = Some("zibeline".to_owned());
                i.offset = Some(offset);
            });
            assert_eq!(ignored, plain, "offset={offset}");
        }
    }

    #[test]
    fn offset_past_end_is_invalid_arg() {
        let text = flat(100);
        let at = |offset: i64| {
            run(
                &text,
                &request(|i| {
                    i.detail = Some("full".to_owned());
                    i.offset = Some(offset);
                }),
            )
        };
        let past = |offset: usize, length: usize| {
            Err(invalid(&format!(
                "offset={offset} is at or past the end of the text (text_length={length}); restart \
                 from offset=0 or omit offset."
            )))
        };
        assert_eq!(at(100), past(100, 100));
        assert_eq!(at(101), past(101, 100));
        assert_eq!(at(i64::from(u32::MAX)), past(usize::try_from(u32::MAX).unwrap(), 100));
        // Au-delà de la borne annoncée (u32::MAX), l'offset est refusé comme hors limites.
        assert_eq!(
            refusal(|i| i.offset = Some(i64::MAX)),
            invalid(&format!("offset={} out of range (0..4294967295).", i64::MAX))
        );
        // Le dernier caractère reste lisible.
        let last = at(99).unwrap();
        assert_eq!(last.text.as_deref(), Some("a"));
        assert_eq!(last.next_offset, None);
        // Sur un texte vide, tout offset non nul est au-delà de la fin.
        let empty = long("");
        let empty_at = |offset: i64| run(&empty, &request(|i| i.offset = Some(offset)));
        assert_eq!(empty_at(1), past(1, 0));
        // Une fenêtre sans offset ne dépasse jamais.
        assert!(run(&text, &request(|i| i.detail = Some("full".to_owned()))).is_ok());
    }

    #[test]
    fn empty_text_offset_zero_gives_note() {
        let empty = long("");
        assert_eq!(empty.char_len(), 0);
        let expected = |passages: Option<Vec<PassageOut>>| Reading {
            summary: None,
            text: None,
            text_length: 0,
            next_offset: None,
            passages,
            note: Some(NO_TEXT.to_owned()),
            summary_truncated: false,
        };
        // Résumé (défaut), texte plein et passages : la même remarque, jamais d'erreur.
        assert_eq!(run(&empty, &request(|_| {})).unwrap(), expected(None));
        let full = request(|i| {
            i.detail = Some("full".to_owned());
            i.offset = Some(0);
        });
        assert_eq!(run(&empty, &full).unwrap(), expected(None));
        let focused = request(|i| i.focus = Some("zibeline".to_owned()));
        assert_eq!(run(&empty, &focused).unwrap(), expected(Some(Vec::new())));
        // Un texte vide ne signale la remarque que lui : un texte d'un caractère n'en a pas.
        assert_eq!(run(&flat(1), &request(|_| {})).unwrap().note, None);
    }

    #[test]
    fn short_text_returned_whole() {
        let whole = |text: &LongText, req: &ReadingRequest| {
            let reading = run(text, req).unwrap();
            assert_eq!(reading.text.as_deref(), Some(text.as_str()));
            assert_eq!(reading.summary, None);
            assert_eq!(reading.next_offset, None);
            assert_eq!(reading.passages, None);
            assert_eq!(reading.note, None);
            assert_eq!(reading.text_length, text.char_len());
        };
        let summary_with = |max_chars: i64| request(|i| i.max_chars = Some(max_chars));
        // Plafond du mode résumé : min(max_chars, 1 200).
        whole(&flat(1), &summary_with(4_000));
        whole(&flat(SUMMARY_CAP_CHARS), &summary_with(4_000));
        whole(&flat(500), &summary_with(500));
        whole(&flat(700), &summary_with(1_000));
        // Une seule fois que le texte dépasse le plafond, on bascule en résumé.
        let over = run(&flat(SUMMARY_CAP_CHARS + 1), &summary_with(4_000)).unwrap();
        assert_eq!((over.text, over.summary.is_some()), (None, true));
        let over = run(&flat(501), &summary_with(500)).unwrap();
        assert_eq!((over.text, over.summary.is_some()), (None, true));
        let over = run(&flat(701), &summary_with(700)).unwrap();
        assert_eq!((over.text, over.summary.is_some()), (None, true));
        // Texte plein : un texte qui tient dans max_chars part en entier, sans next_offset.
        let full = |max_chars: i64| {
            request(|i| {
                i.detail = Some("full".to_owned());
                i.max_chars = Some(max_chars);
            })
        };
        whole(&flat(500), &full(500));
        whole(&flat(2_000), &full(2_000));
        let paged = run(&flat(501), &full(500)).unwrap();
        assert_eq!(chars(paged.text.as_deref().unwrap()), 500);
        assert_eq!(paged.next_offset, Some(500));
    }

    #[test]
    fn summary_mode_shape_is_algorithm_independent() {
        let text = long(&paragraphs(30, 4));
        assert!(text.char_len() > 10_000);
        let official = OfficialSummary::new(["Analyse officielle de la décision."], ["TITRAGE"]);
        let rules = [
            DispositifRule::None,
            DispositifRule::ConseilConstitutionnel,
            DispositifRule::ConseilEtat,
        ];
        for max_chars in [500_i64, 777, 1_199, 1_200, 1_201, 4_000, 40_000] {
            let cap = usize::try_from(max_chars).unwrap().min(SUMMARY_CAP_CHARS);
            for official in [None, Some(&official)] {
                for rule in rules {
                    let req = request(|i| i.max_chars = Some(max_chars));
                    let reading = read(&text, official, rule, &req).unwrap();
                    let summary = reading.summary.as_ref().expect("un texte long a un résumé");
                    assert!(
                        ["official", "dispositif", "lead"].contains(&summary.source.as_str()),
                        "source {:?}",
                        summary.source
                    );
                    assert_ne!(summary.text, "");
                    let used = chars(&summary.text);
                    assert!(used <= cap, "max_chars={max_chars} : {used} caractères");
                    // Le texte, les passages et les pages n'accompagnent pas le résumé.
                    assert_eq!(reading.text, None);
                    assert_eq!(reading.next_offset, None);
                    assert_eq!(reading.passages, None);
                    assert_eq!(reading.note, None);
                    assert_eq!(reading.text_length, text.char_len());
                }
            }
        }
    }

    fn is_official(summary: &Summary) -> bool {
        summary.source == SummarySource::Official
    }

    #[test]
    fn short_text_keeps_only_an_official_summary() {
        let lead = Summary { source: SummarySource::Lead, text: String::new(), truncated: false };
        let official_summary = Summary { source: SummarySource::Official, ..lead.clone() };
        assert!(is_official(&official_summary) && !is_official(&lead));
        let text = long("Un court texte, rendu en entier.");
        let official =
            OfficialSummary::new(["Analyse officielle de la décision."], Vec::<String>::new());
        let req = request(|_| {});
        for rule in [DispositifRule::None, DispositifRule::ConseilEtat] {
            for official in [None, Some(&official)] {
                let reading = read(&text, official, rule, &req).unwrap();
                assert_eq!(reading.text.as_deref(), Some(text.as_str()));
                // Un résumé n'accompagne le texte entier que s'il est officiel (05 §6.2).
                assert!(reading.summary.as_ref().is_none_or(is_official));
                // Et il l'accompagne bien dès qu'un sommaire officiel existe.
                assert_eq!(reading.summary.is_some(), official.is_some());
            }
        }
        // Sans sommaire officiel, le début du texte n'est jamais répété.
        assert_eq!(read(&text, None, DispositifRule::None, &req).unwrap().summary, None);
        // D-07 : un sommaire officiel trivial (« X ») n'en est pas un, le texte entier est rendu
        // seul ; jumeau ci-dessus : une analyse assez longue l'accompagne.
        let trivial = OfficialSummary::new(["X"], Vec::<String>::new());
        let reading = read(&text, Some(&trivial), DispositifRule::None, &req).unwrap();
        assert_eq!((reading.text.as_deref(), reading.summary), (Some(text.as_str()), None));
    }

    #[test]
    fn full_mode_does_not_summarise() {
        let text = long(&paragraphs(5, 3));
        let official = OfficialSummary::new(["Analyse officielle."], Vec::<String>::new());
        let req = request(|i| i.detail = Some("full".to_owned()));
        let reading = read(&text, Some(&official), DispositifRule::ConseilEtat, &req).unwrap();
        assert_eq!(reading.summary, None);
        assert!(reading.text.is_some());
    }

    #[test]
    fn paged_reading_reconstructs_full_text() {
        let accents = long(&paragraphs(12, 6));
        let exotic = long(&format!(
            "{}\n\n{}\n\n{}",
            "日本語のテキスト。".repeat(200),
            "Émoji 😀 et œuvre : ".repeat(150),
            "fin.".repeat(300)
        ));
        let one_paragraph = long(&sentence(7).repeat(80));
        // Un barème : une ligne par paragraphe, cellules séparées par « | » (L4-01).
        let row = "<tr><td><p>Comprise entre N 000 € et N 999 € </p></td><td><br/>N <br/></td><td></td></tr>";
        let rows: Vec<String> = (0..60).map(|i| row.replace('N', &i.to_string())).collect();
        let rows = rows.concat();
        let table = LongText::from_html(&format!("<p>Tableau I</p><table>{rows}</table>")).unwrap();
        assert!(table.as_str().contains("Comprise entre 7 000 € et 7 999 € | 7\n\n"));
        for text in [&accents, &exotic, &one_paragraph, &table] {
            for max_chars in [500_i64, 777, 1_200, 4_000, 40_000] {
                let max = usize::try_from(max_chars).unwrap();
                let mut rebuilt = String::new();
                let mut offset = 0_u32;
                let mut pages = 0;
                loop {
                    let req = request(|i| {
                        i.detail = Some("full".to_owned());
                        i.offset = Some(i64::from(offset));
                        i.max_chars = Some(max_chars);
                    });
                    let page = run(text, &req).unwrap();
                    let body = page.text.expect("une page a du texte");
                    let size = chars(&body);
                    assert!(size <= max, "page de {size} caractères > {max}");
                    assert_ne!(body, "");
                    assert_eq!(page.text_length, text.char_len());
                    assert_eq!((page.summary, page.passages, page.note), (None, None, None));
                    rebuilt.push_str(&body);
                    pages += 1;
                    assert!(pages <= 2_000, "pas de progression");
                    let width = u32::try_from(chars(&body)).unwrap();
                    let Some(next) = page.next_offset else {
                        assert_eq!(offset + width, text.char_len());
                        break;
                    };
                    // next_offset est la position exacte de la fin de la page : G3.
                    assert_eq!(next, offset + width);
                    offset = next;
                }
                assert_eq!(rebuilt, text.as_str(), "max_chars={max_chars}");
            }
        }
    }

    #[test]
    fn a_page_can_start_anywhere() {
        // Un offset arbitraire est respecté tel quel (03 §7.6, étape 2), accents compris.
        let text = long("Élève œuvré à l'école : été.");
        let req = request(|i| {
            i.detail = Some("full".to_owned());
            i.offset = Some(6);
        });
        let page = run(&text, &req).unwrap();
        assert_eq!(page.text.as_deref(), Some("œuvré à l'école : été."));
        assert_eq!(page.text_length, 28);
    }

    /// Texte à neuf paragraphes dont certains portent le mot rare « zibeline ».
    fn zibeline_text() -> (LongText, Vec<(u32, String)>) {
        let paras = [
            "Le droit civil règle les rapports entre les personnes.",
            "La zibeline zibeline zibeline est un petit animal.",
            "Les contrats se forment par l'échange des consentements.",
            "La zibeline vit dans les forêts boréales de Sibérie où elle chasse de nuit comme de jour.",
            "La responsabilité suppose une faute, un dommage et un lien de causalité.",
            "Les successions s'ouvrent par le décès.",
        ];
        let text = long(&paras.join("\n\n"));
        let starts = text.paragraphs().map(|(at, para)| (at, para.to_owned())).collect();
        (text, starts)
    }

    fn focused(text: &LongText, words: &str, count: i64, max_chars: i64) -> Reading {
        let req = request(|i| {
            i.focus = Some(words.to_owned());
            i.passages = Some(count);
            i.max_chars = Some(max_chars);
        });
        run(text, &req).unwrap()
    }

    #[test]
    fn focus_returns_ranked_passages_with_offsets() {
        let (text, paras) = zibeline_text();
        let reading = focused(&text, "zibeline", 3, 4_000);
        assert_eq!(reading.summary, None);
        assert_eq!(reading.text, None);
        assert_eq!(reading.next_offset, None);
        assert_eq!(reading.note, None);
        assert_eq!(reading.text_length, text.char_len());
        // La première occurrence (trois fois, paragraphe court) devant la seconde.
        let expected = vec![(paras[1].0, paras[1].1.clone()), (paras[3].0, paras[3].1.clone())];
        let seen = reading.passages.as_ref().unwrap();
        let seen: Vec<(u32, String)> = seen.iter().map(|p| (p.offset, p.text.clone())).collect();
        assert_eq!(seen, expected);
        // Chaque offset est utilisable comme offset de fenêtre (05 §5.0.3).
        for passage in reading.passages.unwrap() {
            let from: String = text.as_str().chars().skip(passage.offset as usize).collect();
            assert!(from.starts_with(&passage.text));
        }
    }

    #[test]
    fn passages_carry_their_bm25_score() {
        // D-02 (get_text-6) : le score du classement n'est plus jeté ; les passages restent triés du
        // mieux noté au moins bon, chaque score est strictement positif.
        let (text, paras) = zibeline_text();
        let reading = focused(&text, "zibeline", 3, 4_000);
        let passages = reading.passages.unwrap();
        let offsets: Vec<u32> = passages.iter().map(|passage| passage.offset).collect();
        assert_eq!(offsets, [paras[1].0, paras[3].0]);
        assert!(passages[0].score > passages[1].score, "{passages:?}");
        assert!(passages.iter().all(|passage| passage.score > 0.0), "{passages:?}");
    }

    #[test]
    fn a_passage_score_is_rounded_to_two_decimals() {
        // Le score exposé est le score BM25 du domaine arrondi à 2 décimales, jamais le brut.
        let (text, _) = zibeline_text();
        let segments: Vec<&str> = text.paragraphs().map(|(_, para)| para).collect();
        let raw = rank(&segments, "zibeline", segments.len());
        let reading = focused(&text, "zibeline", 3, 4_000);
        let passages = reading.passages.unwrap();
        assert_eq!(passages.len(), 2);
        for (passage, scored) in passages.iter().zip(&raw) {
            let hundredths = passage.score * 100.0;
            assert!((hundredths - hundredths.round()).abs() < 1e-9, "{}", passage.score);
            assert!((passage.score - (scored.score * 100.0).round() / 100.0).abs() < 1e-12);
            // Le brut a plus de 2 décimales : l'arrondi est bien appliqué.
            assert!((scored.score - passage.score).abs() > 1e-9, "{}", scored.score);
        }
    }

    #[test]
    fn a_cut_passage_keeps_the_score_of_its_segment() {
        // Jumeau : un passage coupé au budget garde le score de son segment entier.
        let body = "La zibeline vit dans les forêts boréales. ".repeat(20);
        let text = long(&format!("Préambule sans rapport.\n\n{body}"));
        let whole = focused(&text, "zibeline", 1, 4_000).passages.unwrap();
        let cut = focused(&text, "zibeline", 1, 500).passages.unwrap();
        assert!(cut[0].text.chars().count() < whole[0].text.chars().count());
        assert_eq!(cut[0].offset, whole[0].offset);
        assert!((cut[0].score - whole[0].score).abs() < 1e-12);
    }

    #[test]
    fn focus_returns_at_most_the_requested_passages() {
        let (text, paras) = zibeline_text();
        let reading = focused(&text, "zibeline", 1, 4_000);
        let passages = reading.passages.unwrap();
        assert_eq!(passages.len(), 1);
        assert_eq!((passages[0].offset, &passages[0].text), (paras[1].0, &paras[1].1));
    }

    #[test]
    fn focus_without_a_match_gives_a_note() {
        let (text, _) = zibeline_text();
        let reading = focused(&text, "inexistantterme", 3, 4_000);
        assert_eq!(
            reading,
            Reading {
                summary: None,
                text: None,
                text_length: text.char_len(),
                next_offset: None,
                passages: Some(Vec::new()),
                note: Some(NO_PASSAGE.to_owned()),
                summary_truncated: false,
            }
        );
    }

    #[test]
    fn focus_ignores_the_offset_even_past_the_end() {
        let (text, _) = zibeline_text();
        let req = request(|i| {
            i.focus = Some("zibeline".to_owned());
            i.offset = Some(1_000_000);
        });
        assert!(run(&text, &req).unwrap().passages.is_some_and(|p| p.len() == 2));
    }

    /// `n` caractères d'une phrase « zibeline », terminée par un point.
    fn zibeline_paragraph(repeats: usize, filler: usize) -> String {
        format!("{}{}.", "zibeline ".repeat(repeats), "mot ".repeat(filler).trim_end())
    }

    /// Texte de trois paragraphes classés dans l'ordre pour « zibeline », puis un hors sujet.
    fn ranked_text(paragraphs: [&str; 3]) -> LongText {
        let text = long(&format!("{}\n\nautre sujet", paragraphs.join("\n\n")));
        let segments: Vec<&str> = text.paragraphs().map(|(_, p)| p).collect();
        let order: Vec<usize> = rank(&segments, "zibeline", 10).iter().map(|s| s.index).collect();
        assert_eq!(order, [0, 1, 2], "prémisse du test : ordre de classement");
        text
    }

    #[test]
    fn focus_cuts_a_passage_that_overflows_when_200_chars_remain() {
        let first = zibeline_paragraph(8, 55);
        let second = zibeline_paragraph(6, 55);
        let third = zibeline_paragraph(1, 5);
        let left = 500 - chars(&first);
        assert!(left >= 200 && chars(&second) > left, "prémisse du test : {left} restants");
        let reading = focused(&ranked_text([&first, &second, &third]), "zibeline", 10, 500);
        let got = reading.passages.unwrap();
        // Le premier tient, le second est coupé pour remplir le reste, le troisième est sans place.
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].text, first);
        assert!(!got[1].text.is_empty() && chars(&got[1].text) <= left);
        assert!(second.starts_with(&got[1].text));
        assert!(got.iter().map(|p| chars(&p.text)).sum::<usize>() <= 500);
    }

    #[test]
    fn focus_skips_a_passage_when_less_than_200_chars_remain() {
        let first = zibeline_paragraph(30, 22);
        let second = zibeline_paragraph(8, 25);
        let third = zibeline_paragraph(1, 5);
        let left = 500 - chars(&first);
        assert!(left < 200 && chars(&second) > left && chars(&third) <= left, "prémisse : {left}");
        let reading = focused(&ranked_text([&first, &second, &third]), "zibeline", 10, 500);
        // Le second ne tient pas et il reste moins de 200 caractères : il est écarté ; le
        // troisième, plus court, tient.
        let texts: Vec<String> = reading.passages.unwrap().into_iter().map(|p| p.text).collect();
        assert_eq!(texts, [first, third]);
    }

    #[test]
    fn focus_cut_threshold_is_exactly_200_remaining_chars() {
        let second = zibeline_paragraph(6, 55);
        let third = zibeline_paragraph(1, 5);
        // Premier passage de 300 caractères : il en reste 200, le second est coupé.
        let exact = zibeline_paragraph(8, 57);
        assert_eq!(chars(&exact), 300);
        let reading = focused(&ranked_text([&exact, &second, &third]), "zibeline", 10, 500);
        let got = reading.passages.unwrap();
        assert_eq!(got.len(), 2);
        assert!(!got[1].text.is_empty() && second.starts_with(&got[1].text));
        // Premier passage de 301 caractères : il en reste 199, le second est écarté.
        let short = zibeline_paragraph(9, 55);
        assert_eq!(chars(&short), 301);
        let reading = focused(&ranked_text([&short, &second, &third]), "zibeline", 10, 500);
        let texts: Vec<String> = reading.passages.unwrap().into_iter().map(|p| p.text).collect();
        assert_eq!(texts, [short, third]);
    }

    #[test]
    fn focus_keeps_a_passage_that_exactly_fills_the_budget() {
        let first = zibeline_paragraph(8, 55);
        let second = zibeline_paragraph(6, 55);
        let third = zibeline_paragraph(1, 5);
        let max_chars = chars(&first) + chars(&second);
        assert!(max_chars >= 500, "prémisse du test");
        let max_chars = i64::try_from(max_chars).unwrap();
        let reading = focused(&ranked_text([&first, &second, &third]), "zibeline", 10, max_chars);
        let texts: Vec<String> = reading.passages.unwrap().into_iter().map(|p| p.text).collect();
        assert_eq!(texts, [first, second]);
    }

    #[test]
    fn focus_cuts_a_first_passage_longer_than_max_chars() {
        let big = zibeline_paragraph(3, 400);
        assert!(chars(&big) > 1_200);
        let text = long(&format!("{big}\n\nautre sujet"));
        let reading = focused(&text, "zibeline", 3, 500);
        let got = reading.passages.unwrap();
        assert_ne!(got.len(), 0);
        let total: usize = got.iter().map(|p| chars(&p.text)).sum();
        assert!(total <= 500 && total > 0, "{total} caractères");
        assert_eq!(got[0].offset, 0);
        assert!(big.starts_with(&got[0].text));
    }

    #[test]
    fn a_long_paragraph_is_split_into_segments_of_at_most_1200_chars() {
        // Un seul paragraphe de plus de 1 200 caractères (le `texte` de l'API n'a pas de saut
        // de ligne, D-26) : le mot rare n'est que dans la dernière partie.
        let body = sentence(1).repeat(40);
        let text = long(&format!("{body}La zibeline est rare. {}", sentence(2).repeat(3)));
        assert_eq!(text.paragraphs().count(), 1);
        let reading = focused(&text, "zibeline", 3, 4_000);
        let got = reading.passages.unwrap();
        assert_eq!(got.len(), 1);
        assert!(got[0].offset > 0, "le passage n'est pas le début du texte");
        assert!(chars(&got[0].text) <= 1_200);
        assert!(got[0].text.contains("zibeline"));
        assert!(!got[0].text.ends_with(' '));
        let from: String = text.as_str().chars().skip(got[0].offset as usize).collect();
        assert!(from.starts_with(&got[0].text));
    }
}
