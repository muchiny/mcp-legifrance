//! Délibération de la CNIL de `/consult/cnil` vers `Deliberation` (03 §4.5 ; 04 §9.1 a à f).
//!
//! La réponse a la forme de celle de `/consult/juri` ([`ConsultJuriResponseDto`]) et suit les
//! mêmes règles que [`decision`](super::decision) : un objet sans identité est un `NotFound`,
//! l'identifiant renvoyé est gardé (compté s'il diffère de celui demandé, repli compté sur le
//! demandé s'il manque), l'origine qui le contredit est seulement comptée, les dates préfèrent la
//! valeur calculée, le texte intégral préfère le HTML complet au texte brut et ses invisibles
//! retirés sont comptés. Les champs courts passent par `clean_field`, sans compter les leurs.
//!
//! Les champs propres aux décisions (`ecli`, `numeroAffaire`, `sommaire`) ne sont pas lus : une
//! délibération n'a ni ECLI ni sommaire officiel. Aucune valeur du corps ne sort d'ici.

use lf_application::ports::CorpusError;
use lf_domain::contract::is_cnil_closure_title;
use lf_domain::date::CivilDate;
use lf_domain::entity::Deliberation;
use lf_domain::id::CnilTextId;
use lf_domain::status::status_of;
use lf_domain::text::{clean_field, fold};

use super::dates::prefer_computed;
use super::endpoint::CNIL;
use super::rules::{check_origin, long_text, require, resolve_id};
use super::{Diagnostics, Field};
use crate::dto::juri::ConsultJuriResponseDto;

/// Champ `text.id` de la réponse.
const ID: Field = Field { endpoint: CNIL, path: "text.id" };
/// Date de la délibération si elle est valide, lue en priorité.
const DATE_COMPUTED: Field = Field { endpoint: CNIL, path: "text.dateTexteComputed" };
/// Date brute de la délibération.
const DATE_TEXTE: Field = Field { endpoint: CNIL, path: "text.dateTexte" };
/// Date de publication si elle est valide, lue en priorité.
const PUBLI_COMPUTED: Field = Field { endpoint: CNIL, path: "text.datePubliComputed" };
/// Date de publication brute.
const DATE_PUBLI: Field = Field { endpoint: CNIL, path: "text.datePubli" };
/// Texte intégral, en HTML (`texteHtml`) ou brut (`texte`).
const TEXTE: Field = Field { endpoint: CNIL, path: "text.texte" };

/// `/consult/cnil` ⇒ `Deliberation` (03 §4.5). Même règle sur `diags` que `decision` : sur
/// `Err(Decode)`, `diags` garde les anomalies relevées avant l'erreur, et
/// `Stats::record_mapping` est appelé sur `Ok` comme sur `Err` (04 §9.3) ; un `NotFound` ne
/// touche pas `diags`.
///
/// Ordre d'évaluation figé, la première erreur l'emporte : l'identité de l'objet, l'identifiant
/// renvoyé, la date du texte, la date de publication, puis le texte intégral. L'origine est
/// contrôlée contre l'identifiant retenu, jamais contre celui demandé.
///
/// # Errors
///
/// [`CorpusError::NotFound`] (`/consult/cnil`) si l'enveloppe `text` est absente ou `null`, ou si
/// l'objet n'a ni `id`, ni `titre`, ni `titreLong` non blanc ; `diags` n'est alors pas touché.
/// [`CorpusError::Decode`] du premier champ refusé : `text.id` (`invalid identifier`),
/// `text.dateTexteComputed`, `text.dateTexte`, `text.datePubliComputed` ou `text.datePubli`
/// (`unreadable date`, seul le champ choisi de chaque paire est lu), `text.texte`
/// (`text too large`).
pub(crate) fn deliberation(
    dto: ConsultJuriResponseDto,
    requested: &CnilTextId,
    diags: &mut Diagnostics,
) -> Result<Deliberation, CorpusError> {
    let cnil = require(dto.text, CNIL)?;
    let deliberation_id = resolve_id(requested, cnil.id.as_deref(), ID, diags)?;
    check_origin(cnil.origine.as_deref(), deliberation_id.as_str(), diags);
    let date_texte = prefer_computed(
        (cnil.date_texte_computed.as_ref(), DATE_COMPUTED),
        (cnil.date_texte.as_ref(), DATE_TEXTE),
        diags,
    )?;
    let date_publication = prefer_computed(
        (cnil.date_publi_computed.as_ref(), PUBLI_COMPUTED),
        (cnil.date_publi.as_ref(), DATE_PUBLI),
        diags,
    )?;
    let text = long_text(cnil.texte_html.as_deref(), cnil.texte.as_deref(), TEXTE, diags)?;
    let num = clean_field(cnil.num.as_deref());
    let title_long = clean_field(cnil.titre_long.as_deref());
    let date_texte =
        reliable_date(date_texte, date_publication, num.as_deref(), title_long.as_deref());
    Ok(Deliberation {
        deliberation_id,
        num,
        nature: clean_field(cnil.nature.as_deref()),
        nature_delib: clean_field(cnil.nature_delib.as_deref()),
        etat: status_of(cnil.etat.as_deref(), None),
        date_texte,
        date_publication,
        title: clean_field(cnil.titre.as_deref()),
        title_long,
        text,
    })
}

/// Noms de mois, repliés (sans accent), dans l'ordre du calendrier.
const MONTHS: [&str; 12] = [
    "janvier",
    "fevrier",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "aout",
    "septembre",
    "octobre",
    "novembre",
    "decembre",
];

/// Date de l'acte retenue, ou `None` quand l'amont n'a donné qu'une date de repli (AM-80).
///
/// Pour des milliers de décisions DF-/DR-, d'avis et de recherches en santé, `dateTexte` est un
/// repli de l'amont (31 décembre de l'année, ou 1er janvier de la suivante) et non la date de
/// l'acte. Une date est **suspecte** si elle tombe un 31 décembre ou un 1er janvier, ou si elle
/// est strictement postérieure à la publication. Une date suspecte est gardée si le titre LONG
/// (jamais le court, fabriqué avec la même date) l'énonce (« du 31 décembre 2019 »). Sinon, si le
/// titre long, hors clôture, énonce « n° <num> du <date> » avec une date qui n'est pas postérieure
/// à la publication, c'est cette date. Sinon `None` : omettre plutôt qu'inventer.
fn reliable_date(
    date: Option<CivilDate>,
    published: Option<CivilDate>,
    num: Option<&str>,
    title_long: Option<&str>,
) -> Option<CivilDate> {
    let date = date?;
    let year_boundary = matches!((date.month(), date.day()), (12, 31) | (1, 1));
    let after_publication = published.is_some_and(|published| date > published);
    if !year_boundary && !after_publication {
        return Some(date);
    }
    let title = title_long?;
    let chars = folded_chars(title);
    if dates_after_du(&chars).any(|stated| stated == date) {
        return Some(date);
    }
    if is_cnil_closure_title(title) {
        return None;
    }
    let recovered = date_of_number(&chars, num?)?;
    published.is_none_or(|published| recovered <= published).then_some(recovered)
}

/// Caractères repliés de `text` (casse, accents) ; « ᵉ » et « ʳ » deviennent « e » et « r »
/// (« 1ᵉʳ »).
fn folded_chars(text: &str) -> Vec<char> {
    fold(text)
        .chars()
        .map(|c| match c {
            'ᵉ' => 'e',
            'ʳ' => 'r',
            other => other,
        })
        .collect()
}

/// Lecteur de caractères repliés.
struct Cursor<'a> {
    chars: &'a [char],
    pos: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) {
        self.pos = self.pos.saturating_add(1);
    }

    fn skip_blanks(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.bump();
        }
    }

    /// Suite maximale de chiffres : sa valeur et sa longueur ; `None` s'il n'y en a pas ou si elle
    /// dépasse `max` chiffres.
    fn number(&mut self, max: usize) -> Option<(u32, usize)> {
        let (mut value, mut len) = (0_u32, 0_usize);
        while let Some(digit) = self.peek().and_then(|c| c.to_digit(10)) {
            value = value.saturating_mul(10).saturating_add(digit);
            len = len.saturating_add(1);
            self.bump();
        }
        (0 < len && len <= max).then_some((value, len))
    }

    /// Suite maximale de lettres.
    fn word(&mut self) -> String {
        let mut word = String::new();
        while let Some(letter) = self.peek().filter(|c| c.is_alphabetic()) {
            word.push(letter);
            self.bump();
        }
        word
    }
}

/// Date écrite à `pos` (juste après un « du ») : jour (1 ou 2 chiffres, « er » toléré pour le 1,
/// avec ou sans blanc), mois en lettres, année à quatre chiffres non suivie d'un chiffre. Les
/// blancs sont ceux d'Unicode, ils peuvent manquer entre le jour et le mois (« 31décembre »).
fn date_at(chars: &[char], pos: usize) -> Option<CivilDate> {
    let mut cursor = Cursor { chars, pos };
    cursor.skip_blanks();
    let (day, _) = cursor.number(2)?;
    cursor.skip_blanks();
    let mut month = cursor.word();
    if day == 1 && month == "er" {
        cursor.skip_blanks();
        month = cursor.word();
    }
    let month =
        MONTHS.iter().zip(1_u8..).find_map(|(name, number)| (*name == month).then_some(number))?;
    cursor.skip_blanks();
    let (year, len) = cursor.number(4)?;
    // Deux chiffres au plus : la conversion ne peut pas échouer, et un jour hors plage est de
    // toute façon refusé par `CivilDate::new`.
    CivilDate::new(
        u16::try_from(year).ok().filter(|_| len == 4)?,
        month,
        u8::try_from(day).unwrap_or(u8::MAX),
    )
}

/// `true` si le caractère de `chars` avant `index` n'est pas alphanumérique (ou n'existe pas) :
/// `index` est un début de mot.
fn starts_a_word(chars: &[char], index: usize) -> bool {
    index.checked_sub(1).and_then(|before| chars.get(before)).is_none_or(|c| !c.is_alphanumeric())
}

/// Les dates écrites « du <jour> <mois> <année> » dans `chars`, le « du » étant un début de mot.
fn dates_after_du(chars: &[char]) -> impl Iterator<Item = CivilDate> + '_ {
    chars.iter().enumerate().filter_map(move |(index, c)| {
        let is_du = *c == 'd'
            && chars.get(index.saturating_add(1)) == Some(&'u')
            && starts_a_word(chars, index);
        if is_du { date_at(chars, index.saturating_add(2)) } else { None }
    })
}

/// Date écrite dans « n° <num> du <date> » : `num` est comparé sans casse, sans espaces ni tirets
/// (des deux côtés) et doit finir sur une fin de mot ; « nº » vaut « n° ».
fn date_of_number(chars: &[char], num: &str) -> Option<CivilDate> {
    let wanted: Vec<char> = fold(num).chars().filter(|c| c.is_alphanumeric()).collect();
    if wanted.is_empty() {
        return None;
    }
    chars.iter().enumerate().find_map(|(index, c)| {
        let marker = *c == 'n'
            && matches!(chars.get(index.saturating_add(1)), Some('°' | 'º'))
            && starts_a_word(chars, index);
        if !marker {
            return None;
        }
        let mut cursor = Cursor { chars, pos: index.saturating_add(2) };
        cursor.skip_blanks();
        for expected in &wanted {
            while cursor.peek().is_some_and(|c| c.is_whitespace() || c == '-') {
                cursor.bump();
            }
            if cursor.peek() != Some(*expected) {
                return None;
            }
            cursor.bump();
        }
        if cursor.peek().is_some_and(char::is_alphanumeric) {
            return None;
        }
        cursor.skip_blanks();
        let is_du =
            cursor.peek() == Some('d') && chars.get(cursor.pos.saturating_add(1)) == Some(&'u');
        if is_du { date_at(chars, cursor.pos.saturating_add(2)) } else { None }
    })
}

#[cfg(test)]
mod tests {
    use lf_domain::contract::Citable;
    use lf_domain::status::LegalStatus;
    use lf_domain::text::LongText;

    use super::*;
    use crate::dto::de::RawDate;
    use crate::dto::juri::{JuriTextDto, SommaireDto};
    use crate::map::fixtures::{decode_as, decode_error, juri_with, read};

    /// Identifiant demandé de la fixture `consult_cnil`.
    const CNIL_ID: &str = "CNILTEXT000053352643";

    /// Identifiant de délibération valide.
    fn cnil_id(raw: &str) -> CnilTextId {
        CnilTextId::parse(raw).unwrap()
    }

    /// `deliberation` de `dto` demandée en `requested`, et les diagnostics qu'elle laisse.
    fn deliberation_of(
        dto: ConsultJuriResponseDto,
        requested: &str,
    ) -> (Result<Deliberation, CorpusError>, Diagnostics) {
        let mut diags = Diagnostics::default();
        let mapped = deliberation(dto, &cnil_id(requested), &mut diags);
        (mapped, diags)
    }

    /// Délibération de la fixture `consult_cnil` modifiée par `edit`, demandée sous l'identifiant
    /// de sa capture, et les diagnostics qu'elle laisse.
    fn edited(edit: impl FnOnce(&mut JuriTextDto)) -> (Deliberation, Diagnostics) {
        let mut dto = decode_as::<ConsultJuriResponseDto>("consult_cnil");
        edit(dto.text.as_mut().unwrap());
        let (mapped, diags) = deliberation_of(dto, CNIL_ID);
        (mapped.unwrap(), diags)
    }

    /// Délibération de la fixture, telle que capturée.
    fn fixture_deliberation() -> (Deliberation, Diagnostics) {
        edited(|_| ())
    }

    /// Texte synthétique d'identifiant [`CNIL_ID`], modifié ensuite par `edit`.
    fn cnil_text(edit: impl FnOnce(&mut JuriTextDto)) -> ConsultJuriResponseDto {
        juri_with(|text| {
            text.id = Some(CNIL_ID.to_owned());
            edit(text);
        })
    }

    /// Modification d'un texte synthétique.
    type Edit = fn(&mut JuriTextDto);

    /// Date illisible, dont la valeur ne doit jamais ressortir.
    fn junk() -> RawDate {
        RawDate::Text("TOPSECRET".to_owned())
    }

    /// Date lisible.
    fn good() -> RawDate {
        RawDate::Millis(1_762_473_600_000)
    }

    #[test]
    fn deliberation_fixture_maps() {
        let (mapped, diags) = fixture_deliberation();
        assert_eq!(mapped.deliberation_id, cnil_id("CNILTEXT000053352643"));
        assert_eq!(mapped.deliberation_id.as_str(), "CNILTEXT000053352643");
        assert_eq!(mapped.num.as_deref(), Some("SAN-2026-002"));
        assert_eq!(mapped.nature.as_deref(), Some("Délibération"));
        assert_eq!(mapped.nature_delib.as_deref(), Some("Sanction"));
        assert_eq!(mapped.etat, Some(LegalStatus::Vigueur));
        assert_eq!(mapped.date_texte, CivilDate::new(2026, 1, 8));
        assert_eq!(mapped.date_publication, CivilDate::new(2026, 1, 14));
        assert_eq!(mapped.title.as_deref(), Some("Délibération SAN-2026-002 du 8 janvier 2026"));
        assert_eq!(
            mapped.title_long.as_deref(),
            Some(
                "Délibération de la formation restreinte n° SAN-2026-002 du 8 janvier 2026 \
                 prononçant une sanction pécuniaire à l'encontre de la société FREE"
            )
        );

        let text = &mapped.text;
        let shape: Vec<(u32, usize)> =
            text.paragraphs().map(|(start, para)| (start, para.chars().count())).collect();
        assert_eq!(shape, [(0, 153), (155, 262), (419, 174)]);
        assert_eq!(text.char_len(), 593);
        assert!(text.as_str().ends_with("traitement des données à [...]"));
        assert_eq!(diags, Diagnostics::default());

        assert_eq!(
            mapped.contract().citation.as_str(),
            "CNIL, délibération n° SAN-2026-002 du 8 janvier 2026"
        );
    }

    #[test]
    fn deliberation_status_unknown_code_is_other() {
        let cases = [
            (Some("EN_COURS"), Some(LegalStatus::Other("EN_COURS".to_owned()))),
            (Some(""), None),
            (Some("  "), None),
            (None, None),
            (Some("vigueur"), Some(LegalStatus::Vigueur)),
        ];
        for (etat, expected) in cases {
            let (mapped, diags) = edited(|text| text.etat = etat.map(str::to_owned));
            assert_eq!(mapped.etat, expected, "{etat:?}");
            assert_eq!(diags, Diagnostics::default(), "{etat:?}");
        }
    }

    #[test]
    fn deliberation_publication_date_prefers_computed() {
        let computed = RawDate::Millis(1_768_348_800_000);
        let plain = RawDate::Millis(1_762_473_600_000);
        let cases = [
            (Some(&computed), Some(&plain), CivilDate::new(2026, 1, 14)),
            (Some(&computed), None, CivilDate::new(2026, 1, 14)),
            (None, Some(&plain), CivilDate::new(2025, 11, 7)),
            (None, None, None),
        ];
        for (computed, plain, expected) in cases {
            let dto = cnil_text(|text| {
                text.date_publi_computed = computed.cloned();
                text.date_publi = plain.cloned();
            });
            let (mapped, diags) = deliberation_of(dto, CNIL_ID);
            assert_eq!(mapped.unwrap().date_publication, expected, "{computed:?} {plain:?}");
            assert_eq!(diags, Diagnostics::default(), "{computed:?} {plain:?}");
        }
    }

    #[test]
    fn deliberation_text_date_prefers_computed() {
        let computed = RawDate::Millis(1_767_830_400_000);
        let plain = RawDate::Millis(1_762_473_600_000);
        let cases = [
            (Some(&computed), Some(&plain), CivilDate::new(2026, 1, 8)),
            (None, Some(&plain), CivilDate::new(2025, 11, 7)),
            (None, None, None),
        ];
        for (computed, plain, expected) in cases {
            let dto = cnil_text(|text| {
                text.date_texte_computed = computed.cloned();
                text.date_texte = plain.cloned();
            });
            let (mapped, _) = deliberation_of(dto, CNIL_ID);
            assert_eq!(mapped.unwrap().date_texte, expected, "{computed:?} {plain:?}");
        }
    }

    #[test]
    fn deliberation_unreadable_dates_are_decode_with_their_path() {
        let cases: [(Edit, &str); 4] = [
            ((|text| text.date_publi = Some(junk())), "text.datePubli: unreadable date"),
            (
                (|text| {
                    text.date_publi_computed = Some(junk());
                    text.date_publi = Some(good());
                }),
                "text.datePubliComputed: unreadable date",
            ),
            ((|text| text.date_texte = Some(junk())), "text.dateTexte: unreadable date"),
            (
                (|text| {
                    text.date_texte_computed = Some(junk());
                    text.date_texte = Some(good());
                }),
                "text.dateTexteComputed: unreadable date",
            ),
        ];
        for (edit, detail) in cases {
            let error = deliberation_of(cnil_text(edit), CNIL_ID).0.unwrap_err();
            assert_eq!(error, decode_error(CNIL, detail), "{detail}");
            assert!(!format!("{error:?} {error}").contains("TOPSECRET"), "{detail}");
        }
    }

    #[test]
    fn deliberation_empty_text_envelope_maps_to_not_found() {
        let before =
            Diagnostics { id_mismatch: 2, invalid_native_ecli: 3, ..Diagnostics::default() };
        for json in ["{}", r#"{"text":null}"#, r#"{"text":{}}"#] {
            let mut diags = before;
            assert_eq!(
                deliberation(read(json, CNIL), &cnil_id(CNIL_ID), &mut diags),
                Err(CorpusError::NotFound { what: "/consult/cnil".into() }),
                "{json}"
            );
            assert_eq!(diags, before, "{json}");
        }
    }

    #[test]
    fn deliberation_id_and_origin_rules() {
        let (mapped, diags) = deliberation_of(
            juri_with(|text| text.id = Some("CNILTEXT000053408671".into())),
            CNIL_ID,
        );
        assert_eq!(mapped.unwrap().deliberation_id, cnil_id("CNILTEXT000053408671"));
        assert_eq!(diags, Diagnostics { id_mismatch: 1, ..Diagnostics::default() });

        let (mapped, diags) = deliberation_of(read(r#"{"text":{"titre":"T"}}"#, CNIL), CNIL_ID);
        assert_eq!(mapped.unwrap().deliberation_id, cnil_id(CNIL_ID));
        assert_eq!(diags, Diagnostics { id_mismatch: 1, ..Diagnostics::default() });

        let (mapped, diags) = deliberation_of(
            juri_with(|text| text.id = Some("JURITEXT000051743650".into())),
            CNIL_ID,
        );
        assert_eq!(mapped, Err(decode_error(CNIL, "text.id: invalid identifier")));
        assert_eq!(diags, Diagnostics::default());

        let (mapped, diags) = deliberation_of(
            juri_with(|text| text.id = Some("CNILTEXT000053352643_08-01-2026".into())),
            CNIL_ID,
        );
        assert_eq!(mapped.unwrap().deliberation_id, cnil_id(CNIL_ID));
        assert_eq!(diags, Diagnostics { upstream_suffix_stripped: 1, ..Diagnostics::default() });

        for (origine, expected) in [("JURI", 1), ("cnil", 0)] {
            let dto = cnil_text(|text| text.origine = Some(origine.to_owned()));
            let (mapped, diags) = deliberation_of(dto, CNIL_ID);
            assert_eq!(mapped.unwrap().deliberation_id, cnil_id(CNIL_ID), "{origine}");
            assert_eq!(diags, Diagnostics { origin_mismatch: expected, ..Diagnostics::default() });
        }
    }

    #[test]
    fn a_decode_error_keeps_the_anomalies_counted_before_it() {
        let json = r#"{"text":{"id":"CNILTEXT000053408671","origine":"JURI","datePubli":"hier"}}"#;
        let (mapped, diags) = deliberation_of(read(json, CNIL), CNIL_ID);
        assert_eq!(mapped, Err(decode_error(CNIL, "text.datePubli: unreadable date")));
        assert_eq!(
            diags,
            Diagnostics { id_mismatch: 1, origin_mismatch: 1, ..Diagnostics::default() }
        );
    }

    #[test]
    fn deliberation_ignores_juri_only_fields() {
        let (plain, _) = fixture_deliberation();
        let (filled, diags) = edited(|text| {
            text.ecli = Some("ECLI:FR:CC:2025:2025.1173.QPC".to_owned());
            text.numero_affaire = vec!["24-13.470".to_owned()];
            text.sommaire = vec![SommaireDto {
                abstrats: Some("A".to_owned()),
                resume_principal: Some("R".to_owned()),
                autre_resume: Some("X".to_owned()),
            }];
        });
        assert_eq!(filled, plain);
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn deliberation_text_falls_back_to_plain() {
        let (mapped, diags) = deliberation_of(
            cnil_text(|text| text.texte = Some("Un seul bloc".to_owned())),
            CNIL_ID,
        );
        let mapped = mapped.unwrap();
        assert_eq!(mapped.text.paragraphs().count(), 1);
        assert_eq!(mapped.text.as_str(), "Un seul bloc");
        assert_eq!(diags, Diagnostics::default());

        let (mapped, diags) =
            deliberation_of(cnil_text(|text| text.texte = Some("a\u{202e}b".to_owned())), CNIL_ID);
        assert_eq!(mapped.unwrap().text.as_str(), "ab");
        assert_eq!(diags, Diagnostics { invisible_chars_removed: 1, ..Diagnostics::default() });
    }

    #[test]
    fn deliberation_text_too_large_is_decode() {
        let size = usize::try_from(LongText::MAX_CHARS).unwrap() + 1;
        let huge = cnil_text(|text| text.texte = Some("a".repeat(size)));
        assert_eq!(
            deliberation_of(huge, CNIL_ID).0,
            Err(decode_error(CNIL, "text.texte: text too large"))
        );
    }

    #[test]
    fn deliberation_titles_have_no_fallback() {
        let (mapped, _) = edited(|text| text.titre = None);
        assert_eq!(mapped.title, None);
        assert!(mapped.title_long.is_some());

        let (mapped, _) = edited(|text| text.titre_long = None);
        assert!(mapped.title.is_some());
        assert_eq!(mapped.title_long, None);

        let (mapped, _) = edited(|text| {
            text.titre = Some("  ".to_owned());
            text.titre_long = Some("\u{200b}".to_owned());
        });
        assert_eq!((mapped.title, mapped.title_long), (None, None));
    }

    // ---- date d'acte factice (AM-80) -----------------------------------------------------------

    /// Date `AAAA-MM-JJ` de l'amont.
    fn iso(date: &str) -> RawDate {
        RawDate::Text(date.to_owned())
    }

    /// `date_texte` d'un texte synthétique : date brute `plain` (ou calculée `computed`),
    /// publication `published`, numéro `num` et titre long `long`.
    fn date_of(
        computed: Option<&str>,
        plain: Option<&str>,
        published: Option<&str>,
        num: Option<&str>,
        long: Option<&str>,
    ) -> Option<CivilDate> {
        let dto = cnil_text(|text| {
            text.date_texte_computed = computed.map(iso);
            text.date_texte = plain.map(iso);
            text.date_publi_computed = published.map(iso);
            text.num = num.map(str::to_owned);
            text.titre_long = long.map(str::to_owned);
        });
        deliberation_of(dto, CNIL_ID).0.unwrap().date_texte
    }

    /// Cas courant : date brute seule.
    fn plain_date(
        date: &str,
        published: Option<&str>,
        num: Option<&str>,
        long: Option<&str>,
    ) -> Option<CivilDate> {
        date_of(None, Some(date), published, num, long)
    }

    fn day(year: u16, month: u8, day: u8) -> Option<CivilDate> {
        CivilDate::new(year, month, day)
    }

    #[test]
    fn placeholder_date_table() {
        // (dateTexte, datePubli, numéro, titre long, date attendue)
        type Row = (
            &'static str,
            Option<&'static str>,
            Option<&'static str>,
            Option<&'static str>,
            Option<CivilDate>,
        );
        let cases: [Row; 14] = [
            // V1 : DF-, publication AVANT le 31/12 factice.
            (
                "2017-12-31",
                Some("2017-11-04"),
                Some("DF-2017-1448"),
                Some("Décision DF-2017-1448 autorisant la société KEYMANTICS à transférer"),
                None,
            ),
            // V2 : publication APRÈS le 31/12 : la garde « date > publication » ne la voit pas.
            (
                "2017-12-31",
                Some("2018-04-05"),
                Some("DF-2017-2654"),
                Some("Décision DF-2017-2654 autorisant la société X"),
                None,
            ),
            // V4 : repli au 1er janvier de l'année suivante.
            (
                "2014-01-01",
                Some("2013-12-06"),
                Some("DR-2013-538"),
                Some("Décision DR-2013-538 autorisant un traitement de recherche"),
                None,
            ),
            // Titre long absent : la date suspecte est retirée (omettre plutôt qu'inventer).
            ("2017-12-31", Some("2017-11-04"), Some("DF-2017-1448"), None, None),
            ("2014-01-01", Some("2013-12-06"), None, None, None),
            // Publication inconnue : le 31/12 reste suspect.
            ("2017-12-31", None, Some("DF-2017-1448"), None, None),
            // V8 : dates légitimes au 31/12, confirmées par le titre long.
            (
                "2019-12-31",
                Some("2020-01-09"),
                Some("MED-2019-036"),
                Some(
                    "Décision n° MED 2019-036 du 31 décembre 2019 mettant en demeure la société Z",
                ),
                day(2019, 12, 31),
            ),
            (
                "2021-12-31",
                Some("2022-01-12"),
                Some("SAN-2021-024"),
                Some(
                    "Délibération de la formation restreinte n° SAN-2021-024 du 31décembre 2021 prononçant une sanction",
                ),
                day(2021, 12, 31),
            ),
            // Jours voisins : 30 décembre et 2 janvier ne sont pas des dates de repli.
            ("2017-12-30", Some("2018-01-05"), Some("DF-2017-1"), None, day(2017, 12, 30)),
            ("2018-01-02", Some("2018-01-05"), Some("DF-2017-1"), None, day(2018, 1, 2)),
            // Date postérieure à la publication hors 31/12 : retirée si le titre ne la dit pas...
            (
                "2017-06-15",
                Some("2017-06-01"),
                Some("DF-2017-1"),
                Some("Décision DF-2017-1 autorisant X"),
                None,
            ),
            // ... gardée si le titre long la dit.
            (
                "2017-06-15",
                Some("2017-06-01"),
                Some("DF-2017-1"),
                Some("Décision n° DF-2017-1 du 15 juin 2017 autorisant X"),
                day(2017, 6, 15),
            ),
            // Date ordinaire, publication ordinaire, titre muet : gardée.
            ("2024-11-15", Some("2024-12-05"), Some("MED-2024-150"), None, day(2024, 11, 15)),
            // Pas de date : pas de date.
            ("", Some("2024-12-05"), None, None, None),
        ];
        for (date, published, num, long, expected) in cases {
            let date = (!date.is_empty()).then_some(date);
            assert_eq!(
                date_of(None, date, published, num, long),
                expected,
                "{date:?} {published:?} {long:?}"
            );
        }
    }

    #[test]
    fn date_equal_to_publication_is_kept() {
        // Égale ou antérieure à la publication (hors 31/12 et 1er janvier) : valide, seule la date
        // STRICTEMENT postérieure est suspecte (tue les mutants `>` et `>=`).
        assert_eq!(plain_date("2017-06-15", Some("2017-06-15"), None, None), day(2017, 6, 15));
        assert_eq!(plain_date("2017-06-14", Some("2017-06-15"), None, None), day(2017, 6, 14));
        assert_eq!(plain_date("2017-05-31", Some("2017-06-01"), None, None), day(2017, 5, 31));
        assert_eq!(plain_date("2016-09-30", Some("2017-06-01"), None, None), day(2016, 9, 30));
    }

    #[test]
    fn date_after_publication_is_dropped() {
        // Strictement postérieure à la publication, d'un jour, d'un mois ou d'une année : retirée
        // quand le titre long ne la dit pas.
        assert_eq!(plain_date("2017-06-16", Some("2017-06-15"), None, None), None);
        assert_eq!(plain_date("2017-07-01", Some("2017-06-30"), None, None), None);
        assert_eq!(plain_date("2018-02-01", Some("2017-12-30"), None, None), None);
        assert_eq!(plain_date("2018-01-15", Some("2017-12-30"), None, None), None);
    }

    #[test]
    fn placeholder_date_series_and_years() {
        // Aucune série ni plage d'années dans la règle : DE, DF, DR, avis, sans série, 2010 à 2021.
        for (num, year) in [
            ("DE-2011-108", 2011_u16),
            ("DF-2010-250", 2010),
            ("DF-2014-127", 2014),
            ("DR-2021-275", 2021),
            ("DR-2019-246", 2019),
            ("2017-301", 2017),
            ("2014-029", 2014),
            ("XYZ-2013-001", 2013),
        ] {
            let end = format!("{year}-12-31");
            let next = format!("{}-01-01", year + 1);
            for date in [&end, &next] {
                assert_eq!(
                    plain_date(date, Some("2010-01-01"), Some(num), None),
                    None,
                    "{num} {date}"
                );
                assert_eq!(
                    plain_date(date, Some("2030-06-01"), Some(num), Some("Décision autorisant X")),
                    None,
                    "{num} {date}"
                );
            }
        }
        // 2013 à 2020 : le 31 décembre de chaque année est retiré, le 30 est gardé.
        for year in 2013_u16..=2020 {
            assert_eq!(plain_date(&format!("{year}-12-31"), None, None, None), None, "{year}");
            assert_eq!(
                plain_date(&format!("{year}-12-30"), None, None, None),
                day(year, 12, 30),
                "{year}"
            );
            assert_eq!(
                plain_date(&format!("{year}-01-02"), None, None, None),
                day(year, 1, 2),
                "{year}"
            );
        }
    }

    #[test]
    fn placeholder_date_computed_plain_matrix() {
        // La valeur retenue est la calculée si elle existe, sinon la brute (`prefer_computed`) ;
        // la règle s'applique à cette valeur seule.
        let long = Some("Décision DF-2017-1448 autorisant X");
        let publi = Some("2017-11-04");
        let num = Some("DF-2017-1448");
        let cases = [
            // (calculée, brute, attendu)
            (Some("2017-12-31"), Some("2017-12-31"), None),
            (Some("2017-12-31"), None, None),
            (None, Some("2017-12-31"), None),
            // Calculée ordinaire : gardée, quelle que soit la brute (même factice).
            (Some("2017-10-05"), Some("2017-12-31"), day(2017, 10, 5)),
            (Some("2017-10-05"), None, day(2017, 10, 5)),
            (Some("2017-10-05"), Some("2017-10-06"), day(2017, 10, 5)),
            // Calculée factice, brute ordinaire : la calculée est celle qui est lue, donc retirée.
            (Some("2017-12-31"), Some("2017-10-05"), None),
            (None, Some("2017-10-05"), day(2017, 10, 5)),
            (None, None, None),
        ];
        for (computed, plain, expected) in cases {
            assert_eq!(
                date_of(computed, plain, publi, num, long),
                expected,
                "{computed:?} {plain:?}"
            );
        }
    }

    #[test]
    fn placeholder_date_title_confirmation_forms() {
        // Titre long qui énonce la date 2014-01-01 sous toutes les formes tolérées.
        let first = [
            "Décision n° DR-2013-538 du 1er janvier 2014 portant autorisation",
            "Décision n° DR-2013-538 du 1 janvier 2014 portant autorisation",
            "Décision n° DR-2013-538 du 01 janvier 2014 portant autorisation",
            "Décision n° DR-2013-538 du 1 er janvier 2014 portant autorisation",
            "Décision n° DR-2013-538 du 1ᵉʳ janvier 2014 portant autorisation",
            "Décision n° DR-2013-538 du 1er\u{a0}janvier\u{a0}2014 portant autorisation",
            "DÉCISION N° DR-2013-538 DU 1ER JANVIER 2014",
            "décision n° DR-2013-538 du 1er janvier 2014",
            "du 1er janvier 2014",
            "Décision n° DR-2013-538 du  1er   janvier   2014 portant autorisation",
        ];
        for long in first {
            assert_eq!(
                plain_date("2014-01-01", Some("2013-12-06"), None, Some(long)),
                day(2014, 1, 1),
                "{long}"
            );
        }
        // 31 décembre : formes et accents.
        let last = [
            "du 31 décembre 2013",
            "du 31 decembre 2013",
            "DU 31 DÉCEMBRE 2013",
            "du 31décembre 2013",
            "du\u{a0}31\u{a0}décembre\u{a0}2013",
            "du\t31\ndécembre 2013",
            "Délibération n° X du 31 décembre 2013, portant avis",
        ];
        for long in last {
            assert_eq!(
                plain_date("2013-12-31", Some("2013-12-06"), None, Some(long)),
                day(2013, 12, 31),
                "{long}"
            );
        }
        // Ne confirment pas : autre jour (« 21 » pour le 1er), autre mois, autre année, ancre « du »
        // absente ou collée à un mot, jour 31 écrit en 131, année tronquée ou prolongée.
        let jan_1st_misses = [
            "du 21 janvier 2014",
            "du 11 janvier 2014",
            "du 10 janvier 2014",
            "du 2 janvier 2014",
            "du 1er février 2014",
            "du 1er janvier 2013",
            "du 1er janvier 2015",
            "du 1er janvier 201",
            "du 1er janvier 20144",
            "du 1er janvier",
            "le 1er janvier 2014",
            "au 1er janvier 2014",
            "ndu 1er janvier 2014",
            "audu 1er janvier 2014",
            "du 1erx janvier 2014",
            "du 101 janvier 2014",
            "Décision autorisant X",
            "",
        ];
        for long in jan_1st_misses {
            assert_eq!(
                plain_date("2014-01-01", Some("2013-12-06"), None, Some(long)),
                None,
                "{long:?}"
            );
        }
        let dec_31st_misses = [
            "et le 31 décembre 2013",
            "au 31 décembre 2013",
            "jusqu'au 31 décembre 2013",
            "du 30 décembre 2013",
            "du 3 décembre 2013",
            "du 31 novembre 2013",
            "du 31 décembre 2012",
            "du 131 décembre 2013",
            "dun 31 décembre 2013",
        ];
        for long in dec_31st_misses {
            assert_eq!(
                plain_date("2013-12-31", Some("2013-12-06"), None, Some(long)),
                None,
                "{long}"
            );
        }
        // Le jour 1 d'un mois autre que janvier ne confirme pas non plus le 1er janvier, et deux
        // dates énoncées : l'une d'elles suffit.
        assert_eq!(
            plain_date(
                "2014-01-01",
                None,
                None,
                Some("du 1er février 2014 et du 1er janvier 2014")
            ),
            day(2014, 1, 1)
        );
    }

    #[test]
    fn placeholder_date_recovered_from_long_title() {
        // Date de repli, mais « n° <num> du <date> » dans le titre long : cette date est reprise
        // si elle n'est pas postérieure à la publication.
        let long = Some("Décision n° DF-2017-1448 du 7 décembre 2017 autorisant la société X");
        let num = Some("DF-2017-1448");
        assert_eq!(plain_date("2017-12-31", Some("2018-04-05"), num, long), day(2017, 12, 7));
        assert_eq!(plain_date("2017-12-31", Some("2017-12-07"), num, long), day(2017, 12, 7));
        assert_eq!(plain_date("2017-12-31", Some("2017-12-06"), num, long), None);
        assert_eq!(plain_date("2017-12-31", None, num, long), day(2017, 12, 7));
        // Avis 2017-301.
        assert_eq!(
            plain_date(
                "2017-12-31",
                Some("2018-01-10"),
                Some("2017-301"),
                Some("Délibération n° 2017-301 du 7 décembre 2017 portant avis sur un projet")
            ),
            day(2017, 12, 7)
        );
        // Le numéro se compare sans espaces, tirets ni casse (des deux côtés).
        for (num, long) in [
            ("MED-2019-036", "Décision n° MED 2019-036 du 7 décembre 2019 mettant en demeure"),
            ("MED 2019 036", "Décision n° med-2019-036 du 7 décembre 2019 mettant en demeure"),
            ("MED-2019-036", "Décision n°MED2019036 du 7 décembre 2019 mettant en demeure"),
        ] {
            let got = plain_date("2019-12-31", Some("2020-02-01"), Some(num), Some(long));
            assert_eq!(got, day(2019, 12, 7), "{num} / {long}");
        }
        // Pas de reprise : autre numéro (même préfixe), numéro plus long, pas de numéro, date
        // illisible, clôture, mot « n° » absent.
        for (num, long) in [
            (Some("DF-2017-1448"), "Décision n° DF-2017-14480 du 7 décembre 2017"),
            (Some("DF-2017-1448"), "Décision n° DF-2017-144 du 7 décembre 2017"),
            (Some("DF-2017-1448"), "Décision n° DF-2017-1449 du 7 décembre 2017"),
            (None, "Décision n° DF-2017-1448 du 7 décembre 2017"),
            (Some("DF-2017-1448"), "Décision n° DF-2017-1448 du 32 décembre 2017"),
            (Some("DF-2017-1448"), "Décision n° DF-2017-1448 le 7 décembre 2017"),
            (Some("DF-2017-1448"), "Décision DF-2017-1448 du 7 décembre 2017"),
            (Some("DF-2017-1448"), "Clôture de la décision n° DF-2017-1448 du 7 décembre 2017"),
            (
                Some("DF-2017-1448"),
                "Courrier de clôture de la décision n° DF-2017-1448 du 7 décembre 2017",
            ),
            (Some("DF-2017-1448"), "Décision n° DF-2017-1448 autorisant X, du 7 décembre 2017"),
        ] {
            assert_eq!(
                plain_date("2017-12-31", Some("2018-04-05"), num, Some(long)),
                None,
                "{num:?} {long}"
            );
        }
        // Un « n° » plus loin que le premier : le bon est trouvé.
        assert_eq!(
            plain_date(
                "2017-12-31",
                Some("2018-04-05"),
                num,
                Some(
                    "Décision n° 2016-1 du 1er mars 2016, modifiant la décision n° DF-2017-1448 du 7 décembre 2017"
                )
            ),
            day(2017, 12, 7)
        );
        // « nº » (ordinal masculin) est lu comme « n° ».
        assert_eq!(
            plain_date(
                "2017-12-31",
                Some("2018-04-05"),
                num,
                Some("Décision nº DF-2017-1448 du 7 décembre 2017")
            ),
            day(2017, 12, 7)
        );
    }

    #[test]
    fn a_number_without_a_letter_or_a_digit_recovers_no_date() {
        let chars = folded_chars("Décision n° DF-2017-1448 du 7 décembre 2017");
        assert_eq!(date_of_number(&chars, "DF-2017-1448"), CivilDate::new(2017, 12, 7));
        assert_eq!(date_of_number(&chars, ""), None);
        assert_eq!(date_of_number(&chars, " - "), None);
    }

    #[test]
    fn date_in_period_phrase_does_not_confirm() {
        let num = Some("DR-2013-538");
        for long in [
            "Décision autorisant un traitement entre le 1er janvier 2010 et le 31 décembre 2013",
            "Décision autorisant un traitement jusqu'au 31 décembre 2013",
            "Décision autorisant un traitement à compter du 1er janvier 2010 jusqu'au 31 décembre 2013 ",
        ] {
            // Le 1er janvier 2010 est précédé de « du » dans la troisième phrase, mais ce n'est pas
            // la date cherchée ; le 31 décembre 2013 n'est jamais précédé de « du ».
            assert_eq!(
                plain_date("2013-12-31", Some("2013-12-06"), num, Some(long)),
                None,
                "{long}"
            );
        }
        // « n° X du 31 décembre 2013 » : gardée.
        assert_eq!(
            plain_date(
                "2013-12-31",
                Some("2014-02-01"),
                num,
                Some(
                    "Décision n° DR-2013-538 du 31 décembre 2013 autorisant un traitement jusqu'au 31 décembre 2015"
                )
            ),
            day(2013, 12, 31)
        );
    }

    /// Texte synthétique de `/consult/cnil` : identifiant, numéro, nature, natureDelib, dates
    /// brutes et titres.
    fn upstream(
        num: &str,
        nature: &str,
        date: &str,
        published: &str,
        title: &str,
        title_long: &str,
    ) -> ConsultJuriResponseDto {
        cnil_text(|text| {
            text.num = Some(num.to_owned());
            text.nature = Some(nature.to_owned());
            text.date_texte = Some(iso(date));
            text.date_publi = Some(iso(published));
            text.titre = Some(title.to_owned());
            text.titre_long = Some(title_long.to_owned());
        })
    }

    #[test]
    #[expect(clippy::too_many_lines, reason = "table de relevés, une ligne par texte")]
    fn placeholder_fixtures_map() {
        // Pas de relevé sandbox de ces douze textes (D1) : réponses synthétiques construites avec
        // les champs lus en production le 2026-11-02 (numéro, nature, dates, début du titre long) ;
        // le titre court est fabriqué comme le fait l'amont (« {Nature} {num} du {dateTexte} »).
        // (numéro, nature, dateTexte brute, datePubli, titre court, titre long, date attendue)
        type Row = (
            &'static str,
            &'static str,
            &'static str,
            &'static str,
            &'static str,
            &'static str,
            Option<CivilDate>,
        );
        let rows: [Row; 11] = [
            (
                "DF-2017-1448",
                "Décision",
                "2017-12-31",
                "2017-11-04",
                "Décision DF-2017-1448 du 31 décembre 2017",
                "Décision DF-2017-1448 autorisant la société KEYMANTICS à transférer",
                None,
            ),
            (
                "DF-2017-2654",
                "Décision",
                "2017-12-31",
                "2018-04-05",
                "Décision DF-2017-2654 du 31 décembre 2017",
                "Décision DF-2017-2654 autorisant la société X",
                None,
            ),
            (
                "DF-2014-127",
                "Décision",
                "2014-12-31",
                "2015-01-15",
                "Décision DF-2014-127 du 31 décembre 2014",
                "Décision DF-2014-127 autorisant la société Y",
                None,
            ),
            (
                "DR-2019-246",
                "Décision",
                "2019-12-31",
                "2019-12-04",
                "Décision DR-2019-246 du 31 décembre 2019",
                "Décision DR-2019-246 autorisant un traitement de recherche",
                None,
            ),
            (
                "DR-2020-391",
                "Décision",
                "2020-12-31",
                "2021-09-28",
                "Décision DR-2020-391 du 31 décembre 2020",
                "Décision DR-2020-391 autorisant un traitement de recherche",
                None,
            ),
            (
                "DR-2013-538",
                "Décision",
                "2014-01-01",
                "2013-12-06",
                "Décision DR-2013-538 du 1er janvier 2014",
                "Décision DR-2013-538 autorisant un traitement de recherche",
                None,
            ),
            (
                "MED-2019-036",
                "Décision",
                "2019-12-31",
                "2020-01-09",
                "Décision MED-2019-036 du 31 décembre 2019",
                "Décision n° MED 2019-036 du 31 décembre 2019 mettant en demeure la société Z",
                day(2019, 12, 31),
            ),
            (
                "SAN-2021-024",
                "Délibération",
                "2021-12-31",
                "2022-01-12",
                "Délibération SAN-2021-024 du 31décembre 2021",
                "Délibération de la formation restreinte n° SAN-2021-024 du 31décembre 2021 prononçant une sanction",
                day(2021, 12, 31),
            ),
            (
                "MEDP-2024-001",
                "Décision",
                "2024-11-18",
                "2024-11-25",
                "Décision MEDP-2024-001 du 18 novembre 2024",
                "Délibération du bureau de la Commission n° MEDP-2024-001 du 18 novembre 2024",
                day(2024, 11, 18),
            ),
            (
                "MED-2022-060",
                "Décision",
                "2022-09-16",
                "2022-09-20",
                "Décision MED-2022-060 du 16 septembre 2022",
                "Clôture de la mise en demeure n° MED-2022-060 du 25 avril 2022",
                day(2022, 9, 16),
            ),
            (
                "MED-2023-018",
                "Décision",
                "2024-01-03",
                "2024-01-10",
                "Décision MED-2023-018 du 3 janvier 2024",
                "Clôture de la décision n°MED-2023-018 du 3 avril 2023",
                day(2024, 1, 3),
            ),
        ];
        for (num, nature, date, published, title, long, expected) in rows {
            let (mapped, diags) =
                deliberation_of(upstream(num, nature, date, published, title, long), CNIL_ID);
            let mapped = mapped.unwrap();
            assert_eq!(mapped.date_texte, expected, "{num}");
            // La date de publication est toujours lue.
            assert_eq!(
                mapped.date_publication.map(|d| d.to_string()).as_deref(),
                Some(published),
                "{num}"
            );
            assert_eq!(diags, Diagnostics::default(), "{num}");
        }
        // Texte réel relevé (MED-2024-150, c110) : date confirmée par son titre long, gardée.
        let recorded = read::<ConsultJuriResponseDto>(
            include_str!(
                "../../tests/fixtures/recorded/consult/cnil/c110/D09-cnil-consult-MED-2024-150-CNILTEXT000050719865.json"
            ),
            CNIL,
        );
        let (mapped, _) = deliberation_of(recorded, "CNILTEXT000050719865");
        let mapped = mapped.unwrap();
        assert_eq!(mapped.date_texte, day(2024, 11, 15));
        assert_eq!(mapped.date_publication, day(2024, 12, 5));
    }
}
