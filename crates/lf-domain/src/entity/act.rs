//! Textes consolidés : loi/décret (LODA) et convention collective (KALI).
//! Même forme côté API (`citations.py:204-228` et `403-436`).

use crate::{
    contract::{Citable, CitationContract, Idcc, Nor},
    date::CivilDate,
    entity::Toc,
    id::{JorfTextId, KaliTextId, LegiTextId},
    status::LegalStatus,
    url::{ResourceKind, legifrance_url},
};

/// Métadonnées et sommaire communs aux textes consolidés.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextBody {
    /// Identifiant chronologique (CID), un [`JorfTextId`] (`JORFTEXT…`). `None` pour une
    /// convention collective : le `cid` KALI vaut `KALITEXT…`, qui n'est pas un `JorfTextId` ;
    /// son type pour KALI est à décider au plan 2f.
    pub cid: Option<JorfTextId>,
    /// Numéro NOR ; `None` : absent ou invalide.
    pub nor: Option<Nor>,
    /// Titre officiel.
    pub title: Option<String>,
    /// Nature, ex. `LOI`.
    pub nature: Option<String>,
    /// État juridique.
    pub etat: Option<LegalStatus>,
    /// Date de parution au JO ; `None` : inconnue.
    pub date_parution: Option<CivilDate>,
    /// Numéro de parution au JO.
    pub num_parution: Option<String>,
    /// Début de la version consolidée ; `None` : date d'entrée en vigueur non déterminée.
    pub date_debut_version: Option<CivilDate>,
    /// Fin de la version consolidée ; `None` : en vigueur sans date de fin (la sentinelle
    /// `2999-01-01` de l'API est passée par [`open_ended`](crate::date::open_ended)).
    pub date_fin_version: Option<CivilDate>,
    /// Référence au JO, ex. `JORF n°0235 du 8 octobre 2016`.
    pub jo_reference: Option<String>,
    /// Appellations usuelles, ex. `loi Lemaire`.
    pub aliases: Vec<String>,
    /// Table des matières en arbre, sections et articles ([`Toc::build`]).
    pub toc: Toc,
}

/// Loi ou décret consolidé (`fr_get_act`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Act {
    /// Identifiant du texte.
    pub text_id: LegiTextId,
    /// Métadonnées et sommaire.
    pub body: TextBody,
}

/// Convention collective (`fr_get_convention`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Convention {
    /// Identifiant du texte.
    pub text_id: KaliTextId,
    /// Métadonnées et sommaire.
    pub body: TextBody,
    /// Convention collective de rattachement (`conteneurs[0].num`).
    pub idcc: Option<Idcc>,
    /// Titre du conteneur (`conteneurs[0].titre`).
    pub container_title: Option<String>,
    /// Identifiant du conteneur, ex. `KALICONT000005635096` (`conteneurs[0].cid`).
    pub container_id: Option<String>,
    /// Bulletin officiel de publication, ex. `BO n°2024-20`.
    pub bulletin: Option<String>,
}

impl Citable for Act {
    fn contract(&self) -> CitationContract {
        let id = self.text_id.as_str();
        CitationContract::from_url(
            legifrance_url(kind_of(self.body.nature.as_deref()), id),
            self.body.title.as_deref(),
            id,
        )
    }
}

/// Page d'un texte LEGI : un code (`nature` « CODE », sans tenir compte de la casse) ou un
/// texte LODA. `LegiTextId` ne distingue pas les deux (D-47).
fn kind_of(nature: Option<&str>) -> ResourceKind {
    if nature.is_some_and(|nature| nature.eq_ignore_ascii_case("CODE")) {
        ResourceKind::Code
    } else {
        ResourceKind::Loda
    }
}

impl Citable for Convention {
    fn contract(&self) -> CitationContract {
        let id = self.text_id.as_str();
        CitationContract::from_url(
            legifrance_url(ResourceKind::Kali, id),
            self.body.title.as_deref(),
            id,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn body(title: &str) -> TextBody {
        TextBody { title: Some(title.into()), ..TextBody::default() }
    }

    #[test]
    fn act_cites_its_title_and_loda_url() {
        let act = Act {
            text_id: LegiTextId::parse("LEGITEXT000033205014").unwrap(),
            // L'appel de note « (1) » du titre est gardé (ADR-0009).
            body: body("LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique (1)"),
        };
        let c = act.contract();
        assert_eq!(
            (c.citation.as_str(), c.source_url.as_str()),
            (
                "LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique (1)",
                "https://www.legifrance.gouv.fr/loda/id/LEGITEXT000033205014"
            )
        );
    }

    #[test]
    fn act_with_nature_code_uses_the_code_url() {
        let url = |nature: &str| {
            let act = Act {
                text_id: LegiTextId::parse("LEGITEXT000006069414").unwrap(),
                body: TextBody { nature: Some(nature.into()), ..body("Code civil") },
            };
            let contract = act.contract();
            (contract.source_url.as_str().to_owned(), contract.citation.as_str().to_owned())
        };
        let code = "https://www.legifrance.gouv.fr/codes/texte_lc/LEGITEXT000006069414";
        let loda = "https://www.legifrance.gouv.fr/loda/id/LEGITEXT000006069414";
        assert_eq!(url("code"), (code.to_owned(), "Code civil".to_owned()));
        assert_eq!(url("CODE").0, code);
        assert_eq!(url("LOI").0, loda);
        // Sans nature : LODA.
        let act = Act {
            text_id: LegiTextId::parse("LEGITEXT000006069414").unwrap(),
            body: body("Code civil"),
        };
        assert_eq!(act.contract().source_url.as_str(), loda);
    }

    #[test]
    fn text_body_default_is_empty() {
        let empty = TextBody::default();
        assert_eq!(
            (
                empty.cid,
                empty.nor,
                empty.title,
                empty.nature,
                empty.etat,
                empty.date_parution,
                empty.num_parution,
                empty.date_debut_version,
                empty.date_fin_version,
                empty.jo_reference,
                empty.aliases,
            ),
            (None, None, None, None, None, None, None, None, None, None, vec![])
        );
        assert_eq!(empty.toc, Toc::default());
    }

    #[test]
    fn text_body_carries_typed_fields() {
        let body = TextBody {
            cid: Some(JorfTextId::parse("JORFTEXT000033202746").unwrap()),
            nor: Nor::parse("aset2450455m"),
            etat: LegalStatus::parse("VIGUEUR"),
            jo_reference: Some("JORF n°0235 du 8 octobre 2016".into()),
            aliases: vec!["loi Lemaire".into()],
            ..TextBody::default()
        };
        assert_eq!(body.cid.as_ref().map(JorfTextId::as_str), Some("JORFTEXT000033202746"));
        assert_eq!(body.nor.as_ref().map(Nor::as_str), Some("ASET2450455M"));
        assert_eq!(body.etat, Some(LegalStatus::Vigueur));
        assert_eq!(body.aliases, ["loi Lemaire"]);
        assert_eq!(body.jo_reference.as_deref(), Some("JORF n°0235 du 8 octobre 2016"));
    }

    #[test]
    fn convention_carries_idcc_container_and_bulletin() {
        let convention = Convention {
            text_id: KaliTextId::parse("KALITEXT000049878950").unwrap(),
            body: body("Avenant"),
            idcc: Idcc::parse("2121"),
            container_title: Some("Convention collective nationale de l'édition".into()),
            container_id: Some("KALICONT000005635096".into()),
            bulletin: Some("BO n°2024-20".into()),
        };
        assert_eq!(convention.idcc.map(Idcc::get), Some(2121));
        assert_eq!(convention.container_id.as_deref(), Some("KALICONT000005635096"));
        assert_eq!(convention.bulletin.as_deref(), Some("BO n°2024-20"));
        assert_eq!(
            convention.container_title.as_deref(),
            Some("Convention collective nationale de l'édition")
        );
        // L'URL ne dépend pas du conteneur.
        assert_eq!(
            convention.contract().source_url.as_str(),
            "https://www.legifrance.gouv.fr/conv_coll/id/KALITEXT000049878950"
        );
    }

    #[test]
    fn convention_uses_conv_coll_url() {
        let convention = Convention {
            text_id: KaliTextId::parse("KALITEXT000049878950").unwrap(),
            body: body("Avenant"),
            idcc: None,
            container_title: None,
            container_id: None,
            bulletin: None,
        };
        let citation = convention.contract();
        assert_eq!(
            citation.source_url.as_str(),
            "https://www.legifrance.gouv.fr/conv_coll/id/KALITEXT000049878950"
        );
        assert_eq!(citation.citation.as_str(), "Avenant");
    }
}
