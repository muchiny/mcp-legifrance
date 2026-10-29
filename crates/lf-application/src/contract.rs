//! Suite de contrat du port `LegalCorpus` (06 §2.2, 02 §12.2), derrière la feature `test-support`.
//!
//! [`corpus_contract_m5`] rejoue les mêmes entrées contre toute implémentation du port et vérifie
//! les mêmes formes de sortie, sur les sept opérations de la v0.1 (`search`, `get_act`,
//! `get_article`, `get_decision`, `get_deliberation`, `get_convention`, `get_company_agreement`) :
//!
//! - **contrat de citation v2** : citation sur une ligne et jamais vide, `source_url` sous
//!   legifrance.gouv.fr (qui se termine par l'identifiant pour une consultation), jamais d'ELI
//!   fabriqué, ECLI de la décision repris tel quel ;
//! - **identifiant** : l'entité rendue porte l'identifiant demandé, et chaque résultat de recherche
//!   un identifiant du fond interrogé ;
//! - **sentinelles** : aucune date à partir de `SENTINEL_FLOOR` (2222-01-01) ne sort du port, elle
//!   est `None` ;
//! - **table des matières** ordonnée et dédoublonnée : en préordre sans saut de profondeur, sans
//!   article en double, et la même suite d'entrées lue d'un bloc ou par pages de deux ;
//! - **déterminisme** : deux appels aux mêmes arguments rendent des valeurs égales ;
//! - **introuvable** : un identifiant absent donne `CorpusError::NotFound`, pour les six
//!   consultations.
//!
//! Les cas viennent d'un fournisseur, [`ContractFixtures`] : [`ContractFixtures::reference`] reprend
//! les identifiants des seize réponses de référence de `lf-adapter-piste`
//! (`tests/fixtures/reference`). Chaque corpus sous test arrange ses réponses autour de ces cas :
//! `FakeCorpus` par [`reference_fake`], `PisteCorpus` par un `wiremock` qui sert les fixtures,
//! `CachedCorpus` par un `FakeCorpus` décoré (R8 : le cache ne dépend pas de l'adapter PISTE).
//!
//! **Ordre des appels.** Chaque opération est appelée deux fois pour un cas présent, une fois pour
//! un cas absent ; l'ordre est celui de [`corpus_contract_m5`], que [`reference_fake`] suit pour
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
        Act, ArticleDraft, ArticleText, CompanyAgreement, Convention, Decision, Deliberation,
        SearchHit, SearchPage, SectionDraft, Snippet, TextBody, Toc, TocDraft, TocEntryRef,
    },
    fond::{Fond, PaginationMode},
    id::{AccoTextId, ArticleId, CnilTextId, DecisionId, DocumentId, KaliTextId, LegiTextId},
    search::{MatchMode, PageSize, QueryText, SearchQuery},
    status::LegalStatus,
    text::LongText,
    url::WEB_BASE,
};

use crate::{
    ports::{CorpusError, LegalCorpus},
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
}

impl ContractFixtures {
    /// Les cas de référence : les identifiants des seize réponses enregistrées par la référence
    /// (`consult_*` et `search_*`), la date de version du 2026-10-15 et, pour chaque consultation,
    /// un identifiant à douze zéros.
    ///
    /// # Panics
    ///
    /// Jamais : tous les identifiants, la date et les requêtes sont des constantes valides.
    #[must_use]
    pub fn reference() -> Self {
        let version_date = VersionDate::parse("2026-10-15").expect("valid version date");
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
        }
    }
}

/// Suite de contrat des sept opérations de la v0.1 (T-M3-05, T-M5-06).
///
/// Panique à la première forme de sortie non conforme, avec un message qui nomme l'opération.
/// V3-12 l'étend aux cinq opérations de M6 et M7.
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

/// Variante de [`corpus_contract_m5`] côté `FakeCorpus` : programme un faux corpus avec les
/// réponses des cas de `fx`, `copies` fois chacune (2 pour un corpus nu, 1 derrière un cache, qui
/// ne rappelle pas le corpus décoré pour la seconde lecture), puis un `NotFound` par consultation
/// absente. Les entités sont construites à la main ; l'ordre des `push` est celui des appels de la
/// suite.
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
    fake
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

    #[tokio::test]
    async fn contract_on_fake_corpus() {
        let fx = ContractFixtures::reference();
        let fake = reference_fake(&fx, 2);
        corpus_contract_m5(&fake, &fx).await;
        // Ni réponse manquante, ni réponse de trop : chaque cas présent est appelé deux fois, et
        // chaque consultation absente une fois.
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
        assert_eq!(fake.calls(), 16 + 7 + 5 * 3);
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
}
