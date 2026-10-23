//! Fonctions communes aux mappers des textes consolidés (04 §9.1 c ; 03 §4.2).
//!
//! Elles lisent les champs courts que partagent `lawDecree`, `kaliText` et `getArticle` :
//! texte rogné (`non_blank`), `cid` chronologique (`cid_of`), NOR (`nor_of`) et appellations
//! (`aliases_of`). Aucune valeur du corps ne sort d'ici : un `cid` refusé devient un
//! [`CorpusError::Decode`] qui ne nomme que l'endpoint et le chemin du champ, un NOR refusé est
//! compté (`invalid_nor`) puis omis.

use lf_application::ports::CorpusError;
use lf_domain::contract::Nor;
use lf_domain::id::DocumentId;
use lf_domain::text::clean_field;

use super::{Diagnostics, Field};

/// `raw` rogné ; `None` s'il est absent ou vide une fois rogné.
pub(super) fn non_blank(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|text| !text.is_empty())
}

/// Identifiant chronologique (`cid`) de `raw`, `None` s'il est absent ou blanc.
///
/// # Errors
///
/// [`CorpusError::Decode`] (`<endpoint>: cid: invalid identifier`) si le `cid` présent n'est pas
/// un identifiant Légifrance valide : un `cid` illisible n'est jamais omis en silence.
pub(super) fn cid_of(
    raw: Option<&str>,
    endpoint: &'static str,
) -> Result<Option<DocumentId>, CorpusError> {
    non_blank(raw)
        .map(|cid| {
            DocumentId::parse(cid).map_err(|error| Field { endpoint, path: "cid" }.domain(&error))
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
    use lf_domain::id::DocumentId;

    use super::*;
    use crate::map::endpoint;

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
        for raw in ["JORFTEXT000033202746", "LEGITEXT000006070721", "KALITEXT000049878950"] {
            let cid = cid_of(Some(raw), endpoint::LAW_DECREE).unwrap().unwrap();
            assert_eq!(cid, DocumentId::parse(raw).unwrap());
            assert_eq!(cid.as_str(), raw);
        }

        assert_eq!(cid_of(None, endpoint::LAW_DECREE), Ok(None));
        assert_eq!(cid_of(Some(" "), endpoint::LAW_DECREE), Ok(None));

        assert_eq!(
            cid_of(Some("FOOTEXT000000000001"), endpoint::LAW_DECREE),
            Err(CorpusError::Decode {
                detail: "/consult/lawDecree: cid: invalid identifier".into()
            })
        );
    }

    #[test]
    fn decode_details_never_contain_the_value() {
        let error = cid_of(Some("SECRET-VALUE-42"), endpoint::KALI_TEXT).unwrap_err();
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
