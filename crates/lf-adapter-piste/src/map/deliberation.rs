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
use lf_domain::entity::Deliberation;
use lf_domain::id::CnilTextId;
use lf_domain::status::status_of;
use lf_domain::text::clean_field;

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

/// `/consult/cnil` ⇒ `Deliberation` (03 §4.5). Même règle sur `diags` que `decision` : à lire
/// seulement après un `Ok`.
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
    Ok(Deliberation {
        deliberation_id,
        num: clean_field(cnil.num.as_deref()),
        nature: clean_field(cnil.nature.as_deref()),
        nature_delib: clean_field(cnil.nature_delib.as_deref()),
        etat: status_of(cnil.etat.as_deref(), None),
        date_texte,
        date_publication,
        title: clean_field(cnil.titre.as_deref()),
        title_long: clean_field(cnil.titre_long.as_deref()),
        text,
    })
}

#[cfg(test)]
mod tests {
    use lf_domain::contract::Citable;
    use lf_domain::date::CivilDate;
    use lf_domain::status::LegalStatus;
    use lf_domain::text::LongText;

    use super::*;
    use crate::dto::de::RawDate;
    use crate::dto::decode::decode;
    use crate::dto::juri::{JuriTextDto, SommaireDto};
    use crate::map::fixtures::{decode_as, juri_with};

    /// Identifiant demandé de la fixture `consult_cnil`.
    const CNIL_ID: &str = "CNILTEXT000053352643";

    /// Identifiant de délibération valide.
    fn cnil_id(raw: &str) -> CnilTextId {
        CnilTextId::parse(raw).unwrap()
    }

    /// `json` lu comme le fait la production.
    fn read(json: &str) -> ConsultJuriResponseDto {
        decode(json, CNIL).unwrap()
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

    /// `CorpusError::Decode` de détail `/consult/cnil: <rest>`.
    fn decode_error(rest: &str) -> CorpusError {
        CorpusError::Decode { detail: format!("/consult/cnil: {rest}") }
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
            assert_eq!(error, decode_error(detail), "{detail}");
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
                deliberation(read(json), &cnil_id(CNIL_ID), &mut diags),
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

        let (mapped, diags) = deliberation_of(read(r#"{"text":{"titre":"T"}}"#), CNIL_ID);
        assert_eq!(mapped.unwrap().deliberation_id, cnil_id(CNIL_ID));
        assert_eq!(diags, Diagnostics { id_mismatch: 1, ..Diagnostics::default() });

        let (mapped, diags) = deliberation_of(
            juri_with(|text| text.id = Some("JURITEXT000051743650".into())),
            CNIL_ID,
        );
        assert_eq!(mapped, Err(decode_error("text.id: invalid identifier")));
        assert_eq!(diags, Diagnostics::default());

        for (origine, expected) in [("JURI", 1), ("cnil", 0)] {
            let dto = cnil_text(|text| text.origine = Some(origine.to_owned()));
            let (mapped, diags) = deliberation_of(dto, CNIL_ID);
            assert_eq!(mapped.unwrap().deliberation_id, cnil_id(CNIL_ID), "{origine}");
            assert_eq!(diags, Diagnostics { origin_mismatch: expected, ..Diagnostics::default() });
        }
    }

    #[test]
    fn deliberation_ignores_juri_only_fields() {
        let (plain, _) = fixture_deliberation();
        let (filled, diags) = edited(|text| {
            text.ecli = Some("ECLI:FR:CC:2025:2025.1173.QPC".to_owned());
            text.numero_affaire = vec!["24-13.470".to_owned()];
            text.sommaire = vec![SommaireDto {
                id: None,
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
            Err(decode_error("text.texte: text too large"))
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
}
