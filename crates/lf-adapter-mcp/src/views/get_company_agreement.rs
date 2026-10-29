//! Vue de sortie de `fr_get_company_agreement` (05 §5.7) : les métadonnées d'un accord d'entreprise.
//!
//! **Aucun champ `text`** (P-036) : Légifrance ne distribue le texte d'un accord que comme pièce
//! jointe `.docx`, que l'outil ne rend pas ; `attachment_note` le dit et renvoie à `source_url`.
//! Les champs absents sont omis (pas de `null`), `themes` et `syndicats` aussi quand ils sont
//! vides. Pas de commentaire de doc sur les champs : le schéma de sortie n'en porte aucun.

use lf_application::output::get_company_agreement::GetCompanyAgreementOutput;
use lf_domain::{
    contract::Citable,
    entity::{ATTACHMENT_NOTE, CompanyAgreement},
};
use schemars::JsonSchema;
use serde::Serialize;

use super::common::{date_unless_sentinel, title_unless_in_citation};

// Les noms de champs sont ceux de la référence (parité, 05 §5.7).
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct CompanyAgreementView {
    pub(crate) agreement_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) nature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) raison_sociale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) siret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) idcc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) code_ape: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) secteur: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) themes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) syndicats: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) date_texte: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) date_effet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) date_depot: Option<String>,
    pub(crate) attachment_note: String,
    pub(crate) citation: String,
    pub(crate) source_url: String,
}

impl From<GetCompanyAgreementOutput> for CompanyAgreementView {
    fn from(output: GetCompanyAgreementOutput) -> Self {
        let CompanyAgreement {
            agreement_id,
            nature,
            title,
            raison_sociale,
            siret,
            idcc,
            code_ape,
            secteur,
            themes,
            syndicats,
            date_texte,
            date_effet,
            date_depot,
        } = &output.agreement;
        let contract = output.agreement.contract();
        // `title` est omis quand la citation le contient (05 §5.0.4) ; une citation coupée à
        // 1 000 caractères ne peut plus le garantir : le titre est alors rendu (03 §5.1).
        let title = if contract.citation.is_truncated() {
            title.as_deref().map(str::trim).filter(|title| !title.is_empty()).map(str::to_owned)
        } else {
            title_unless_in_citation(title.as_deref(), contract.citation.as_str())
        };
        Self {
            agreement_id: agreement_id.to_string(),
            title,
            nature: nature.clone(),
            raison_sociale: raison_sociale.clone(),
            siret: siret.clone(),
            idcc: idcc.map(|idcc| idcc.to_string()),
            code_ape: code_ape.clone(),
            secteur: secteur.clone(),
            themes: themes.clone(),
            syndicats: syndicats.clone(),
            date_texte: date_unless_sentinel(*date_texte),
            date_effet: date_unless_sentinel(*date_effet),
            date_depot: date_unless_sentinel(*date_depot),
            attachment_note: ATTACHMENT_NOTE.to_owned(),
            citation: contract.citation.as_str().to_owned(),
            source_url: contract.source_url.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use lf_domain::{contract::Idcc, date::CivilDate, id::AccoTextId};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::schema::output_schema;

    /// L'accord de la fixture `consult_acco.json` (05 §5.7).
    fn corsair() -> CompanyAgreement {
        CompanyAgreement {
            agreement_id: AccoTextId::parse("ACCOTEXT000053643828").unwrap(),
            nature: Some("ACCORD".to_owned()),
            title: Some("Accord d'entreprise relatif au télétravail".to_owned()),
            raison_sociale: Some("CORSAIR (Télétravail)".to_owned()),
            siret: Some("32862158600143".to_owned()),
            idcc: Idcc::parse("275"),
            code_ape: Some("5110Z".to_owned()),
            secteur: Some("Transports aériens de passagers".to_owned()),
            themes: vec!["Télétravail".to_owned()],
            syndicats: vec!["CFTC".to_owned(), "CFE-CGC".to_owned(), "Autre".to_owned()],
            date_texte: CivilDate::new(2025, 12, 19),
            date_effet: CivilDate::new(2025, 12, 20),
            date_depot: CivilDate::new(2026, 2, 27),
        }
    }

    fn view(agreement: CompanyAgreement) -> Value {
        serde_json::to_value(CompanyAgreementView::from(GetCompanyAgreementOutput { agreement }))
            .unwrap()
    }

    /// Un accord qui ne porte que son identifiant.
    fn bare() -> CompanyAgreement {
        CompanyAgreement {
            nature: None,
            title: None,
            raison_sociale: None,
            siret: None,
            idcc: None,
            code_ape: None,
            secteur: None,
            themes: Vec::new(),
            syndicats: Vec::new(),
            date_texte: None,
            date_effet: None,
            date_depot: None,
            ..corsair()
        }
    }

    #[test]
    fn the_view_is_the_documented_example() {
        let expected = json!({
            "agreement_id": "ACCOTEXT000053643828",
            "nature": "ACCORD",
            "raison_sociale": "CORSAIR (Télétravail)",
            "siret": "32862158600143",
            "idcc": "275",
            "code_ape": "5110Z",
            "secteur": "Transports aériens de passagers",
            "themes": ["Télétravail"],
            "syndicats": ["CFTC", "CFE-CGC", "Autre"],
            "date_texte": "2025-12-19",
            "date_effet": "2025-12-20",
            "date_depot": "2026-02-27",
            "attachment_note": "Full text is only distributed by Legifrance as a .docx attachment; consult source_url for the human-readable page. This tool returns metadata only.",
            "citation": "Accord d'entreprise relatif au télétravail (CORSAIR (Télétravail))",
            "source_url": "https://www.legifrance.gouv.fr/acco/id/ACCOTEXT000053643828"
        });
        let actual = view(corsair());
        assert_eq!(actual, expected);
        // 664 caractères de JSON compact (05 §5.7).
        assert_eq!(serde_json::to_string(&actual).unwrap().chars().count(), 664);
    }

    #[test]
    fn agreement_view_has_no_text_field() {
        // Aucun `text` dans la sortie, ni dans le schéma : le texte n'existe qu'en `.docx` (P-036).
        let actual = view(corsair());
        let keys: Vec<&str> = actual.as_object().unwrap().keys().map(String::as_str).collect();
        assert!(!keys.contains(&"text"), "{keys:?}");
        let schema = Value::Object(output_schema::<CompanyAgreementView>().as_ref().clone());
        let properties = schema["properties"].as_object().unwrap();
        assert!(!properties.contains_key("text"));
        assert!(!properties.keys().any(|name| name.contains("text") && name != "date_texte"));
        // La note de pièce jointe, elle, est toujours là, avec la citation et l'URL source.
        assert!(properties.contains_key("attachment_note"));
        assert_eq!(actual["attachment_note"], ATTACHMENT_NOTE);
        assert!(
            actual["source_url"]
                .as_str()
                .unwrap()
                .starts_with("https://www.legifrance.gouv.fr/acco/id/")
        );
    }

    #[test]
    fn absent_fields_are_omitted_not_null() {
        let actual = view(bare());
        let keys: Vec<&str> = actual.as_object().unwrap().keys().map(String::as_str).collect();
        // Seuls les champs toujours présents restent, dans l'ordre de la référence.
        assert_eq!(
            keys.iter().copied().collect::<std::collections::BTreeSet<_>>(),
            ["agreement_id", "attachment_note", "citation", "source_url"].into()
        );
        // Sans titre ni raison sociale, la citation est le repli sur l'identifiant.
        assert_eq!(actual["citation"], "Légifrance ACCOTEXT000053643828");
    }

    #[test]
    fn the_title_is_omitted_when_the_citation_contains_it() {
        // Titre et raison sociale : la citation « {titre} ({raison sociale}) » contient le titre.
        assert!(view(corsair()).get("title").is_none());
        // Sans raison sociale, la citation est le titre lui-même : omis aussi.
        let alone = CompanyAgreement { raison_sociale: None, ..corsair() };
        assert!(view(alone).get("title").is_none());
    }

    #[test]
    fn a_truncated_citation_renders_the_title() {
        // Un titre de 1 200 caractères : la citation est coupée à 1 000, le titre est rendu en entier.
        let long = format!("Accord {}", "très long ".repeat(120));
        let agreement = CompanyAgreement { title: Some(long.clone()), ..corsair() };
        let actual = view(agreement);
        assert_eq!(actual["title"], long.trim());
        assert!(actual["citation"].as_str().unwrap().chars().count() <= 1_000);
        // Un titre vide ou blanc n'est jamais rendu, même sur une citation coupée.
        let blank = CompanyAgreement { title: Some("   ".to_owned()), ..corsair() };
        assert!(view(blank).get("title").is_none());
    }

    #[test]
    fn sentinel_dates_are_omitted() {
        // 2222-02-22 et 2999-01-01 valent « pas de date » (03 §6.2).
        let agreement = CompanyAgreement {
            date_effet: CivilDate::new(2222, 2, 22),
            date_depot: CivilDate::new(2999, 1, 1),
            ..corsair()
        };
        let actual = view(agreement);
        assert!(actual.get("date_effet").is_none());
        assert!(actual.get("date_depot").is_none());
        assert_eq!(actual["date_texte"], "2025-12-19");
    }

    #[test]
    fn the_idcc_is_rendered_without_leading_zeros() {
        let agreement = CompanyAgreement { idcc: Idcc::parse("0000000000000275"), ..corsair() };
        assert_eq!(view(agreement)["idcc"], "275");
    }

    #[test]
    fn the_output_schema_lists_the_optional_fields_as_not_required() {
        let schema = Value::Object(output_schema::<CompanyAgreementView>().as_ref().clone());
        assert_eq!(schema["type"], "object");
        assert_eq!(
            schema["required"],
            json!(["agreement_id", "attachment_note", "citation", "source_url"])
        );
        assert!(schema.get("$schema").is_none());
    }

    #[test]
    fn the_view_matches_its_snapshot() {
        // Règle S : les snapshots vivent dans `src/snapshots/`, pas dans `src/views/snapshots/`.
        insta::with_settings!({snapshot_path => "../snapshots"}, {
            insta::assert_json_snapshot!(view(corsair()));
        });
    }
}
