//! Règles de mapping communes aux consultations : enveloppe et identité (04 §9.1 a), texte long
//! (04 §9.1 f).
//!
//! Une réponse dont l'enveloppe est absente, `null` ou vide, ou dont l'objet n'a ni `id`, ni
//! `cid`, ni titre non blanc, n'est pas un texte : c'est un `NotFound` (04 §8.4, §11). Le texte
//! long est rendu par `LongText::best_of`, qui préfère le HTML complet au texte brut ; les
//! caractères invisibles retirés sont comptés.

use lf_application::ports::CorpusError;
use lf_domain::text::LongText;

use super::{Diagnostics, Field, not_found};
use crate::dto::acco::AccordDto;
use crate::dto::article::ArticleDto;
use crate::dto::consult_text::ConsultTextDto;
use crate::dto::juri::JuriTextDto;

/// Objet d'une réponse de consultation qui porte ses champs d'identité.
pub(crate) trait Identified {
    /// Les trois champs d'identité, dans l'ordre `id`, `cid` puis titre ; `None` pour une place
    /// que la forme n'a pas.
    fn identity(&self) -> [Option<&str>; 3];
}

impl Identified for ConsultTextDto {
    fn identity(&self) -> [Option<&str>; 3] {
        [self.id.as_deref(), self.cid.as_deref(), self.title.as_deref()]
    }
}

impl Identified for ArticleDto {
    fn identity(&self) -> [Option<&str>; 3] {
        [self.id.as_deref(), self.cid.as_deref(), None]
    }
}

impl Identified for JuriTextDto {
    fn identity(&self) -> [Option<&str>; 3] {
        [self.id.as_deref(), self.titre.as_deref(), self.titre_long.as_deref()]
    }
}

impl Identified for AccordDto {
    fn identity(&self) -> [Option<&str>; 3] {
        [self.id.as_deref(), self.titre_texte.as_deref(), None]
    }
}

/// Au moins un des `fields` est présent et non vide après suppression des blancs.
fn any_present(fields: &[Option<&str>]) -> bool {
    fields.iter().flatten().any(|field| !field.trim().is_empty())
}

/// Règle (a) : l'objet de la réponse, ou `NotFound`.
///
/// # Errors
///
/// [`CorpusError::NotFound`] (`what` : le gabarit `endpoint`) si l'enveloppe est absente ou
/// `null`, ou si l'objet n'a ni `id`, ni `cid`, ni titre non blanc.
pub(crate) fn require<T: Identified>(
    candidate: Option<T>,
    endpoint: &str,
) -> Result<T, CorpusError> {
    candidate.filter(|dto| any_present(&dto.identity())).ok_or_else(|| not_found(endpoint))
}

/// Règle (f) : le meilleur texte de `html` et de `plain`, et les invisibles retirés comptés.
///
/// # Errors
///
/// [`CorpusError::Decode`] (`text too large`) du champ `field` si le texte dépasse la taille
/// permise.
pub(crate) fn long_text(
    html: Option<&str>,
    plain: Option<&str>,
    field: Field,
    diags: &mut Diagnostics,
) -> Result<LongText, CorpusError> {
    let text = LongText::best_of(html, plain).map_err(|error| field.domain(&error))?;
    diags.add_invisible(text.invisible_removed());
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::acco::ConsultAccoResponseDto;
    use crate::dto::article::GetArticleResponseDto;
    use crate::dto::decode::decode;
    use crate::dto::juri::ConsultJuriResponseDto;
    use crate::map::endpoint;
    use crate::map::fixtures::{NAMES, decode_as};

    /// `json` lu dans `T` comme le fait la production.
    fn read<T: serde::de::DeserializeOwned>(json: &str, endpoint: &str) -> T {
        decode(json, endpoint).expect("test body decodes")
    }

    /// `NotFound` de `endpoint`.
    fn missing(endpoint: &str) -> CorpusError {
        CorpusError::NotFound { what: endpoint.to_owned() }
    }

    /// Passe `json` par `require` pour la forme et l'endpoint de `endpoint`.
    fn require_envelope(json: &str, endpoint: &str) -> Result<(), CorpusError> {
        match endpoint {
            endpoint::LAW_DECREE | endpoint::KALI_TEXT => {
                require(Some(read::<ConsultTextDto>(json, endpoint)), endpoint).map(drop)
            }
            endpoint::GET_ARTICLE => {
                require(read::<GetArticleResponseDto>(json, endpoint).article, endpoint).map(drop)
            }
            endpoint::JURI | endpoint::CNIL => {
                require(read::<ConsultJuriResponseDto>(json, endpoint).text, endpoint).map(drop)
            }
            _ => require(read::<ConsultAccoResponseDto>(json, endpoint).acco, endpoint).map(drop),
        }
    }

    #[test]
    fn consult_empty_envelope_maps_to_not_found() {
        let cases = [
            ("{}", endpoint::LAW_DECREE),
            ("{}", endpoint::KALI_TEXT),
            (r#"{"article":null}"#, endpoint::GET_ARTICLE),
            ("{}", endpoint::GET_ARTICLE),
            (r#"{"article":{}}"#, endpoint::GET_ARTICLE),
            (r#"{"text":null}"#, endpoint::JURI),
            (r#"{"text":{}}"#, endpoint::JURI),
            (r#"{"text":null}"#, endpoint::CNIL),
            (r#"{"text":{}}"#, endpoint::CNIL),
            (r#"{"acco":null}"#, endpoint::ACCO),
            ("{}", endpoint::ACCO),
            (r#"{"acco":{}}"#, endpoint::ACCO),
        ];
        for (json, endpoint) in cases {
            assert_eq!(require_envelope(json, endpoint), Err(missing(endpoint)), "{json}");
        }
    }

    #[test]
    fn object_without_identity_is_not_found() {
        let cases = [
            (r#"{"nature":"LOI","etat":"VIGUEUR"}"#, endpoint::LAW_DECREE),
            (r#"{"article":{"num":"9","texte":"t"}}"#, endpoint::GET_ARTICLE),
            (r#"{"text":{"texte":"t","nature":"ARRET"}}"#, endpoint::JURI),
            (r#"{"acco":{"siret":"32862158600143"}}"#, endpoint::ACCO),
            (r#"{"title":"  "}"#, endpoint::KALI_TEXT),
            (r#"{"title":"　"}"#, endpoint::KALI_TEXT),
        ];
        for (json, endpoint) in cases {
            assert_eq!(require_envelope(json, endpoint), Err(missing(endpoint)), "{json}");
        }
    }

    #[test]
    fn any_one_identity_field_is_enough() {
        let cases = [
            (r#"{"id":"X"}"#, endpoint::LAW_DECREE),
            (r#"{"cid":"X"}"#, endpoint::LAW_DECREE),
            (r#"{"title":"X"}"#, endpoint::KALI_TEXT),
            (r#"{"article":{"id":"X"}}"#, endpoint::GET_ARTICLE),
            (r#"{"article":{"cid":"X"}}"#, endpoint::GET_ARTICLE),
            (r#"{"text":{"id":"X"}}"#, endpoint::JURI),
            (r#"{"text":{"titre":"X"}}"#, endpoint::JURI),
            (r#"{"text":{"titreLong":"X"}}"#, endpoint::CNIL),
            (r#"{"acco":{"id":"X"}}"#, endpoint::ACCO),
            (r#"{"acco":{"titreTexte":"X"}}"#, endpoint::ACCO),
        ];
        for (json, endpoint) in cases {
            assert_eq!(require_envelope(json, endpoint), Ok(()), "{json}");
        }
    }

    #[test]
    fn require_returns_the_dto_unchanged() {
        let dto = read::<ConsultTextDto>(r#"{"id":"X","nature":"LOI"}"#, endpoint::LAW_DECREE);
        let expected = read::<ConsultTextDto>(r#"{"id":"X","nature":"LOI"}"#, endpoint::LAW_DECREE);
        assert_eq!(require(Some(dto), endpoint::LAW_DECREE), Ok(expected));
    }

    #[test]
    fn consult_fixtures_have_an_identity() {
        let consult: Vec<&str> =
            NAMES.iter().copied().filter(|name| name.starts_with("consult_")).collect();
        assert_eq!(consult.len(), 8);
        for name in consult {
            let result = match name {
                "consult_loda" => {
                    require(Some(decode_as::<ConsultTextDto>(name)), endpoint::LAW_DECREE).map(drop)
                }
                "consult_kali" => {
                    require(Some(decode_as::<ConsultTextDto>(name)), endpoint::KALI_TEXT).map(drop)
                }
                "consult_article" => {
                    require(decode_as::<GetArticleResponseDto>(name).article, endpoint::GET_ARTICLE)
                        .map(drop)
                }
                "consult_acco" => {
                    require(decode_as::<ConsultAccoResponseDto>(name).acco, endpoint::ACCO)
                        .map(drop)
                }
                "consult_cnil" => {
                    require(decode_as::<ConsultJuriResponseDto>(name).text, endpoint::CNIL)
                        .map(drop)
                }
                _ => require(decode_as::<ConsultJuriResponseDto>(name).text, endpoint::JURI)
                    .map(drop),
            };
            assert_eq!(result, Ok(()), "{name}");
        }
    }

    #[test]
    fn any_present_ignores_blank_and_absent_fields() {
        assert!(!any_present(&[None, None, None]));
        assert!(!any_present(&[Some(""), Some("  "), None]));
        assert!(!any_present(&[]));
        assert!(any_present(&[None, Some(" x "), None]));
    }

    /// Champ du texte d'une décision.
    const TEXTE: Field = Field { endpoint: endpoint::JURI, path: "text.texte" };

    #[test]
    fn long_text_prefers_html() {
        let mut diags = Diagnostics::default();
        let text = long_text(Some("<p>a</p><p>b</p>"), Some("a b"), TEXTE, &mut diags).unwrap();
        let paragraphs: Vec<&str> = text.paragraphs().map(|(_, para)| para).collect();
        assert_eq!(paragraphs, ["a", "b"]);
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn long_text_falls_back_to_plain() {
        let mut diags = Diagnostics::default();
        let text =
            long_text(None, Some("Lorsque R<P, le taux T = 5 %."), TEXTE, &mut diags).unwrap();
        assert_eq!(text.as_str(), "Lorsque R<P, le taux T = 5 %.");
        assert_eq!(text.paragraphs().count(), 1);
        assert_eq!(text.char_len(), 29);

        let blank = long_text(Some("   "), Some("Plain"), TEXTE, &mut diags).unwrap();
        assert_eq!(blank.as_str(), "Plain");
    }

    #[test]
    fn long_text_keeps_a_complete_html() {
        let mut diags = Diagnostics::default();
        let text = long_text(
            Some("<p>Alinéa un.</p><p>Alinéa deux.</p>"),
            Some("Alinéa un. Alinéa deux."),
            TEXTE,
            &mut diags,
        )
        .unwrap();
        assert_eq!(text.as_str(), "Alinéa un.\n\nAlinéa deux.");
        assert_eq!(text.paragraphs().count(), 2);
        assert_eq!(text.char_len(), 24);
    }

    #[test]
    fn long_text_without_either_is_empty() {
        let mut diags = Diagnostics::default();
        let text = long_text(None, None, TEXTE, &mut diags).unwrap();
        assert!(text.is_empty());
        assert_eq!(text.as_str(), "");
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn long_text_counts_invisible_chars() {
        let mut diags = Diagnostics { invisible_chars_removed: 2, ..Diagnostics::default() };
        let plain = long_text(None, Some("a\u{202e}b"), TEXTE, &mut diags).unwrap();
        assert_eq!(plain.as_str(), "ab");
        assert_eq!(diags.invisible_chars_removed, 3);

        let html = long_text(Some("<p>a&#173;b</p>"), None, TEXTE, &mut diags).unwrap();
        assert_eq!(html.as_str(), "ab");
        assert_eq!(diags.invisible_chars_removed, 4);
    }

    #[test]
    fn long_text_too_large_is_decode_without_the_text() {
        let mut diags = Diagnostics::default();
        let huge = "a".repeat(16_000_001);
        let error = long_text(None, Some(&huge), TEXTE, &mut diags).unwrap_err();
        assert_eq!(
            error,
            CorpusError::Decode { detail: "/consult/juri: text.texte: text too large".into() }
        );
        assert_eq!(diags, Diagnostics::default());
    }
}
