//! Accord d'entreprise de `/consult/acco` vers `CompanyAgreement` (03 §4.7 ; 04 §9.2).
//!
//! Les métadonnées seules sont lues : le texte de l'accord n'existe qu'en pièce jointe `.docx`,
//! et ni `data`, ni `attachment`, ni `fileSize`, ni `numero` ne sont employés. Un objet sans
//! identité est un `NotFound`. L'identifiant renvoyé est gardé (compté s'il diffère de celui
//! demandé, repli compté sur le demandé s'il manque). Les champs courts passent par
//! `clean_field`, sans compter les invisibles qu'ils perdent. Les dates de fin et de diffusion ne
//! sont pas lues. Aucune valeur du corps ne sort d'ici.

use lf_application::ports::CorpusError;
use lf_domain::contract::Idcc;
use lf_domain::entity::CompanyAgreement;
use lf_domain::id::AccoTextId;
use lf_domain::text::clean_field;

use super::dates::civil;
use super::endpoint::ACCO;
use super::rules::{require, resolve_id};
use super::{Diagnostics, Field};
use crate::dto::acco::{ConsultAccoResponseDto, LibelleDto};

/// Champ `acco.id` de la réponse.
const ID: Field = Field { endpoint: ACCO, path: "acco.id" };
/// Date du texte.
const DATE_TEXTE: Field = Field { endpoint: ACCO, path: "acco.dateTexte" };
/// Date d'effet.
const DATE_EFFET: Field = Field { endpoint: ACCO, path: "acco.dateEffet" };
/// Date de dépôt.
const DATE_DEPOT: Field = Field { endpoint: ACCO, path: "acco.dateDepot" };

/// `/consult/acco` ⇒ `CompanyAgreement` (03 §4.7). Sur `Err(Decode)`, `diags` garde les
/// anomalies relevées avant l'erreur : `Stats::record_mapping` est appelé sur `Ok` comme sur
/// `Err` (04 §9.3) ; un `NotFound` ne touche pas `diags`.
///
/// # Errors
///
/// [`CorpusError::NotFound`] (`/consult/acco`) si l'enveloppe `acco` est absente ou `null`, ou si
/// l'objet n'a ni `id` ni `titreTexte` non blanc. [`CorpusError::Decode`] du premier champ
/// refusé : `acco.id` (`invalid identifier`), `acco.dateTexte`, `acco.dateEffet` ou
/// `acco.dateDepot` (`unreadable date`).
pub(crate) fn agreement(
    dto: ConsultAccoResponseDto,
    requested: &AccoTextId,
    diags: &mut Diagnostics,
) -> Result<CompanyAgreement, CorpusError> {
    let acco = require(dto.acco, ACCO)?;
    let agreement_id = resolve_id(requested, acco.id.as_deref(), ID, diags)?;
    let date_texte = civil(acco.date_texte.as_ref(), DATE_TEXTE, diags)?;
    let date_effet = civil(acco.date_effet.as_ref(), DATE_EFFET, diags)?;
    let date_depot = civil(acco.date_depot.as_ref(), DATE_DEPOT, diags)?;
    Ok(CompanyAgreement {
        agreement_id,
        nature: clean_field(acco.nature.as_deref()),
        title: clean_field(acco.titre_texte.as_deref()),
        raison_sociale: clean_field(acco.raison_sociale.as_deref()),
        siret: clean_field(acco.siret.as_deref()),
        idcc: acco.code_idcc.as_deref().and_then(Idcc::parse),
        code_ape: clean_field(acco.code_ape.as_deref()),
        secteur: clean_field(acco.secteur.as_deref()),
        themes: labels(&acco.themes),
        syndicats: labels(&acco.syndicats),
        date_texte,
        date_effet,
        date_depot,
    })
}

/// Libellés nettoyés un à un : une entrée vide après nettoyage est écartée, l'ordre et les
/// doublons sont gardés.
fn labels(entries: &[LibelleDto]) -> Vec<String> {
    entries.iter().filter_map(|entry| clean_field(entry.libelle.as_deref())).collect()
}

#[cfg(test)]
mod tests {
    use lf_domain::contract::Citable;
    use lf_domain::date::CivilDate;

    use super::*;
    use crate::map::fixtures::{decode_as, decode_error, read};

    /// Identifiant demandé de la fixture `consult_acco`.
    const ACCO_ID: &str = "ACCOTEXT000053643828";

    /// Identifiant d'accord valide.
    fn acco_id(raw: &str) -> AccoTextId {
        AccoTextId::parse(raw).unwrap()
    }

    /// Réponse dont l'objet `acco` porte `fields` (champs JSON, sans accolades).
    fn wrapped(fields: &str) -> String {
        format!(r#"{{"acco":{{{fields}}}}}"#)
    }

    /// Réponse dont l'objet `acco` porte l'identifiant [`ACCO_ID`] puis `fields`.
    fn with_id(fields: &str) -> String {
        let separator = if fields.is_empty() { "" } else { "," };
        wrapped(&format!(r#""id":"{ACCO_ID}"{separator}{fields}"#))
    }

    /// `agreement` de `json` demandé en `requested`, et les diagnostics qu'il laisse.
    fn agreement_of(
        json: &str,
        requested: &str,
    ) -> (Result<CompanyAgreement, CorpusError>, Diagnostics) {
        let mut diags = Diagnostics::default();
        let mapped = agreement(read(json, ACCO), &acco_id(requested), &mut diags);
        (mapped, diags)
    }

    /// Accord de `with_id(fields)`, demandé sous [`ACCO_ID`], et les diagnostics qu'il laisse.
    fn agreement_with(fields: &str) -> (CompanyAgreement, Diagnostics) {
        let (mapped, diags) = agreement_of(&with_id(fields), ACCO_ID);
        (mapped.unwrap(), diags)
    }

    /// `CorpusError::NotFound` de l'endpoint.
    fn not_found() -> CorpusError {
        CorpusError::NotFound { what: "/consult/acco".into() }
    }

    /// Un seul compteur d'anomalies, `id_mismatch`.
    fn mismatch(count: u32) -> Diagnostics {
        Diagnostics { id_mismatch: count, ..Diagnostics::default() }
    }

    #[test]
    fn consult_acco_fixture_maps() {
        let mut diags = Diagnostics::default();
        let dto = decode_as::<ConsultAccoResponseDto>("consult_acco");
        let mapped = agreement(dto, &acco_id(ACCO_ID), &mut diags).unwrap();
        assert_eq!(mapped.agreement_id, acco_id(ACCO_ID));
        assert_eq!(mapped.agreement_id.as_str(), ACCO_ID);
        assert_eq!(mapped.nature.as_deref(), Some("ACCORD"));
        assert_eq!(mapped.title.as_deref(), Some("Accord d'entreprise relatif au télétravail"));
        assert_eq!(mapped.raison_sociale.as_deref(), Some("CORSAIR (Télétravail)"));
        assert_eq!(mapped.siret.as_deref(), Some("32862158600143"));
        assert_eq!(mapped.idcc, Idcc::parse("275"));
        assert_eq!(mapped.idcc.map(Idcc::get), Some(275));
        assert_eq!(mapped.code_ape.as_deref(), Some("5110Z"));
        assert_eq!(mapped.secteur.as_deref(), Some("Transports aériens de passagers"));
        assert_eq!(mapped.themes, ["Télétravail"]);
        assert_eq!(mapped.syndicats, ["CFTC", "CFE-CGC", "Autre"]);
        assert_eq!(mapped.date_texte, CivilDate::new(2025, 12, 19));
        assert_eq!(mapped.date_effet, CivilDate::new(2025, 12, 20));
        assert_eq!(mapped.date_depot, CivilDate::new(2026, 2, 27));
        assert_eq!(diags, Diagnostics::default());

        let contract = mapped.contract();
        assert_eq!(
            contract.citation.as_str(),
            "Accord d'entreprise relatif au télétravail (CORSAIR (Télétravail))"
        );
        assert_eq!(
            contract.source_url.as_str(),
            "https://www.legifrance.gouv.fr/acco/id/ACCOTEXT000053643828"
        );
    }

    #[test]
    fn acco_without_identity_maps_to_not_found() {
        let before =
            Diagnostics { id_mismatch: 2, non_midnight_timestamps: 3, ..Diagnostics::default() };
        let cases = [
            r#"{"acco":null}"#,
            "{}",
            r#"{"executionTime":1,"dereferenced":false}"#,
            r#"{"acco":{}}"#,
            r#"{"acco":{"nature":"ACCORD","raisonSociale":"X"}}"#,
            r#"{"acco":{"id":"  ","titreTexte":"  "}}"#,
        ];
        for json in cases {
            let mut diags = before;
            assert_eq!(
                agreement(read(json, ACCO), &acco_id(ACCO_ID), &mut diags),
                Err(not_found()),
                "{json}"
            );
            assert_eq!(diags, before, "{json}");
        }
    }

    #[test]
    fn agreement_without_id_uses_the_requested_one_and_counts() {
        for json in [
            r#"{"acco":{"titreTexte":"Accord X"}}"#,
            r#"{"acco":{"id":" ","titreTexte":"Accord X"}}"#,
        ] {
            let (mapped, diags) = agreement_of(json, ACCO_ID);
            let mapped = mapped.unwrap();
            assert_eq!(mapped.agreement_id, acco_id(ACCO_ID), "{json}");
            assert_eq!(mapped.title.as_deref(), Some("Accord X"), "{json}");
            assert_eq!(diags, mismatch(1), "{json}");
        }

        let (mapped, diags) = agreement_of(r#"{"acco":{"id":"  ","titreTexte":"<br>"}}"#, ACCO_ID);
        let mapped = mapped.unwrap();
        assert_eq!(mapped.agreement_id, acco_id(ACCO_ID));
        assert_eq!(mapped.title, None);
        assert_eq!(diags, mismatch(1));
    }

    #[test]
    fn agreement_keeps_the_returned_id_and_counts_the_mismatch() {
        let json = wrapped(r#""id":"ACCOTEXT000051888026""#);
        let (mapped, diags) = agreement_of(&json, ACCO_ID);
        let mapped = mapped.unwrap();
        assert_eq!(mapped.agreement_id, acco_id("ACCOTEXT000051888026"));
        assert!(mapped.contract().source_url.as_str().ends_with("ACCOTEXT000051888026"));
        assert_eq!(diags, mismatch(1));

        let json = wrapped(r#""id":" accotext000053643828 ""#);
        let (mapped, diags) = agreement_of(&json, ACCO_ID);
        assert_eq!(mapped.unwrap().agreement_id, acco_id(ACCO_ID));
        assert_eq!(diags, Diagnostics::default());

        let json = wrapped(r#""id":"ACCOTEXT000053643828_01-01-2020""#);
        let (mapped, diags) = agreement_of(&json, ACCO_ID);
        assert_eq!(mapped.unwrap().agreement_id, acco_id(ACCO_ID));
        assert_eq!(diags, Diagnostics { upstream_suffix_stripped: 1, ..Diagnostics::default() });
    }

    #[test]
    fn a_decode_error_keeps_the_anomalies_counted_before_it() {
        let json = wrapped(r#""id":"ACCOTEXT000051888026","dateDepot":"hier""#);
        let (mapped, diags) = agreement_of(&json, ACCO_ID);
        assert_eq!(mapped, Err(decode_error(ACCO, "acco.dateDepot: unreadable date")));
        assert_eq!(diags, mismatch(1));
    }

    #[test]
    fn agreement_with_a_foreign_or_malformed_id_is_a_decode_error() {
        for id in ["JURITEXT000051743650", "SECRET-ID-42", "ACCOTEXT1"] {
            let (mapped, diags) = agreement_of(&wrapped(&format!(r#""id":"{id}""#)), ACCO_ID);
            let error = mapped.unwrap_err();
            assert_eq!(error, decode_error(ACCO, "acco.id: invalid identifier"), "{id}");
            assert!(!format!("{error:?} {error}").contains("SECRET"), "{id}");
            assert_eq!(diags.id_mismatch, 0, "{id}");
        }
    }

    #[test]
    fn agreement_idcc_accepts_string_and_number() {
        let cases = [
            (r#""275""#, Some(275)),
            ("275", Some(275)),
            (r#""0000000000000275""#, Some(275)),
            (r#""0275""#, Some(275)),
            (r#""10000""#, None),
            (r#""0""#, None),
            (r#""0000""#, None),
            (r#""abc""#, None),
            (r#""""#, None),
            ("2.5", None),
            ("true", None),
            ("null", None),
        ];
        for (value, expected) in cases {
            let (mapped, diags) = agreement_with(&format!(r#""codeIdcc":{value}"#));
            assert_eq!(mapped.idcc.map(Idcc::get), expected, "{value}");
            assert_eq!(diags, Diagnostics::default(), "{value}");
        }
        let (absent, diags) = agreement_with("");
        assert_eq!(absent.idcc, None);
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn agreement_secteur_empty_is_none() {
        for secteur in ["", "  ", "<br>"] {
            let (mapped, _) = agreement_with(&format!(r#""secteur":"{secteur}""#));
            assert_eq!(mapped.secteur, None, "{secteur:?}");
        }
        let (mapped, _) = agreement_with(r#""secteur":"Transport","nature":"""#);
        assert_eq!(mapped.secteur.as_deref(), Some("Transport"));
        assert_eq!(mapped.nature, None);
    }

    #[test]
    fn agreement_labels_are_cleaned_one_by_one() {
        let (mapped, diags) = agreement_with(
            r#""themes":[{"libelle":"Télé<b>travail</b>"},{"libelle":""},{"libelle":null},
            {"code":"9"},{"libelle":"  <br> "},{"libelle":"Salaires"},{"libelle":"Salaires"}],
            "syndicats":[{"libelle":"CFTC"},{"libelle":" CGT "}]"#,
        );
        assert_eq!(mapped.themes, ["Télétravail", "Salaires", "Salaires"]);
        assert_eq!(mapped.syndicats, ["CFTC", "CGT"]);
        assert_eq!(diags, Diagnostics::default());

        for fields in [r#""themes":null,"syndicats":null"#, ""] {
            let (mapped, _) = agreement_with(fields);
            assert!(mapped.themes.is_empty() && mapped.syndicats.is_empty(), "{fields}");
        }
    }

    #[test]
    fn agreement_dates_accept_every_encoding() {
        let (mapped, diags) = agreement_with(
            r#""dateTexte":1766102400000,"dateEffet":"2025-12-20",
            "dateDepot":"2026-02-27T00:00:00.000+0000""#,
        );
        assert_eq!(mapped.date_texte, CivilDate::new(2025, 12, 19));
        assert_eq!(mapped.date_effet, CivilDate::new(2025, 12, 20));
        assert_eq!(mapped.date_depot, CivilDate::new(2026, 2, 27));
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn agreement_sentinel_dates_are_none() {
        let (mapped, diags) = agreement_with(
            r#""dateTexte":32472144000000,"dateEffet":"2999-01-01",
            "dateDepot":"2999-01-01T00:00:00.000+0000""#,
        );
        assert_eq!((mapped.date_texte, mapped.date_effet, mapped.date_depot), (None, None, None));
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn agreement_non_midnight_timestamp_is_counted() {
        let (mapped, diags) = agreement_with(r#""dateDepot":1772150400001"#);
        assert_eq!(mapped.date_depot, CivilDate::new(2026, 2, 27));
        assert_eq!(diags, Diagnostics { non_midnight_timestamps: 1, ..Diagnostics::default() });
    }

    #[test]
    fn agreement_unreadable_text_date_is_a_decode_error() {
        for field in ["dateTexte", "dateEffet", "dateDepot"] {
            let json = with_id(&format!(r#""{field}":"SECRET-DATE-42""#));
            let error = agreement_of(&json, ACCO_ID).0.unwrap_err();
            assert_eq!(
                error,
                decode_error(ACCO, &format!("acco.{field}: unreadable date")),
                "{field}"
            );
            assert!(!format!("{error:?} {error}").contains("SECRET"), "{field}");
        }
    }

    #[test]
    fn agreement_ignores_the_unmapped_dates_and_fields() {
        let (plain, _) = agreement_with(r#""titreTexte":"Accord X","dateTexte":1766102400000"#);
        let (filled, diags) = agreement_with(
            r#""titreTexte":"Accord X","dateTexte":1766102400000,
            "dateFin":"SECRET-DATE-42","dateDiffusion":"SECRET-DATE-42","fileSize":[1],
            "numero":"T1","conformeVersionIntegrale":true,"signataires":["91"]"#,
        );
        assert_eq!(filled, plain);
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn agreement_short_fields_are_cleaned_without_counting() {
        let (mapped, diags) = agreement_with(
            "\"titreTexte\":\"Accord <mark>télé\u{200b}travail</mark>\",\
             \"raisonSociale\":\"COR\u{00ad}SAIR\",\
             \"themes\":[{\"libelle\":\"A\u{200b}\u{200b}b\"}]",
        );
        assert_eq!(mapped.title.as_deref(), Some("Accord télétravail"));
        assert_eq!(mapped.raison_sociale.as_deref(), Some("CORSAIR"));
        assert_eq!(mapped.themes, ["Ab"]);
        assert_eq!(diags.invisible_chars_removed, 0);
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn agreement_errors_never_carry_a_body_value() {
        let cases = [
            wrapped(r#""id":"SECRET-ID-42","titreTexte":"SECRET-TITLE""#),
            with_id(r#""titreTexte":"SECRET-TITLE","dateTexte":"SECRET-DATE-42""#),
            with_id(r#""titreTexte":"SECRET-TITLE","dateEffet":"SECRET-DATE-42""#),
            with_id(r#""titreTexte":"SECRET-TITLE","dateDepot":"SECRET-DATE-42""#),
            wrapped(r#""nature":"SECRET-NATURE""#),
        ];
        for json in cases {
            let error = agreement_of(&json, ACCO_ID).0.unwrap_err();
            let shown = format!("{error:?} {error}");
            assert!(!shown.contains("SECRET"), "{shown}");
        }
    }
}
