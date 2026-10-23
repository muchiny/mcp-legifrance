//! Fonctions communes aux mappers des textes consolidés (04 §9.1 c, e, g ; §9.2 ; 03 §4.2).
//!
//! Elles lisent les champs courts que partagent `lawDecree`, `kaliText` et `getArticle` :
//! texte rogné (`non_blank`), `cid` chronologique (`cid_of`), NOR (`nor_of`) et appellations
//! (`aliases_of`). [`text_body`] assemble avec elles le [`TextBody`] commun à `lawDecree` et
//! `kaliText`. Aucune valeur du corps ne sort d'ici : un `cid` ou une date refusés deviennent un
//! [`CorpusError::Decode`] qui ne nomme que l'endpoint et le chemin du champ, un NOR refusé est
//! compté (`invalid_nor`) puis omis.

use lf_application::ports::CorpusError;
use lf_domain::contract::Nor;
use lf_domain::date::CivilDate;
use lf_domain::entity::TextBody;
use lf_domain::id::DocumentId;
use lf_domain::status::status_of;
use lf_domain::text::clean_field;

use super::dates::civil;
use super::rules::parse_counted;
use super::{Diagnostics, Field, toc};
use crate::dto::consult_text::ConsultTextDto;

/// Métadonnées et table des matières d'un texte consolidé lu pour `endpoint`.
///
/// Ordre d'évaluation figé, la première erreur l'emporte : `cid`, puis les dates `dateParution`,
/// `dateDebutVersion` et `dateFinVersion`, puis la table des matières. Le début de la version est
/// `suffix_start` (date du suffixe de l'identifiant renvoyé) s'il est connu, sinon
/// `dateDebutVersion`, qui est lu dans les deux cas. Les textes courts (titre, nature, numéro de
/// parution, référence au JO) passent par `clean_field` ; le statut est `etat`, à défaut
/// `jurisState`. NOR refusés, horodatages hors minuit, articles écartés et invisibles de la table
/// des matières sont comptés dans `diags`.
///
/// # Errors
///
/// [`CorpusError::Decode`] du premier champ refusé : `<endpoint>: cid: invalid identifier`,
/// `<endpoint>: <date>: unreadable date` (`dateParution`, `dateDebutVersion`, `dateFinVersion`)
/// ou l'erreur de la table des matières (`toc::build`). Seuls les horodatages hors minuit des
/// dates lues avant l'erreur sont alors comptés.
pub(super) fn text_body(
    dto: ConsultTextDto,
    endpoint: &'static str,
    suffix_start: Option<CivilDate>,
    diags: &mut Diagnostics,
) -> Result<TextBody, CorpusError> {
    let field = |path| Field { endpoint, path };
    let cid = cid_of(dto.cid.as_deref(), endpoint, diags)?;
    let date_parution = civil(dto.date_parution.as_ref(), field("dateParution"), diags)?;
    let version_start = civil(dto.date_debut_version.as_ref(), field("dateDebutVersion"), diags)?;
    let date_fin_version = civil(dto.date_fin_version.as_ref(), field("dateFinVersion"), diags)?;
    let toc = toc::build(dto.sections, dto.articles, endpoint, diags)?;
    Ok(TextBody {
        cid,
        nor: nor_of(dto.nor.as_deref(), diags),
        title: clean_field(dto.title.as_deref()),
        nature: clean_field(dto.nature.as_deref()),
        etat: status_of(dto.etat.as_deref(), dto.juris_state.as_deref()),
        date_parution,
        num_parution: clean_field(dto.num_parution.as_deref()),
        date_debut_version: suffix_start.or(version_start),
        date_fin_version,
        jo_reference: clean_field(dto.jorf_text.as_deref()),
        aliases: aliases_of(&dto.appellations),
        toc,
    })
}

/// `raw` rogné ; `None` s'il est absent ou vide une fois rogné.
pub(super) fn non_blank(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|text| !text.is_empty())
}

/// Identifiant chronologique (`cid`) de `raw`, `None` s'il est absent ou blanc.
///
/// Lu par `parse_counted` (règle c) : un suffixe de version retiré d'un `cid` autre que
/// `LEGITEXT` est compté dans `diags` (`upstream_suffix_stripped`).
///
/// # Errors
///
/// [`CorpusError::Decode`] (`<endpoint>: cid: invalid identifier`) si le `cid` présent n'est pas
/// un identifiant Légifrance valide : un `cid` illisible n'est jamais omis en silence.
pub(super) fn cid_of(
    raw: Option<&str>,
    endpoint: &'static str,
    diags: &mut Diagnostics,
) -> Result<Option<DocumentId>, CorpusError> {
    non_blank(raw)
        .map(|cid| {
            parse_counted::<DocumentId>(cid, diags)
                .map_err(|error| Field { endpoint, path: "cid" }.domain(&error))
        })
        .transpose()
}

/// NOR de `raw`, `None` s'il est absent, blanc ou illisible ; un NOR présent mais illisible est
/// compté (`invalid_nor`) et omis, sans erreur.
pub(super) fn nor_of(raw: Option<&str>, diags: &mut Diagnostics) -> Option<Nor> {
    let raw = non_blank(raw)?;
    let nor = Nor::parse(raw);
    if nor.is_none() {
        diags.note_invalid_nor();
    }
    nor
}

/// Appellations nettoyées par `clean_field`, dans l'ordre reçu, celles qui sont vides écartées.
pub(super) fn aliases_of(appellations: &[String]) -> Vec<String> {
    appellations.iter().filter_map(|alias| clean_field(Some(alias))).collect()
}

#[cfg(test)]
mod tests {
    use lf_domain::entity::{TocArticle, TocNode, TocSection};
    use lf_domain::id::{ArticleId, DocumentId, SectionId};
    use lf_domain::status::LegalStatus;

    use super::*;
    use crate::dto::consult_text::{ConsultTextDto, TocArticleDto, TocSectionDto};
    use crate::dto::de::RawDate;
    use crate::map::endpoint;
    use crate::map::fixtures::{d, text};

    /// Date brute écrite en texte.
    fn date(value: &str) -> RawDate {
        RawDate::Text(value.to_owned())
    }

    /// `text_body` de `dto` pour `/consult/lawDecree`, sans suffixe, et les diagnostics qu'il
    /// laisse.
    fn body_of(dto: ConsultTextDto) -> (Result<TextBody, CorpusError>, Diagnostics) {
        let mut diags = Diagnostics::default();
        let body = text_body(dto, endpoint::LAW_DECREE, None, &mut diags);
        (body, diags)
    }

    /// Erreur d'une date illisible au champ `path` de `endpoint`.
    fn unreadable(endpoint: &str, path: &str) -> CorpusError {
        CorpusError::Decode { detail: format!("{endpoint}: {path}: unreadable date") }
    }

    /// `levels` sections imbriquées, titrées `S1` à `S<levels>`, sans article.
    fn chain(levels: usize) -> Vec<TocSectionDto> {
        let mut sections = Vec::new();
        for level in (1..=levels).rev() {
            let title = Some(format!("S{level}"));
            sections = vec![TocSectionDto { title, sections, ..TocSectionDto::default() }];
        }
        sections
    }

    #[test]
    fn loda_status_falls_back_to_juris_state() {
        let cases = [
            (None, Some("Vigueur"), Some(LegalStatus::Vigueur)),
            (Some("ABROGE"), Some("Vigueur"), Some(LegalStatus::Abroge)),
            (Some("  "), Some("VIGUEUR_ETEN"), Some(LegalStatus::VigueurEtendue)),
            (None, None, None),
        ];
        for (etat, juris_state, expected) in cases {
            let dto = ConsultTextDto {
                etat: etat.map(str::to_owned),
                juris_state: juris_state.map(str::to_owned),
                ..ConsultTextDto::default()
            };
            let (body, diags) = body_of(dto);
            assert_eq!(body.unwrap().etat, expected, "{etat:?} {juris_state:?}");
            assert_eq!(diags, Diagnostics::default());
        }
    }

    #[test]
    fn short_fields_are_cleaned_and_never_empty() {
        let dto = ConsultTextDto {
            title: text!(" Loi  n° 1 <mark>x</mark> "),
            nature: text!("  "),
            num_parution: text!(" 0235 "),
            jorf_text: text!("<i>JORF</i> n°0235"),
            ..ConsultTextDto::default()
        };
        let body = body_of(dto).0.unwrap();
        assert_eq!(
            (
                body.title.as_deref(),
                body.nature.as_deref(),
                body.num_parution.as_deref(),
                body.jo_reference.as_deref(),
            ),
            (Some("Loi n° 1 x"), None, Some("0235"), Some("JORF n°0235"))
        );

        let blank = ConsultTextDto {
            title: text!("<br>"),
            nature: text!("<b> </b>"),
            num_parution: text!("\n"),
            jorf_text: text!(" <i></i> "),
            ..ConsultTextDto::default()
        };
        let body = body_of(blank).0.unwrap();
        assert_eq!(
            (body.title, body.nature, body.num_parution, body.jo_reference),
            (None, None, None, None)
        );
    }

    #[test]
    fn a_decode_error_names_the_first_failing_field() {
        let cid_first = ConsultTextDto {
            cid: text!("FOO"),
            date_parution: Some(date("hier")),
            ..ConsultTextDto::default()
        };
        assert_eq!(
            body_of(cid_first).0,
            Err(CorpusError::Decode {
                detail: "/consult/lawDecree: cid: invalid identifier".into()
            })
        );

        let two_dates = ConsultTextDto {
            cid: text!("JORFTEXT000033202746"),
            date_parution: Some(date("hier")),
            date_debut_version: Some(date("hier")),
            ..ConsultTextDto::default()
        };
        assert_eq!(body_of(two_dates).0, Err(unreadable(endpoint::LAW_DECREE, "dateParution")));

        let start_only =
            ConsultTextDto { date_debut_version: Some(date("hier")), ..ConsultTextDto::default() };
        let error = body_of(start_only).0.unwrap_err();
        assert_eq!(
            error,
            CorpusError::Decode {
                detail: "/consult/lawDecree: dateDebutVersion: unreadable date".into()
            }
        );
        assert!(!format!("{error:?} {error}").contains("hier"), "{error:?}");

        let start_then_end = ConsultTextDto {
            date_debut_version: Some(date("hier")),
            date_fin_version: Some(date("demain")),
            ..ConsultTextDto::default()
        };
        assert_eq!(
            body_of(start_then_end).0,
            Err(unreadable(endpoint::LAW_DECREE, "dateDebutVersion"))
        );

        // La date de fin passe avant la table des matières, qui n'est lue qu'en dernier.
        let end_then_toc = ConsultTextDto {
            date_fin_version: Some(date("demain")),
            sections: chain(65),
            ..ConsultTextDto::default()
        };
        assert_eq!(
            body_of(end_then_toc).0,
            Err(unreadable(endpoint::LAW_DECREE, "dateFinVersion"))
        );
        let toc_only = ConsultTextDto { sections: chain(65), ..ConsultTextDto::default() };
        assert_eq!(
            body_of(toc_only).0,
            Err(CorpusError::Decode {
                detail: "/consult/lawDecree: sections: table of contents too deep".into()
            })
        );
    }

    #[test]
    fn decode_errors_name_the_endpoint_given() {
        let cases = [
            (
                ConsultTextDto { cid: text!("FOO"), ..ConsultTextDto::default() },
                "cid",
                "invalid identifier",
            ),
            (
                ConsultTextDto { date_parution: Some(date("hier")), ..ConsultTextDto::default() },
                "dateParution",
                "unreadable date",
            ),
            (
                ConsultTextDto {
                    date_fin_version: Some(date("hier")),
                    ..ConsultTextDto::default()
                },
                "dateFinVersion",
                "unreadable date",
            ),
        ];
        for (dto, path, category) in cases {
            let mut diags = Diagnostics::default();
            assert_eq!(
                text_body(dto, endpoint::KALI_TEXT, None, &mut diags),
                Err(CorpusError::Decode {
                    detail: format!("/consult/kaliText: {path}: {category}")
                })
            );
            assert_eq!(diags, Diagnostics::default(), "{path}");
        }
    }

    #[test]
    fn text_body_builds_the_toc_from_sections_and_root_articles() {
        let dto = ConsultTextDto {
            sections: vec![TocSectionDto {
                id: text!("LEGISCTA000033205040"),
                title: text!("Titre Ier"),
                ..TocSectionDto::default()
            }],
            articles: vec![
                TocArticleDto { id: text!("LEGIARTI000033205126"), ..TocArticleDto::default() },
                TocArticleDto { id: text!("XYZ"), ..TocArticleDto::default() },
            ],
            ..ConsultTextDto::default()
        };
        let (body, diags) = body_of(dto);
        let toc = body.unwrap().toc;
        assert_eq!(toc.roots().len(), 2);
        assert_eq!(
            toc.roots(),
            [
                TocNode::Article(TocArticle {
                    article_id: ArticleId::parse("LEGIARTI000033205126").unwrap(),
                    num: None,
                    etat: None,
                    inline_text: None,
                }),
                TocNode::Section(TocSection {
                    id: SectionId::parse("LEGISCTA000033205040").ok(),
                    title: text!("Titre Ier"),
                    etat: None,
                    children: vec![],
                }),
            ]
        );
        assert_eq!(toc.dropped(), 1);
        assert_eq!(diags, Diagnostics { toc_dropped: 1, ..Diagnostics::default() });
    }

    #[test]
    fn text_body_prefers_the_suffix_start() {
        for (suffix, expected) in [(Some(d(2023, 9, 8)), d(2023, 9, 8)), (None, d(2020, 1, 1))] {
            let dto = ConsultTextDto {
                date_debut_version: Some(date("2020-01-01")),
                ..ConsultTextDto::default()
            };
            let mut diags = Diagnostics::default();
            let body = text_body(dto, endpoint::LAW_DECREE, suffix, &mut diags).unwrap();
            assert_eq!(body.date_debut_version, Some(expected), "{suffix:?}");
            assert_eq!(diags, Diagnostics::default());
        }
    }

    #[test]
    fn non_blank_trims_and_drops_blank_text() {
        assert_eq!(non_blank(None), None);
        assert_eq!(non_blank(Some("")), None);
        assert_eq!(non_blank(Some(" \n\t")), None);
        assert_eq!(non_blank(Some("  x ")), Some("x"));
    }

    #[test]
    fn nor_is_validated_and_an_invalid_one_is_counted() {
        let mut diags = Diagnostics::default();
        let valid = nor_of(Some("ecfi1524250l"), &mut diags);
        assert_eq!(valid.as_ref().map(lf_domain::contract::Nor::as_str), Some("ECFI1524250L"));
        assert_eq!(diags.invalid_nor, 0);

        assert_eq!(nor_of(Some("ECFI152425L"), &mut diags), None);
        assert_eq!(diags.invalid_nor, 1);

        assert_eq!(nor_of(None, &mut diags), None);
        assert_eq!(nor_of(Some("  "), &mut diags), None);
        assert_eq!(diags.invalid_nor, 1);

        let mut diags = Diagnostics { invalid_nor: 4, ..Diagnostics::default() };
        assert_eq!(nor_of(Some("nope"), &mut diags), None);
        assert_eq!(diags.invalid_nor, 5);
    }

    #[test]
    fn cid_accepts_every_text_family_and_rejects_the_rest() {
        let mut diags = Diagnostics::default();
        for raw in ["JORFTEXT000033202746", "LEGITEXT000006070721", "KALITEXT000049878950"] {
            let cid = cid_of(Some(raw), endpoint::LAW_DECREE, &mut diags).unwrap().unwrap();
            assert_eq!(cid, DocumentId::parse(raw).unwrap());
            assert_eq!(cid.as_str(), raw);
        }

        assert_eq!(cid_of(None, endpoint::LAW_DECREE, &mut diags), Ok(None));
        assert_eq!(cid_of(Some(" "), endpoint::LAW_DECREE, &mut diags), Ok(None));

        assert_eq!(
            cid_of(Some("FOOTEXT000000000001"), endpoint::LAW_DECREE, &mut diags),
            Err(CorpusError::Decode {
                detail: "/consult/lawDecree: cid: invalid identifier".into()
            })
        );
        assert_eq!(diags, Diagnostics::default());

        let cid = cid_of(Some("JORFTEXT000033202746_08-10-2016"), endpoint::KALI_TEXT, &mut diags);
        assert_eq!(cid, Ok(DocumentId::parse("JORFTEXT000033202746").ok()));
        assert_eq!(diags, Diagnostics { upstream_suffix_stripped: 1, ..Diagnostics::default() });
    }

    #[test]
    fn a_suffixed_cid_is_stripped_and_counted_outside_legitext() {
        let cases = [
            ("JORFTEXT000033202746_08-10-2016", "JORFTEXT000033202746", 1),
            ("KALITEXT000049878950_08-09-2023", "KALITEXT000049878950", 1),
            ("LEGITEXT000006070721_09-04-2026", "LEGITEXT000006070721", 0),
            ("JORFTEXT000033202746", "JORFTEXT000033202746", 0),
        ];
        for (raw, kept, stripped) in cases {
            let (body, diags) =
                body_of(ConsultTextDto { cid: text!(raw), ..ConsultTextDto::default() });
            assert_eq!(body.unwrap().cid.as_ref().map(DocumentId::as_str), Some(kept), "{raw}");
            assert_eq!(
                diags,
                Diagnostics { upstream_suffix_stripped: stripped, ..Diagnostics::default() },
                "{raw}"
            );
        }
    }

    #[test]
    fn a_malformed_cid_suffix_is_refused_and_not_counted() {
        let (body, diags) = body_of(ConsultTextDto {
            cid: text!("JORFTEXT000033202746_31-02-2016"),
            ..ConsultTextDto::default()
        });
        assert_eq!(
            body,
            Err(CorpusError::Decode {
                detail: "/consult/lawDecree: cid: invalid identifier".into()
            })
        );
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn decode_details_never_contain_the_value() {
        let error =
            cid_of(Some("SECRET-VALUE-42"), endpoint::KALI_TEXT, &mut Diagnostics::default())
                .unwrap_err();
        let shown = format!("{error:?} {error}");
        assert!(!shown.contains("SECRET"), "{shown}");
    }

    #[test]
    fn aliases_drop_blank_entries_and_keep_order() {
        let appellations =
            ["loi Lemaire", " ", "<br>", "loi <mark>numérique</mark>"].map(String::from);
        assert_eq!(aliases_of(&appellations), ["loi Lemaire", "loi numérique"]);
    }
}
