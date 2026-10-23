//! Texte de loi ou de décret consolidé de `/consult/lawDecree` vers `Act` (04 §9.1 a, b, c, e,
//! g ; §9.2).
//!
//! Un objet sans identité est un `NotFound`. L'identifiant renvoyé est gardé, même s'il diffère
//! de celui demandé (compté) ; absent ou blanc, c'est l'identifiant demandé (compté aussi). Le
//! suffixe de version `_JJ-MM-AAAA` de l'identifiant renvoyé, retiré de `text_id`, donne le
//! début de la version, à défaut `dateDebutVersion`. Le reste est le [`TextBody`] commun aux
//! textes consolidés.
//!
//! [`TextBody`]: lf_domain::entity::TextBody

use lf_application::ports::CorpusError;
use lf_domain::entity::Act;
use lf_domain::id::LegiTextId;

use super::body::text_body;
use super::endpoint::LAW_DECREE;
use super::rules::{require, resolve_id, suffix_start};
use super::{Diagnostics, Field};
use crate::dto::consult_text::ConsultTextDto;

/// Champ `id` de la réponse.
const ID: Field = Field { endpoint: LAW_DECREE, path: "id" };

/// Loi ou décret consolidé de `dto`, demandé sous `requested` ; ses anomalies sont comptées dans
/// `diags`.
///
/// # Errors
///
/// [`CorpusError::NotFound`] (`/consult/lawDecree`) si l'objet n'a ni `id`, ni `cid`, ni titre
/// non blanc ; `diags` n'est alors pas touché. [`CorpusError::Decode`]
/// (`/consult/lawDecree: id: invalid identifier`) si l'identifiant renvoyé n'est pas un
/// `LEGITEXT` lisible, puis toute erreur de `text_body`.
pub(crate) fn act(
    dto: ConsultTextDto,
    requested: &LegiTextId,
    diags: &mut Diagnostics,
) -> Result<Act, CorpusError> {
    let dto = require(Some(dto), LAW_DECREE)?;
    let text_id = resolve_id(requested, dto.id.as_deref(), ID, diags)?;
    let suffix = dto.id.as_deref().and_then(suffix_start);
    let body = text_body(dto, LAW_DECREE, suffix, diags)?;
    Ok(Act { text_id, body })
}

#[cfg(test)]
mod tests {
    use lf_domain::contract::{Citable, Nor};
    use lf_domain::date::CivilDate;
    use lf_domain::id::DocumentId;
    use lf_domain::status::LegalStatus;

    use super::*;
    use crate::dto::de::RawDate;
    use crate::map::fixtures::decode_as;

    /// `Some(valeur.to_owned())` : une macro et non une fonction, que `clippy::unnecessary_wraps`
    /// refuserait.
    macro_rules! text {
        ($value:expr) => {
            Some($value.to_owned())
        };
    }

    /// Identifiant demandé dans la plupart des tests.
    const REQUESTED: &str = "LEGITEXT000033205014";

    /// Identifiant `LEGITEXT` valide.
    fn legi(raw: &str) -> LegiTextId {
        LegiTextId::parse(raw).unwrap()
    }

    /// Date civile valide.
    fn d(year: u16, month: u8, day: u8) -> CivilDate {
        CivilDate::new(year, month, day).unwrap()
    }

    /// Date brute écrite en texte.
    fn date(value: &str) -> RawDate {
        RawDate::Text(value.to_owned())
    }

    /// Texte d'identifiant `id`, sans autre champ.
    fn with_id(id: &str) -> ConsultTextDto {
        ConsultTextDto { id: text!(id), ..ConsultTextDto::default() }
    }

    /// `act` de `dto` demandé en `requested`, et les diagnostics qu'il laisse.
    fn act_of(dto: ConsultTextDto, requested: &str) -> (Result<Act, CorpusError>, Diagnostics) {
        let mut diags = Diagnostics::default();
        let act = act(dto, &legi(requested), &mut diags);
        (act, diags)
    }

    #[test]
    fn act_fixture_maps() {
        let title = "LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique (1)";
        let (act, diags) = act_of(decode_as::<ConsultTextDto>("consult_loda"), REQUESTED);
        let act = act.unwrap();
        assert_eq!(act.text_id, legi(REQUESTED));
        assert_eq!(act.text_id.as_str(), "LEGITEXT000033205014");

        let body = &act.body;
        assert_eq!(body.cid.as_ref().map(DocumentId::as_str), Some("JORFTEXT000033202746"));
        assert_eq!(body.cid.as_ref().map(DocumentId::prefix), Some("JORFTEXT"));
        assert_eq!(body.nor.as_ref().map(Nor::as_str), Some("ECFI1524250L"));
        assert_eq!(body.title.as_deref(), Some(title));
        assert_eq!(body.nature.as_deref(), Some("LOI"));
        assert_eq!(body.etat, Some(LegalStatus::Vigueur));
        assert_eq!(body.date_parution, Some(d(2016, 10, 8)));
        assert_eq!(body.num_parution.as_deref(), Some("0235"));
        assert_eq!(body.date_debut_version, Some(d(2023, 9, 8)));
        assert_eq!(body.date_fin_version, None);
        assert_eq!(body.jo_reference.as_deref(), Some("JORF n°0235 du 8 octobre 2016"));
        assert_eq!(body.aliases, ["loi Lemaire", "loi numérique", "loi Lemaire [2016]"]);
        assert_eq!((body.toc.len(), body.toc.dropped(), body.toc.roots().len()), (88, 0, 3));
        assert_eq!(diags, Diagnostics::default());

        let contract = act.contract();
        assert_eq!(
            contract.source_url.as_str(),
            "https://www.legifrance.gouv.fr/loda/id/LEGITEXT000033205014"
        );
        assert_eq!(contract.citation.as_str(), title);
    }

    #[test]
    fn act_invalid_nor_is_none_with_a_diagnostic() {
        let dto = ConsultTextDto { nor: text!("pas-un-nor"), ..with_id(REQUESTED) };
        let (act, diags) = act_of(dto, REQUESTED);
        assert_eq!(act.unwrap().body.nor, None);
        assert_eq!(diags, Diagnostics { invalid_nor: 1, ..Diagnostics::default() });
    }

    #[test]
    fn act_empty_object_is_not_found() {
        let before = Diagnostics { id_mismatch: 2, invalid_nor: 3, ..Diagnostics::default() };
        let cases = [
            ConsultTextDto::default(),
            ConsultTextDto { title: text!("  "), ..ConsultTextDto::default() },
            ConsultTextDto {
                nor: text!("pas-un-nor"),
                nature: text!("LOI"),
                ..ConsultTextDto::default()
            },
        ];
        for dto in cases {
            let mut diags = before;
            assert_eq!(
                act(dto, &legi(REQUESTED), &mut diags),
                Err(CorpusError::NotFound { what: "/consult/lawDecree".into() })
            );
            assert_eq!(diags, before);
        }
    }

    #[test]
    fn act_with_only_a_title_uses_the_requested_id() {
        let dto = ConsultTextDto { title: text!("Loi n° 1"), ..ConsultTextDto::default() };
        let (act, diags) = act_of(dto, REQUESTED);
        let act = act.unwrap();
        assert_eq!(act.text_id, legi(REQUESTED));
        assert_eq!(act.body.title.as_deref(), Some("Loi n° 1"));
        assert_eq!(act.body.date_debut_version, None);
        assert_eq!(diags, Diagnostics { id_mismatch: 1, ..Diagnostics::default() });
    }

    #[test]
    fn act_keeps_the_returned_id_on_mismatch() {
        let (act, diags) = act_of(with_id("LEGITEXT000006069414"), REQUESTED);
        assert_eq!(act.unwrap().text_id, legi("LEGITEXT000006069414"));
        assert_eq!(diags, Diagnostics { id_mismatch: 1, ..Diagnostics::default() });

        let (act, diags) = act_of(with_id(" legitext000033205014_08-09-2023 "), REQUESTED);
        let act = act.unwrap();
        assert_eq!(act.text_id, legi(REQUESTED));
        assert_eq!(act.body.date_debut_version, Some(d(2023, 9, 8)));
        assert_eq!((diags.id_mismatch, diags.upstream_suffix_stripped), (0, 0));
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn act_unparsable_id_is_a_decode_error() {
        for raw in ["JORFTEXT000033202746", "LEGITEXT000033205014_31-02-2023", "n'importe quoi"] {
            let (act, diags) = act_of(with_id(raw), REQUESTED);
            let error = act.unwrap_err();
            assert_eq!(
                error,
                CorpusError::Decode { detail: "/consult/lawDecree: id: invalid identifier".into() },
                "{raw}"
            );
            assert!(!format!("{error:?} {error}").contains(raw), "{raw}");
            assert_eq!(diags, Diagnostics::default(), "{raw}");
        }
    }

    #[test]
    fn act_version_start_is_the_suffix_then_date_debut_version() {
        let cases = [
            ("LEGITEXT000033205014_08-09-2023", "2020-01-01", Some(d(2023, 9, 8))),
            (REQUESTED, "2020-01-01", Some(d(2020, 1, 1))),
            (REQUESTED, "2999-01-01", None),
            ("LEGITEXT000033205014_01-01-2999", "2020-01-01", Some(d(2020, 1, 1))),
        ];
        for (id, start, expected) in cases {
            let dto = ConsultTextDto { date_debut_version: Some(date(start)), ..with_id(id) };
            let (act, diags) = act_of(dto, REQUESTED);
            assert_eq!(act.unwrap().body.date_debut_version, expected, "{id} {start}");
            assert_eq!(diags, Diagnostics::default(), "{id} {start}");
        }

        let unreadable = ConsultTextDto {
            date_debut_version: Some(date("hier")),
            ..with_id("LEGITEXT000033205014_08-09-2023")
        };
        assert_eq!(
            act_of(unreadable, REQUESTED).0,
            Err(CorpusError::Decode {
                detail: "/consult/lawDecree: dateDebutVersion: unreadable date".into()
            })
        );

        for (end, expected) in [("2999-01-01", None), ("2024-05-01", Some(d(2024, 5, 1)))] {
            let dto = ConsultTextDto { date_fin_version: Some(date(end)), ..with_id(REQUESTED) };
            let (act, _) = act_of(dto, REQUESTED);
            assert_eq!(act.unwrap().body.date_fin_version, expected, "{end}");
        }
    }

    #[test]
    fn act_nature_is_cleaned_before_the_contract() {
        let code = "https://www.legifrance.gouv.fr/codes/texte_lc/LEGITEXT000006069414";
        let loda = "https://www.legifrance.gouv.fr/loda/id/LEGITEXT000006069414";
        let cases =
            [(" code\n", "code", code), ("<b>CODE</b>", "CODE", code), ("LOI", "LOI", loda)];
        for (raw, nature, url) in cases {
            let dto = ConsultTextDto {
                nature: text!(raw),
                title: text!("Code civil"),
                ..with_id("LEGITEXT000006069414")
            };
            let (act, _) = act_of(dto, "LEGITEXT000006069414");
            let act = act.unwrap();
            assert_eq!(act.body.nature.as_deref(), Some(nature), "{raw}");
            assert_eq!(act.contract().source_url.as_str(), url, "{raw}");
        }
    }
}
