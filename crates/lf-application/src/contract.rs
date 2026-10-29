//! Suite de contrat du port `LegalCorpus` (06 §2.2, 02 §12.2), derrière la feature `test-support`.
//!
//! [`corpus_contract`] rejoue les mêmes entrées contre toute implémentation du port et vérifie
//! les mêmes formes de sortie, sur les douze opérations : [`corpus_contract_m5`] pour les sept de
//! la v0.1 (`search`, `get_act`, `get_article`, `get_decision`, `get_deliberation`,
//! `get_convention`, `get_company_agreement`) et [`corpus_contract_m6_m7`] pour les cinq de M6 et
//! M7 (`find_article`, `find_by_number`, `list_codes`, `article_versions`, `related_links`) :
//!
//! - **contrat de citation v2** : citation sur une ligne et jamais vide, `source_url` sous
//!   legifrance.gouv.fr (qui se termine par l'identifiant pour une consultation), jamais d'ELI
//!   fabriqué, ECLI de la décision repris tel quel ;
//! - **identifiant** : l'entité rendue porte l'identifiant demandé, et chaque résultat de recherche
//!   un identifiant du fond interrogé ; `find_article` rend l'article du numéro demandé, dans les
//!   deux variantes de [`ArticleAt`] ;
//! - **sentinelles** : aucune date à partir de `SENTINEL_FLOOR` (2222-01-01) ne sort du port, elle
//!   est `None` ;
//! - **table des matières** ordonnée et dédoublonnée : en préordre sans saut de profondeur, sans
//!   article en double, et la même suite d'entrées lue d'un bloc ou par pages de deux ;
//! - **listes** : codes sans doublon, tous titrés, dont le code de référence ; versions d'un
//!   article sans doublon, de la plus ancienne à la plus récente, dont la version demandée ; liens
//!   d'un article jamais vides (un titre, ou une cible) ;
//! - **déterminisme** : deux appels aux mêmes arguments rendent des valeurs égales ;
//! - **introuvable** : un identifiant absent donne `CorpusError::NotFound` pour les six
//!   consultations, `article_versions` et `find_article` (numéro absent, en `Today` comme en
//!   `On`). Les deux autres opérations M6 et M7 suivent PISTE (04 §11.1) : une recherche par
//!   numéro sans résultat est une liste vide, jamais une erreur, et `related_links` d'un article
//!   inconnu est une erreur (`NotFound`, ou `Upstream` comme le 500 de la sandbox), jamais des
//!   liens vides inventés.
//!
//! Les cas viennent d'un fournisseur, [`ContractFixtures`] : [`ContractFixtures::reference`] reprend
//! les identifiants des seize réponses de référence de `lf-adapter-piste`
//! (`tests/fixtures/reference`) et ceux des fixtures enregistrées de M6 et M7
//! (`tests/fixtures/recorded/{search/m6,consult/*/m6,list/code/m6,*/m7}`). Chaque corpus sous test
//! arrange ses réponses autour de ces cas : `FakeCorpus` par [`reference_fake`], `PisteCorpus` par
//! un `wiremock` qui sert les fixtures, `CachedCorpus` par un `FakeCorpus` décoré (R8 : le cache ne
//! dépend pas de l'adapter PISTE).
//!
//! **Ordre des appels.** Chaque opération est appelée deux fois pour un cas présent, une fois pour
//! un cas absent ; l'ordre est celui de [`corpus_contract`], que [`reference_fake`] suit pour
//! programmer les files de `FakeCorpus` (une file par méthode).
//!
//! Le module est du code de test compilé en dehors de `cfg(test)` : un échec de contrat est une
//! panique, d'où l'`expect` des résultats. Il est couvert par son propre module de tests.

#![expect(
    clippy::expect_used,
    reason = "suite de contrat : un résultat inattendu est un échec du test, donc une panique"
)]

use std::collections::HashSet;
use std::num::NonZeroU32;

use lf_domain::{
    contract::{Citable, CitationContract},
    date::{CivilDate, SENTINEL_FLOOR, VersionDate},
    entity::{
        Act, ArticleDraft, ArticleText, ArticleVersion, CodeArticleRef, CodeListing,
        CompanyAgreement, Convention, Decision, Deliberation, RelatedLink, RelatedLinks,
        RelatedNature, SearchHit, SearchPage, SectionDraft, Snippet, TextBody, Toc, TocDraft,
        TocEntryRef,
    },
    fond::{Fond, PaginationMode},
    id::{
        AccoTextId, ArticleId, CnilTextId, DecisionId, DocumentId, KaliTextId, LegiArtiId,
        LegiTextId,
    },
    search::{MatchMode, NumberKind, NumberQuery, NumberValue, PageSize, QueryText, SearchQuery},
    status::LegalStatus,
    text::LongText,
    url::WEB_BASE,
};

use crate::{
    ports::{ArticleAt, CorpusError, LegalCorpus},
    testing::FakeCorpus,
};

/// Identifiants qu'aucun corpus ne connaît : douze zéros, comme les sondes enregistrées en sandbox
/// (`tests/fixtures/recorded/consult/*/inconnu`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbsentIds {
    /// Texte LEGI absent.
    pub act: LegiTextId,
    /// Article absent.
    pub article: ArticleId,
    /// Décision absente.
    pub decision: DecisionId,
    /// Délibération absente.
    pub deliberation: CnilTextId,
    /// Convention absente.
    pub convention: KaliTextId,
    /// Accord absent.
    pub agreement: AccoTextId,
}

/// Les cas de la suite de contrat : mêmes entrées pour tous les corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractFixtures {
    /// Date de version des recherches LODA et CODE, et de `get_act`.
    pub version_date: VersionDate,
    /// Une recherche par fond, page 1, taille par défaut.
    pub searches: Vec<SearchQuery>,
    /// Texte LEGI consultable.
    pub act: LegiTextId,
    /// Article consultable.
    pub article: ArticleId,
    /// Décisions consultables : une `JURITEXT`, une `CONSTEXT`, une `CETATEXT`.
    pub decisions: Vec<DecisionId>,
    /// Délibération CNIL consultable.
    pub deliberation: CnilTextId,
    /// Convention collective consultable.
    pub convention: KaliTextId,
    /// Accord d'entreprise consultable.
    pub agreement: AccoTextId,
    /// Un identifiant absent par type de consultation.
    pub absent: AbsentIds,
    /// Code qui porte `article` sous le numéro `article_num` : texte de `find_article`, présent
    /// dans `list_codes`.
    pub code: LegiTextId,
    /// Numéro de `article` dans `code`.
    pub article_num: String,
    /// Numéro qu'aucun article de `code` ne porte.
    pub absent_article_num: String,
    /// Recherches par numéro qui donnent au moins un résultat.
    pub numbers: Vec<NumberQuery>,
    /// Recherches par numéro sans résultat : une liste vide, jamais une erreur.
    pub absent_numbers: Vec<NumberQuery>,
}

impl ContractFixtures {
    /// Les cas de référence : les identifiants des seize réponses enregistrées par la référence
    /// (`consult_*` et `search_*`), la date de version du 2026-10-15 et, pour chaque consultation,
    /// un identifiant à douze zéros. Pour M6 et M7, l'article 9 du Code civil (celui de
    /// `consult_article`, aussi enregistré par `getArticleWithIdAndNum`), la recherche par ECLI,
    /// par numéro d'article et par IDCC, et trois cas sans résultat (numéro d'article inconnu, ECLI
    /// inconnu, NOR inconnu), tous enregistrés en sandbox.
    ///
    /// # Panics
    ///
    /// Jamais : tous les identifiants, la date et les requêtes sont des constantes valides.
    #[must_use]
    pub fn reference() -> Self {
        let version_date = VersionDate::parse("2026-10-15").expect("valid version date");
        let number = |fond: Fond, kind: NumberKind, value: &str| {
            let value = NumberValue::parse(value).expect("valid number value");
            NumberQuery::new(fond, kind, value, version_date).expect("applicable number kind")
        };
        let query = |fond: Fond| {
            SearchQuery::new(
                fond,
                QueryText::parse("golden").expect("valid query text"),
                NonZeroU32::MIN,
                PageSize::default(),
                MatchMode::default(),
                None,
                fond.needs_version_date().then_some(version_date),
            )
            .expect("valid search query")
        };
        let decision = |id: &str| DecisionId::parse(id).expect("valid decision id");
        Self {
            version_date,
            searches: Fond::ALL.into_iter().map(query).collect(),
            act: LegiTextId::parse("LEGITEXT000033205014").expect("valid act id"),
            article: ArticleId::parse("LEGIARTI000006419288").expect("valid article id"),
            decisions: vec![
                decision("JURITEXT000051743650"),
                decision("CONSTEXT000052555275"),
                decision("CETATEXT000054178503"),
            ],
            deliberation: CnilTextId::parse("CNILTEXT000053352643").expect("valid cnil id"),
            convention: KaliTextId::parse("KALITEXT000049878950").expect("valid kali id"),
            agreement: AccoTextId::parse("ACCOTEXT000053643828").expect("valid acco id"),
            absent: AbsentIds {
                act: LegiTextId::parse("LEGITEXT000000000000").expect("valid act id"),
                article: ArticleId::parse("LEGIARTI000000000000").expect("valid article id"),
                decision: decision("JURITEXT000000000000"),
                deliberation: CnilTextId::parse("CNILTEXT000000000000").expect("valid cnil id"),
                convention: KaliTextId::parse("KALITEXT000000000000").expect("valid kali id"),
                agreement: AccoTextId::parse("ACCOTEXT000000000000").expect("valid acco id"),
            },
            code: LegiTextId::parse("LEGITEXT000006070721").expect("valid code id"),
            article_num: "9".to_owned(),
            absent_article_num: "99999".to_owned(),
            numbers: vec![
                number(Fond::Juri, NumberKind::Ecli, "ECLI:FR:CCASS:2025:C100399"),
                number(Fond::Code, NumberKind::Article, "9"),
                number(Fond::Kali, NumberKind::Idcc, "2121"),
            ],
            absent_numbers: vec![
                number(Fond::Juri, NumberKind::Ecli, "ECLI:FR:CCASS:1900:C000000"),
                number(Fond::Loda, NumberKind::Nor, "ZZZZ0000000Z"),
            ],
        }
    }
}

/// Suite de contrat des douze opérations du port (T-M3-05, T-M5-06, V3-12) : les sept de la v0.1
/// puis les cinq de M6 et M7.
///
/// Panique à la première forme de sortie non conforme, avec un message qui nomme l'opération.
///
/// # Panics
///
/// Si `corpus` ne respecte pas le contrat décrit dans la documentation du module, ou s'il n'a pas
/// de réponse pour l'un des cas de `fx`.
pub async fn corpus_contract<C: LegalCorpus>(corpus: &C, fx: &ContractFixtures) {
    corpus_contract_m5(corpus, fx).await;
    corpus_contract_m6_m7(corpus, fx).await;
}

/// Suite de contrat des cinq opérations de M6 et M7, dans l'ordre du port : `find_article`,
/// `find_by_number`, `list_codes`, `article_versions` et `related_links`. Un corpus qui ne couvre
/// que la v0.1 n'en exécute que [`corpus_contract_m5`].
///
/// # Panics
///
/// Si `corpus` ne respecte pas le contrat décrit dans la documentation du module, ou s'il n'a pas
/// de réponse pour l'un des cas de `fx`.
pub async fn corpus_contract_m6_m7<C: LegalCorpus>(corpus: &C, fx: &ContractFixtures) {
    check_find_article(corpus, fx).await;
    check_find_by_number(corpus, fx).await;
    check_list_codes(corpus, fx).await;
    check_article_versions(corpus, fx).await;
    check_related_links(corpus, fx).await;
}

/// Suite de contrat des sept opérations de la v0.1 (`search`, `get_act`, `get_article`,
/// `get_decision`, `get_deliberation`, `get_convention`, `get_company_agreement`).
///
/// # Panics
///
/// Si `corpus` ne respecte pas le contrat décrit dans la documentation du module, ou s'il n'a pas
/// de réponse pour l'un des cas de `fx`.
pub async fn corpus_contract_m5<C: LegalCorpus>(corpus: &C, fx: &ContractFixtures) {
    check_searches(corpus, fx).await;
    check_act(corpus, fx).await;
    check_article(corpus, fx).await;
    check_decisions(corpus, fx).await;
    check_deliberation(corpus, fx).await;
    check_convention(corpus, fx).await;
    check_agreement(corpus, fx).await;
    check_absent(corpus, fx).await;
}

async fn check_searches<C: LegalCorpus>(corpus: &C, fx: &ContractFixtures) {
    for query in &fx.searches {
        let fond = query.fond();
        let page = corpus.search(query).await.expect("search of a present case");
        let again = corpus.search(query).await.expect("second search of a present case");
        assert_eq!(page, again, "search {fond:?} is deterministic");
        assert_eq!(page.fond, fond, "search {fond:?} answers the fond asked");
        assert_eq!(page.query, query.text().as_str(), "search {fond:?} echoes the query");
        let shown = u64::try_from(page.hits.len()).expect("hit count fits u64");
        assert!(shown <= page.total, "search {fond:?}: {shown} hits for a total of {}", page.total);
        let size = usize::try_from(query.page_size().get()).expect("page size fits usize");
        assert!(page.hits.len() <= size, "search {fond:?}: more hits than the page size");
        assert!(!page.hits.is_empty(), "search {fond:?}: the reference case has results");
        for hit in &page.hits {
            let id = hit.id.as_str();
            assert_eq!(hit.fond, fond, "search {fond:?}: hit {id} belongs to the fond");
            assert!(
                id.starts_with(fond.hit_prefix()),
                "search {fond:?}: hit {id} has a foreign id"
            );
            assert_citation(&hit.contract(), id);
            assert_no_sentinel(id, &[hit.date, hit.version_start]);
        }
    }
}

async fn check_act<C: LegalCorpus>(corpus: &C, fx: &ContractFixtures) {
    let act = corpus.get_act(&fx.act, fx.version_date).await.expect("get_act of a present id");
    let again = corpus.get_act(&fx.act, fx.version_date).await.expect("second get_act");
    assert_eq!(act, again, "get_act is deterministic");
    assert_eq!(act.text_id, fx.act, "get_act echoes the requested id");
    assert_consulted(&act.contract(), fx.act.as_str());
    let body = &act.body;
    assert_no_sentinel("get_act", &[body.date_parution, body.date_debut_version]);
    assert_no_sentinel("get_act", &[body.date_fin_version]);
    assert!(!body.toc.is_empty(), "get_act: the reference text has a table of contents");
    assert_toc("get_act", &body.toc);
}

async fn check_article<C: LegalCorpus>(corpus: &C, fx: &ContractFixtures) {
    let article = corpus.get_article(&fx.article).await.expect("get_article of a present id");
    let again = corpus.get_article(&fx.article).await.expect("second get_article");
    assert_eq!(article, again, "get_article is deterministic");
    assert_eq!(article.article_id, fx.article, "get_article echoes the requested id");
    assert_consulted(&article.contract(), fx.article.as_str());
    assert_no_sentinel("get_article", &[article.date_debut, article.date_fin]);
    assert!(!article.text.is_empty(), "get_article: the reference article has a text");
}

async fn check_decisions<C: LegalCorpus>(corpus: &C, fx: &ContractFixtures) {
    for id in &fx.decisions {
        let decision = corpus.get_decision(id).await.expect("get_decision of a present id");
        let again = corpus.get_decision(id).await.expect("second get_decision");
        assert_eq!(decision, again, "get_decision {id} is deterministic");
        assert_eq!(decision.decision_id, *id, "get_decision echoes the requested id");
        let contract = decision.contract();
        assert_consulted(&contract, id.as_str());
        assert_eq!(contract.ecli, decision.ecli, "get_decision {id}: only the native ECLI");
        assert_no_sentinel(id.as_str(), &[decision.date_texte]);
        assert!(!decision.text.is_empty(), "get_decision {id}: the reference text is not empty");
    }
}

async fn check_deliberation<C: LegalCorpus>(corpus: &C, fx: &ContractFixtures) {
    let id = &fx.deliberation;
    let deliberation = corpus.get_deliberation(id).await.expect("get_deliberation");
    let again = corpus.get_deliberation(id).await.expect("second get_deliberation");
    assert_eq!(deliberation, again, "get_deliberation is deterministic");
    assert_eq!(deliberation.deliberation_id, *id, "get_deliberation echoes the requested id");
    assert_consulted(&deliberation.contract(), id.as_str());
    assert_no_sentinel(
        "get_deliberation",
        &[deliberation.date_texte, deliberation.date_publication],
    );
    assert!(!deliberation.text.is_empty(), "get_deliberation: the reference text is not empty");
}

async fn check_convention<C: LegalCorpus>(corpus: &C, fx: &ContractFixtures) {
    let id = &fx.convention;
    let convention = corpus.get_convention(id).await.expect("get_convention");
    let again = corpus.get_convention(id).await.expect("second get_convention");
    assert_eq!(convention, again, "get_convention is deterministic");
    assert_eq!(convention.text_id, *id, "get_convention echoes the requested id");
    assert_consulted(&convention.contract(), id.as_str());
    let body = &convention.body;
    assert_no_sentinel("get_convention", &[body.date_parution, body.date_debut_version]);
    assert_no_sentinel("get_convention", &[body.date_fin_version]);
    assert_toc("get_convention", &body.toc);
}

async fn check_agreement<C: LegalCorpus>(corpus: &C, fx: &ContractFixtures) {
    let id = &fx.agreement;
    let agreement = corpus.get_company_agreement(id).await.expect("get_company_agreement");
    let again = corpus.get_company_agreement(id).await.expect("second get_company_agreement");
    assert_eq!(agreement, again, "get_company_agreement is deterministic");
    assert_eq!(agreement.agreement_id, *id, "get_company_agreement echoes the requested id");
    assert_consulted(&agreement.contract(), id.as_str());
    let dates = [agreement.date_texte, agreement.date_effet, agreement.date_depot];
    assert_no_sentinel("get_company_agreement", &dates);
}

async fn check_absent<C: LegalCorpus>(corpus: &C, fx: &ContractFixtures) {
    let absent = &fx.absent;
    let answers = [
        ("get_act", corpus.get_act(&absent.act, fx.version_date).await.map(drop)),
        ("get_article", corpus.get_article(&absent.article).await.map(drop)),
        ("get_decision", corpus.get_decision(&absent.decision).await.map(drop)),
        ("get_deliberation", corpus.get_deliberation(&absent.deliberation).await.map(drop)),
        ("get_convention", corpus.get_convention(&absent.convention).await.map(drop)),
        ("get_company_agreement", corpus.get_company_agreement(&absent.agreement).await.map(drop)),
    ];
    for (method, answer) in answers {
        assert!(
            matches!(answer, Err(CorpusError::NotFound { .. })),
            "{method} of an absent id must be NotFound, got {answer:?}"
        );
    }
}

/// `find_article` (M6) : l'article du numéro demandé, à l'identique pour `Today` et pour `On`
/// (D5), puis `NotFound` pour un numéro qu'aucun article du code ne porte, dans les deux variantes.
async fn check_find_article<C: LegalCorpus>(corpus: &C, fx: &ContractFixtures) {
    let (text, num, day) = (&fx.code, fx.article_num.as_str(), fx.version_date);
    let variants = [("today", ArticleAt::Today(day)), ("on a date", ArticleAt::On(day))];
    for (label, at) in variants {
        let article = corpus.find_article(text, num, at).await.expect("find_article of a number");
        let again = corpus.find_article(text, num, at).await.expect("second find_article");
        assert_eq!(article, again, "find_article {label} is deterministic");
        assert_eq!(
            article.article_id, fx.article,
            "find_article {label} answers the article of that number"
        );
        let echoed = article.num.as_deref();
        assert!(
            echoed.is_some_and(|echoed| echoed.trim().eq_ignore_ascii_case(num)),
            "find_article {label} echoes the number, got {echoed:?}"
        );
        assert_consulted(&article.contract(), fx.article.as_str());
        assert_no_sentinel("find_article", &[article.date_debut, article.date_fin]);
        assert!(!article.text.is_empty(), "find_article {label}: the reference article has a text");
    }
    for (label, at) in variants {
        let answer = corpus.find_article(text, &fx.absent_article_num, at).await.map(drop);
        assert!(
            matches!(answer, Err(CorpusError::NotFound { .. })),
            "find_article {label} of an absent number must be NotFound, got {answer:?}"
        );
    }
}

/// `find_by_number` (M6) : des résultats du fond interrogé, au contrat de citation, puis une liste
/// vide (et non une erreur) pour un numéro sans résultat.
async fn check_find_by_number<C: LegalCorpus>(corpus: &C, fx: &ContractFixtures) {
    for query in &fx.numbers {
        let what = format!("{} {}", query.fond.name(), query.kind.as_str());
        let hits = corpus.find_by_number(query).await.expect("find_by_number of a present number");
        let again = corpus.find_by_number(query).await.expect("second find_by_number");
        assert_eq!(hits, again, "find_by_number {what} is deterministic");
        assert!(!hits.is_empty(), "find_by_number {what}: the reference case has results");
        for hit in &hits {
            let id = hit.id.as_str();
            assert_eq!(hit.fond, query.fond, "find_by_number {what}: hit {id} belongs to the fond");
            assert!(
                id.starts_with(query.fond.hit_prefix()),
                "find_by_number {what}: hit {id} has a foreign id"
            );
            assert_citation(&hit.contract(), id);
            assert_no_sentinel(id, &[hit.date, hit.version_start]);
            if query.kind == NumberKind::Article {
                let carries = hit.articles.iter().any(|found| {
                    let num = found.num.as_deref();
                    num.is_some_and(|n| n.trim().eq_ignore_ascii_case(query.value.as_str()))
                });
                assert!(carries, "find_by_number {what}: hit {id} carries the article asked");
            }
        }
    }
    for query in &fx.absent_numbers {
        let what = format!("{} {}", query.fond.name(), query.kind.as_str());
        let hits = corpus.find_by_number(query).await;
        assert!(
            matches!(&hits, Ok(found) if found.is_empty()),
            "find_by_number {what} of an absent number must be an empty list, got {hits:?}"
        );
    }
}

/// `list_codes` (M6) : des codes sans doublon, tous titrés, dont le code de référence.
async fn check_list_codes<C: LegalCorpus>(corpus: &C, fx: &ContractFixtures) {
    let codes = corpus.list_codes().await.expect("list_codes");
    let again = corpus.list_codes().await.expect("second list_codes");
    assert_eq!(codes, again, "list_codes is deterministic");
    let distinct: HashSet<&str> = codes.iter().map(|code| code.id.as_str()).collect();
    assert_eq!(distinct.len(), codes.len(), "list_codes: codes are deduplicated");
    assert!(
        codes.iter().all(|code| !code.title.trim().is_empty()),
        "list_codes: every code has a title"
    );
    assert!(codes.iter().any(|code| code.id == fx.code), "list_codes lists the reference code");
}

/// `article_versions` (M7) : les versions de l'article, sans doublon, de la plus ancienne à la
/// plus récente, dont celle qu'on demande ; `NotFound` pour un article absent.
async fn check_article_versions<C: LegalCorpus>(corpus: &C, fx: &ContractFixtures) {
    let id = &fx.article;
    let versions = corpus.article_versions(id).await.expect("article_versions of a present id");
    let again = corpus.article_versions(id).await.expect("second article_versions");
    assert_eq!(versions, again, "article_versions is deterministic");
    assert!(
        versions.iter().any(|version| version.id.as_str() == id.as_str()),
        "article_versions lists the requested version"
    );
    let distinct: HashSet<&str> = versions.iter().map(|version| version.id.as_str()).collect();
    assert_eq!(distinct.len(), versions.len(), "article_versions: versions are deduplicated");
    let starts: Vec<CivilDate> = versions.iter().filter_map(|version| version.date_debut).collect();
    assert!(starts.is_sorted(), "article_versions: versions come oldest first");
    for version in &versions {
        let version_id = version.id.as_str();
        assert_no_sentinel(version_id, &[version.date_debut, version.date_fin]);
        if let (Some(start), Some(end)) = (version.date_debut, version.date_fin) {
            assert!(start <= end, "article_versions: version {version_id} ends before it starts");
        }
    }
    let absent = corpus.article_versions(&fx.absent.article).await.map(drop);
    assert!(
        matches!(absent, Err(CorpusError::NotFound { .. })),
        "article_versions of an absent id must be NotFound, got {absent:?}"
    );
}

/// `related_links` (M7) : des liens qui gardent un titre ou une cible, sans date sentinelle ; une
/// erreur (jamais des liens vides inventés) pour un article absent.
async fn check_related_links<C: LegalCorpus>(corpus: &C, fx: &ContractFixtures) {
    let id = &fx.article;
    let links = corpus.related_links(id).await.expect("related_links of a present id");
    let again = corpus.related_links(id).await.expect("second related_links");
    assert_eq!(links, again, "related_links is deterministic");
    let every: Vec<&RelatedLink> = links.cites.iter().chain(&links.cited_by).collect();
    assert!(!every.is_empty(), "related_links: the reference article has links");
    for link in every {
        let titled = link.title.as_deref().is_some_and(|title| !title.trim().is_empty());
        assert!(link.target.is_some() || titled, "related_links: a link keeps a title or a target");
        assert_no_sentinel("related_links", &[link.date]);
    }
    let absent = corpus.related_links(&fx.absent.article).await.map(drop);
    assert!(
        matches!(absent, Err(CorpusError::NotFound { .. } | CorpusError::Upstream { .. })),
        "related_links of an absent id must be an error, got {absent:?}"
    );
}

/// Contrat de citation v2 d'une entité (ADR-0009) : citation sur une ligne et jamais vide, page
/// legifrance.gouv.fr, aucun ELI (l'API n'en donne pas, V-102).
fn assert_citation(contract: &CitationContract, id: &str) {
    let citation = contract.citation.as_str();
    assert!(!citation.is_empty(), "{id}: citation is never empty");
    assert!(!citation.contains('\n'), "{id}: citation is a single line");
    let url = contract.source_url.as_str();
    assert!(url.starts_with(WEB_BASE), "{id}: source_url {url} is a Légifrance page");
    assert!(contract.eli.is_none(), "{id}: no ELI is ever built");
}

/// Contrat de citation d'une consultation : en plus, l'URL désigne l'identifiant demandé.
fn assert_consulted(contract: &CitationContract, id: &str) {
    assert_citation(contract, id);
    let url = contract.source_url.as_str();
    assert!(url.ends_with(id), "{id}: source_url {url} points at the document");
}

/// Aucune date de la zone sentinelle (2999-01-01, 2222-02-22) : l'API les rend, le port les écarte.
fn assert_no_sentinel(what: &str, dates: &[Option<CivilDate>]) {
    for date in dates.iter().flatten() {
        assert!(*date < SENTINEL_FLOOR, "{what}: the sentinel date {date} must be None");
    }
}

/// Table des matières bien formée : préordre sans saut de profondeur, articles sans doublon, et
/// lecture par pages de deux identique à la lecture d'un bloc.
fn assert_toc(what: &str, toc: &Toc) {
    let whole = toc.page(0, NonZeroU32::MAX).expect("the whole table of contents");
    assert_eq!(whole.total, toc.len(), "{what}: toc total");
    assert_eq!(whole.next_offset, None, "{what}: one block reads the whole toc");
    let mut previous = 0;
    for entry in &whole.entries {
        assert!(
            (1..=previous + 1).contains(&entry.depth),
            "{what}: toc depth {} follows depth {previous}",
            entry.depth
        );
        previous = entry.depth;
    }
    let articles: Vec<&str> = whole
        .entries
        .iter()
        .filter_map(|entry| match entry.node {
            TocEntryRef::Article(article) => Some(article.article_id.as_str()),
            TocEntryRef::Section { .. } => None,
        })
        .collect();
    let distinct: HashSet<&str> = articles.iter().copied().collect();
    assert_eq!(distinct.len(), articles.len(), "{what}: toc articles are deduplicated");
    let two = NonZeroU32::new(2).expect("two is not zero");
    let mut stitched = Vec::new();
    let mut next = Some(0);
    while let Some(offset) = next {
        let page = toc.page(offset, two).expect("a page inside the toc");
        stitched.extend(page.entries);
        next = page.next_offset;
    }
    assert_eq!(stitched, whole.entries, "{what}: paged reading equals the block reading");
}

/// Variante de [`corpus_contract`] côté `FakeCorpus` : programme un faux corpus avec les
/// réponses des cas de `fx`, `copies` fois chacune (2 pour un corpus nu, 1 derrière un cache, qui
/// ne rappelle pas le corpus décoré pour la seconde lecture), puis la réponse de chaque cas absent
/// (une fois : une erreur ou une liste vide n'est jamais mise en cache). Les entités sont
/// construites à la main ; l'ordre des `push` est celui des appels de la suite.
///
/// Un article absent de `related_links` répond `Upstream` 500, comme la sandbox (04 §11.1).
///
/// # Panics
///
/// Si `fx.article` n'est pas un `LEGIARTI` : les versions d'un article ne sont pas celles d'un
/// article de convention collective.
#[must_use]
pub fn reference_fake(fx: &ContractFixtures, copies: usize) -> FakeCorpus {
    let fake = FakeCorpus::new();
    let missing = |what: &str| CorpusError::NotFound { what: what.to_owned() };
    for query in &fx.searches {
        for _ in 0..copies {
            fake.push_search(Ok(sample_page(query)));
        }
    }
    for _ in 0..copies {
        fake.push_get_act(Ok(sample_act(&fx.act, fx.version_date)));
        fake.push_get_article(Ok(sample_article(&fx.article, fx.version_date)));
    }
    for id in &fx.decisions {
        for _ in 0..copies {
            fake.push_get_decision(Ok(sample_decision(id)));
        }
    }
    for _ in 0..copies {
        fake.push_get_deliberation(Ok(sample_deliberation(&fx.deliberation)));
        fake.push_get_convention(Ok(sample_convention(&fx.convention, fx.version_date)));
        fake.push_get_company_agreement(Ok(sample_agreement(&fx.agreement)));
    }
    fake.push_get_act(Err(missing("/consult/lawDecree")));
    fake.push_get_article(Err(missing("/consult/getArticle")));
    fake.push_get_decision(Err(missing("/consult/juri")));
    fake.push_get_deliberation(Err(missing("/consult/cnil")));
    fake.push_get_convention(Err(missing("/consult/kaliText")));
    fake.push_get_company_agreement(Err(missing("/consult/acco")));
    push_m6_m7(&fake, fx, copies);
    fake
}

/// Réponses de M6 et M7, dans l'ordre des appels de [`corpus_contract_m6_m7`].
fn push_m6_m7(fake: &FakeCorpus, fx: &ContractFixtures, copies: usize) {
    // `find_article` : `Today` puis `On`, `copies` fois chacun, puis un numéro absent par variante.
    let found = ArticleText {
        num: Some(fx.article_num.clone()),
        ..sample_article(&fx.article, fx.version_date)
    };
    for _ in 0..2 * copies {
        fake.push_find_article(Ok(found.clone()));
    }
    for _ in 0..2 {
        let what = "/consult/getArticleWithIdAndNum".to_owned();
        fake.push_find_article(Err(CorpusError::NotFound { what }));
    }
    for query in &fx.numbers {
        for _ in 0..copies {
            fake.push_find_by_number(Ok(sample_hits(query, &fx.article)));
        }
    }
    for _ in &fx.absent_numbers {
        fake.push_find_by_number(Ok(Vec::new()));
    }
    for _ in 0..copies {
        fake.push_list_codes(Ok(sample_codes(&fx.code)));
        fake.push_article_versions(Ok(sample_versions(&fx.article)));
        fake.push_related_links(Ok(sample_links()));
    }
    let what = "/consult/getArticleByCid".to_owned();
    fake.push_article_versions(Err(CorpusError::NotFound { what }));
    fake.push_related_links(Err(CorpusError::Upstream {
        status: Some(500),
        retryable: false,
        detail: "PISTE answers 500 for an unknown article".to_owned(),
    }));
}

/// Page d'un résultat du fond de `query` ; le hit a un identifiant du fond et un titre.
fn sample_page(query: &SearchQuery) -> SearchPage {
    let fond = query.fond();
    let id = DocumentId::parse(&format!("{}000000000001", fond.hit_prefix()))
        .expect("prefix and twelve digits");
    let mut hit = SearchHit::new(fond, id, None).expect("the id belongs to the fond");
    hit.title = Some(format!("Résultat {}", fond.name()));
    hit.snippet = Snippet::new("un extrait du résultat");
    SearchPage::new(
        fond,
        query.text().as_str(),
        Some(1),
        None,
        vec![hit],
        0,
        PaginationMode::Defaut,
    )
}

/// Corps de texte à la table des matières désordonnée dans le brouillon : un article et une
/// section titrée qui porte un article, pour que le tri par `int_ordre` joue.
fn sample_body(title: &str, nature: &str, since: VersionDate) -> TextBody {
    let article = |id: &str, num: &str, int_ordre: i64| ArticleDraft {
        id: Some(id.to_owned()),
        num: Some(num.to_owned()),
        int_ordre: Some(int_ordre),
        ..ArticleDraft::default()
    };
    let draft = TocDraft {
        sections: vec![SectionDraft {
            id: Some("LEGISCTA000033205040".to_owned()),
            title: Some("Titre Ier".to_owned()),
            int_ordre: Some(2),
            articles: vec![article("LEGIARTI000033205126", "1", 1)],
            ..SectionDraft::default()
        }],
        articles: vec![article("LEGIARTI000006419288", "L1", 1)],
    };
    TextBody {
        title: Some(title.to_owned()),
        nature: Some(nature.to_owned()),
        etat: Some(LegalStatus::Vigueur),
        date_debut_version: Some(since.as_civil()),
        toc: Toc::build(draft).expect("a shallow table of contents"),
        ..TextBody::default()
    }
}

/// Un résultat du fond de `query`, d'identifiant à douze chiffres ; pour un numéro d'article, il
/// porte l'article `article` sous le numéro cherché.
fn sample_hits(query: &NumberQuery, article: &ArticleId) -> Vec<SearchHit> {
    let fond = query.fond;
    let id = DocumentId::parse(&format!("{}000000000002", fond.hit_prefix()))
        .expect("prefix and twelve digits");
    let mut hit = SearchHit::new(fond, id, None).expect("the id belongs to the fond");
    hit.title = Some(format!("Résultat {} {}", fond.name(), query.kind.as_str()));
    if query.kind == NumberKind::Article {
        let num = Some(query.value.as_str().to_owned());
        hit.articles = vec![CodeArticleRef::new(article.clone(), num, "")];
    }
    vec![hit]
}

/// Le code de référence, en vigueur, puis un code abrogé.
fn sample_codes(code: &LegiTextId) -> Vec<CodeListing> {
    vec![
        CodeListing {
            id: code.clone(),
            title: "Code de test".to_owned(),
            etat: Some(LegalStatus::Vigueur),
        },
        CodeListing {
            id: LegiTextId::parse("LEGITEXT000006069577").expect("valid code id"),
            title: "Code de test abrogé".to_owned(),
            etat: Some(LegalStatus::Abroge),
        },
    ]
}

/// Trois versions de l'article, de la plus ancienne à celle qu'on demande, qui est en vigueur.
fn sample_versions(article: &ArticleId) -> Vec<ArticleVersion> {
    let version =
        |id: &str, etat: LegalStatus, start: Option<CivilDate>, end: Option<CivilDate>| {
            ArticleVersion {
                id: LegiArtiId::parse(id).expect("valid article id"),
                num: Some("9".to_owned()),
                etat: Some(etat),
                date_debut: start,
                date_fin: end,
            }
        };
    vec![
        version(
            "LEGIARTI000050396803",
            LegalStatus::Modifie,
            CivilDate::new(1990, 1, 1),
            CivilDate::new(2000, 1, 1),
        ),
        version(
            "LEGIARTI000050396821",
            LegalStatus::Abroge,
            CivilDate::new(2000, 1, 1),
            CivilDate::new(2010, 1, 1),
        ),
        version(article.as_str(), LegalStatus::Vigueur, CivilDate::new(2010, 1, 1), None),
    ]
}

/// Un lien vers un texte consultable, cité et citant, et un lien sans cible qui garde son titre
/// (un article du Journal officiel, 04 §7.3).
fn sample_links() -> RelatedLinks {
    let cited = RelatedLink {
        target: Some(DocumentId::parse("LEGITEXT000006070721").expect("valid text id")),
        nature: RelatedNature::Loda,
        title: Some("Loi de test".to_owned()),
        date: CivilDate::new(2008, 6, 3),
    };
    let journal = RelatedLink {
        target: None,
        nature: RelatedNature::Jorf,
        title: Some("Circulaire de test".to_owned()),
        date: None,
    };
    RelatedLinks { cites: vec![cited.clone()], cited_by: vec![cited, journal] }
}

fn sample_text(text: &str) -> LongText {
    LongText::from_plain(text).expect("a short plain text")
}

fn sample_act(id: &LegiTextId, since: VersionDate) -> Act {
    Act { text_id: id.clone(), body: sample_body("Loi de test", "LOI", since) }
}

fn sample_convention(id: &KaliTextId, since: VersionDate) -> Convention {
    Convention {
        text_id: id.clone(),
        body: sample_body("Convention de test", "CONVENTION COLLECTIVE", since),
        idcc: None,
        container_title: None,
        container_id: None,
        bulletin: None,
    }
}

fn sample_article(id: &ArticleId, since: VersionDate) -> ArticleText {
    ArticleText {
        article_id: id.clone(),
        num: Some("9".to_owned()),
        etat: Some(LegalStatus::Vigueur),
        code_title: Some("Code de test".to_owned()),
        parent: None,
        path: Vec::new(),
        full_sections_titre: None,
        date_debut: Some(since.as_civil()),
        date_fin: None,
        text: sample_text("Chacun a droit au respect de sa vie privée."),
        nota: None,
    }
}

fn sample_decision(id: &DecisionId) -> Decision {
    Decision {
        decision_id: id.clone(),
        ecli: None,
        num: None,
        juridiction: None,
        formation: None,
        solution: None,
        numero_affaire: Vec::new(),
        date_texte: CivilDate::new(2025, 11, 7),
        nature: None,
        title: Some(format!("Décision de test {id}")),
        text: sample_text("Le texte de la décision."),
        official_summary: lf_domain::entity::OfficialSummary::default(),
        cited_case_law: None,
        applied_texts: vec![],
    }
}

fn sample_deliberation(id: &CnilTextId) -> Deliberation {
    Deliberation {
        deliberation_id: id.clone(),
        num: None,
        nature: None,
        nature_delib: None,
        etat: None,
        date_texte: CivilDate::new(2026, 1, 8),
        date_publication: None,
        title: Some("Délibération de test".to_owned()),
        title_long: None,
        text: sample_text("Le texte de la délibération."),
    }
}

fn sample_agreement(id: &AccoTextId) -> CompanyAgreement {
    CompanyAgreement {
        agreement_id: id.clone(),
        nature: None,
        title: Some("Accord de test".to_owned()),
        raison_sociale: Some("Entreprise de test".to_owned()),
        siret: None,
        idcc: None,
        code_ape: None,
        secteur: None,
        themes: Vec::new(),
        syndicats: Vec::new(),
        date_texte: CivilDate::new(2025, 3, 1),
        date_effet: None,
        date_depot: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::testing::PortRequest;

    #[tokio::test]
    async fn contract_on_fake_corpus() {
        let fx = ContractFixtures::reference();
        let fake = reference_fake(&fx, 2);
        corpus_contract(&fake, &fx).await;
        // Ni réponse manquante, ni réponse de trop : chaque cas présent est appelé deux fois, et
        // chaque cas absent une fois, sur les douze opérations.
        assert_eq!(fake.unserved(), 0);
        assert_eq!(fake.calls_of("search"), 16);
        assert_eq!(fake.calls_of("get_decision"), 7);
        for method in [
            "get_act",
            "get_article",
            "get_deliberation",
            "get_convention",
            "get_company_agreement",
        ] {
            assert_eq!(fake.calls_of(method), 3, "{method}");
        }
        // M6 : deux variantes d'`ArticleAt` à deux appels, plus un numéro absent pour chacune ;
        // trois recherches présentes à deux appels, plus deux absentes ; deux lectures des codes.
        assert_eq!(fake.calls_of("find_article"), 2 * 2 + 2);
        assert_eq!(fake.calls_of("find_by_number"), 3 * 2 + 2);
        assert_eq!(fake.calls_of("list_codes"), 2);
        // M7 : deux lectures et un article absent.
        assert_eq!(fake.calls_of("article_versions"), 3);
        assert_eq!(fake.calls_of("related_links"), 3);
        assert_eq!(fake.calls(), 16 + 7 + 5 * 3 + 6 + 8 + 2 + 3 + 3);
    }

    #[tokio::test]
    async fn find_article_is_asked_in_both_variants_of_article_at() {
        let fx = ContractFixtures::reference();
        let fake = reference_fake(&fx, 2);
        corpus_contract_m6_m7(&fake, &fx).await;
        let (today, on) = (ArticleAt::Today(fx.version_date), ArticleAt::On(fx.version_date));
        let asked: Vec<(String, ArticleAt)> = fake
            .seen_requests()
            .into_iter()
            .filter_map(|request| match request {
                PortRequest::FindArticle(text, num, at) => {
                    assert_eq!(text, fx.code);
                    Some((num, at))
                }
                _ => None,
            })
            .collect();
        let (num, absent) = (fx.article_num.clone(), fx.absent_article_num.clone());
        let expected = [
            (num.clone(), today),
            (num.clone(), today),
            (num.clone(), on),
            (num, on),
            (absent.clone(), today),
            (absent, on),
        ];
        assert_eq!(asked, expected);
    }

    #[tokio::test]
    async fn the_v0_1_suite_alone_leaves_the_m6_and_m7_answers_unused() {
        let fx = ContractFixtures::reference();
        let fake = reference_fake(&fx, 2);
        corpus_contract_m5(&fake, &fx).await;
        assert_eq!(fake.calls(), 16 + 7 + 5 * 3);
        for method in
            ["find_article", "find_by_number", "list_codes", "article_versions", "related_links"]
        {
            assert_eq!(fake.calls_of(method), 0, "{method}");
        }
    }

    #[test]
    fn the_reference_cases_cover_every_fond_and_every_decision_kind() {
        let fx = ContractFixtures::reference();
        let fonds: Vec<Fond> = fx.searches.iter().map(SearchQuery::fond).collect();
        assert_eq!(fonds, Fond::ALL);
        let kinds: Vec<Fond> = fx.decisions.iter().map(DecisionId::fond).collect();
        assert_eq!(kinds, [Fond::Juri, Fond::Constit, Fond::Cetat]);
        // La date de version n'est posée que pour LODA et CODE.
        for query in &fx.searches {
            assert_eq!(query.version_date().is_some(), query.fond().needs_version_date());
        }
    }

    #[tokio::test]
    #[should_panic(expected = "get_article echoes the requested id")]
    async fn a_corpus_that_answers_another_article_is_refused() {
        let fx = ContractFixtures::reference();
        let mut other = fx.clone();
        other.article = ArticleId::parse("LEGIARTI000006419289").unwrap();
        corpus_contract_m5(&reference_fake(&other, 2), &fx).await;
    }

    #[tokio::test]
    #[should_panic(expected = "get_decision of a present id")]
    async fn a_corpus_without_a_programmed_answer_is_refused() {
        let fx = ContractFixtures::reference();
        let fake = reference_fake(&fx, 2);
        // Les réponses des décisions sont consommées avant la suite : la file est vide.
        for id in &fx.decisions {
            let _ = fake.get_decision(id).await;
            let _ = fake.get_decision(id).await;
        }
        corpus_contract_m5(&fake, &fx).await;
    }

    #[tokio::test]
    #[should_panic(expected = "get_act of an absent id must be NotFound")]
    async fn an_absent_id_that_is_found_is_refused() {
        let fx = ContractFixtures::reference();
        let fake = FakeCorpus::new();
        fake.push_get_act(Ok(sample_act(&fx.absent.act, fx.version_date)));
        check_absent(&fake, &fx).await;
    }

    #[test]
    #[should_panic(expected = "the sentinel date 2999-01-01 must be None")]
    fn a_sentinel_date_is_refused() {
        assert_no_sentinel("x", &[CivilDate::new(2025, 1, 1), CivilDate::new(2999, 1, 1)]);
    }

    #[test]
    fn dates_before_the_sentinel_floor_are_kept() {
        assert_no_sentinel("x", &[None, CivilDate::new(2221, 12, 31)]);
    }

    // ---- M6 et M7 : les cas de référence, puis un témoin négatif par règle ----------------------

    #[test]
    fn the_reference_cases_cover_the_number_searches_of_m6() {
        let fx = ContractFixtures::reference();
        let pairs = |queries: &[NumberQuery]| -> Vec<(Fond, NumberKind)> {
            queries.iter().map(|query| (query.fond, query.kind)).collect()
        };
        assert_eq!(
            pairs(&fx.numbers),
            [
                (Fond::Juri, NumberKind::Ecli),
                (Fond::Code, NumberKind::Article),
                (Fond::Kali, NumberKind::Idcc)
            ]
        );
        assert_eq!(
            pairs(&fx.absent_numbers),
            [(Fond::Juri, NumberKind::Ecli), (Fond::Loda, NumberKind::Nor)]
        );
        // La recherche par numéro d'article est celle de `find_article`, à la date de référence ;
        // les autres n'ont pas de date de version.
        assert_eq!(fx.numbers[1].value.as_str(), fx.article_num);
        assert_eq!(fx.numbers[1].version_date, Some(fx.version_date));
        assert_eq!(fx.numbers[0].version_date, None);
        assert_ne!(fx.article_num, fx.absent_article_num);
    }

    /// Un `FakeCorpus` qui répond `answers` à chaque appel de `find_article`, dans l'ordre.
    fn fake_finding(answers: Vec<Result<ArticleText, CorpusError>>) -> FakeCorpus {
        let fake = FakeCorpus::new();
        for answer in answers {
            fake.push_find_article(answer);
        }
        fake
    }

    fn article_9(id: &str, fx: &ContractFixtures) -> ArticleText {
        let id = ArticleId::parse(id).unwrap();
        ArticleText { num: Some(fx.article_num.clone()), ..sample_article(&id, fx.version_date) }
    }

    #[tokio::test]
    #[should_panic(expected = "find_article today answers the article of that number")]
    async fn a_find_article_that_answers_another_article_is_refused() {
        let fx = ContractFixtures::reference();
        let other = article_9("LEGIARTI000006419289", &fx);
        check_find_article(&fake_finding(vec![Ok(other.clone()), Ok(other)]), &fx).await;
    }

    #[tokio::test]
    #[should_panic(expected = "find_article today echoes the number")]
    async fn a_find_article_that_answers_another_number_is_refused() {
        let fx = ContractFixtures::reference();
        let article =
            ArticleText { num: Some("10".to_owned()), ..article_9(fx.article.as_str(), &fx) };
        check_find_article(&fake_finding(vec![Ok(article.clone()), Ok(article)]), &fx).await;
    }

    #[tokio::test]
    #[should_panic(expected = "find_article today of an absent number must be NotFound")]
    async fn a_find_article_that_finds_an_absent_number_is_refused() {
        let fx = ContractFixtures::reference();
        let article = article_9(fx.article.as_str(), &fx);
        check_find_article(&fake_finding(vec![Ok(article); 5]), &fx).await;
    }

    /// Un `FakeCorpus` qui répond `hits` deux fois à `find_by_number`, pour un seul cas présent.
    fn fake_numbering(hits: &[SearchHit]) -> FakeCorpus {
        let fake = FakeCorpus::new();
        for _ in 0..2 {
            fake.push_find_by_number(Ok(hits.to_vec()));
        }
        fake
    }

    /// `fx` réduit à la seule recherche par numéro `query`.
    fn only_number(query: NumberQuery) -> ContractFixtures {
        ContractFixtures {
            numbers: vec![query],
            absent_numbers: Vec::new(),
            ..ContractFixtures::reference()
        }
    }

    #[tokio::test]
    #[should_panic(expected = "find_by_number JURI ecli: the reference case has results")]
    async fn a_number_search_without_results_is_refused() {
        let fx = only_number(ContractFixtures::reference().numbers[0].clone());
        check_find_by_number(&fake_numbering(&[]), &fx).await;
    }

    #[tokio::test]
    #[should_panic(expected = "belongs to the fond")]
    async fn a_hit_of_another_fond_is_refused() {
        let fx = only_number(ContractFixtures::reference().numbers[0].clone());
        let foreign = NumberQuery { fond: Fond::Cetat, ..fx.numbers[0].clone() };
        let hits = sample_hits(&foreign, &fx.article);
        check_find_by_number(&fake_numbering(&hits), &fx).await;
    }

    #[tokio::test]
    #[should_panic(expected = "has a foreign id")]
    async fn a_hit_with_a_foreign_identifier_is_refused() {
        let fx = only_number(ContractFixtures::reference().numbers[0].clone());
        let mut hits = sample_hits(&fx.numbers[0], &fx.article);
        hits[0].id = DocumentId::parse("LEGITEXT000006070721").unwrap();
        check_find_by_number(&fake_numbering(&hits), &fx).await;
    }

    #[tokio::test]
    #[should_panic(expected = "carries the article asked")]
    async fn an_article_search_whose_hits_carry_no_article_is_refused() {
        let fx = only_number(ContractFixtures::reference().numbers[1].clone());
        let mut hits = sample_hits(&fx.numbers[0], &fx.article);
        hits[0].articles.clear();
        check_find_by_number(&fake_numbering(&hits), &fx).await;
    }

    #[tokio::test]
    #[should_panic(expected = "of an absent number must be an empty list")]
    async fn an_absent_number_with_results_is_refused() {
        let reference = ContractFixtures::reference();
        let fx = ContractFixtures {
            numbers: Vec::new(),
            absent_numbers: vec![reference.numbers[0].clone()],
            ..reference
        };
        let fake = FakeCorpus::new();
        fake.push_find_by_number(Ok(sample_hits(&fx.absent_numbers[0], &fx.article)));
        check_find_by_number(&fake, &fx).await;
    }

    #[tokio::test]
    #[should_panic(expected = "of an absent number must be an empty list")]
    async fn an_absent_number_that_fails_is_refused() {
        let reference = ContractFixtures::reference();
        let fx = ContractFixtures {
            numbers: Vec::new(),
            absent_numbers: vec![reference.absent_numbers[1].clone()],
            ..reference
        };
        let fake = FakeCorpus::new();
        fake.push_find_by_number(Err(CorpusError::NotFound {
            what: "/consult/getJoWithNor".into(),
        }));
        check_find_by_number(&fake, &fx).await;
    }

    fn fake_listing(codes: &[CodeListing]) -> FakeCorpus {
        let fake = FakeCorpus::new();
        for _ in 0..2 {
            fake.push_list_codes(Ok(codes.to_vec()));
        }
        fake
    }

    #[tokio::test]
    #[should_panic(expected = "list_codes: codes are deduplicated")]
    async fn a_code_listed_twice_is_refused() {
        let fx = ContractFixtures::reference();
        let code = sample_codes(&fx.code)[0].clone();
        check_list_codes(&fake_listing(&[code.clone(), code]), &fx).await;
    }

    #[tokio::test]
    #[should_panic(expected = "list_codes: every code has a title")]
    async fn an_untitled_code_is_refused() {
        let fx = ContractFixtures::reference();
        let untitled = CodeListing { title: "  ".to_owned(), ..sample_codes(&fx.code)[0].clone() };
        check_list_codes(&fake_listing(&[untitled]), &fx).await;
    }

    #[tokio::test]
    #[should_panic(expected = "list_codes lists the reference code")]
    async fn a_listing_without_the_reference_code_is_refused() {
        let fx = ContractFixtures::reference();
        check_list_codes(&fake_listing(&sample_codes(&fx.code)[1..]), &fx).await;
    }

    fn fake_versioning(versions: &[ArticleVersion]) -> FakeCorpus {
        let fake = FakeCorpus::new();
        for _ in 0..2 {
            fake.push_article_versions(Ok(versions.to_vec()));
        }
        fake
    }

    #[tokio::test]
    #[should_panic(expected = "article_versions lists the requested version")]
    async fn versions_without_the_requested_one_are_refused() {
        let fx = ContractFixtures::reference();
        let versions = sample_versions(&fx.article)[..2].to_vec();
        check_article_versions(&fake_versioning(&versions), &fx).await;
    }

    #[tokio::test]
    #[should_panic(expected = "article_versions: versions are deduplicated")]
    async fn a_version_listed_twice_is_refused() {
        let fx = ContractFixtures::reference();
        let mut versions = sample_versions(&fx.article);
        versions.push(versions[2].clone());
        check_article_versions(&fake_versioning(&versions), &fx).await;
    }

    #[tokio::test]
    #[should_panic(expected = "article_versions: versions come oldest first")]
    async fn versions_from_the_most_recent_are_refused() {
        let fx = ContractFixtures::reference();
        let mut versions = sample_versions(&fx.article);
        versions.reverse();
        check_article_versions(&fake_versioning(&versions), &fx).await;
    }

    #[tokio::test]
    #[should_panic(expected = "ends before it starts")]
    async fn a_version_that_ends_before_it_starts_is_refused() {
        let fx = ContractFixtures::reference();
        let mut versions = sample_versions(&fx.article);
        versions[2].date_fin = CivilDate::new(2005, 1, 1);
        check_article_versions(&fake_versioning(&versions), &fx).await;
    }

    #[tokio::test]
    #[should_panic(expected = "article_versions of an absent id must be NotFound")]
    async fn an_absent_article_with_versions_is_refused() {
        let fx = ContractFixtures::reference();
        let fake = fake_versioning(&sample_versions(&fx.article));
        fake.push_article_versions(Ok(sample_versions(&fx.article)));
        check_article_versions(&fake, &fx).await;
    }

    fn fake_linking(links: &RelatedLinks) -> FakeCorpus {
        let fake = FakeCorpus::new();
        for _ in 0..2 {
            fake.push_related_links(Ok(links.clone()));
        }
        fake
    }

    #[tokio::test]
    #[should_panic(expected = "related_links: the reference article has links")]
    async fn an_article_without_links_is_refused() {
        let fx = ContractFixtures::reference();
        check_related_links(&fake_linking(&RelatedLinks::default()), &fx).await;
    }

    #[tokio::test]
    #[should_panic(expected = "related_links: a link keeps a title or a target")]
    async fn a_link_without_title_or_target_is_refused() {
        let fx = ContractFixtures::reference();
        let mut links = sample_links();
        links.cited_by[1].title = Some(" ".to_owned());
        check_related_links(&fake_linking(&links), &fx).await;
    }

    #[tokio::test]
    #[should_panic(expected = "related_links of an absent id must be an error")]
    async fn an_absent_article_with_empty_links_is_refused() {
        let fx = ContractFixtures::reference();
        let fake = fake_linking(&sample_links());
        fake.push_related_links(Ok(RelatedLinks::default()));
        check_related_links(&fake, &fx).await;
    }

    #[tokio::test]
    async fn an_absent_article_may_be_not_found_or_an_upstream_error() {
        // PISTE répond 500 à `relatedLinksArticle` d'un article inconnu (`Upstream`) ; un corpus
        // sans cette particularité répond `NotFound` : les deux sont admis.
        let fx = ContractFixtures::reference();
        let fake = fake_linking(&sample_links());
        fake.push_related_links(Err(CorpusError::NotFound { what: "/consult/x".to_owned() }));
        check_related_links(&fake, &fx).await;
    }
}
