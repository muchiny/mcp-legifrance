//! Harnais de test en processus (feature `test-support`, 06 §6.1) : un vrai client rmcp relié au
//! serveur par un tube mémoire (`tokio::io::duplex`), sans processus ni réseau.
//!
//! - [`connect`] sert n'importe quel `ServerHandler` derrière le chemin de production
//!   (`PreLifecycleFilter`, EOF, versions), et rend le client.
//! - [`Fixture`] monte un [`LegifranceServer`] sur les doubles de `lf-application`
//!   ([`FakeCorpus`], [`MemoryAuditLog`], [`FixedClock`]) tout en gardant des poignées dessus : un
//!   test d'outil programme le corpus, appelle l'outil par le client, puis relit les appels du port
//!   et les lignes d'audit.
//!
//! Jamais activée en dépendance normale : `cargo xtask check-arch` (R10) n'admet la feature que
//! dans les `[dev-dependencies]` des autres membres.
#![expect(
    clippy::expect_used,
    reason = "harnais de test : un échec de montage fait tomber le test"
)]

use std::{sync::Arc, time::Duration};

use lf_application::{
    audited::{AuditMode, AuditPolicy},
    budget::ResponseBudget,
    ports::{
        ArticleAt, AuditError, AuditLog, AuditRecord, CorpusError, LegalCorpus, PisteEnvironment,
    },
    service::{ConfigurationState, LegalService, ServiceSettings},
    testing::{FakeCorpus, FixedClock, MemoryAuditLog},
};
use lf_domain::{
    date::{CivilDate, VersionDate},
    entity::{
        Act, ArticleText, ArticleVersion, CodeListing, CompanyAgreement, Convention, Decision,
        Deliberation, RelatedLinks, SearchHit, SearchPage,
    },
    id::{AccoTextId, ArticleId, CnilTextId, DecisionId, KaliTextId, LegiTextId},
    search::{NumberQuery, SearchQuery},
};
use rmcp::{RoleClient, ServerHandler, ServiceExt, service::RunningService};
use tokio_util::sync::CancellationToken;

use crate::{LegifranceServer, lifecycle::serve_io};

/// Capacité du tube mémoire entre le client et le serveur, en octets.
const PIPE_BYTES: usize = 64 * 1024;

/// Sert `server` sur un tube mémoire, derrière le chemin de production (`serve_io`), et rend le
/// client rmcp connecté (poignée de main faite). Laisser tomber le client ferme le tube : le
/// serveur voit l'EOF et s'arrête.
///
/// # Panics
///
/// Si la poignée de main échoue.
pub async fn connect<S: ServerHandler>(server: S) -> RunningService<RoleClient, ()> {
    let (client_io, server_io) = tokio::io::duplex(PIPE_BYTES);
    let (read, write) = tokio::io::split(server_io);
    tokio::spawn(serve_io(server, read, write, CancellationToken::new()));
    ().serve(client_io).await.expect("the in-process client handshake")
}

/// Corpus partagé : le service possède son corpus, le test en garde une poignée.
pub struct SharedCorpus(pub Arc<FakeCorpus>);

impl LegalCorpus for SharedCorpus {
    async fn search(&self, query: &SearchQuery) -> Result<SearchPage, CorpusError> {
        self.0.search(query).await
    }

    async fn get_act(&self, id: &LegiTextId, date: VersionDate) -> Result<Act, CorpusError> {
        self.0.get_act(id, date).await
    }

    async fn get_article(&self, id: &ArticleId) -> Result<ArticleText, CorpusError> {
        self.0.get_article(id).await
    }

    async fn get_decision(&self, id: &DecisionId) -> Result<Decision, CorpusError> {
        self.0.get_decision(id).await
    }

    async fn get_deliberation(&self, id: &CnilTextId) -> Result<Deliberation, CorpusError> {
        self.0.get_deliberation(id).await
    }

    async fn get_convention(&self, id: &KaliTextId) -> Result<Convention, CorpusError> {
        self.0.get_convention(id).await
    }

    async fn get_company_agreement(
        &self,
        id: &AccoTextId,
    ) -> Result<CompanyAgreement, CorpusError> {
        self.0.get_company_agreement(id).await
    }

    async fn find_article(
        &self,
        text: &LegiTextId,
        num: &str,
        at: ArticleAt,
    ) -> Result<ArticleText, CorpusError> {
        self.0.find_article(text, num, at).await
    }

    async fn find_by_number(&self, query: &NumberQuery) -> Result<Vec<SearchHit>, CorpusError> {
        self.0.find_by_number(query).await
    }

    async fn list_codes(&self) -> Result<Vec<CodeListing>, CorpusError> {
        self.0.list_codes().await
    }

    async fn article_versions(&self, id: &ArticleId) -> Result<Vec<ArticleVersion>, CorpusError> {
        self.0.article_versions(id).await
    }

    async fn related_links(&self, id: &ArticleId) -> Result<RelatedLinks, CorpusError> {
        self.0.related_links(id).await
    }
}

/// Journal d'audit partagé : le service possède son journal, le test en garde une poignée.
pub struct SharedAudit(pub Arc<MemoryAuditLog>);

impl AuditLog for SharedAudit {
    async fn record(&self, record: &AuditRecord) -> Result<(), AuditError> {
        self.0.record(record).await
    }
}

/// Le serveur monté sur les doubles partagés d'une [`Fixture`].
pub type FixtureServer = LegifranceServer<SharedCorpus, SharedAudit, FixedClock>;

/// Un serveur de test et les poignées sur ce qu'il contient.
///
/// Les champs sont publics : un test change `settings` (mode d'audit, échéance) ou `today` avant
/// d'appeler [`Fixture::server`] ou [`Fixture::connect`].
pub struct Fixture {
    /// Corpus factice du service : programmer les réponses (`push_*`), relire les appels.
    pub corpus: Arc<FakeCorpus>,
    /// Journal d'audit en mémoire du service : relire les lignes (`records`).
    pub audit: Arc<MemoryAuditLog>,
    /// Réglages du service : audit strict, échéance de 60 s, bac à sable, identifiants configurés.
    pub settings: ServiceSettings,
    /// « Aujourd'hui » à Paris pour l'horloge figée : 2026-10-29.
    pub today: CivilDate,
}

impl Default for Fixture {
    fn default() -> Self {
        Self {
            corpus: Arc::new(FakeCorpus::new()),
            audit: Arc::new(MemoryAuditLog::new()),
            settings: ServiceSettings {
                policy: AuditPolicy {
                    mode: AuditMode::Strict,
                    deadline: Duration::from_secs(60),
                    env: PisteEnvironment::Sandbox,
                    server_version: env!("CARGO_PKG_VERSION"),
                },
                configuration: ConfigurationState {
                    credentials_configured: true,
                    ..ConfigurationState::default()
                },
                budget: ResponseBudget,
            },
            today: CivilDate::new(2026, 10, 29).expect("a valid civil date"),
        }
    }
}

impl Fixture {
    /// Un serveur neuf sur les doubles de la fixture (aucun routeur d'outil ne le remplit encore :
    /// les tranches y ajoutent leurs outils).
    #[must_use]
    pub fn server(&self) -> FixtureServer {
        #[expect(
            clippy::default_trait_access,
            reason = "chrono n'est pas une dépendance de ce crate : l'instant par défaut est l'époque Unix"
        )]
        let now = Default::default();
        let clock = FixedClock::new(now, self.today);
        let service = LegalService::new(
            SharedCorpus(Arc::clone(&self.corpus)),
            SharedAudit(Arc::clone(&self.audit)),
            clock,
            self.settings.clone(),
        );
        LegifranceServer::new(Arc::new(service))
    }

    /// [`Fixture::server`] servi par [`connect`] : le client rmcp connecté.
    pub async fn connect(&self) -> RunningService<RoleClient, ()> {
        connect(self.server()).await
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use lf_application::{
        input::CanonicalInput, ports::AuditStatus, testing::PortRequest, tools::ToolName,
    };
    use lf_domain::{
        fond::Fond,
        search::{MatchMode, NumberKind, NumberValue, PageSize, QueryText},
    };
    use pretty_assertions::assert_eq;

    use super::*;

    #[tokio::test]
    async fn connect_lists_no_tools_on_empty_router() {
        let fixture = Fixture::default();
        let client = connect(fixture.server()).await;
        assert_eq!(client.list_all_tools().await.unwrap().len(), 0);
        let info = client.peer_info().expect("the server introduced itself");
        assert_eq!(info.server_info.as_ref().unwrap().name, "mcp-legifrance");
    }

    #[tokio::test]
    async fn the_fixture_connects_its_own_server() {
        let client = Fixture::default().connect().await;
        assert_eq!(client.list_all_tools().await.unwrap().len(), 0);
    }

    #[test]
    fn the_default_fixture_is_strict_with_credentials_on_a_fixed_day() {
        let fixture = Fixture::default();
        assert_eq!(fixture.settings.policy.mode, AuditMode::Strict);
        assert_eq!(fixture.settings.policy.deadline, Duration::from_secs(60));
        assert!(fixture.settings.configuration.credentials_configured);
        assert_eq!(fixture.today.to_string(), "2026-10-29");
        assert_eq!(fixture.audit.records().len(), 0);
        assert_eq!(fixture.corpus.calls(), 0);
    }

    #[test]
    fn the_server_reads_the_settings_of_the_fixture() {
        let mut fixture = Fixture::default();
        fixture.settings.policy.deadline = Duration::from_secs(7);
        assert_eq!(fixture.server().service.settings().policy.deadline, Duration::from_secs(7));
    }

    #[tokio::test]
    async fn the_shared_audit_handle_sees_what_the_service_records() {
        let fixture = Fixture::default();
        let shared = SharedAudit(Arc::clone(&fixture.audit));
        #[expect(
            clippy::default_trait_access,
            reason = "chrono n'est pas une dépendance de ce crate : l'instant par défaut est l'époque Unix"
        )]
        let ts = Default::default();
        let record = AuditRecord {
            ts,
            tool: ToolName::FR_COVERAGE,
            input: CanonicalInput::new("{}".to_owned()),
            output_count_or_size: 0,
            duration_ms: 0,
            status: AuditStatus::Ok,
            error_code: None,
            error: None,
            env: PisteEnvironment::Sandbox,
            server_version: "0.0.0",
        };
        shared.record(&record).await.unwrap();
        assert_eq!(fixture.audit.records().len(), 1);
        assert_eq!(fixture.audit.records()[0].tool, ToolName::FR_COVERAGE);
    }

    #[tokio::test]
    async fn the_shared_corpus_forwards_the_twelve_operations_to_the_fake() {
        let corpus = Arc::new(FakeCorpus::new());
        let shared = SharedCorpus(Arc::clone(&corpus));
        let query = SearchQuery::new(
            Fond::Juri,
            QueryText::parse("bail").unwrap(),
            NonZeroU32::MIN,
            PageSize::default(),
            MatchMode::default(),
            None,
            None,
        )
        .unwrap();
        let number = NumberQuery::new(
            Fond::Juri,
            NumberKind::Case,
            NumberValue::parse("24-13.470").unwrap(),
            VersionDate::MIN,
        )
        .unwrap();
        let text: LegiTextId = "LEGITEXT000006070721".parse().unwrap();
        let article: ArticleId = "LEGIARTI000006419288".parse().unwrap();
        let date = VersionDate::parse("2026-10-29").unwrap();

        // Réponses non programmées : seul le transfert des appels au faux est éprouvé.
        let _ = shared.search(&query).await;
        let _ = shared.get_act(&text, date).await;
        let _ = shared.get_article(&article).await;
        let _ = shared.get_decision(&"JURITEXT000037999394".parse().unwrap()).await;
        let _ = shared.get_deliberation(&"CNILTEXT000053352643".parse().unwrap()).await;
        let _ = shared.get_convention(&"KALITEXT000005652329".parse().unwrap()).await;
        let _ = shared.get_company_agreement(&"ACCOTEXT000037731479".parse().unwrap()).await;
        let _ = shared.find_article(&text, "9", ArticleAt::Today(date)).await;
        let _ = shared.find_by_number(&number).await;
        let _ = shared.list_codes().await;
        let _ = shared.article_versions(&article).await;
        let _ = shared.related_links(&article).await;

        let methods: Vec<&str> = corpus.seen_requests().iter().map(PortRequest::method).collect();
        assert_eq!(
            methods,
            [
                "search",
                "get_act",
                "get_article",
                "get_decision",
                "get_deliberation",
                "get_convention",
                "get_company_agreement",
                "find_article",
                "find_by_number",
                "list_codes",
                "article_versions",
                "related_links",
            ]
        );
    }
}
