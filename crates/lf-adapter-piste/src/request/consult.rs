//! Corps des six consultations (04 §7.1, §14.3).
//!
//! L'identifiant va dans le corps de la requête, jamais dans le chemin : le gabarit d'endpoint
//! (`endpoint`) ne porte ni identifiant ni paramètre, et ne dépend que de la variante. Les clés
//! `searchedString` et `fromSuggest` de la référence ne sont jamais envoyées.
//!
//! Les identifiants viennent du domaine, déjà normalisés (casse, blancs, suffixe de version
//! retirés) : `body` rend le texte normalisé. `json!` construit le corps sans échec possible.

use lf_domain::{
    date::VersionDate,
    id::{AccoTextId, ArticleId, CnilTextId, DecisionId, KaliTextId, LegiTextId},
};
use serde_json::json;

use crate::map::endpoint::{ACCO, CNIL, GET_ARTICLE, JURI, KALI_TEXT, LAW_DECREE};

/// Une consultation et son identifiant, empruntés à l'appelant.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Consultation<'a> {
    /// Texte de loi, de décret ou code à une date de version : `textId` et `date`.
    LawDecree(&'a LegiTextId, VersionDate),
    /// Article : `id`.
    Article(&'a ArticleId),
    /// Décision de justice : `textId`.
    Decision(&'a DecisionId),
    /// Délibération de la CNIL : `textId`.
    Deliberation(&'a CnilTextId),
    /// Convention collective : `id`.
    Convention(&'a KaliTextId),
    /// Accord d'entreprise : `id`.
    Agreement(&'a AccoTextId),
}

impl Consultation<'_> {
    /// Gabarit d'endpoint de `map::endpoint`, sans paramètre.
    pub(crate) fn endpoint(self) -> &'static str {
        match self {
            Self::LawDecree(..) => LAW_DECREE,
            Self::Article(_) => GET_ARTICLE,
            Self::Decision(_) => JURI,
            Self::Deliberation(_) => CNIL,
            Self::Convention(_) => KALI_TEXT,
            Self::Agreement(_) => ACCO,
        }
    }

    /// Corps JSON de la requête, infaillible.
    pub(crate) fn body(self) -> Vec<u8> {
        let body = match self {
            Self::LawDecree(id, date) => json!({ "textId": id.as_str(), "date": date.to_string() }),
            Self::Article(id) => json!({ "id": id.as_str() }),
            Self::Decision(id) => json!({ "textId": id.as_str() }),
            Self::Deliberation(id) => json!({ "textId": id.as_str() }),
            Self::Convention(id) => json!({ "id": id.as_str() }),
            Self::Agreement(id) => json!({ "id": id.as_str() }),
        };
        body.to_string().into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn body_of(consultation: Consultation<'_>) -> Value {
        serde_json::from_slice(&consultation.body()).expect("the body is JSON")
    }

    fn date(text: &str) -> VersionDate {
        VersionDate::parse(text).expect("a valid date")
    }

    #[test]
    fn each_consultation_has_its_endpoint_and_its_body_key() {
        let law = LegiTextId::parse("LEGITEXT000033205014").expect("a LEGITEXT");
        let article = ArticleId::parse("KALIARTI000049886059").expect("an article");
        let decision = DecisionId::parse("CETATEXT000054178503").expect("a decision");
        let cnil = CnilTextId::parse("CNILTEXT000053352643").expect("a CNIL text");
        let kali = KaliTextId::parse("KALITEXT000049878950").expect("a KALITEXT");
        let acco = AccoTextId::parse("ACCOTEXT000053643828").expect("an ACCOTEXT");

        let table = [
            (
                Consultation::LawDecree(&law, date("2026-10-15")),
                "/consult/lawDecree",
                json!({"textId": "LEGITEXT000033205014", "date": "2026-10-15"}),
            ),
            (
                Consultation::Article(&article),
                "/consult/getArticle",
                json!({"id": "KALIARTI000049886059"}),
            ),
            (
                Consultation::Decision(&decision),
                "/consult/juri",
                json!({"textId": "CETATEXT000054178503"}),
            ),
            (
                Consultation::Deliberation(&cnil),
                "/consult/cnil",
                json!({"textId": "CNILTEXT000053352643"}),
            ),
            (
                Consultation::Convention(&kali),
                "/consult/kaliText",
                json!({"id": "KALITEXT000049878950"}),
            ),
            (
                Consultation::Agreement(&acco),
                "/consult/acco",
                json!({"id": "ACCOTEXT000053643828"}),
            ),
        ];
        for (consultation, endpoint, body) in table {
            assert_eq!((consultation.endpoint(), body_of(consultation)), (endpoint, body));
        }
    }

    #[test]
    fn the_body_carries_the_normalized_identifier() {
        let law = LegiTextId::parse("  legitext000033205014_01-01-2020 ").expect("a LEGITEXT");
        assert_eq!(
            body_of(Consultation::LawDecree(&law, date("1804-03-21"))),
            json!({"textId": "LEGITEXT000033205014", "date": "1804-03-21"})
        );
    }

    #[test]
    fn an_endpoint_template_never_holds_an_identifier() {
        let decision = DecisionId::parse("JURITEXT000051743650").expect("a decision");
        let endpoint = Consultation::Decision(&decision).endpoint();
        assert!(!endpoint.contains("JURITEXT"));
        assert!(endpoint.chars().all(|c| c.is_ascii_alphabetic() || c == '/'));
    }
}
