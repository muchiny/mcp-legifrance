//! Entités renvoyées par le corpus. Les entités consultables et chaque
//! `SearchHit` implémentent [`Citable`](crate::contract::Citable) ;
//! `TextBody` et la table des matières ([`Toc`], ses nœuds et ses brouillons) non.
//!
//! La citation suit la règle de la famille (03 §5.4), puis le repli
//! `Légifrance {ID}` sur l'identifiant propre de l'entité (`id` pour un hit,
//! même quand l'URL emploie le `cid`, ruling C12) ; `ecli` n'est renseigné que
//! par `Decision`, depuis son ECLI natif ; `eli` reste `None` tant qu'aucune
//! entité ne porte d'ELI natif (V-102, ruling C6).
//!
//! Les champs publics en français (`etat`, `juridiction`, `raison_sociale`…)
//! reprennent les clés JSON de sortie de la référence (`citations.py:213-227`,
//! `264-275`, `341-354`, `381-394`, `465-485`) : §0, noms de champs JSON tels
//! quels, parité de migration.

mod act;
mod agreement;
mod article;
mod decision;
mod deliberation;
mod search;
mod toc;

pub use act::{Act, Convention, TextBody};
pub use agreement::{ATTACHMENT_NOTE, CompanyAgreement};
pub use article::ArticleText;
pub use decision::Decision;
pub use deliberation::Deliberation;
pub use search::{CodeArticleRef, SNIPPET_MAX_CHARS, SearchHit, SearchPage};
pub use toc::{
    ArticleDraft, SectionDraft, TOC_MAX_DEPTH, Toc, TocArticle, TocDraft, TocEntry, TocEntryRef,
    TocNode, TocPage, TocSection,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contract::{Citable, CitationContract},
        fond::Fond,
        id::{AccoTextId, AnyId, ArticleId, CnilTextId, DecisionId, KaliTextId, LegiTextId},
    };
    use pretty_assertions::assert_eq;

    fn article(id: &str, title: Option<&str>) -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(id).unwrap(),
            num: None,
            etat: None,
            code_title: title.map(Into::into),
            full_sections_titre: None,
            date_debut: None,
            date_fin: None,
            text: None,
        }
    }

    fn decision(id: &str, title: Option<&str>) -> Decision {
        Decision {
            decision_id: DecisionId::parse(id).unwrap(),
            ecli: None,
            num: None,
            juridiction: None,
            formation: None,
            solution: None,
            numero_affaire: vec![],
            date_texte: None,
            nature: None,
            title: title.map(Into::into),
            text: None,
        }
    }

    fn hit(fond: Fond, id: &str, cid: Option<&str>, title: Option<&str>) -> SearchHit {
        SearchHit {
            fond,
            id: AnyId::parse(id).unwrap(),
            cid: cid.map(|c| AnyId::parse(c).unwrap()),
            title: title.map(Into::into),
            nature: None,
            etat: None,
            idcc: None,
            raison_sociale: None,
            articles: vec![],
        }
    }

    /// Les onze contrats des sept `impl Citable`, tous champs optionnels à
    /// `None` sauf le titre, avec l'identifiant propre attendu au repli.
    fn contracts(title: Option<&str>) -> Vec<(CitationContract, &'static str)> {
        let body = || TextBody { title: title.map(Into::into), ..TextBody::default() };
        vec![
            (
                Act { text_id: LegiTextId::parse("LEGITEXT000033205014").unwrap(), body: body() }
                    .contract(),
                "LEGITEXT000033205014",
            ),
            (
                Convention {
                    text_id: KaliTextId::parse("KALITEXT000049878950").unwrap(),
                    body: body(),
                }
                .contract(),
                "KALITEXT000049878950",
            ),
            (article("LEGIARTI000006419288", title).contract(), "LEGIARTI000006419288"),
            (article("KALIARTI000049886059", title).contract(), "KALIARTI000049886059"),
            (decision("JURITEXT000048465611", title).contract(), "JURITEXT000048465611"),
            (decision("CONSTEXT000052555275", title).contract(), "CONSTEXT000052555275"),
            (decision("CETATEXT000054178503", title).contract(), "CETATEXT000054178503"),
            (
                Deliberation {
                    deliberation_id: CnilTextId::parse("CNILTEXT000053352643").unwrap(),
                    num: None,
                    nature: None,
                    nature_delib: None,
                    etat: None,
                    date_texte: None,
                    date_publication: None,
                    title: title.map(Into::into),
                    title_long: None,
                    text: None,
                }
                .contract(),
                "CNILTEXT000053352643",
            ),
            (
                CompanyAgreement {
                    agreement_id: AccoTextId::parse("ACCOTEXT000053643828").unwrap(),
                    nature: None,
                    title: title.map(Into::into),
                    raison_sociale: None,
                    siret: None,
                    idcc: None,
                    code_ape: None,
                    secteur: None,
                    themes: vec![],
                    syndicats: vec![],
                    date_texte: None,
                    date_effet: None,
                    date_depot: None,
                }
                .contract(),
                "ACCOTEXT000053643828",
            ),
            (
                hit(Fond::Juri, "JURITEXT000048465611", Some("JURITEXT000011111111"), title)
                    .contract(),
                "JURITEXT000048465611",
            ),
            (
                hit(Fond::Acco, "ACCOTEXT000053643828", None, title).contract(),
                "ACCOTEXT000053643828",
            ),
        ]
    }

    #[test]
    fn citation_never_empty() {
        // Ni titre, ni numéro, ni date (ou un titre blanc) : repli `Légifrance
        // {ID}` (03 §5.1) ; ni ECLI ni ELI fabriqués (ruling C6 : `eli` toujours
        // `None`).
        for title in [None, Some(" \n\t")] {
            for (c, id) in contracts(title) {
                let fallback = format!("Légifrance {id}");
                assert_eq!(
                    (c.citation.as_str(), c.citation.is_truncated(), c.ecli, c.eli),
                    (fallback.as_str(), false, None, None),
                    "{id} {title:?}"
                );
            }
        }
        // Titre de 2 000 caractères : coupé en fin de mot, `…` compris, 996
        // caractères ici (la dernière espace des 999 premiers est à l'indice 995).
        let long = "mot ".repeat(500);
        let cut = format!("{}mot…", "mot ".repeat(248));
        // Titre multiligne : une ligne.
        let multiline = "Accord\nrelatif  au\r\ntélétravail";
        let cases = [
            (long.as_str(), cut.as_str(), true),
            (multiline, "Accord relatif au télétravail", false),
        ];
        for (title, expected, truncated) in cases {
            for (c, id) in contracts(Some(title)) {
                assert_eq!(
                    (c.citation.as_str(), c.citation.is_truncated()),
                    (expected, truncated),
                    "{id}"
                );
            }
        }
    }
}
