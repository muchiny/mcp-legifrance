//! Règles de mapping communes aux consultations : enveloppe et identité (04 §9.1 a), identifiant
//! renvoyé, suffixe de version et origine (04 §9.1 b, c, d), texte long (04 §9.1 f).
//!
//! Une réponse dont l'enveloppe est absente, `null` ou vide, ou dont l'objet n'a ni `id`, ni
//! `cid`, ni titre non blanc, n'est pas un texte : c'est un `NotFound` (04 §8.4, §11). Le texte
//! long est rendu par `LongText::best_of`, qui préfère le HTML complet au texte brut ; les
//! caractères invisibles retirés sont comptés.

use std::fmt::Display;
use std::str::FromStr;

use lf_application::ports::CorpusError;
use lf_domain::DomainError;
use lf_domain::date::{CivilDate, open_ended};
use lf_domain::id::parse_versioned;
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

/// Préfixes d'identifiant et valeur du champ d'origine de leur famille (04 §9.1 d), pour les sept
/// familles dont l'origine est mesurée.
const ORIGINS: [(&str, &str); 7] = [
    ("LEGITEXT", "LEGI"),
    ("JURITEXT", "JURI"),
    ("CONSTEXT", "CONSTIT"),
    ("CETATEXT", "CETAT"),
    ("CNILTEXT", "CNIL"),
    ("KALITEXT", "KALI"),
    ("ACCOTEXT", "ACCO"),
];

/// Règles (b) et (c) : l'identifiant d'une réponse.
///
/// `returned` absent ou blanc ⇒ l'identifiant demandé, compté en `id_mismatch`. Sinon il est lu
/// par [`parse_counted`] ; s'il diffère de `requested`, l'entité garde l'identifiant lu et la
/// différence est comptée.
///
/// # Errors
///
/// [`CorpusError::Decode`] (`invalid identifier`) du champ `field` si `returned` n'est pas un
/// identifiant du type `T` ou porte un suffixe mal formé.
pub(crate) fn resolve_id<T>(
    requested: &T,
    returned: Option<&str>,
    field: Field,
    diags: &mut Diagnostics,
) -> Result<T, CorpusError>
where
    T: FromStr<Err = DomainError> + Display + PartialEq + Clone,
{
    let Some(raw) = returned.filter(|raw| !raw.trim().is_empty()) else {
        diags.note_id_mismatch();
        return Ok(requested.clone());
    };
    let parsed = parse_counted::<T>(raw, diags).map_err(|error| field.domain(&error))?;
    if parsed != *requested {
        diags.note_id_mismatch();
    }
    Ok(parsed)
}

/// Lit un identifiant : casse, blancs et suffixe `_JJ-MM-AAAA` bien formé sont tolérés, le suffixe
/// est retiré. Un suffixe retiré d'un identifiant autre que `LEGITEXT` est compté (06 D-28).
///
/// Règle (c) de tout identifiant lu, principal ou secondaire (`cid`, texte parent d'un article).
/// Un identifiant que le mapper peut encore écarter après sa lecture (résultat de recherche,
/// `cid` ou article d'un résultat) est lu par le domaine, puis compté par [`note_suffix`] une fois
/// retenu.
///
/// # Errors
///
/// L'erreur du domaine de `T::from_str` : préfixe ou forme invalide, suffixe mal formé (dans ce
/// cas le compteur reste à zéro).
pub(crate) fn parse_counted<T>(raw: &str, diags: &mut Diagnostics) -> Result<T, DomainError>
where
    T: FromStr<Err = DomainError> + Display,
{
    let id: T = raw.parse()?;
    note_suffix(raw, &id.to_string(), diags);
    Ok(id)
}

/// Compte le suffixe retiré de `raw`, hors `LEGITEXT` : `normalized` est l'identifiant lu.
///
/// Seconde moitié de [`parse_counted`], pour un identifiant compté seulement s'il est retenu.
pub(crate) fn note_suffix(raw: &str, normalized: &str, diags: &mut Diagnostics) {
    if suffix_dropped(raw, normalized) {
        diags.note_suffix_stripped();
    }
}

/// Vrai si `raw` portait un suffixe retiré, hors `LEGITEXT` : `normalized` est l'identifiant lu.
fn suffix_dropped(raw: &str, normalized: &str) -> bool {
    raw.contains('_') && !normalized.starts_with("LEGITEXT")
}

/// Date du suffixe de version d'un `LEGITEXT`, sentinelles écartées (`open_ended`).
///
/// Un identifiant sans suffixe, d'une autre famille ou au suffixe illisible rend `None`.
pub(crate) fn suffix_start(raw: &str) -> Option<CivilDate> {
    open_ended(parse_versioned(raw).ok()?.version_start)
}

/// Règle (d), contrôle seul : compte `origin_mismatch` si `origin` contredit la famille de `id`.
///
/// Un `origin` absent ou blanc, une famille dont l'origine n'est pas mesurée, ou un accord (casse
/// ignorée, blancs retirés) ne comptent rien.
pub(crate) fn check_origin(origin: Option<&str>, id: &str, diags: &mut Diagnostics) {
    let Some(origin) = origin.map(str::trim).filter(|origin| !origin.is_empty()) else {
        return;
    };
    if expected_origin(id).is_some_and(|expected| !expected.eq_ignore_ascii_case(origin)) {
        diags.note_origin_mismatch();
    }
}

/// Origine attendue pour la famille de `id`, d'après ses huit premières lettres.
fn expected_origin(id: &str) -> Option<&'static str> {
    let prefix = id.get(..8)?;
    ORIGINS.iter().find(|(known, _)| *known == prefix).map(|(_, origin)| *origin)
}

#[cfg(test)]
mod tests {
    use lf_domain::id::{
        AccoTextId, ArticleId, CnilTextId, DecisionId, DocumentId, KaliTextId, LegiTextId,
    };

    use super::*;
    use crate::dto::acco::ConsultAccoResponseDto;
    use crate::dto::article::GetArticleResponseDto;
    use crate::dto::juri::ConsultJuriResponseDto;
    use crate::dto::search::SearchResponseDto;
    use crate::map::endpoint;
    use crate::map::fixtures::{self, NAMES, d, decode_as, read};

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

    /// Champ `id` de la consultation d'un texte.
    const ID: Field = Field { endpoint: endpoint::LAW_DECREE, path: "id" };

    /// `resolve_id` en [`ID`], avec les diagnostics qu'il laisse.
    fn resolve<T>(requested: &T, returned: Option<&str>) -> (Result<T, CorpusError>, Diagnostics)
    where
        T: std::str::FromStr<Err = lf_domain::DomainError> + std::fmt::Display + PartialEq + Clone,
    {
        let mut diags = Diagnostics::default();
        let result = resolve_id(requested, returned, ID, &mut diags);
        (result, diags)
    }

    /// `parse_counted` d'un identifiant de document, avec les diagnostics qu'il laisse.
    fn count(raw: &str) -> (Result<DocumentId, lf_domain::DomainError>, Diagnostics) {
        let mut diags = Diagnostics::default();
        let result = parse_counted::<DocumentId>(raw, &mut diags);
        (result, diags)
    }

    /// Diagnostics d'un seul `id_mismatch`.
    fn one_mismatch() -> Diagnostics {
        Diagnostics { id_mismatch: 1, ..Diagnostics::default() }
    }

    /// Diagnostics d'un seul suffixe retiré.
    fn one_stripped() -> Diagnostics {
        Diagnostics { upstream_suffix_stripped: 1, ..Diagnostics::default() }
    }

    /// Diagnostics d'une seule origine en désaccord.
    fn one_origin() -> Diagnostics {
        Diagnostics { origin_mismatch: 1, ..Diagnostics::default() }
    }

    /// Identifiant `titles[0].id` de chacun des résultats des fixtures de recherche.
    fn search_hit_ids() -> Vec<String> {
        NAMES
            .iter()
            .filter(|name| name.starts_with("search_"))
            .flat_map(|name| decode_as::<SearchResponseDto>(name).results)
            .map(|result| result.titles[0].id.clone().expect("hit id"))
            .collect()
    }

    #[test]
    fn suffix_is_version_date() {
        let loda = decode_as::<ConsultTextDto>("consult_loda");
        let id = loda.id.as_deref().unwrap();
        assert_eq!(id, "LEGITEXT000033205014_08-09-2023");
        let start = Field { endpoint: endpoint::LAW_DECREE, path: "dateDebutVersion" };
        let mut diags = Diagnostics::default();
        let civil = crate::map::dates::civil(loda.date_debut_version.as_ref(), start, &mut diags);
        assert_eq!(suffix_start(id), civil.unwrap());
        assert_eq!(suffix_start(id), Some(d(2023, 9, 8)));

        let requested = LegiTextId::parse("LEGITEXT000033205014").unwrap();
        let (result, diags) = resolve(&requested, Some(id));
        assert_eq!(result, Ok(requested));
        assert_eq!(diags, Diagnostics::default());

        let search = decode_as::<SearchResponseDto>("search_loda");
        let hit = &search.results[1];
        let hit_id = hit.titles[0].id.as_deref().unwrap();
        assert_eq!(hit_id, "LEGITEXT000033696785_28-12-2016");
        let field = Field { endpoint: endpoint::SEARCH, path: "results[].date" };
        let mut date_diags = Diagnostics::default();
        let date = crate::map::dates::civil(hit.date.as_ref(), field, &mut date_diags);
        assert_eq!(suffix_start(hit_id), date.unwrap());
        assert_eq!(suffix_start(hit_id), Some(d(2016, 12, 28)));
    }

    #[test]
    fn suffix_start_reads_only_a_legitext_suffix() {
        assert_eq!(suffix_start("LEGITEXT000033205014"), None);
        assert_eq!(suffix_start(" legitext000033205014_08-09-2023 "), Some(d(2023, 9, 8)));
        assert_eq!(suffix_start("LEGITEXT000033205014_01-01-2999"), None);
        assert_eq!(suffix_start("LEGITEXT000033205014_01-01-2222"), None);
        assert_eq!(suffix_start("LEGITEXT000033205014_31-12-2221"), Some(d(2221, 12, 31)));
        assert_eq!(suffix_start("JURITEXT000048465611_01-01-2000"), None);
        assert_eq!(suffix_start("LEGITEXT000033205014_31-02-2023"), None);
    }

    /// `resolve_id` avec l'identifiant demandé lui-même rendu, pour le type `T`.
    fn assert_same_id<T>(requested: &str)
    where
        T: std::str::FromStr<Err = lf_domain::DomainError>
            + std::fmt::Display
            + PartialEq
            + Clone
            + std::fmt::Debug,
    {
        let id: T = requested.parse().unwrap();
        let (result, diags) = resolve(&id, Some(requested));
        assert_eq!(result.unwrap(), id, "{requested}");
        assert_eq!(diags, Diagnostics::default(), "{requested}");
    }

    #[test]
    fn matching_ids_leave_no_diagnostic() {
        for fixture in fixtures::ALL.iter().filter(|fixture| fixture.name.starts_with("consult_")) {
            let requested = fixture.requested;
            match fixture.name {
                "consult_acco" => assert_same_id::<AccoTextId>(requested),
                "consult_article" => assert_same_id::<ArticleId>(requested),
                "consult_cnil" => assert_same_id::<CnilTextId>(requested),
                "consult_kali" => assert_same_id::<KaliTextId>(requested),
                "consult_loda" => assert_same_id::<LegiTextId>(requested),
                _ => assert_same_id::<DecisionId>(requested),
            }
        }
    }

    #[test]
    fn returned_id_differing_from_requested_is_kept_and_counted() {
        let requested = LegiTextId::parse("LEGITEXT000033205014").unwrap();
        let (result, diags) = resolve(&requested, Some("LEGITEXT000006070721"));
        assert_eq!(result, Ok(LegiTextId::parse("LEGITEXT000006070721").unwrap()));
        assert_eq!(diags, one_mismatch());

        let requested = DecisionId::parse("JURITEXT000051743650").unwrap();
        let (result, diags) = resolve(&requested, Some("CETATEXT000054178503"));
        assert_eq!(result, Ok(DecisionId::parse("CETATEXT000054178503").unwrap()));
        assert_eq!(diags, one_mismatch());
    }

    #[test]
    fn requested_kaliarti_with_legiarti_body_keeps_the_returned_id() {
        let requested = ArticleId::parse("KALIARTI000049886059").unwrap();
        let (result, diags) = resolve(&requested, Some("LEGIARTI000006419288"));
        let returned = result.unwrap();
        assert_eq!(returned, ArticleId::parse("LEGIARTI000006419288").unwrap());
        assert_eq!(returned.as_str(), "LEGIARTI000006419288");
        assert_eq!(diags, one_mismatch());
    }

    #[test]
    fn missing_or_blank_returned_id_falls_back_to_requested() {
        let requested = LegiTextId::parse("LEGITEXT000033205014").unwrap();
        for returned in [None, Some(""), Some("  ")] {
            let (result, diags) = resolve(&requested, returned);
            assert_eq!(result, Ok(requested.clone()), "{returned:?}");
            assert_eq!(diags, one_mismatch(), "{returned:?}");
        }
    }

    #[test]
    fn malformed_returned_id_is_a_decode_error_without_the_value() {
        let requested = LegiTextId::parse("LEGITEXT000033205014").unwrap();
        for returned in ["LEGITEXT0000", "JURITEXT000051743650"] {
            let (result, diags) = resolve(&requested, Some(returned));
            let error = result.unwrap_err();
            assert_eq!(
                error,
                CorpusError::Decode { detail: "/consult/lawDecree: id: invalid identifier".into() }
            );
            assert!(!format!("{error:?} {error}").contains(returned), "{returned}");
            assert_eq!(diags, Diagnostics::default(), "{returned}");
        }
    }

    #[test]
    fn upstream_suffix_on_other_prefix_is_stripped_with_diagnostic() {
        let requested = AccoTextId::parse("ACCOTEXT000053643828").unwrap();
        let (result, diags) = resolve(&requested, Some("ACCOTEXT000053643828_01-01-2020"));
        assert_eq!(result, Ok(requested));
        assert_eq!(diags, one_stripped());

        for raw in [
            "LEGIARTI000006419288_08-09-2023",
            "CETATEXT000054178503_01-01-2020",
            " accotext000053643828_01-01-2020 ",
            "CNILTEXT000053352643_08-01-2026",
        ] {
            let (result, diags) = count(raw);
            assert!(result.is_ok(), "{raw}");
            assert!(!result.unwrap().as_str().contains('_'), "{raw}");
            assert_eq!(diags, one_stripped(), "{raw}");
        }
        for raw in ["ACCOTEXT000053643828", "CNILTEXT000053352643"] {
            assert_eq!(count(raw).1, Diagnostics::default(), "{raw}");
        }
    }

    #[test]
    fn legitext_suffix_is_not_counted() {
        let raw = "LEGITEXT000033205014_08-09-2023";
        let mut diags = Diagnostics::default();
        assert_eq!(
            parse_counted::<LegiTextId>(raw, &mut diags),
            Ok(LegiTextId::parse("LEGITEXT000033205014").unwrap())
        );
        assert_eq!(diags, Diagnostics::default());
        let (result, diags) = count(raw);
        assert!(result.is_ok());
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn malformed_suffix_is_refused_and_not_counted() {
        for raw in
            ["JURITEXT000051743650_foo", "ACCOTEXT000053643828_31-02-2023", "LEGITEXT000033205014_"]
        {
            let (result, diags) = count(raw);
            let error = result.unwrap_err().to_string();
            assert!(error.starts_with("invalid version suffix"), "{raw}: {error}");
            assert_eq!(diags, Diagnostics::default(), "{raw}");
        }
        let requested = DecisionId::parse("JURITEXT000051743650").unwrap();
        let (result, diags) = resolve(&requested, Some("JURITEXT000051743650_foo"));
        assert_eq!(
            result,
            Err(CorpusError::Decode {
                detail: "/consult/lawDecree: id: invalid identifier".into()
            })
        );
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn suffix_dropped_table() {
        assert!(suffix_dropped("ACCOTEXT000053643828_01-01-2020", "ACCOTEXT000053643828"));
        assert!(!suffix_dropped("LEGITEXT000033205014_08-09-2023", "LEGITEXT000033205014"));
        assert!(!suffix_dropped("ACCOTEXT000053643828", "ACCOTEXT000053643828"));
    }

    #[test]
    fn note_suffix_counts_only_a_stripped_suffix_outside_legitext() {
        let mut diags = Diagnostics::default();
        note_suffix("ACCOTEXT000053643828", "ACCOTEXT000053643828", &mut diags);
        note_suffix("LEGITEXT000033205014_08-09-2023", "LEGITEXT000033205014", &mut diags);
        assert_eq!(diags, Diagnostics::default());
        note_suffix("ACCOTEXT000053643828_01-01-2020", "ACCOTEXT000053643828", &mut diags);
        assert_eq!(diags, one_stripped());
    }

    #[test]
    fn search_fixture_hit_ids_parse_without_diagnostic() {
        let ids = search_hit_ids();
        assert_eq!(ids.len(), 17);
        let mut diags = Diagnostics::default();
        for id in &ids {
            assert!(parse_counted::<DocumentId>(id, &mut diags).is_ok(), "{id}");
        }
        assert_eq!(diags, Diagnostics::default());

        let suffixed: Vec<&String> = ids.iter().filter(|id| id.contains('_')).collect();
        assert_eq!(suffixed.len(), 3);
        assert!(suffixed.iter().all(|id| id.starts_with("LEGITEXT")));

        let count_of = |prefix: &str| ids.iter().filter(|id| id.starts_with(prefix)).count();
        let counts = [
            count_of("LEGITEXT"),
            count_of("JURITEXT"),
            count_of("CONSTEXT"),
            count_of("CETATEXT"),
            count_of("CNILTEXT"),
            count_of("ACCOTEXT"),
            count_of("KALITEXT"),
        ];
        assert_eq!(counts, [3, 2, 3, 3, 2, 2, 2]);
    }

    #[test]
    fn origine_mismatch_emits_diagnostic() {
        let cases = [
            (Some("CETAT"), "JURITEXT000051743650", 1),
            (Some("JURI"), "LEGITEXT000033205014", 1),
            (Some("juri "), "JURITEXT000051743650", 0),
            (None, "JURITEXT000051743650", 0),
            (Some(""), "JURITEXT000051743650", 0),
            (Some("  "), "JURITEXT000051743650", 0),
            (Some("LEGI"), "JORFTEXT000033202746", 0),
            (Some("LEGI"), "LEGIARTI000006419288", 0),
            (Some("KALI"), "KALIARTI000005805295", 0),
            (Some("JURI"), "JURI", 0),
        ];
        for (origin, id, expected) in cases {
            let mut diags = Diagnostics::default();
            check_origin(origin, id, &mut diags);
            let want = if expected == 1 { one_origin() } else { Diagnostics::default() };
            assert_eq!(diags, want, "{origin:?} {id}");
        }
    }

    #[test]
    fn origin_matches_on_every_fixture() {
        let mut diags = Diagnostics::default();
        for name in ["consult_juri", "consult_juri_cetat", "consult_juri_constit", "consult_cnil"] {
            let text = decode_as::<ConsultJuriResponseDto>(name).text.unwrap();
            assert!(text.origine.is_some(), "{name}");
            check_origin(text.origine.as_deref(), text.id.as_deref().unwrap(), &mut diags);
        }
        let hits: Vec<_> = NAMES
            .iter()
            .filter(|name| name.starts_with("search_"))
            .flat_map(|name| decode_as::<SearchResponseDto>(name).results)
            .collect();
        assert_eq!(hits.len(), 17);
        for hit in &hits {
            check_origin(hit.origin.as_deref(), hit.titles[0].id.as_deref().unwrap(), &mut diags);
        }
        assert_eq!(diags, Diagnostics::default());

        for (prefix, _) in ORIGINS {
            assert!(expected_origin(&format!("{prefix}000000000001")).is_some(), "{prefix}");
        }
        assert_eq!(expected_origin("JORFTEXT000033202746"), None);
        assert_eq!(expected_origin("JURI"), None);
    }
}
