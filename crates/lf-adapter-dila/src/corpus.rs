//! `DilaCorpus` : le port [`LegalCorpus`] sur l'index tantivy des dumps LEGI (M9, T-M9-03).
//!
//! # Ce que le corpus sait faire
//!
//! Les dumps de la DILA portent le fonds **LEGI** seul : les codes et les textes consolidés (lois,
//! décrets, arrêtés). Décision 00 §5 (M9 : LEGI seul) : huit opérations sont servies hors ligne,
//! `search` (fonds `CODE` et `LODA`), `get_act`, `get_article`, `find_article`, `find_by_number`
//! (numéro d'article, numéro de texte, NOR), `list_codes`, `article_versions` et `related_links`.
//! Tout le reste (décisions, délibérations CNIL, conventions KALI, accords ACCO, ELI, numéros
//! d'affaire, d'IDCC, ECLI…) rend [`CorpusError::Config`] de nature `Settings` : le modèle y lit un
//! texte fixe qui dit que cette source ne couvre pas l'opération.
//!
//! # Lectures
//!
//! Chaque lecture passe par `tokio::task::spawn_blocking` (`DilaCorpus::run`) : tantivy lit des
//! fichiers mappés en mémoire et décode des segments, ce qui ne doit jamais tenir un fil de
//! l'exécuteur. Le corpus se clone à bas prix (un `Arc`) ; les lectures n'écrivent rien.
//!
//! La recherche est **plein texte français** (voir [`crate::index::french_analyzer`]) sur le titre
//! et le corps des articles et des textes, restreinte à ce qui est en vigueur à la date de la
//! requête. Les résultats sont les **textes** (un `LEGITEXT` par résultat), classés par leur
//! meilleur document ; pour un code, chaque résultat cite ses articles trouvés. Le classement
//! porte sur les 2 000 meilleurs documents ; `total` compte les textes de cet ensemble (une borne
//! basse quand la requête en trouve davantage). La période d'une requête `LODA` s'applique à la
//! date de signature du texte ; celle d'un numéro (`within`) est ignorée.
//!
//! # Écriture
//!
//! [`DilaCorpus::sync`] fait appliquer au répertoire d'archives ce qui manque ([`sync_directory`])
//! dans l'index, sur un fil bloquant. Les lectures voient chaque archive appliquée dès son commit,
//! pas avant. La fraîcheur (quel global, quel incrément) est celle du [`crate::Registry`].

use std::{
    collections::HashMap,
    future::ready,
    path::{Path, PathBuf},
    sync::Arc,
};

use lf_application::ports::{ArticleAt, CorpusError, LegalCorpus};
use lf_domain::{
    date::{CivilDate, DateRange, VersionDate},
    entity::{
        Act, ArticleText, ArticleVersion, CodeArticleRef, CodeListing, CompanyAgreement,
        Convention, Decision, Deliberation, RelatedLinks, SearchHit, SearchPage,
    },
    fond::{Fond, PaginationMode},
    id::{AccoTextId, ArticleId, CnilTextId, DecisionId, KaliTextId, LegiTextId},
    search::{NumberKind, NumberQuery, SearchQuery},
};

use crate::{
    error::{IngestError, StoreError},
    index::{Engine, IndexStore, TextKey, View, fold_num, one_copy_per_id, read_failure},
    ingest::IngestLimits,
    map::{
        HIT_MAX_ARTICLES, TOC_MAX_NODES, article_ref, article_text, article_version, build_toc,
        code_listing, not_found, related_link, text_body, text_hit, unsupported,
    },
    model::Article,
    path::{DocKind, Tree},
    store::StoredDoc,
    sync::{SyncReport, sync_directory},
};

/// Un travail d'écriture : il reçoit l'écrivain de l'index et rend le rapport de synchronisation.
/// Un type unique, pour qu'une seule instanciation de `write_blocking` porte toute la couverture.
type WriteJob = Box<dyn FnOnce(IndexStore) -> Result<SyncReport, IngestError> + Send>;

/// Documents gardés pour classer une recherche ou un numéro.
const CANDIDATES: usize = 2_000;

/// Gabarits d'API que l'adapter remplace : ce que `NotFound` nomme, pour les journaux.
const LAW_DECREE: &str = "/consult/lawDecree";
const GET_ARTICLE: &str = "/consult/getArticle";
const ARTICLE_WITH_NUM: &str = "/consult/getArticleWithIdAndNum";
const CANONICAL_VERSIONS: &str = "/search/canonicalArticleVersion";
const RELATED_LINKS: &str = "/consult/relatedLinksArticle";

/// Le corpus juridique hors ligne, sur l'index des dumps LEGI.
#[derive(Debug, Clone)]
pub struct DilaCorpus {
    engine: Arc<Engine>,
}

impl DilaCorpus {
    /// Ouvre l'index du dossier `dir`, le crée vide s'il n'existe pas.
    ///
    /// # Errors
    ///
    /// [`StoreError`] si le dossier est inutilisable ou si l'index a un autre schéma.
    pub fn open(dir: &Path) -> Result<Self, StoreError> {
        Engine::open(dir).map(Self::from_engine)
    }

    /// Un corpus vide en mémoire (tests, petits dumps).
    ///
    /// # Errors
    ///
    /// [`StoreError`] si l'index ne peut pas être créé.
    pub fn in_memory() -> Result<Self, StoreError> {
        Engine::in_memory().map(Self::from_engine)
    }

    /// Un corpus sur un moteur déjà ouvert.
    pub fn from_engine(engine: Engine) -> Self {
        Self { engine: Arc::new(engine) }
    }

    /// Le moteur : le nombre de documents, l'analyse d'un texte.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// L'écrivain de l'index (un seul à la fois), pour ingérer des archives à la main.
    ///
    /// # Errors
    ///
    /// [`StoreError`] si un autre écrivain tient le verrou.
    pub fn writer(&self) -> Result<IndexStore, StoreError> {
        IndexStore::new(Arc::clone(&self.engine))
    }

    /// Applique à l'index les archives de `archives` que le registre `registry` n'a pas encore
    /// vues ([`sync_directory`]), sur un fil bloquant. Les lectures en cours continuent sur
    /// l'état précédent et voient chaque archive à son commit.
    ///
    /// # Errors
    ///
    /// Les erreurs de [`sync_directory`] ; [`IngestError::Store`] si un autre écrivain tient le
    /// verrou de l'index ; [`IngestError::Io`] si le fil de synchronisation s'est arrêté.
    pub async fn sync(
        &self,
        archives: PathBuf,
        registry: PathBuf,
        limits: IngestLimits,
    ) -> Result<SyncReport, IngestError> {
        let job: WriteJob =
            Box::new(move |mut store| sync_directory(&archives, &registry, &mut store, &limits));
        self.write_blocking(job).await
    }

    /// Exécute `job` sur un fil bloquant avec l'écrivain de l'index.
    async fn write_blocking(&self, job: WriteJob) -> Result<SyncReport, IngestError> {
        let engine = Arc::clone(&self.engine);
        tokio::task::spawn_blocking(move || job(IndexStore::new(engine)?)).await.map_err(
            |join| IngestError::Io {
                action: "run the synchronisation",
                source: std::io::Error::other(join),
            },
        )?
    }

    /// Exécute `work` sur un fil bloquant, sur une vue figée de l'index.
    ///
    /// Une panique du travail, ou son annulation, devient une erreur de lecture : le serveur
    /// continue.
    pub(crate) async fn run<T, F>(&self, work: F) -> Result<T, CorpusError>
    where
        T: Send + 'static,
        F: FnOnce(&View<'_>) -> Result<T, CorpusError> + Send + 'static,
    {
        let engine = Arc::clone(&self.engine);
        match tokio::task::spawn_blocking(move || work(&engine.view())).await {
            Ok(result) => result,
            Err(join) => Err(read_failure(&join)),
        }
    }
}

/// Famille de textes d'un fond servi.
fn tree_of(fond: Fond) -> Result<Tree, CorpusError> {
    match fond {
        Fond::Code => Ok(Tree::Code),
        Fond::Loda => Ok(Tree::Tnc),
        Fond::Juri | Fond::Constit | Fond::Cetat | Fond::Cnil | Fond::Kali | Fond::Acco => {
            Err(unsupported(&format!("a search in the {} fund", fond.name())))
        }
    }
}

/// L'identifiant `LEGIARTI` d'un article ; un article KALI n'est pas dans les dumps LEGI.
fn legi_article(id: &ArticleId) -> Result<&str, CorpusError> {
    match id {
        ArticleId::Legi(id) => Ok(id.as_str()),
        ArticleId::Kali(_) => Err(unsupported("a KALI article")),
    }
}

/// L'article `id`, `None` s'il n'est pas dans l'index.
fn article_of(view: &View<'_>, id: &str) -> Result<Option<Article>, CorpusError> {
    let doc = view.by_id(DocKind::Article, id)?;
    Ok(doc.and_then(|doc| doc.document.as_article().cloned()))
}

/// Les résultats d'un même texte : le dossier et les articles trouvés dedans.
#[derive(Debug)]
struct Group {
    dir_cid: String,
    articles: Vec<StoredDoc>,
}

/// Regroupe les documents par dossier de texte, dans l'ordre de leur première apparition (le
/// meilleur document d'un texte donne le rang du texte). Les versions de texte ouvrent un groupe
/// sans article.
#[expect(
    clippy::indexing_slicing,
    reason = "`index` vient de la table `slot` ou du `push` juste au-dessus : il désigne un groupe existant"
)]
fn group_by_text(docs: Vec<StoredDoc>) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    let mut slot: HashMap<String, usize> = HashMap::new();
    for doc in docs {
        let index = *slot.entry(doc.path.cid.clone()).or_insert_with(|| {
            groups.push(Group { dir_cid: doc.path.cid.clone(), articles: Vec::new() });
            groups.len() - 1
        });
        if doc.document.as_article().is_some() {
            groups[index].articles.push(doc);
        }
    }
    groups
}

/// Les résultats de `groups`, un par texte ; un groupe sans version de texte dans l'index est
/// écarté et compté. Chaque résultat prend pour extrait le début de son meilleur article ; pour
/// un code (`with_articles`), il cite aussi jusqu'à [`HIT_MAX_ARTICLES`] articles.
fn hits_of(
    view: &View<'_>,
    fond: Fond,
    groups: &[Group],
    with_articles: bool,
) -> Result<(Vec<SearchHit>, u32), CorpusError> {
    let mut hits = Vec::with_capacity(groups.len());
    let mut dropped = 0;
    for group in groups {
        let text = view.text_of_dir(&group.dir_cid)?;
        let hit = text
            .as_ref()
            .and_then(|doc| doc.document.as_text_version())
            .and_then(|text| text_hit(fond, text));
        let Some(mut hit) = hit else {
            dropped += 1;
            continue;
        };
        let cited: Vec<CodeArticleRef> = group
            .articles
            .iter()
            .filter_map(|doc| doc.document.as_article())
            .filter_map(article_ref)
            .take(HIT_MAX_ARTICLES)
            .collect();
        hit.snippet = cited.first().and_then(|cited| cited.snippet.clone());
        if with_articles {
            hit.articles = cited;
        }
        hits.push(hit);
    }
    Ok((hits, dropped))
}

/// Les groupes dont le texte a été signé dans `range` (inclus).
fn signed_within(
    view: &View<'_>,
    groups: Vec<Group>,
    range: DateRange,
) -> Result<Vec<Group>, CorpusError> {
    let (start, end) = (range.start().as_civil(), range.end().as_civil());
    let mut kept = Vec::new();
    for group in groups {
        let text = view.text_of_dir(&group.dir_cid)?;
        let signed = text
            .as_ref()
            .and_then(|doc| doc.document.as_text_version())
            .and_then(|text| text.chronicle.date_texte);
        if signed.is_some_and(|date| start <= date && date <= end) {
            kept.push(group);
        }
    }
    Ok(kept)
}

/// `search` : les textes dont un article ou le titre répond à la requête.
fn search_page(view: &View<'_>, query: &SearchQuery) -> Result<SearchPage, CorpusError> {
    let fond = query.fond();
    let tree = tree_of(fond)?;
    let text = query.text().as_str();
    let at = query.version_date().map(VersionDate::as_civil);
    let candidates = view.search(tree, text, query.mode(), at, CANDIDATES)?;
    let mut groups =
        candidates.map_or_else(Vec::new, |ranked| group_by_text(one_copy_per_id(ranked.docs)));
    if let Some(range) = query.date_range() {
        groups = signed_within(view, groups, range)?;
    }
    let total = u64::try_from(groups.len()).unwrap_or(u64::MAX);
    let size = usize::try_from(query.page_size().get()).unwrap_or(usize::MAX);
    let offset = usize::try_from(query.page().get() - 1).unwrap_or(usize::MAX).saturating_mul(size);
    let window = groups.get(offset..).unwrap_or_default();
    let window = window.get(..size).unwrap_or(window);
    let (hits, dropped) = hits_of(view, fond, window, fond == Fond::Code)?;
    Ok(SearchPage::new(fond, text, Some(total), None, hits, dropped, PaginationMode::Defaut))
}

/// `get_act` : le texte et son sommaire à la date demandée.
fn act(view: &View<'_>, id: &LegiTextId, date: VersionDate) -> Result<Act, CorpusError> {
    let text = view.by_id(DocKind::TextVersion, id.as_str())?;
    let text = text
        .as_ref()
        .and_then(|doc| doc.document.as_text_version())
        .ok_or_else(|| not_found(LAW_DECREE))?;
    let structure = view.by_id(DocKind::TextStruct, id.as_str())?;
    let links = structure
        .as_ref()
        .and_then(|doc| doc.document.as_text_struct())
        .map_or(&[][..], |structure| structure.structure.as_slice());
    let toc = build_toc(view, links, date.as_civil(), TOC_MAX_NODES)?;
    Ok(Act { text_id: id.clone(), body: text_body(text, toc) })
}

/// `find_article` : la version en vigueur à `at` de l'article numéroté `num` du texte.
fn find_article(
    view: &View<'_>,
    text: &LegiTextId,
    num: &str,
    at: CivilDate,
) -> Result<ArticleText, CorpusError> {
    let dir = view
        .by_id(DocKind::TextVersion, text.as_str())?
        .ok_or_else(|| not_found(ARTICLE_WITH_NUM))?
        .path
        .cid;
    let found = view.articles_numbered(&dir, &fold_num(num), at)?;
    let article = found
        .first()
        .and_then(|doc| doc.document.as_article())
        .ok_or_else(|| not_found(ARTICLE_WITH_NUM))?;
    article_text(article)
}

/// Les résultats qui sont des versions de texte, une copie par identifiant.
fn text_hits(fond: Fond, docs: Vec<StoredDoc>) -> Vec<SearchHit> {
    one_copy_per_id(docs)
        .iter()
        .filter_map(|doc| doc.document.as_text_version())
        .filter_map(|text| text_hit(fond, text))
        .collect()
}

/// Les textes de la famille `tree` qui portent un article numéroté `value`, avec cet article.
fn articles_numbered(
    view: &View<'_>,
    fond: Fond,
    tree: Tree,
    value: &str,
    at: Option<CivilDate>,
) -> Result<Vec<SearchHit>, CorpusError> {
    let ranked = view.articles_numbered_in(tree, &fold_num(value), at, CANDIDATES)?;
    let groups = group_by_text(one_copy_per_id(ranked.docs));
    hits_of(view, fond, &groups, fond == Fond::Code).map(|(hits, _)| hits)
}

/// `find_by_number` : numéro d'article, numéro de texte ou NOR.
fn by_number(view: &View<'_>, query: &NumberQuery) -> Result<Vec<SearchHit>, CorpusError> {
    let value = query.value.as_str();
    let at = query.version_date.map(VersionDate::as_civil);
    match (query.fond, query.kind) {
        (Fond::Code, NumberKind::Article) => {
            articles_numbered(view, Fond::Code, Tree::Code, value, at)
        }
        (Fond::Loda, NumberKind::Article) => {
            articles_numbered(view, Fond::Loda, Tree::Tnc, value, at)
        }
        (Fond::Loda, NumberKind::Text) => {
            let ranked = view.texts_where(TextKey::Number, &fold_num(value), at, CANDIDATES)?;
            Ok(text_hits(Fond::Loda, ranked.docs))
        }
        (Fond::Loda, NumberKind::Nor) => {
            let nor = value.trim().to_uppercase();
            let ranked = view.texts_where(TextKey::Nor, &nor, None, CANDIDATES)?;
            Ok(text_hits(Fond::Loda, ranked.docs))
        }
        (Fond::Loda, NumberKind::Eli) => Err(unsupported("a search by ELI")),
        _ => Err(unsupported(&format!("a search by {} number", query.kind.as_str()))),
    }
}

/// `list_codes` : les codes, abrogés compris, par titre.
fn codes(view: &View<'_>) -> Result<Vec<CodeListing>, CorpusError> {
    let mut codes: Vec<CodeListing> = view
        .codes()?
        .iter()
        .filter_map(|doc| doc.document.as_text_version())
        .filter_map(code_listing)
        .collect();
    codes.sort_by(|a, b| a.title.cmp(&b.title).then_with(|| a.id.cmp(&b.id)));
    Ok(codes)
}

/// `article_versions` : les versions que l'article connaît de lui-même.
fn versions(view: &View<'_>, id: &str) -> Result<Vec<ArticleVersion>, CorpusError> {
    let article = article_of(view, id)?.ok_or_else(|| not_found(CANONICAL_VERSIONS))?;
    Ok(article.versions.iter().filter_map(article_version).collect())
}

/// `related_links` : `sens="source"` (l'article cite) et `sens="cible"` (l'article est cité).
fn related(view: &View<'_>, id: &str) -> Result<RelatedLinks, CorpusError> {
    let article = article_of(view, id)?.ok_or_else(|| not_found(RELATED_LINKS))?;
    let mut links = RelatedLinks::default();
    for link in &article.links {
        match link.direction.as_deref().map(str::to_ascii_lowercase).as_deref() {
            Some("source") => links.cites.push(related_link(link)),
            Some("cible") => links.cited_by.push(related_link(link)),
            _ => {}
        }
    }
    Ok(links)
}

impl LegalCorpus for DilaCorpus {
    async fn search(&self, query: &SearchQuery) -> Result<SearchPage, CorpusError> {
        let query = query.clone();
        self.run(move |view| search_page(view, &query)).await
    }

    async fn get_act(&self, id: &LegiTextId, date: VersionDate) -> Result<Act, CorpusError> {
        let id = id.clone();
        self.run(move |view| act(view, &id, date)).await
    }

    async fn get_article(&self, id: &ArticleId) -> Result<ArticleText, CorpusError> {
        let id = legi_article(id)?.to_owned();
        self.run(move |view| {
            let article = article_of(view, &id)?.ok_or_else(|| not_found(GET_ARTICLE))?;
            article_text(&article)
        })
        .await
    }

    fn get_decision(
        &self,
        _id: &DecisionId,
    ) -> impl Future<Output = Result<Decision, CorpusError>> + Send {
        ready(Err(unsupported("a court decision")))
    }

    fn get_deliberation(
        &self,
        _id: &CnilTextId,
    ) -> impl Future<Output = Result<Deliberation, CorpusError>> + Send {
        ready(Err(unsupported("a CNIL deliberation")))
    }

    fn get_convention(
        &self,
        _id: &KaliTextId,
    ) -> impl Future<Output = Result<Convention, CorpusError>> + Send {
        ready(Err(unsupported("a collective agreement")))
    }

    fn get_company_agreement(
        &self,
        _id: &AccoTextId,
    ) -> impl Future<Output = Result<CompanyAgreement, CorpusError>> + Send {
        ready(Err(unsupported("a company agreement")))
    }

    async fn find_article(
        &self,
        text: &LegiTextId,
        num: &str,
        at: ArticleAt,
    ) -> Result<ArticleText, CorpusError> {
        let (text, num) = (text.clone(), num.to_owned());
        self.run(move |view| find_article(view, &text, &num, at.date().as_civil())).await
    }

    async fn find_by_number(&self, query: &NumberQuery) -> Result<Vec<SearchHit>, CorpusError> {
        let query = query.clone();
        self.run(move |view| by_number(view, &query)).await
    }

    async fn list_codes(&self) -> Result<Vec<CodeListing>, CorpusError> {
        self.run(codes).await
    }

    async fn article_versions(&self, id: &ArticleId) -> Result<Vec<ArticleVersion>, CorpusError> {
        let id = legi_article(id)?.to_owned();
        self.run(move |view| versions(view, &id)).await
    }

    async fn related_links(&self, id: &ArticleId) -> Result<RelatedLinks, CorpusError> {
        let id = legi_article(id)?.to_owned();
        self.run(move |view| related(view, &id)).await
    }
}

#[cfg(test)]
mod tests {
    use std::{
        num::NonZeroU32,
        sync::atomic::{AtomicBool, Ordering},
        time::Duration,
    };

    use lf_application::{
        contract::{ContractFixtures, corpus_contract_m5, reference_fake},
        testing::FakeCorpus,
    };
    use lf_domain::{
        entity::{RelatedNature, Toc, TocEntryRef},
        id::{ArticleId, DocumentId},
        search::{MatchMode, NumberValue, PageSize, QueryText},
        status::LegalStatus,
    };

    use super::*;
    use crate::{
        archive::ArchiveKind,
        model::{
            Chronicle, Document, Link, LinkTarget, Section, SectionRef, StructLink, TextContext,
            TextStruct, TextVersion,
        },
        store::DocStore,
        testing::stored_doc,
    };

    const CODE: &str = "LEGITEXT000033205014";
    const LAW_DIR: &str = "JORFTEXT000024583863";
    const LAW: &str = "LEGITEXT000024586520";
    const DECREE_DIR: &str = "JORFTEXT000030000001";
    const DECREE: &str = "LEGITEXT000030000002";
    const SECTION: &str = "LEGISCTA000033205040";
    const NINE: &str = "LEGIARTI000006419288";
    const NINE_OLD: &str = "LEGIARTI000006419287";
    const ONE: &str = "LEGIARTI000033205126";
    const DEFERRED: &str = "LEGIARTI000033205200";
    const REPLACED: &str = "LEGIARTI000033205300";
    const POSTAL: &str = "LEGIARTI000033205400";
    const LAW_ARTICLE: &str = "LEGIARTI000024586522";
    const DECREE_ARTICLE: &str = "LEGIARTI000030000003";

    fn date(year: u16, month: u8, day: u8) -> Option<CivilDate> {
        CivilDate::new(year, month, day)
    }

    fn version_date(raw: &str) -> VersionDate {
        VersionDate::parse(raw).unwrap()
    }

    fn today() -> VersionDate {
        version_date("2026-10-15")
    }

    #[allow(clippy::too_many_arguments, reason = "constructeur de données de test")]
    fn article(
        id: &str,
        num: &str,
        window: (Option<CivilDate>, Option<CivilDate>),
        etat: Option<&str>,
        dir: &str,
        nature: &str,
        title: &str,
        content: &str,
    ) -> Article {
        Article {
            id: id.into(),
            num: Some(num.into()),
            etat: etat.map(Into::into),
            kind: Some("AUTONOME".into()),
            date_debut: window.0,
            date_fin: window.1,
            context: TextContext {
                cid: Some(dir.into()),
                nature: Some(nature.into()),
                title: Some(title.into()),
                sections: Vec::new(),
            },
            versions: Vec::new(),
            nota: String::new(),
            content: content.into(),
            links: Vec::new(),
        }
    }

    fn link_to(target: LinkTarget, id: &str, num: Option<&str>) -> StructLink {
        StructLink {
            target,
            id: id.into(),
            cid: Some(id.into()),
            num: num.map(Into::into),
            level: None,
            etat: Some("VIGUEUR".into()),
            debut: date(2016, 10, 9),
            fin: date(2999, 1, 1),
            title: None,
        }
    }

    fn text_version(
        id: &str,
        cid: &str,
        nature: &str,
        title: &str,
        num: Option<&str>,
        nor: Option<&str>,
        signed: Option<CivilDate>,
    ) -> TextVersion {
        TextVersion {
            id: id.into(),
            nature: Some(nature.into()),
            chronicle: Chronicle {
                cid: Some(cid.into()),
                num: num.map(Into::into),
                nor: nor.map(Into::into),
                date_publi: signed,
                date_texte: signed,
                last_modified: None,
            },
            title: Some(title.into()),
            title_full: Some(title.into()),
            etat: Some("VIGUEUR".into()),
            date_debut: date(2010, 2, 4),
            date_fin: date(2999, 1, 1),
            authority: None,
            ministry: None,
            visas: String::new(),
            signatories: String::new(),
            tp: String::new(),
            nota: String::new(),
            abro: String::new(),
            rect: String::new(),
        }
    }

    /// Le corpus de test : un code (deux versions de l'article 9, un article différé, un article
    /// entièrement modifié, une section), deux textes non codifiés et un doublon `non_vigueur`.
    #[expect(clippy::too_many_lines, reason = "jeu de données de test, lu de haut en bas")]
    fn put_world(store: &mut impl DocStore) {
        let open = (date(2016, 10, 9), date(2999, 1, 1));
        let code_title = "Code golden de test";
        let put = |store: &mut dyn DocStore, sub: &str, dir: &str, document: Document, mtime| {
            store.put(stored_doc(sub, dir, document, mtime)).unwrap();
        };
        let in_code = |id, num, window, etat, content: &str| {
            article(id, num, window, etat, CODE, "CODE", code_title, content)
        };

        let mut nine = in_code(
            NINE,
            "9",
            open,
            Some("VIGUEUR"),
            "<p>Chacun a droit au respect de sa vie privée. Le golden standard du règlement.</p>",
        );
        nine.context.sections = vec![SectionRef {
            id: SECTION.into(),
            title: "Titre Ier".into(),
            date_debut: date(2016, 10, 9),
            date_fin: None,
        }];
        nine.versions = vec![
            link_to(LinkTarget::Article, NINE_OLD, Some("9")),
            link_to(LinkTarget::Article, NINE, Some("9")),
        ];
        nine.versions[0].etat = Some("ABROGE".into());
        nine.versions[0].debut = date(2000, 1, 1);
        nine.versions[0].fin = date(2016, 10, 9);
        nine.versions[1].fin = date(2999, 1, 1);
        nine.links = vec![
            Link {
                link_type: Some("CITATION".into()),
                direction: Some("source".into()),
                id: Some("JORFTEXT000000519770".into()),
                text_cid: Some("JORFTEXT000000519770".into()),
                text_nature: Some("DECRET".into()),
                text_number: Some("80-217".into()),
                text_nor: None,
                text_signed: date(1980, 3, 20),
                title: "Décret 80-217".into(),
            },
            Link {
                link_type: Some("MODIFIE".into()),
                direction: Some("CIBLE".into()),
                id: Some("JORFTEXT000033202746".into()),
                text_cid: None,
                text_nature: Some("LOI".into()),
                text_number: None,
                text_nor: None,
                text_signed: date(2999, 1, 1),
                title: "Loi pour une République numérique".into(),
            },
            Link {
                link_type: Some("CONCORDANCE".into()),
                direction: None,
                id: None,
                text_cid: None,
                text_nature: None,
                text_number: None,
                text_nor: None,
                text_signed: None,
                title: "sans sens".into(),
            },
        ];
        let docs = [
            nine,
            in_code(
                NINE_OLD,
                "9",
                (date(2000, 1, 1), date(2016, 10, 9)),
                Some("ABROGE"),
                "<p>ancienne rédaction</p>",
            ),
            in_code(ONE, "1", open, Some("VIGUEUR"), "<p>golden premier</p>"),
            in_code(
                DEFERRED,
                "9",
                (date(2222, 2, 22), date(2999, 1, 1)),
                Some("VIGUEUR_DIFF"),
                "<p>golden différé</p>",
            ),
            in_code(
                REPLACED,
                "9",
                (date(2999, 1, 1), date(2999, 1, 1)),
                None,
                "<p>golden entièrement modifié</p>",
            ),
            in_code(POSTAL, "L. 36-11", open, Some("VIGUEUR"), "<p>le courrier</p>"),
        ];
        for doc in docs {
            put(store, "code_en_vigueur", CODE, Document::Article(doc), 10);
        }

        let mut top = link_to(LinkTarget::Section, SECTION, None);
        top.title = Some("Titre Ier".into());
        let mut old = link_to(LinkTarget::Article, NINE_OLD, Some("9"));
        old.debut = date(2000, 1, 1);
        old.fin = date(2016, 10, 9);
        let mut deferred = link_to(LinkTarget::Article, DEFERRED, Some("9"));
        deferred.debut = date(2222, 2, 22);
        let structure = vec![
            top,
            link_to(LinkTarget::Article, NINE, Some("9")),
            old,
            deferred,
            link_to(LinkTarget::Article, POSTAL, Some("L. 36-11")),
        ];
        put(
            store,
            "code_en_vigueur",
            CODE,
            Document::TextStruct(TextStruct {
                id: CODE.into(),
                nature: Some("CODE".into()),
                chronicle: Chronicle::default(),
                versions: Vec::new(),
                structure,
            }),
            10,
        );
        put(
            store,
            "code_en_vigueur",
            CODE,
            Document::Section(Section {
                id: SECTION.into(),
                title: None,
                context: TextContext::default(),
                structure: vec![
                    link_to(LinkTarget::Article, ONE, Some("1")),
                    link_to(LinkTarget::Section, "LEGISCTA000033205041", None),
                ],
            }),
            10,
        );
        let mut code = text_version(CODE, CODE, "CODE", code_title, None, None, date(2016, 10, 7));
        code.date_debut = date(2016, 10, 9);
        put(store, "code_en_vigueur", CODE, Document::TextVersion(code), 10);

        let law_title = "Loi golden relative aux règlements";
        let law_article = |content: &str| {
            article(
                LAW_ARTICLE,
                "1",
                (date(2010, 2, 4), date(2999, 1, 1)),
                Some("VIGUEUR"),
                LAW_DIR,
                "LOI",
                law_title,
                content,
            )
        };
        put(
            store,
            "TNC_en_vigueur",
            LAW_DIR,
            Document::Article(law_article("<p>Le golden texte de la loi</p>")),
            20,
        );
        // le même article sous `non_vigueur`, plus ancien : un seul résultat
        put(
            store,
            "TNC_non_vigueur",
            LAW_DIR,
            Document::Article(law_article("<p>Le golden texte de la loi</p>")),
            5,
        );
        put(
            store,
            "TNC_en_vigueur",
            LAW_DIR,
            Document::TextVersion(text_version(
                LAW,
                LAW_DIR,
                "LOI",
                law_title,
                Some("2010-1"),
                Some("ABCD1034567L"),
                date(2010, 2, 3),
            )),
            20,
        );
        put(
            store,
            "TNC_en_vigueur",
            LAW_DIR,
            Document::TextStruct(TextStruct {
                id: LAW.into(),
                nature: Some("LOI".into()),
                chronicle: Chronicle::default(),
                versions: Vec::new(),
                structure: vec![link_to(LinkTarget::Article, LAW_ARTICLE, Some("1"))],
            }),
            20,
        );

        let decree_title = "Décret golden d'application";
        put(
            store,
            "TNC_en_vigueur",
            DECREE_DIR,
            Document::Article(article(
                DECREE_ARTICLE,
                "1",
                (date(2012, 5, 1), date(2999, 1, 1)),
                Some("VIGUEUR"),
                DECREE_DIR,
                "DECRET",
                decree_title,
                "<p>golden décret</p>",
            )),
            30,
        );
        put(
            store,
            "TNC_en_vigueur",
            DECREE_DIR,
            Document::TextVersion(text_version(
                DECREE,
                DECREE_DIR,
                "DECRET",
                decree_title,
                Some("2012-300"),
                None,
                date(2012, 4, 30),
            )),
            30,
        );
    }

    fn world() -> DilaCorpus {
        let corpus = DilaCorpus::in_memory().unwrap();
        let mut store = corpus.writer().unwrap();
        put_world(&mut store);
        store.commit().unwrap();
        corpus
    }

    fn search_for(
        fond: Fond,
        text: &str,
        mode: MatchMode,
        page: u32,
        size: u32,
        at: &str,
    ) -> SearchQuery {
        SearchQuery::new(
            fond,
            QueryText::parse(text).unwrap(),
            NonZeroU32::new(page).unwrap(),
            PageSize::new(size).unwrap(),
            mode,
            None,
            fond.needs_version_date().then(|| version_date(at)),
        )
        .unwrap()
    }

    fn quick(fond: Fond, text: &str) -> SearchQuery {
        search_for(fond, text, MatchMode::Any, 1, 10, "2026-10-15")
    }

    fn ids(page: &SearchPage) -> Vec<&str> {
        page.hits.iter().map(|hit| hit.id.as_str()).collect()
    }

    fn article_id(raw: &str) -> ArticleId {
        ArticleId::parse(raw).unwrap()
    }

    fn text_id(raw: &str) -> LegiTextId {
        LegiTextId::parse(raw).unwrap()
    }

    /// Le texte fixe d'une lecture de l'index qui échoue : aucune cause n'y figure.
    fn is_read_failure(error: &CorpusError) -> bool {
        *error == read_failure(&"any cause")
    }

    /// Rejoue `run` en faisant échouer tour à tour chaque accès à l'index, du premier au dernier :
    /// chaque panne doit remonter en erreur de lecture, jamais être avalée ni dire sa cause.
    async fn every_read_fault<T, Fut>(corpus: &DilaCorpus, mut run: impl FnMut() -> Fut) -> u64
    where
        Fut: Future<Output = Result<T, CorpusError>>,
    {
        let engine = corpus.engine();
        let mut faults = 0;
        loop {
            let access = faults + 1;
            engine.fail_nth_access(access);
            let result = run().await;
            if engine.fault_pending() {
                engine.fail_nth_access(0);
                assert!(result.is_ok(), "the read fails by itself");
                assert!(faults > 0, "the read does not touch the index");
                return faults;
            }
            let error = result.err();
            let surfaced = error.as_ref().is_some_and(is_read_failure);
            assert!(surfaced, "access {access} gave {error:?}");
            faults += 1;
        }
    }

    /// Les identifiants des articles de la table, dans l'ordre de lecture.
    fn article_ids(toc: &Toc) -> Vec<String> {
        let whole = toc.page(0, NonZeroU32::MAX).unwrap();
        whole
            .entries
            .iter()
            .filter_map(|entry| match entry.node {
                TocEntryRef::Article(article) => Some(article.article_id.as_str().to_owned()),
                TocEntryRef::Section { .. } => None,
            })
            .collect()
    }

    /// Les textes signés en 2010, toutes versions confondues.
    fn signed_in_2010() -> SearchQuery {
        let range =
            DateRange::new(Some(version_date("2010-01-01")), Some(version_date("2010-12-31")))
                .unwrap();
        SearchQuery::new(
            Fond::Loda,
            QueryText::parse("golden").unwrap(),
            NonZeroU32::MIN,
            PageSize::default(),
            MatchMode::Any,
            range,
            Some(today()),
        )
        .unwrap()
    }

    /// Le corpus des dumps sous la suite de contrat de la v0.1. Les dumps LEGI ne portent ni
    /// décision, ni délibération, ni convention, ni accord, ni les fonds autres que `CODE` et
    /// `LODA` : ces cas-là sont servis par le faux de référence, dont la file avance aussi sur
    /// les appels que le corpus des dumps traite lui-même.
    struct Mixed {
        dila: DilaCorpus,
        fake: FakeCorpus,
    }

    impl LegalCorpus for Mixed {
        async fn search(&self, query: &SearchQuery) -> Result<SearchPage, CorpusError> {
            let faked = self.fake.search(query).await;
            if matches!(query.fond(), Fond::Code | Fond::Loda) {
                self.dila.search(query).await
            } else {
                faked
            }
        }
        async fn get_act(&self, id: &LegiTextId, date: VersionDate) -> Result<Act, CorpusError> {
            drop(self.fake.get_act(id, date).await);
            self.dila.get_act(id, date).await
        }
        async fn get_article(&self, id: &ArticleId) -> Result<ArticleText, CorpusError> {
            drop(self.fake.get_article(id).await);
            self.dila.get_article(id).await
        }
        async fn get_decision(&self, id: &DecisionId) -> Result<Decision, CorpusError> {
            self.fake.get_decision(id).await
        }
        async fn get_deliberation(&self, id: &CnilTextId) -> Result<Deliberation, CorpusError> {
            self.fake.get_deliberation(id).await
        }
        async fn get_convention(&self, id: &KaliTextId) -> Result<Convention, CorpusError> {
            self.fake.get_convention(id).await
        }
        async fn get_company_agreement(
            &self,
            id: &AccoTextId,
        ) -> Result<CompanyAgreement, CorpusError> {
            self.fake.get_company_agreement(id).await
        }
        async fn find_article(
            &self,
            text: &LegiTextId,
            num: &str,
            at: ArticleAt,
        ) -> Result<ArticleText, CorpusError> {
            self.dila.find_article(text, num, at).await
        }
        async fn find_by_number(&self, query: &NumberQuery) -> Result<Vec<SearchHit>, CorpusError> {
            self.dila.find_by_number(query).await
        }
        async fn list_codes(&self) -> Result<Vec<CodeListing>, CorpusError> {
            self.dila.list_codes().await
        }
        async fn article_versions(
            &self,
            id: &ArticleId,
        ) -> Result<Vec<ArticleVersion>, CorpusError> {
            self.dila.article_versions(id).await
        }
        async fn related_links(&self, id: &ArticleId) -> Result<RelatedLinks, CorpusError> {
            self.dila.related_links(id).await
        }
    }

    #[tokio::test]
    async fn contract_on_dila_corpus() {
        let fixtures = ContractFixtures::reference();
        let mixed = Mixed { dila: world(), fake: reference_fake(&fixtures, 2) };
        corpus_contract_m5(&mixed, &fixtures).await;
    }

    #[tokio::test]
    #[expect(clippy::panic, reason = "une panique du travail est le cas testé")]
    async fn index_runs_in_spawn_blocking() {
        let corpus = world();
        let caller = std::thread::current().id();
        let worker = corpus.run(|_| Ok(std::thread::current().id())).await.unwrap();
        assert_ne!(worker, caller, "the read runs on the blocking pool");

        // une seule forme de travail, donc une seule instanciation de `run` : il dort sur le fil
        // bloquant, note qu'il a fini, puis panique si on le lui demande
        let job = |pause: u64, explode: bool, done: Arc<AtomicBool>| {
            move |_: &View<'_>| -> Result<(), CorpusError> {
                std::thread::sleep(Duration::from_millis(pause));
                done.store(true, Ordering::SeqCst);
                if explode {
                    std::panic::panic_any("boom");
                }
                Ok(())
            }
        };

        // pendant qu'une lecture dort sur le fil bloquant, l'exécuteur (un seul fil ici) tourne
        let slow_done = Arc::new(AtomicBool::new(false));
        let ticked_before_end = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&ticked_before_end);
        let slow = corpus.run(job(200, false, Arc::clone(&slow_done)));
        let (flag, seen_by_tick) = (Arc::clone(&slow_done), seen);
        let tick = async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            seen_by_tick.store(!flag.load(Ordering::SeqCst), Ordering::SeqCst);
        };
        let (slow, ()) = tokio::join!(slow, tick);
        slow.unwrap();
        assert!(ticked_before_end.load(Ordering::SeqCst), "the executor was not blocked");

        // une panique du travail est une erreur de lecture, pas un arrêt
        let failed = corpus.run(job(0, true, Arc::new(AtomicBool::new(false)))).await.unwrap_err();
        assert!(is_read_failure(&failed));
        assert!(corpus.list_codes().await.is_ok(), "the corpus still answers");
    }

    #[tokio::test]
    #[expect(clippy::panic, reason = "une panique du travail est le cas testé")]
    async fn a_panicking_write_is_an_io_error_and_frees_the_writer() {
        let corpus = DilaCorpus::in_memory().unwrap();
        let outcome = corpus.write_blocking(Box::new(|_| std::panic::panic_any("boom"))).await;
        let message = outcome.unwrap_err().to_string();
        assert!(message.starts_with("cannot run the synchronisation: "), "{message}");
        assert!(corpus.writer().is_ok(), "the lock was released by the unwinding");
    }

    #[tokio::test]
    async fn the_mixed_corpus_serves_every_local_read_from_the_dumps() {
        let fixtures = ContractFixtures::reference();
        let mixed = Mixed { dila: world(), fake: reference_fake(&fixtures, 2) };
        let at = ArticleAt::On(version_date("2026-10-15"));
        let found = mixed.find_article(&text_id(CODE), "9", at).await.unwrap();
        assert_eq!(found.article_id.as_str(), NINE);
        let hits = mixed.find_by_number(&number(Fond::Code, NumberKind::Article, "9")).await;
        assert_eq!(hits.unwrap().len(), 1);
        assert_eq!(mixed.list_codes().await.unwrap().len(), 1);
        assert_eq!(mixed.article_versions(&article_id(NINE)).await.unwrap().len(), 2);
        assert_eq!(mixed.related_links(&article_id(NINE)).await.unwrap().cites.len(), 1);
    }

    #[tokio::test]
    async fn every_failed_read_reaches_the_model_as_a_fixed_text() {
        let corpus = world();
        let at = || ArticleAt::On(version_date("2026-10-15"));
        let (code, law) = (text_id(CODE), text_id(LAW));
        let nine = article_id(NINE);
        let in_code = quick(Fond::Code, "golden");
        let in_laws = quick(Fond::Loda, "golden");
        let signed = signed_in_2010();
        let by_article = number(Fond::Code, NumberKind::Article, "9");
        let by_law_article = number(Fond::Loda, NumberKind::Article, "1");
        let by_text = number(Fond::Loda, NumberKind::Text, "2010-1");
        let by_nor = number(Fond::Loda, NumberKind::Nor, "ABCD1034567L");

        every_read_fault(&corpus, || corpus.search(&in_code)).await;
        every_read_fault(&corpus, || corpus.search(&in_laws)).await;
        every_read_fault(&corpus, || corpus.search(&signed)).await;
        every_read_fault(&corpus, || corpus.get_act(&code, today())).await;
        every_read_fault(&corpus, || corpus.get_act(&law, today())).await;
        every_read_fault(&corpus, || corpus.get_article(&nine)).await;
        every_read_fault(&corpus, || corpus.find_article(&code, "9", at())).await;
        every_read_fault(&corpus, || corpus.find_by_number(&by_article)).await;
        every_read_fault(&corpus, || corpus.find_by_number(&by_law_article)).await;
        every_read_fault(&corpus, || corpus.find_by_number(&by_text)).await;
        every_read_fault(&corpus, || corpus.find_by_number(&by_nor)).await;
        every_read_fault(&corpus, || corpus.list_codes()).await;
        every_read_fault(&corpus, || corpus.article_versions(&nine)).await;
        every_read_fault(&corpus, || corpus.related_links(&nine)).await;
        // la panne passe, la lecture suivante est de nouveau saine
        assert!(corpus.get_article(&nine).await.is_ok());
    }

    #[tokio::test]
    async fn search_finds_texts_with_their_articles() {
        let corpus = world();
        let page = corpus.search(&quick(Fond::Code, "golden")).await.unwrap();
        assert_eq!(ids(&page), [CODE]);
        assert_eq!(page.total, 1);
        let hit = &page.hits[0];
        assert_eq!(hit.title.as_deref(), Some("Code golden de test"));
        assert_eq!(hit.etat, Some(LegalStatus::Vigueur));
        let found: Vec<&str> = hit.articles.iter().map(|a| a.article_id.as_str()).collect();
        // l'article 9 abrogé, le différé et l'entièrement modifié sont hors vigueur
        assert!(found.contains(&NINE) && found.contains(&ONE), "{found:?}");
        assert!(!found.contains(&NINE_OLD) && !found.contains(&DEFERRED), "{found:?}");
        assert!(!found.contains(&REPLACED), "{found:?}");
        assert!(hit.snippet.is_some());

        let page = corpus.search(&quick(Fond::Loda, "golden")).await.unwrap();
        assert_eq!(page.total, 2, "the duplicate under non_vigueur is one result");
        assert!(ids(&page).contains(&LAW) && ids(&page).contains(&DECREE));
        assert!(page.hits.iter().all(|hit| hit.articles.is_empty()), "only a code lists articles");
    }

    #[tokio::test]
    async fn search_ignores_accents_and_inflection() {
        let corpus = world();
        for query in ["reglement", "règlements", "REGLEMENT"] {
            let page = corpus.search(&quick(Fond::Loda, query)).await.unwrap();
            let found = ids(&page);
            assert!(found.contains(&LAW), "{query}: {found:?}");
        }
        let page = corpus.search(&quick(Fond::Code, "reglement")).await.unwrap();
        assert_eq!(ids(&page), [CODE], "the article text says règlement");
    }

    #[tokio::test]
    async fn search_modes_differ_on_the_words() {
        let corpus = world();
        let any = search_for(Fond::Code, "golden courrier", MatchMode::Any, 1, 10, "2026-10-15");
        let all = search_for(Fond::Code, "premier courrier", MatchMode::All, 1, 10, "2026-10-15");
        let any_all =
            search_for(Fond::Code, "premier courrier", MatchMode::Any, 1, 10, "2026-10-15");
        assert_eq!(corpus.search(&any).await.unwrap().total, 1);
        assert_eq!(corpus.search(&all).await.unwrap().total, 0, "no one article has both words");
        assert_eq!(corpus.search(&any_all).await.unwrap().total, 1);

        let exact = |text| search_for(Fond::Code, text, MatchMode::Exact, 1, 10, "2026-10-15");
        // « au » est un mot vide : la phrase garde son trou
        assert_eq!(corpus.search(&exact("droit au respect")).await.unwrap().total, 1);
        assert_eq!(corpus.search(&exact("respect droit")).await.unwrap().total, 0);
        assert_eq!(corpus.search(&exact("respect")).await.unwrap().total, 1, "one word is a term");
        assert_eq!(corpus.search(&exact("le la les")).await.unwrap().total, 0, "only stop words");
    }

    #[tokio::test]
    async fn search_follows_the_version_date() {
        let corpus = world();
        let old = |at| search_for(Fond::Code, "ancienne", MatchMode::Any, 1, 10, at);
        assert_eq!(corpus.search(&old("2010-01-01")).await.unwrap().total, 1);
        assert_eq!(corpus.search(&old("2016-10-09")).await.unwrap().total, 0, "fin is exclusive");
        assert_eq!(corpus.search(&old("2026-10-15")).await.unwrap().total, 0);
        let new = |at| search_for(Fond::Code, "premier", MatchMode::Any, 1, 10, at);
        assert_eq!(corpus.search(&new("2016-10-09")).await.unwrap().total, 1, "debut is inclusive");
        assert_eq!(corpus.search(&new("2016-10-08")).await.unwrap().total, 0);
    }

    #[tokio::test]
    async fn search_pages_over_texts_and_filters_on_signature_date() {
        let corpus = world();
        let page = |n| search_for(Fond::Loda, "golden", MatchMode::Any, n, 1, "2026-10-15");
        let (first, second, third) = (
            corpus.search(&page(1)).await.unwrap(),
            corpus.search(&page(2)).await.unwrap(),
            corpus.search(&page(3)).await.unwrap(),
        );
        assert_eq!((first.hits.len(), second.hits.len(), third.hits.len()), (1, 1, 0));
        assert_eq!((first.total, second.total, third.total), (2, 2, 2));
        assert_ne!(ids(&first), ids(&second));

        assert_eq!(ids(&corpus.search(&signed_in_2010()).await.unwrap()), [LAW]);
    }

    #[tokio::test]
    async fn search_refuses_the_funds_the_dumps_do_not_hold() {
        let corpus = world();
        for fond in [Fond::Juri, Fond::Constit, Fond::Cetat, Fond::Cnil, Fond::Kali, Fond::Acco] {
            let error = corpus.search(&quick(fond, "golden")).await.unwrap_err();
            let fund = format!("a search in the {} fund", fond.name());
            assert_eq!(error, unsupported(&fund));
        }
    }

    #[tokio::test]
    async fn get_act_builds_the_table_of_contents_at_the_date() {
        let corpus = world();
        let act = corpus.get_act(&text_id(CODE), today()).await.unwrap();
        assert_eq!(act.body.title.as_deref(), Some("Code golden de test"));
        assert_eq!(act.body.nature.as_deref(), Some("CODE"));
        let toc = &act.body.toc;
        assert_eq!(article_ids(toc), [ONE, NINE, POSTAL]);
        let whole = toc.page(0, NonZeroU32::MAX).unwrap();
        let shape: Vec<(u16, String)> = whole
            .entries
            .iter()
            .map(|entry| match entry.node {
                TocEntryRef::Section { title } => (entry.depth, title.to_owned()),
                TocEntryRef::Article(article) => {
                    (entry.depth, article.article_id.as_str().to_owned())
                }
            })
            .collect();
        // la section avec son article, puis l'article 9 en vigueur et l'article L. 36-11 ; ni la
        // version abrogée ni la version différée
        assert_eq!(
            shape,
            [
                (1, "Titre Ier".to_owned()),
                (2, ONE.to_owned()),
                (1, NINE.to_owned()),
                (1, POSTAL.to_owned()),
            ]
        );

        let before = corpus.get_act(&text_id(CODE), version_date("2010-01-01")).await.unwrap();
        assert_eq!(
            article_ids(&before.body.toc),
            [NINE_OLD],
            "in 2010 the old article 9 is the one in force"
        );

        let law = corpus.get_act(&text_id(LAW), today()).await.unwrap();
        assert_eq!(law.body.cid.as_ref().map(DocumentId::as_str), Some(LAW_DIR));
        assert_eq!(law.body.toc.len(), 1);
        let unknown = corpus.get_act(&text_id("LEGITEXT000000000000"), today()).await;
        assert_eq!(unknown.err(), Some(not_found(LAW_DECREE)));
    }

    #[tokio::test]
    async fn get_act_without_a_structure_file_has_an_empty_table() {
        let corpus = DilaCorpus::in_memory().unwrap();
        let mut store = corpus.writer().unwrap();
        let text = text_version(CODE, CODE, "CODE", "Code seul", None, None, None);
        store.put(stored_doc("code_en_vigueur", CODE, Document::TextVersion(text), 1)).unwrap();
        store.commit().unwrap();
        let act = corpus.get_act(&text_id(CODE), today()).await.unwrap();
        assert!(act.body.toc.is_empty());
    }

    #[tokio::test]
    async fn get_article_reads_any_version_by_id() {
        let corpus = world();
        let current = corpus.get_article(&article_id(NINE)).await.unwrap();
        assert_eq!(current.num.as_deref(), Some("9"));
        assert_eq!(current.etat, Some(LegalStatus::Vigueur));
        assert_eq!(current.code_title.as_deref(), Some("Code golden de test"));
        assert_eq!(current.path, ["Titre Ier"]);
        assert_eq!((current.date_debut, current.date_fin), (date(2016, 10, 9), None));
        assert!(current.text.as_str().contains("respect de sa vie privée"));

        let old = corpus.get_article(&article_id(NINE_OLD)).await.unwrap();
        assert_eq!(old.etat, Some(LegalStatus::Abroge));
        assert_eq!(old.date_fin, date(2016, 10, 9));

        let replaced = corpus.get_article(&article_id(REPLACED)).await.unwrap();
        assert_eq!((replaced.etat, replaced.date_debut, replaced.date_fin), (None, None, None));

        let missing = corpus.get_article(&article_id("LEGIARTI000000000000")).await;
        assert_eq!(missing.err(), Some(not_found(GET_ARTICLE)));
        let kali = corpus.get_article(&article_id("KALIARTI000005805295")).await.unwrap_err();
        assert_eq!(kali, unsupported("a KALI article"));
    }

    #[tokio::test]
    async fn find_article_picks_the_version_in_force() {
        let corpus = world();
        let at = |raw| ArticleAt::On(version_date(raw));
        let now = corpus.find_article(&text_id(CODE), "9", at("2026-10-15")).await.unwrap();
        assert_eq!(now.article_id.as_str(), NINE);
        let then = corpus.find_article(&text_id(CODE), "9", at("2010-01-01")).await.unwrap();
        assert_eq!(then.article_id.as_str(), NINE_OLD);
        let today =
            corpus.find_article(&text_id(CODE), "9", ArticleAt::Today(today())).await.unwrap();
        assert_eq!(today.article_id.as_str(), NINE);

        // même numéro, autre écriture
        let postal = corpus.find_article(&text_id(CODE), "L36-11", at("2026-10-15")).await.unwrap();
        assert_eq!(postal.article_id.as_str(), POSTAL);

        for (text, num, when) in [
            (CODE, "9", "1990-01-01"),
            (CODE, "404", "2026-10-15"),
            (CODE, "°", "2026-10-15"),
            ("LEGITEXT000000000000", "9", "2026-10-15"),
        ] {
            let missing = corpus.find_article(&text_id(text), num, at(when)).await;
            assert_eq!(missing.err(), Some(not_found(ARTICLE_WITH_NUM)), "{text} {num} {when}");
        }
        // un article de loi non codifiée se trouve par le texte LEGITEXT, pas par le dossier
        let law = corpus.find_article(&text_id(LAW), "1", at("2026-10-15")).await.unwrap();
        assert_eq!(law.article_id.as_str(), LAW_ARTICLE);
    }

    fn number(fond: Fond, kind: NumberKind, value: &str) -> NumberQuery {
        NumberQuery::new(fond, kind, NumberValue::parse(value).unwrap(), today()).unwrap()
    }

    #[tokio::test]
    async fn find_by_number_serves_articles_texts_and_nor() {
        let corpus = world();
        let hits =
            corpus.find_by_number(&number(Fond::Code, NumberKind::Article, "9")).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id.as_str(), CODE);
        let numbers: Vec<&str> = hits[0].articles.iter().filter_map(|a| a.num.as_deref()).collect();
        assert_eq!(numbers, ["9"], "only the version in force");

        let hits =
            corpus.find_by_number(&number(Fond::Loda, NumberKind::Article, "1")).await.unwrap();
        let found: Vec<&str> = hits.iter().map(|hit| hit.id.as_str()).collect();
        assert!(found.contains(&LAW) && found.contains(&DECREE), "{found:?}");

        let hits =
            corpus.find_by_number(&number(Fond::Loda, NumberKind::Text, "2010-1")).await.unwrap();
        assert_eq!(hits.iter().map(|hit| hit.id.as_str()).collect::<Vec<_>>(), [LAW]);

        let hits = corpus
            .find_by_number(&number(Fond::Loda, NumberKind::Nor, " abcd1034567l "))
            .await
            .unwrap();
        assert_eq!(hits.iter().map(|hit| hit.id.as_str()).collect::<Vec<_>>(), [LAW]);
        let none = corpus
            .find_by_number(&number(Fond::Loda, NumberKind::Nor, "ZZZZ0000000Z"))
            .await
            .unwrap();
        assert_eq!(none.len(), 0);
    }

    #[tokio::test]
    async fn find_by_number_refuses_what_the_dumps_do_not_hold() {
        let corpus = world();
        for (fond, kind, value) in [
            (Fond::Loda, NumberKind::Eli, "eli/loi/2016/10/7/2016-1321"),
            (Fond::Juri, NumberKind::Case, "24-13.470"),
            (Fond::Kali, NumberKind::Idcc, "1090"),
            (Fond::Cetat, NumberKind::Ecli, "ECLI:FR:CEASS:2024:1"),
        ] {
            let error = corpus.find_by_number(&number(fond, kind, value)).await.unwrap_err();
            let expected = if kind == NumberKind::Eli {
                unsupported("a search by ELI")
            } else {
                unsupported(&format!("a search by {} number", kind.as_str()))
            };
            assert_eq!(error, expected, "{fond} {kind:?}");
        }
    }

    #[tokio::test]
    async fn the_other_consultations_are_not_in_the_dumps() {
        let corpus = world();
        let errors = [
            corpus.get_decision(&"JURITEXT000051743650".parse().unwrap()).await.unwrap_err(),
            corpus.get_deliberation(&"CNILTEXT000053352643".parse().unwrap()).await.unwrap_err(),
            corpus.get_convention(&"KALITEXT000049878950".parse().unwrap()).await.unwrap_err(),
            corpus
                .get_company_agreement(&"ACCOTEXT000053643828".parse().unwrap())
                .await
                .unwrap_err(),
        ];
        assert_eq!(
            errors,
            [
                unsupported("a court decision"),
                unsupported("a CNIL deliberation"),
                unsupported("a collective agreement"),
                unsupported("a company agreement"),
            ]
        );
    }

    #[tokio::test]
    async fn list_codes_gives_the_codes_by_title() {
        let corpus = world();
        let codes = corpus.list_codes().await.unwrap();
        assert_eq!(codes.len(), 1, "laws and decrees are not codes");
        assert_eq!(codes[0].id.as_str(), CODE);
        assert_eq!(codes[0].title, "Code golden de test");
        assert_eq!(codes[0].etat, Some(LegalStatus::Vigueur));

        // deux codes de même titre : le second critère est l'identifiant
        let mut store = corpus.writer().unwrap();
        for id in ["LEGITEXT000006070722", "LEGITEXT000006070721"] {
            let code = text_version(id, id, "CODE", "Code civil", None, None, None);
            store.put(stored_doc("code_en_vigueur", id, Document::TextVersion(code), 1)).unwrap();
        }
        store.commit().unwrap();
        let listed: Vec<(String, String)> = corpus
            .list_codes()
            .await
            .unwrap()
            .into_iter()
            .map(|code| (code.title, code.id.as_str().to_owned()))
            .collect();
        let expected = [
            ("Code civil", "LEGITEXT000006070721"),
            ("Code civil", "LEGITEXT000006070722"),
            ("Code golden de test", CODE),
        ];
        assert_eq!(listed, expected.map(|(title, id)| (title.to_owned(), id.to_owned())));
    }

    #[tokio::test]
    async fn article_versions_and_related_links_come_from_the_article() {
        let corpus = world();
        let versions = corpus.article_versions(&article_id(NINE)).await.unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].id.as_str(), NINE_OLD);
        assert_eq!(versions[0].etat, Some(LegalStatus::Abroge));
        assert_eq!(
            (versions[0].date_debut, versions[0].date_fin),
            (date(2000, 1, 1), date(2016, 10, 9))
        );
        assert_eq!((versions[1].date_debut, versions[1].date_fin), (date(2016, 10, 9), None));

        let links = corpus.related_links(&article_id(NINE)).await.unwrap();
        assert_eq!(links.cites.len(), 1, "sens=source");
        assert_eq!(links.cites[0].nature, RelatedNature::Jorf);
        assert_eq!(links.cites[0].title.as_deref(), Some("Décret 80-217"));
        assert_eq!(links.cites[0].date, date(1980, 3, 20));
        assert_eq!(links.cited_by.len(), 1, "sens=cible, in any case");
        assert_eq!(links.cited_by[0].date, None, "2999-01-01 is no date");

        let missing = article_id("LEGIARTI000000000000");
        let versions = corpus.article_versions(&missing).await;
        assert_eq!(versions.err(), Some(not_found(CANONICAL_VERSIONS)));
        let links = corpus.related_links(&missing).await;
        assert_eq!(links.err(), Some(not_found(RELATED_LINKS)));
        let kali = article_id("KALIARTI000005805295");
        let kali_error = Some(unsupported("a KALI article"));
        assert_eq!(corpus.article_versions(&kali).await.err(), kali_error);
        assert_eq!(corpus.related_links(&kali).await.err(), kali_error);
    }

    #[tokio::test]
    async fn an_oversized_toc_is_too_large_and_a_cycle_is_too_deep() {
        let corpus = DilaCorpus::in_memory().unwrap();
        let mut store = corpus.writer().unwrap();
        let looping = Section {
            id: SECTION.into(),
            title: Some("Boucle".into()),
            context: TextContext::default(),
            structure: vec![link_to(LinkTarget::Section, SECTION, None)],
        };
        store.put(stored_doc("code_en_vigueur", CODE, Document::Section(looping), 1)).unwrap();
        // une chaîne de 65 sections : une de plus que la profondeur du domaine
        for level in 1..=65_u64 {
            let next = format!("LEGISCTA{:012}", level + 1);
            let structure = if level < 65 {
                vec![link_to(LinkTarget::Section, &next, None)]
            } else {
                Vec::new()
            };
            let section = Section {
                id: format!("LEGISCTA{level:012}"),
                title: Some(format!("Niveau {level}")),
                context: TextContext::default(),
                structure,
            };
            store.put(stored_doc("code_en_vigueur", CODE, Document::Section(section), 1)).unwrap();
        }
        store.commit().unwrap();
        let view = corpus.engine().view();
        let at = date(2026, 10, 15).unwrap();
        let chain_from =
            |level: u64| [link_to(LinkTarget::Section, &format!("LEGISCTA{level:012}"), None)];
        let chain = build_toc(&view, &chain_from(1), at, TOC_MAX_NODES).unwrap_err();
        assert_eq!(chain, crate::map::too_deep(), "65 levels are one too many");
        let exact = build_toc(&view, &chain_from(2), at, TOC_MAX_NODES).unwrap();
        assert_eq!(exact.len(), 64, "64 levels are the bound");
        let links = [link_to(LinkTarget::Section, SECTION, None)];
        let deep = build_toc(&view, &links, at, TOC_MAX_NODES).unwrap_err();
        assert_eq!(deep, crate::map::too_deep(), "a cycle of sections stops at the guard");
        let many = [
            link_to(LinkTarget::Article, NINE, Some("9")),
            link_to(LinkTarget::Article, ONE, Some("1")),
            link_to(LinkTarget::Article, POSTAL, Some("2")),
        ];
        let large = build_toc(&view, &many, at, 2).unwrap_err();
        let detail = "table of contents above the entry bound".to_owned();
        assert_eq!(large, CorpusError::TooLarge { detail });
        // une section dont le document manque reste, avec le titre de son lien
        let mut dangling = link_to(LinkTarget::Section, "LEGISCTA000099999999", None);
        dangling.title = Some("Titre fantôme".into());
        let toc = build_toc(&view, &[dangling], at, TOC_MAX_NODES).unwrap();
        assert_eq!(toc.len(), 1);
    }

    #[test]
    fn grouping_keeps_the_rank_of_the_first_document_of_each_text() {
        let corpus = world();
        let view = corpus.engine().view();
        let ranked = view.search(Tree::Tnc, "golden", MatchMode::Any, None, 50).unwrap().unwrap();
        let groups = group_by_text(one_copy_per_id(ranked.docs));
        let dirs: Vec<&str> = groups.iter().map(|group| group.dir_cid.as_str()).collect();
        assert_eq!(dirs.len(), 2);
        assert!(groups.iter().all(|group| group.articles.len() == 1));
        let text = view.text_of_dir(LAW_DIR).unwrap().unwrap();
        assert!(text.document.as_text_version().is_some());
    }

    #[tokio::test]
    async fn a_hit_without_a_text_version_is_dropped_and_counted() {
        let corpus = DilaCorpus::in_memory().unwrap();
        let mut store = corpus.writer().unwrap();
        let orphan = article(
            LAW_ARTICLE,
            "1",
            (date(2010, 2, 4), date(2999, 1, 1)),
            Some("VIGUEUR"),
            LAW_DIR,
            "LOI",
            "Loi",
            "<p>golden orphelin</p>",
        );
        store.put(stored_doc("TNC_en_vigueur", LAW_DIR, Document::Article(orphan), 1)).unwrap();
        store.commit().unwrap();
        let page = corpus.search(&quick(Fond::Loda, "golden")).await.unwrap();
        assert_eq!((page.hits.len(), page.dropped, page.total), (0, 1, 1));
    }

    /// Copie de la mini-archive du global et de l'incrément, sous des noms de la DILA.
    fn archive_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let copy = |from: &str, to: &str| {
            std::fs::copy(crate::testing::fixture_path(from), dir.path().join(to)).unwrap();
        };
        copy("archives/legi-global-mini.tar.gz", "Freemium_legi_global_20250713-140000.tar.gz");
        copy("archives/legi-incr-20261028-mini.tar.gz", "LEGI_20261028-212713.tar.gz");
        dir
    }

    #[tokio::test]
    async fn sync_fills_the_index_like_the_memory_store_and_survives_a_reopen() {
        let archives = archive_dir();
        let work = tempfile::tempdir().unwrap();
        let registry = work.path().join("registry.toml");
        let index_dir = work.path().join("index");

        let corpus = DilaCorpus::open(&index_dir).unwrap();
        assert!(corpus.engine().is_empty());
        let report = corpus
            .sync(archives.path().to_path_buf(), registry.clone(), IngestLimits::default())
            .await
            .unwrap();
        assert_eq!(report.applied.len(), 2);
        assert_eq!(report.applied[0].name.kind(), ArchiveKind::Global);

        // le même dossier rangé dans un MemoryStore donne autant de documents
        let mut memory = crate::store::MemoryStore::new();
        let mem_registry = work.path().join("memory.toml");
        sync_directory(archives.path(), &mem_registry, &mut memory, &IngestLimits::default())
            .unwrap();
        assert_eq!(corpus.engine().len(), u64::try_from(memory.len()).unwrap());
        assert!(!corpus.engine().is_empty());

        // les articles du petit code des architectes se lisent
        let architects = text_id("LEGITEXT000006074232");
        let act = corpus.get_act(&architects, today()).await.unwrap();
        assert!(act.body.title.is_some());
        let codes = corpus.list_codes().await.unwrap();
        assert!(codes.iter().any(|code| code.id == architects), "{codes:?}");

        // rien de neuf : la seconde passe est vide
        let again = corpus
            .sync(archives.path().to_path_buf(), registry.clone(), IngestLimits::default())
            .await
            .unwrap();
        assert!(again.is_up_to_date());

        // un autre écrivain tient le verrou : la synchronisation le dit
        let held = corpus.writer().unwrap();
        let blocked = corpus
            .sync(archives.path().to_path_buf(), registry.clone(), IngestLimits::default())
            .await;
        let message = blocked.unwrap_err().to_string();
        assert!(message.starts_with("document store failure: cannot lock the index"), "{message}");
        drop(held);

        // l'index est sur disque : un corpus rouvert répond comme avant
        let documents = corpus.engine().len();
        drop(corpus);
        let reopened = DilaCorpus::open(&index_dir).unwrap();
        assert_eq!(reopened.engine().len(), documents);
        assert!(reopened.get_act(&architects, today()).await.is_ok());
    }

    #[tokio::test]
    async fn sync_reports_a_missing_directory_as_an_io_error() {
        let corpus = DilaCorpus::in_memory().unwrap();
        let work = tempfile::tempdir().unwrap();
        let outcome = corpus
            .sync(
                work.path().join("absent"),
                work.path().join("registry.toml"),
                IngestLimits::default(),
            )
            .await;
        let message = outcome.unwrap_err().to_string();
        assert!(message.starts_with("cannot list the archive directory: "), "{message}");
    }
}
