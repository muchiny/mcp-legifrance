//! Composant interne de mcp-legifrance : cache mémoire du port `LegalCorpus` (02 §3.4, §7.2,
//! ADR-0007).
//!
//! Rôle : [`CachedCorpus`], décorateur de n'importe quel `LegalCorpus`. Il garde en mémoire les
//! réponses des douze opérations du port, sans jamais parler à PISTE : l'adapter ne connaît pas le
//! cache, le cache ne connaît pas PISTE (R8), et un adapter DILA sans cache se branche tel quel.
//!
//! Comportement :
//! - **Clé** : `(environnement, opération typée)`, construite à partir des types du domaine
//!   normalisés, jamais du JSON envoyé à PISTE ; stable sur la journée de Paris, parce que la
//!   `VersionDate` est au jour près (D-01). Une réponse du bac à sable n'est jamais resservie en
//!   production (D-15).
//! - **Durée de vie** par opération : recherches 1 h, consultations 24 h, liste des codes 7 jours
//!   ([`CachePolicy`], 00 §9).
//! - **Coalescence** : des appels concurrents identiques n'évaluent qu'une seule fois l'appel
//!   décoré (`moka::future::Cache::entry(..).or_try_insert_with`).
//! - **Exclusions** : aucune erreur n'est mise en cache, ni aucune réponse vide (recherche sans
//!   résultat, liste vide) ; l'appelant reçoit quand même la réponse vide, en `Ok`.
//! - **Capacité** bornée en octets : un poids approximatif par entrée, saturé à `u32::MAX`
//!   (64 Mio par défaut, constante en v0.1).
//! - Sur un succès en cache : ni jeton OAuth, ni attente du limiteur, ni réessai.
//!
//! Piège connu et accepté (02 §3.4, piège 3) : si le futur qui initialise une entrée est
//! abandonné, `moka` relance un autre appelant en attente ; au-delà de 200 abandons consécutifs
//! sur la même clé, il panique. Ce cas pathologique n'a pas de remède côté décorateur.
//!
//! Modules (privés) : `key` (clé et opération), `policy` (durées, capacité, poids) et `corpus`
//! (`CachedCorpus`). Sont publics `CachedCorpus` et `CachePolicy`.

mod corpus;
mod key;
mod policy;

pub use corpus::CachedCorpus;
pub use policy::CachePolicy;

/// Fabriques partagées par les tests des trois modules (aucun test n'y vit).
#[cfg(test)]
mod fixtures {
    use std::num::NonZeroU32;

    use lf_application::ports::ArticleAt;
    use lf_domain::{
        date::VersionDate,
        entity::{
            Act, ArticleText, ArticleVersion, CodeArticleRef, CodeListing, CompanyAgreement,
            Convention, Decision, Deliberation, OfficialSummary, RelatedLink, RelatedLinks,
            RelatedNature, SearchHit, SearchPage, Snippet, TextBody, Toc,
        },
        fond::{Fond, PaginationMode},
        id::{ArticleId, DocumentId, LegiTextId},
        search::{
            MatchMode, NumberKind, NumberQuery, NumberValue, PageSize, QueryText, SearchQuery,
        },
        text::LongText,
    };

    use crate::key::Op;

    pub(crate) fn day(text: &str) -> VersionDate {
        VersionDate::parse(text).unwrap()
    }

    pub(crate) fn text_id() -> LegiTextId {
        "LEGITEXT000006070721".parse().unwrap()
    }

    pub(crate) fn article_id() -> ArticleId {
        "LEGIARTI000006419288".parse().unwrap()
    }

    /// Requête du fond `fond`, page 1, avec une date de version facultative.
    pub(crate) fn search_query(
        fond: Fond,
        text: &str,
        version_date: Option<VersionDate>,
    ) -> SearchQuery {
        SearchQuery::new(
            fond,
            QueryText::parse(text).unwrap(),
            NonZeroU32::MIN,
            PageSize::default(),
            MatchMode::default(),
            None,
            version_date,
        )
        .unwrap()
    }

    /// Requête JURI, page 1, sans date.
    pub(crate) fn query(text: &str) -> SearchQuery {
        search_query(Fond::Juri, text, None)
    }

    pub(crate) fn number_query(value: &str) -> NumberQuery {
        NumberQuery::new(
            Fond::Juri,
            NumberKind::Case,
            NumberValue::parse(value).unwrap(),
            day("2026-10-29"),
        )
        .unwrap()
    }

    /// Une valeur de chaque variante de `Op`, dans l'ordre de la définition.
    pub(crate) fn every_op() -> Vec<Op> {
        let today = day("2026-10-29");
        vec![
            Op::Search(search_query(Fond::Loda, "bail", Some(today))),
            Op::Act(text_id(), today),
            Op::Article(article_id()),
            Op::Decision("JURITEXT000037999394".parse().unwrap()),
            Op::Deliberation("CNILTEXT000053352643".parse().unwrap()),
            Op::Convention("KALITEXT000005652329".parse().unwrap()),
            Op::Agreement("ACCOTEXT000037731479".parse().unwrap()),
            Op::FindArticle(text_id(), "9".into(), ArticleAt::Today(today)),
            Op::FindByNumber(number_query("24-13.470")),
            Op::Codes,
            Op::Versions(article_id()),
            Op::Related(article_id()),
        ]
    }

    // Les entités ci-dessous remplissent chaque champ textuel avec `filler` : `weight(entité)`
    // croît alors de `(n - 1) * champs` quand `filler` passe de 1 à `n` octets (voir
    // `policy::tests`). Les extraits de résultats sont de taille fixe (plafonnés à 280).

    fn long(filler: &str) -> LongText {
        LongText::from_plain(filler).unwrap()
    }

    /// Résultat de recherche : 4 champs `filler`.
    pub(crate) fn hit(filler: &str) -> SearchHit {
        let mut hit =
            SearchHit::new(Fond::Juri, DocumentId::parse("JURITEXT000037999394").unwrap(), None)
                .unwrap();
        hit.title = Some(filler.into());
        hit.nature = Some(filler.into());
        hit.raison_sociale = Some(filler.into());
        hit.snippet = Snippet::new("un extrait de taille fixe");
        hit.articles =
            vec![CodeArticleRef::new(article_id(), Some(filler.into()), "extrait d'article")];
        hit
    }

    /// Page de `hits` résultats : 1 + 4 × `hits` champs `filler`.
    pub(crate) fn page(filler: &str, hits: usize) -> SearchPage {
        let hits = (0..hits).map(|_| hit(filler)).collect();
        SearchPage::new(Fond::Juri, filler, Some(2), None, hits, 0, PaginationMode::Defaut)
    }

    /// Page sans résultat.
    pub(crate) fn empty_page() -> SearchPage {
        page("bail", 0)
    }

    /// Article : 7 champs `filler`.
    pub(crate) fn article(filler: &str) -> ArticleText {
        ArticleText {
            article_id: article_id(),
            num: Some(filler.into()),
            etat: None,
            code_title: Some(filler.into()),
            parent: None,
            path: vec![filler.to_owned(), filler.to_owned()],
            full_sections_titre: Some(filler.into()),
            date_debut: None,
            date_fin: None,
            text: long(filler),
            nota: Some(long(filler)),
        }
    }

    /// Métadonnées de texte : 10 champs `filler`, dont une table des matières à deux niveaux.
    fn body(filler: &str) -> TextBody {
        use lf_domain::entity::{ArticleDraft, SectionDraft, TocDraft};
        let draft = TocDraft {
            sections: vec![SectionDraft {
                id: Some("LEGISCTA000033205040".into()),
                title: Some(filler.into()),
                articles: vec![ArticleDraft {
                    id: Some("LEGIARTI000033205126".into()),
                    num: Some(filler.into()),
                    content_html: Some(format!("<p>{filler}</p>")),
                    ..ArticleDraft::default()
                }],
                ..SectionDraft::default()
            }],
            articles: vec![ArticleDraft {
                id: Some("LEGIARTI000006419288".into()),
                num: Some(filler.into()),
                ..ArticleDraft::default()
            }],
        };
        TextBody {
            title: Some(filler.into()),
            nature: Some(filler.into()),
            num_parution: Some(filler.into()),
            jo_reference: Some(filler.into()),
            aliases: vec![filler.to_owned(), filler.to_owned()],
            toc: Toc::build(draft).unwrap(),
            ..TextBody::default()
        }
    }

    /// Texte consolidé : 10 champs `filler`.
    pub(crate) fn act(filler: &str) -> Act {
        Act { text_id: text_id(), body: body(filler) }
    }

    /// Convention collective : 13 champs `filler`.
    pub(crate) fn convention(filler: &str) -> Convention {
        Convention {
            text_id: "KALITEXT000005652329".parse().unwrap(),
            body: body(filler),
            idcc: None,
            container_title: Some(filler.into()),
            container_id: Some(filler.into()),
            bulletin: Some(filler.into()),
        }
    }

    /// Décision : 13 champs `filler`.
    pub(crate) fn decision(filler: &str) -> Decision {
        let two = || vec![filler.to_owned(), filler.to_owned()];
        Decision {
            decision_id: "JURITEXT000037999394".parse().unwrap(),
            ecli: None,
            num: Some(filler.into()),
            juridiction: Some(filler.into()),
            formation: Some(filler.into()),
            solution: Some(filler.into()),
            numero_affaire: two(),
            date_texte: None,
            nature: Some(filler.into()),
            title: Some(filler.into()),
            text: long(filler),
            official_summary: OfficialSummary { analyses: two(), titrages: two() },
        }
    }

    /// Délibération : 6 champs `filler`.
    pub(crate) fn deliberation(filler: &str) -> Deliberation {
        Deliberation {
            deliberation_id: "CNILTEXT000053352643".parse().unwrap(),
            num: Some(filler.into()),
            nature: Some(filler.into()),
            nature_delib: Some(filler.into()),
            etat: None,
            date_texte: None,
            date_publication: None,
            title: Some(filler.into()),
            title_long: Some(filler.into()),
            text: long(filler),
        }
    }

    /// Accord d'entreprise : 10 champs `filler`.
    pub(crate) fn agreement(filler: &str) -> CompanyAgreement {
        let two = || vec![filler.to_owned(), filler.to_owned()];
        CompanyAgreement {
            agreement_id: "ACCOTEXT000037731479".parse().unwrap(),
            nature: Some(filler.into()),
            title: Some(filler.into()),
            raison_sociale: Some(filler.into()),
            siret: Some(filler.into()),
            idcc: None,
            code_ape: Some(filler.into()),
            secteur: Some(filler.into()),
            themes: two(),
            syndicats: two(),
            date_texte: None,
            date_effet: None,
            date_depot: None,
        }
    }

    /// Liste de codes à `n` entrées : `n` champs `filler`.
    pub(crate) fn codes(filler: &str, n: usize) -> Vec<CodeListing> {
        (0..n)
            .map(|_| CodeListing { id: text_id(), title: filler.to_owned(), etat: None })
            .collect()
    }

    /// Versions d'un article, `n` entrées : `n` champs `filler`.
    pub(crate) fn versions(filler: &str, n: usize) -> Vec<ArticleVersion> {
        (0..n)
            .map(|_| ArticleVersion {
                id: "LEGIARTI000006419288".parse().unwrap(),
                num: Some(filler.into()),
                etat: None,
                date_debut: None,
                date_fin: None,
            })
            .collect()
    }

    fn link(filler: &str) -> RelatedLink {
        RelatedLink {
            target: None,
            nature: RelatedNature::Loda,
            title: Some(filler.into()),
            date: None,
        }
    }

    /// Liens d'un article, `cites` et `cited_by` entrées : `cites + cited_by` champs `filler`.
    pub(crate) fn related(filler: &str, cites: usize, cited_by: usize) -> RelatedLinks {
        RelatedLinks {
            cites: (0..cites).map(|_| link(filler)).collect(),
            cited_by: (0..cited_by).map(|_| link(filler)).collect(),
        }
    }
}
