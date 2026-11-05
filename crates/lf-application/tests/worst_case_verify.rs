//! Test d'intégration de pire cas de `fr_verify_citations` (plan 1.1.1, section 4.4 « Après la
//! vague 3 », R1-17, et le « budget à 50 citations » d'U10, R1-12) : UN texte de 50 citations qui
//! mêle tout ce que la 1.1.1 a changé, rendu par le vrai cas d'usage (`LegalService::verify_citations`,
//! la méthode que l'outil MCP appelle : décodage, échéance d'`Audited`, `verify_text`, ligne d'audit)
//! sur un corpus de test déterministe, en temps virtuel (`start_paused`).
//!
//! Le texte (48 citations distinctes et 2 répétitions, dans l'ordre de `cases()`) :
//!
//! - articles de codes à la date du jour, trouvés, absents, en échec amont ;
//! - articles de codes à une date passée (U01) : version trouvée seulement à cette date, absente à
//!   cette date, avec la variante de clé « 1er » puis « 1 » ;
//! - articles d'une loi non codifiée (U10) : trouvés, absents du sommaire, modificatifs (« aucune
//!   version en vigueur »), datés (le sommaire du jour sert de repli), et lois qu'aucun index ne
//!   connaît ;
//! - cours administratives d'appel (U04), présentes, absentes, en écart de siège et de date ;
//! - requêtes jointes du Conseil d'État (U11) trouvées par l'un de leurs numéros, par le repli
//!   plein texte, ou introuvables, et décisions du Conseil d'État citées à la suite (U08) ;
//! - délibérations de la CNIL (U06, U07), dont une sans date fiable ;
//! - pourvois de la Cour de cassation (U12) : mauvaise chambre, tête non lue, date à tirets,
//!   pourvois joints ;
//! - citations tranchées hors ligne (année future, sigle ambigu, droit de l'Union) et deux
//!   répétitions, qui ne coûtent aucun appel.
//!
//! # Ce que les trois tests mesurent
//!
//! - `fifty_mixed_citations_...` : la liste exacte des 50 verdicts (statut et note), les compteurs
//!   (`found`, `checked`, `truncated`), le nombre **exact** d'appels du port, méthode par méthode
//!   (85), sa borne documentée, la concurrence (au plus 4 résolutions en vol), la durée virtuelle
//!   (entre le travail divisé par 4 et le travail en série, sous le budget de 80 % du délai) et la
//!   ligne d'audit. Toutes les notes tiennent en 200 caractères.
//! - `a_slow_upstream_...` : chaque appel dure 4 s ; le budget de temps (80 % de 60 s) est atteint.
//!   Les citations non finies sont `unresolvable` avec la note « Not checked: time budget. », les
//!   autres gardent leur verdict exact, aucun appel ne part après le budget, la réponse n'est pas
//!   une erreur d'échéance, la durée ne dépasse pas le budget.
//! - `fifty_five_...` : au-delà de `max_citations` (50 par défaut), `truncated` est vrai, `found`
//!   compte tout, et les citations en trop ne coûtent aucun appel.
//!
//! # Rapport avec les gardes qui existent déjà
//!
//! Ce test ne les remplace pas, il leur ajoute le mélange : `verify.rs` (`mod tests`) garde les
//! charges homogènes (`twenty_caa_citations_cost_one_get_decision_each`,
//! `forty_unfound_ce_citations_cost_two_searches_each`,
//! `fifty_unfound_ce_citations_stay_within_the_time_budget`,
//! `fifty_absent_text_articles_stay_within_the_time_budget`), `crates/lf-adapter-mcp/tests/budgets_m6.rs`
//! la taille de la réponse sur le fil (100 citations de 1 000 caractères, 45 000 caractères), et
//! `crates/lf-adapter-piste/src/corpus/m6.rs` les appels HTTP de l'adapter derrière chaque appel du
//! port (repli daté, étage 2) : un corpus de test de `lf-application` ne les voit pas (R7). Les
//! comptes de ce fichier sont donc ceux du **port**, et ne valent que pour la tête de la branche de
//! correctifs (règle de comptage de la section 4.2 : ce n'est pas un garde à vert sur les souches).
//!
//! Le corpus n'est pas `FakeCorpus` : sa file de réponses par méthode se vide dans l'ordre des
//! appels, qui, avec quatre résolutions en vol, n'est pas stable d'un ordonnancement à l'autre. Les
//! réponses de `ScriptedCorpus` sont indexées par les arguments de l'appel, et tout appel qui n'a
//! pas de réponse écrite est relevé (`unscripted`) et fait échouer le test.
//!
//! Le harnais n'existe qu'avec la feature `test-support` (`--all-features`). Tout le fichier est sous
//! `cfg(test)` : les aides partagent les règles de lint des tests (`unwrap` et `expect` permis,
//! `clippy.toml`).

#![cfg(feature = "test-support")]
#![expect(
    clippy::arithmetic_side_effects,
    reason = "tests : valeurs bornées par les données du test"
)]

#[cfg(test)]
mod worst_case {
    use std::{
        collections::HashMap,
        future::pending,
        sync::{
            Arc, Mutex, MutexGuard, PoisonError,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use chrono::{TimeZone, Utc};
    use lf_application::{
        audited::{AuditMode, AuditPolicy},
        budget::ResponseBudget,
        citation::{CheckStatus, CitationCheck, Note, notes::NOTE_MAX_CHARS},
        input::{CanonicalInput, ToolCall, VerifyCitationsInput},
        output::verify_citations::VerifyCitationsOutput,
        ports::{
            ArticleAt, AuditError, AuditLog, AuditRecord, AuditStatus, ConfigKind, CorpusError,
            LegalCorpus, PisteEnvironment,
        },
        service::{ConfigurationState, LegalService, ServiceSettings},
        testing::{FixedClock, MemoryAuditLog},
        tools::ToolName,
    };
    use lf_domain::{
        date::{CivilDate, VersionDate},
        entity::{
            Act, ArticleDraft, ArticleText, ArticleVersion, CodeListing, CompanyAgreement,
            Convention, Decision, Deliberation, OfficialSummary, RelatedLinks, SearchHit,
            SearchPage, TextBody, Toc, TocDraft,
        },
        fond::{Fond, PaginationMode},
        id::{AccoTextId, ArticleId, CnilTextId, DecisionId, DocumentId, KaliTextId, LegiTextId},
        search::{NumberQuery, SearchQuery},
        status::LegalStatus,
        text::LongText,
    };
    use pretty_assertions::assert_eq;
    use tokio::time::Instant;

    // ---- constantes du monde simulé ------------------------------------------------------------

    /// Jour de Paris de l'horloge figée.
    const TODAY: &str = "2026-10-29";
    const CIVIL: &str = "LEGITEXT000006070721";
    const TRAVAIL: &str = "LEGITEXT000006072050";
    /// La loi pour une République numérique : un texte non codifié.
    const LAW: &str = "LEGITEXT000033202746";
    const LAW_TITLE: &str = "LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique (1)";

    /// Délai de l'outil (`LF_TOOL_DEADLINE_S`) et part que la résolution s'en réserve (80 %,
    /// `BUDGET_PERCENT` de `citation/policy.rs`).
    const DEADLINE: Duration = Duration::from_secs(60);
    const BUDGET: Duration = Duration::from_secs(48);
    /// Résolutions en vol au plus (`CONCURRENCY` de `citation/policy.rs`, constante privée).
    const CONCURRENCY: usize = 4;

    /// Appels du port, par méthode, que ce texte provoque (calculés à la main, case par case, dans
    /// `CASES`) ; leur somme est le total exact attendu.
    const EXPECTED_CALLS: [(&str, usize); 9] = [
        ("list_codes", 1),
        ("find_article", 26),
        ("get_act", 3),
        ("get_article", 4),
        ("find_by_number", 34),
        ("search", 3),
        ("get_decision", 12),
        ("get_deliberation", 2),
        ("related_links", 0),
    ];
    const EXPECTED_TOTAL: usize = 85;

    /// Les répétitions du texte : (rang de la répétition, rang de sa première occurrence), en
    /// comptant à partir de 0.
    const REPEATS: [(usize, usize); 2] = [(48, 0), (49, 32)];

    /// Borne des appels du port par citation, déduite du code et des bornes du plan 1.1.1. Un
    /// article de texte non codifié coûte au plus 1 recherche du texte, 2 variantes de clé (U01
    /// point 7), 2 lectures de sommaire (jour voulu, puis jour courant, U10) et 1 lecture de
    /// l'article, soit 6. Une décision du Conseil d'État coûte au plus 3 recherches exactes (les
    /// numéros de `ce_forms`), 1 recherche plein texte de repli (U11 : « N + 1 appels ») et 5
    /// consultations (`CONSULT_LIMIT`), soit 9. La liste des codes se lit une fois par appel.
    const PORT_CALLS_PER_CITATION_MAX: usize = 9;

    // ---- le corpus de test ---------------------------------------------------------------------

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Un appel du port : méthode, clé (ses arguments), instant de départ depuis le début du test.
    #[derive(Debug, Clone)]
    struct Call {
        method: &'static str,
        key: String,
        started: Duration,
    }

    type Table<T> = HashMap<String, Result<T, CorpusError>>;

    /// Corpus dont les réponses sont indexées par les arguments de l'appel, avec un délai par appel
    /// (temps de `tokio`), la liste ordonnée des appels, la concurrence maximale et les appels sans
    /// réponse écrite.
    struct ScriptedCorpus {
        delay: Duration,
        begin: Instant,
        codes: Vec<CodeListing>,
        find_article: Table<ArticleText>,
        find_by_number: Table<Vec<SearchHit>>,
        /// (fragment de la requête en `Debug`, page rendue) : le premier fragment présent gagne.
        search: Vec<(String, SearchPage)>,
        get_act: Table<Act>,
        get_article: Table<ArticleText>,
        get_decision: Table<Decision>,
        get_deliberation: Table<Deliberation>,
        calls: Mutex<Vec<Call>>,
        unscripted: Mutex<Vec<String>>,
        in_flight: AtomicUsize,
        max_in_flight: AtomicUsize,
    }

    /// Libère à sa chute l'emplacement pris dans le compteur d'appels en vol (futur abandonné
    /// compris : le budget de temps abandonne les appels qu'il coupe).
    struct Slot<'a>(&'a AtomicUsize);

    impl Drop for Slot<'_> {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    impl ScriptedCorpus {
        fn new(delay: Duration) -> Self {
            Self {
                delay,
                begin: Instant::now(),
                codes: Vec::new(),
                find_article: Table::new(),
                find_by_number: Table::new(),
                search: Vec::new(),
                get_act: Table::new(),
                get_article: Table::new(),
                get_decision: Table::new(),
                get_deliberation: Table::new(),
                calls: Mutex::new(Vec::new()),
                unscripted: Mutex::new(Vec::new()),
                in_flight: AtomicUsize::new(0),
                max_in_flight: AtomicUsize::new(0),
            }
        }

        /// Note l'appel, tient un emplacement en vol, attend le délai.
        async fn enter(&self, method: &'static str, key: &str) -> Slot<'_> {
            lock(&self.calls).push(Call {
                method,
                key: key.to_owned(),
                started: Instant::now().duration_since(self.begin),
            });
            let now = self.in_flight.fetch_add(1, Ordering::SeqCst).saturating_add(1);
            self.max_in_flight.fetch_max(now, Ordering::SeqCst);
            // Créé avant l'attente : un appel que le budget abandonne pendant le délai libère
            // son emplacement.
            let slot = Slot(&self.in_flight);
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            slot
        }

        /// La réponse écrite pour `key`, ou une erreur de configuration et un relevé.
        fn answer<T: Clone>(
            &self,
            method: &str,
            table: &Table<T>,
            key: &str,
        ) -> Result<T, CorpusError> {
            table.get(key).cloned().unwrap_or_else(|| self.missing(method, key))
        }

        fn missing<T>(&self, method: &str, key: &str) -> Result<T, CorpusError> {
            lock(&self.unscripted).push(format!("{method}({key})"));
            Err(CorpusError::Config {
                kind: ConfigKind::Settings,
                detail: format!("ScriptedCorpus: no answer for {method}({key})"),
            })
        }

        fn calls(&self) -> Vec<Call> {
            lock(&self.calls).clone()
        }

        fn calls_of(&self, method: &str) -> usize {
            lock(&self.calls).iter().filter(|call| call.method == method).count()
        }

        fn unscripted(&self) -> Vec<String> {
            lock(&self.unscripted).clone()
        }

        fn max_concurrency(&self) -> usize {
            self.max_in_flight.load(Ordering::SeqCst)
        }
    }

    /// Clé d'un `find_article` : texte, numéro et jour (« today » ou « on »).
    fn key_article(text: &str, num: &str, at: ArticleAt) -> String {
        match at {
            ArticleAt::Today(day) => format!("{text}|{num}|today {day}"),
            ArticleAt::On(day) => format!("{text}|{num}|on {day}"),
        }
    }

    /// Clé d'un `find_by_number` : fond, nature du numéro, valeur.
    fn key_number(query: &NumberQuery) -> String {
        format!("{}|{:?}|{}", query.fond.name(), query.kind, query.value.as_str())
    }

    impl LegalCorpus for ScriptedCorpus {
        async fn search(&self, query: &SearchQuery) -> Result<SearchPage, CorpusError> {
            let shown = format!("{query:?}");
            let _slot = self.enter("search", &shown).await;
            match self.search.iter().find(|(fragment, _)| shown.contains(fragment.as_str())) {
                Some((_, page)) => Ok(page.clone()),
                None => self.missing("search", &shown),
            }
        }

        async fn get_act(&self, id: &LegiTextId, date: VersionDate) -> Result<Act, CorpusError> {
            let key = format!("{}|{date}", id.as_str());
            let _slot = self.enter("get_act", &key).await;
            self.answer("get_act", &self.get_act, &key)
        }

        async fn get_article(&self, id: &ArticleId) -> Result<ArticleText, CorpusError> {
            let _slot = self.enter("get_article", id.as_str()).await;
            self.answer("get_article", &self.get_article, id.as_str())
        }

        async fn get_decision(&self, id: &DecisionId) -> Result<Decision, CorpusError> {
            let _slot = self.enter("get_decision", id.as_str()).await;
            self.answer("get_decision", &self.get_decision, id.as_str())
        }

        async fn get_deliberation(&self, id: &CnilTextId) -> Result<Deliberation, CorpusError> {
            let _slot = self.enter("get_deliberation", id.as_str()).await;
            self.answer("get_deliberation", &self.get_deliberation, id.as_str())
        }

        async fn get_convention(&self, id: &KaliTextId) -> Result<Convention, CorpusError> {
            let _slot = self.enter("get_convention", id.as_str()).await;
            self.missing("get_convention", id.as_str())
        }

        async fn get_company_agreement(
            &self,
            id: &AccoTextId,
        ) -> Result<CompanyAgreement, CorpusError> {
            let _slot = self.enter("get_company_agreement", id.as_str()).await;
            self.missing("get_company_agreement", id.as_str())
        }

        async fn find_article(
            &self,
            text: &LegiTextId,
            num: &str,
            at: ArticleAt,
        ) -> Result<ArticleText, CorpusError> {
            let key = key_article(text.as_str(), num, at);
            let _slot = self.enter("find_article", &key).await;
            self.answer("find_article", &self.find_article, &key)
        }

        async fn find_by_number(&self, query: &NumberQuery) -> Result<Vec<SearchHit>, CorpusError> {
            let key = key_number(query);
            let _slot = self.enter("find_by_number", &key).await;
            self.answer("find_by_number", &self.find_by_number, &key)
        }

        async fn list_codes(&self) -> Result<Vec<CodeListing>, CorpusError> {
            let _slot = self.enter("list_codes", "").await;
            Ok(self.codes.clone())
        }

        async fn article_versions(
            &self,
            id: &ArticleId,
        ) -> Result<Vec<ArticleVersion>, CorpusError> {
            let _slot = self.enter("article_versions", id.as_str()).await;
            self.missing("article_versions", id.as_str())
        }

        async fn related_links(&self, id: &ArticleId) -> Result<RelatedLinks, CorpusError> {
            let _slot = self.enter("related_links", id.as_str()).await;
            self.missing("related_links", id.as_str())
        }
    }

    /// Corpus partagé : le service possède le sien, le test en garde une poignée.
    struct Shared(Arc<ScriptedCorpus>);

    impl LegalCorpus for Shared {
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
        async fn article_versions(
            &self,
            id: &ArticleId,
        ) -> Result<Vec<ArticleVersion>, CorpusError> {
            self.0.article_versions(id).await
        }
        async fn related_links(&self, id: &ArticleId) -> Result<RelatedLinks, CorpusError> {
            self.0.related_links(id).await
        }
    }

    /// Journal d'audit partagé : le service possède le sien, le test en garde une poignée.
    struct SharedLog(Arc<MemoryAuditLog>);

    impl AuditLog for SharedLog {
        async fn record(&self, record: &AuditRecord) -> Result<(), AuditError> {
            self.0.record(record).await
        }
    }

    // ---- les entités ---------------------------------------------------------------------------

    fn civil(year: u16, month: u8, day: u8) -> CivilDate {
        CivilDate::new(year, month, day).unwrap()
    }

    fn version(text: &str) -> VersionDate {
        VersionDate::parse(text).unwrap()
    }

    fn not_found() -> CorpusError {
        CorpusError::NotFound { what: "/consult".to_owned() }
    }

    fn legi(n: u32) -> String {
        format!("LEGIARTI{n:012}")
    }

    /// Un article lu : numéro, état, début et fin de la version.
    fn article(
        id: &str,
        num: Option<&str>,
        etat: Option<LegalStatus>,
        since: Option<CivilDate>,
        until: Option<CivilDate>,
    ) -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(id).unwrap(),
            num: num.map(str::to_owned),
            etat,
            code_title: None,
            parent: None,
            path: vec![],
            full_sections_titre: None,
            date_debut: since,
            date_fin: until,
            text: LongText::from_plain("Texte officiel de l'article.").unwrap(),
            nota: None,
        }
    }

    fn hit(fond: Fond, id: &str, title: Option<&str>) -> SearchHit {
        let mut record = SearchHit::new(fond, DocumentId::parse(id).unwrap(), None).unwrap();
        record.title = title.map(str::to_owned);
        record
    }

    fn page(fond: Fond, hits: Vec<SearchHit>) -> SearchPage {
        SearchPage::new(fond, "q", None, None, hits, 0, PaginationMode::Defaut)
    }

    fn decision(
        id: &str,
        date: CivilDate,
        juridiction: Option<&str>,
        formation: Option<&str>,
    ) -> Decision {
        Decision {
            decision_id: DecisionId::parse(id).unwrap(),
            ecli: None,
            num: None,
            juridiction: juridiction.map(str::to_owned),
            formation: formation.map(str::to_owned),
            solution: None,
            numero_affaire: vec![],
            date_texte: Some(date),
            nature: None,
            title: None,
            text: LongText::from_plain("Texte de la décision.").unwrap(),
            official_summary: OfficialSummary::default(),
            cited_case_law: None,
            applied_texts: vec![],
        }
    }

    fn deliberation(id: &str, num: &str, date: Option<CivilDate>) -> Deliberation {
        Deliberation {
            deliberation_id: CnilTextId::parse(id).unwrap(),
            num: Some(num.to_owned()),
            nature: None,
            nature_delib: None,
            etat: None,
            date_texte: date,
            date_publication: None,
            title: None,
            title_long: None,
            text: LongText::default(),
        }
    }

    /// Sommaire à plat de la loi : les articles qui y figurent sans version (modificatifs).
    fn law_act() -> Act {
        let articles = [1_u32, 36, 37, 38, 39]
            .into_iter()
            .map(|n| ArticleDraft {
                id: Some(legi(n)),
                num: Some(n.to_string()),
                etat: None,
                ..ArticleDraft::default()
            })
            .collect();
        let toc = Toc::build(TocDraft { sections: vec![], articles }).unwrap();
        Act {
            text_id: LegiTextId::parse(LAW).unwrap(),
            body: TextBody { toc, ..TextBody::default() },
        }
    }

    /// Article modificatif de la loi : ni état ni date.
    fn amending(n: u32) -> ArticleText {
        article(&legi(n), None, None, None, None)
    }

    // ---- le monde : une table des réponses par citation ----------------------------------------

    const MODERN_CAA: &str =
        "CAA de MARSEILLE, 5ème chambre, 02/10/2026, 25MA02308, Inédit au recueil Lebon";

    #[expect(clippy::too_many_lines, reason = "une réponse écrite par cas du texte")]
    fn world(delay: Duration) -> ScriptedCorpus {
        let mut corpus = ScriptedCorpus::new(delay);
        let today = ArticleAt::Today(version(TODAY));
        let on = |day: &str| ArticleAt::On(version(day));
        corpus.codes = vec![
            CodeListing {
                id: LegiTextId::parse(CIVIL).unwrap(),
                title: "Code civil".to_owned(),
                etat: Some(LegalStatus::Vigueur),
            },
            CodeListing {
                id: LegiTextId::parse(TRAVAIL).unwrap(),
                title: "Code du travail".to_owned(),
                etat: Some(LegalStatus::Vigueur),
            },
        ];
        let in_force = |n: u32, num: &str, since: CivilDate| {
            article(&legi(n), Some(num), Some(LegalStatus::Vigueur), Some(since), None)
        };
        let past = |n: u32, num: &str, etat: LegalStatus, since: CivilDate, until: CivilDate| {
            article(&legi(n), Some(num), Some(etat), Some(since), Some(until))
        };
        let fa = &mut corpus.find_article;

        // Codes, à la date du jour : trouvés (1 appel chacun), absents, en échec amont.
        for (n, num) in [(1240_u32, "1240"), (1241, "1241"), (1242, "1242")] {
            fa.insert(key_article(CIVIL, num, today), Ok(in_force(n, num, civil(2016, 10, 1))));
        }
        for (n, num) in [(100_u32, "L1234-5"), (101, "L1237-19")] {
            fa.insert(key_article(TRAVAIL, num, today), Ok(in_force(n, num, civil(2008, 5, 1))));
        }
        fa.insert(key_article(CIVIL, "9993", today), Err(not_found()));
        fa.insert(key_article(TRAVAIL, "L9994-1", today), Err(not_found()));
        fa.insert(
            key_article(CIVIL, "1244", today),
            Err(CorpusError::Upstream {
                status: Some(503),
                retryable: true,
                detail: "down".to_owned(),
            }),
        );

        // Codes, à une date passée (U01) : la version de l'époque, remplacée depuis.
        fa.insert(
            key_article(CIVIL, "1382", on("2015-01-01")),
            Ok(past(200, "1382", LegalStatus::Modifie, civil(2011, 1, 1), civil(2016, 10, 1))),
        );
        fa.insert(
            key_article(CIVIL, "1383", on("2015-01-01")),
            Ok(past(201, "1383", LegalStatus::Modifie, civil(2011, 1, 1), civil(2016, 10, 1))),
        );
        fa.insert(
            key_article(TRAVAIL, "L1221-1", on("2008-01-01")),
            Ok(past(202, "L1221-1", LegalStatus::Abroge, civil(2007, 1, 1), civil(2008, 5, 1))),
        );
        // « 1er » : la première variante de la clé échoue, la seconde (« 1 ») trouve.
        fa.insert(key_article(CIVIL, "1er", on("1990-01-01")), Err(not_found()));
        fa.insert(
            key_article(CIVIL, "1", on("1990-01-01")),
            Ok(past(203, "1", LegalStatus::Modifie, civil(1980, 1, 1), civil(2016, 2, 10))),
        );
        // Absents à cette date.
        fa.insert(key_article(CIVIL, "9990", on("1990-01-01")), Err(not_found()));
        fa.insert(key_article(CIVIL, "9991", on("1990-01-01")), Err(not_found()));
        fa.insert(key_article(TRAVAIL, "L9992-1", on("2008-01-01")), Err(not_found()));

        // La loi (U10) : « 1er » trouvé par la seconde variante ; les autres échouent à
        // `find_article`, c'est le sommaire qui tranche.
        fa.insert(key_article(LAW, "1er", today), Err(not_found()));
        fa.insert(key_article(LAW, "1", today), Ok(in_force(1, "1", civil(2016, 10, 9))));
        for num in ["150", "151", "36", "37", "38"] {
            fa.insert(key_article(LAW, num, today), Err(not_found()));
        }
        fa.insert(key_article(LAW, "153", on("2017-01-01")), Err(not_found()));
        fa.insert(key_article(LAW, "154", on("2018-06-01")), Err(not_found()));
        fa.insert(key_article(LAW, "39", on("2017-01-01")), Err(not_found()));

        // Sommaires : celui du jour, celui du 2018-06-01 ; aucun ne l'est au 2017-01-01 (le
        // sommaire du jour sert alors de repli).
        corpus.get_act.insert(format!("{LAW}|{TODAY}"), Ok(law_act()));
        corpus.get_act.insert(format!("{LAW}|2018-06-01"), Ok(law_act()));
        corpus.get_act.insert(format!("{LAW}|2017-01-01"), Err(not_found()));
        for n in [36_u32, 37, 38, 39] {
            corpus.get_article.insert(legi(n), Ok(amending(n)));
        }

        // Index des textes numérotés.
        let mut law_hit = hit(Fond::Loda, LAW, Some(LAW_TITLE));
        law_hit.nature = Some("LOI".to_owned());
        law_hit.date = CivilDate::new(2016, 10, 9);
        let by_number = &mut corpus.find_by_number;
        by_number.insert("LODA|Text|2016-1321".to_owned(), Ok(vec![law_hit]));
        by_number.insert("LODA|Text|2015-9999".to_owned(), Ok(vec![]));
        by_number.insert("LODA|Text|2014-9998".to_owned(), Ok(vec![]));

        // CAA (U04) : trois présentes, deux absentes, une en écart de siège et de date.
        for (rank, number) in [(8_u32, "25MA02308"), (9, "25MA02309"), (10, "25MA02310")] {
            let id = format!("CETATEXT0000549557{rank:02}");
            by_number.insert(
                format!("CETAT|Decision|{number}"),
                Ok(vec![hit(Fond::Cetat, &id, Some(MODERN_CAA))]),
            );
            corpus.get_decision.insert(
                id.clone(),
                Ok(decision(&id, civil(2026, 10, 2), Some("CAA de MARSEILLE"), None)),
            );
        }
        by_number.insert("CETAT|Decision|24LY01111".to_owned(), Ok(vec![]));
        by_number.insert("CETAT|Decision|24PA01112".to_owned(), Ok(vec![]));
        let seat_id = "CETATEXT000054955711";
        by_number.insert(
            "CETAT|Decision|25MA02311".to_owned(),
            Ok(vec![hit(Fond::Cetat, seat_id, Some(MODERN_CAA))]),
        );
        corpus.get_decision.insert(
            seat_id.to_owned(),
            Ok(decision(seat_id, civil(2026, 10, 2), Some("CAA de MARSEILLE"), None)),
        );

        // Conseil d'État (U11) : Rouquette, retrouvé par son second numéro.
        let rouquette = "CETATEXT000008000361";
        by_number.insert("CETAT|Decision|194658".to_owned(), Ok(vec![]));
        by_number.insert(
            "CETAT|Decision|196116".to_owned(),
            Ok(vec![hit(
                Fond::Cetat,
                rouquette,
                Some("Conseil d'Etat, Assemblée, du 5 mars 1999, 194658 196116, publié au recueil Lebon"),
            )]),
        );
        corpus
            .get_decision
            .insert(rouquette.to_owned(), Ok(decision(rouquette, civil(1999, 3, 5), None, None)));
        // Sarran : aucun des deux numéros n'est un `NUM_DEC` exact, le repli plein texte le trouve.
        let sarran = "CETATEXT000008008495";
        by_number.insert("CETAT|Decision|200286".to_owned(), Ok(vec![]));
        by_number.insert("CETAT|Decision|200287".to_owned(), Ok(vec![]));
        corpus.search.push((
            "200286".to_owned(),
            page(
                Fond::Cetat,
                vec![hit(
                    Fond::Cetat,
                    sarran,
                    Some("Conseil d'Etat, Assemblée, du 30 octobre 1998, 200286 200287, publié au recueil Lebon"),
                )],
            ),
        ));
        corpus
            .get_decision
            .insert(sarran.to_owned(), Ok(decision(sarran, civil(1998, 10, 30), None, None)));
        // Trois numéros et un seul, introuvables : les recherches exactes puis le repli.
        for number in ["410000", "410001", "410002", "410100"] {
            by_number.insert(format!("CETAT|Decision|{number}"), Ok(vec![]));
        }
        corpus.search.push(("410000".to_owned(), page(Fond::Cetat, vec![])));
        corpus.search.push(("410100".to_owned(), page(Fond::Cetat, vec![])));

        // Conseil d'État (U08) : la seconde de deux décisions citées à la suite.
        for (number, id, date) in [
            ("328757", "CETATEXT000022513010", civil(2010, 7, 23)),
            ("328758", "CETATEXT000022513011", civil(2010, 7, 24)),
        ] {
            let title = format!(
                "Conseil d'État, {}/07/2010, {number}",
                date.to_string().rsplit('-').next().unwrap()
            );
            by_number.insert(
                format!("CETAT|Decision|{number}"),
                Ok(vec![hit(Fond::Cetat, id, Some(&title))]),
            );
            corpus.get_decision.insert(id.to_owned(), Ok(decision(id, date, None, None)));
        }

        // CNIL (U06, U07) : datée et confirmée, sans date fiable, introuvable.
        let san = "CNILTEXT000053352643";
        let df = "CNILTEXT000035945338";
        by_number.insert(
            "CNIL|Deliberation|SAN-2026-002".to_owned(),
            Ok(vec![hit(Fond::Cnil, san, None)]),
        );
        by_number.insert(
            "CNIL|Deliberation|DF-2017-1448".to_owned(),
            Ok(vec![hit(Fond::Cnil, df, None)]),
        );
        by_number.insert("CNIL|Deliberation|SAN-2020-099".to_owned(), Ok(vec![]));
        corpus
            .get_deliberation
            .insert(san.to_owned(), Ok(deliberation(san, "SAN-2026-002", Some(civil(2026, 1, 8)))));
        corpus.get_deliberation.insert(df.to_owned(), Ok(deliberation(df, "DF-2017-1448", None)));

        // Cour de cassation (U12) : un pourvoi, une chambre sociale du 25 novembre 2015.
        for (number, id) in [
            ("14-24.444", "JURITEXT000031539701"),
            ("14-24.445", "JURITEXT000031539702"),
            ("14-24.446", "JURITEXT000031539703"),
            ("14-24.447", "JURITEXT000031539704"),
        ] {
            by_number.insert(format!("JURI|Case|{number}"), Ok(vec![hit(Fond::Juri, id, None)]));
            corpus.get_decision.insert(
                id.to_owned(),
                Ok(decision(id, civil(2015, 11, 25), None, Some("CHAMBRE_SOCIALE"))),
            );
        }
        corpus
    }

    // ---- les 50 citations et leurs verdicts ----------------------------------------------------

    /// Une citation du texte, son verdict exact et le nombre d'appels du port qu'elle provoque
    /// (une répétition en provoque 0 : elle partage la résolution de la première).
    struct Case {
        /// Ce qui précède la citation dans le texte sans en faire partie (tête illisible, décision
        /// citée à la suite) ; l'extrait rendu (`raw`) ne le contient pas.
        lead: &'static str,
        raw: &'static str,
        status: CheckStatus,
        note: String,
        calls: usize,
    }

    fn case(raw: &'static str, status: CheckStatus, note: impl Into<String>, calls: usize) -> Case {
        Case { lead: "", raw, status, note: note.into(), calls }
    }

    /// La même citation, précédée de `lead` dans le texte.
    fn led(lead: &'static str, case: Case) -> Case {
        Case { lead, ..case }
    }

    /// Une décision de 1966 du Conseil d'État citée sans numéro, juste avant une décision de 2010
    /// (U08) : le parseur ne lit que la seconde, comme une seule citation.
    const CE_1966: &str =
        "CE, Ass., 30 mars 1966, Compagnie générale d'énergie radio-électrique, p. 257 ; ";

    /// Note d'une version lue à une date passée.
    fn past_note(on: &str, etat: &str, ended: &str) -> String {
        format!(
            "Version in force on {on} (etat={etat}, ended {ended}); omit the date to read the \
             current text."
        )
    }

    const RETRY: &str = "The article may exist in an earlier or later version; retry with a date.";
    const TRY_OTHER: &str =
        "The article may exist in an earlier or later version; try another date.";

    /// Note d'absence d'un article (U01-N) : `hint` dit si une date a été donnée.
    fn absent_note(label: &str, num: &str, day: &str, hint: &str) -> String {
        format!("{label} has no article {num} in force on {day}. {hint}")
    }

    /// Note d'un article modificatif (U10). Le titre de la loi est raccourci par « … » : la note
    /// laisse la place à la mention de plage qui peut la suivre (`article_without_version_text`
    /// de `notes.rs`), si bien que le titre de 68 caractères perd ses 12 derniers.
    fn amending_note(day: &str, num: &str) -> String {
        format!(
            "No version in force on {day}: art. {num} of LOI n° 2016-1321 du 7 octobre 2016 pour \
             une Rép… amends other texts; read it with fr_get_text."
        )
    }

    const NOT_CONFIRMED_LODA: &str = "Could not confirm existence: nothing was found in LODA, \
         which does not prove that it does not exist.";
    const NOT_CONFIRMED_CETAT: &str = "Could not confirm existence: nothing was found in CETAT, \
         which does not prove that it does not exist.";
    const NOT_CONFIRMED_CNIL: &str = "Could not confirm existence: nothing was found in CNIL, \
         which does not prove that it does not exist.";
    const CAA_COVERAGE: &str = "Légifrance only holds a selection of CAA decisions; absence is not \
         proof (see fr_coverage).";
    const HEAD_NOT_READ: &str = "Only the case number was matched: the court, chamber or date \
         written before it was not read and not checked.";

    /// Les 50 citations, dans l'ordre du texte. Le décompte des appels (dernière colonne) donne
    /// `EXPECTED_CALLS` : `list_codes` (1, pour tout le texte) + 84 ci-dessous.
    #[expect(clippy::too_many_lines, reason = "une ligne par citation du texte")]
    fn cases() -> Vec<Case> {
        use CheckStatus::{Ambiguous, Invalid, NotFound, Unresolvable, Verified};
        let since = |day: &str| format!("Current version in force since {day}.");
        vec![
            // 1-3 : codes, à la date du jour, trouvés (1 `find_article`).
            case("article 1240 du code civil", Verified, since("2016-10-01"), 1),
            case("article 1241 du code civil", Verified, since("2016-10-01"), 1),
            case("article 1242 du code civil", Verified, since("2016-10-01"), 1),
            // 4-5 : code du travail.
            case("article L. 1234-5 du code du travail", Verified, since("2008-05-01"), 1),
            case("article L. 1237-19 du code du travail", Verified, since("2008-05-01"), 1),
            // 6-9 : codes, à une date passée, version de l'époque (U01) ; 9 : « 1er » puis « 1 ».
            case(
                "article 1382 du code civil en vigueur au 1er janvier 2015",
                Verified,
                past_note("2015-01-01", "MODIFIE", "2016-10-01"),
                1,
            ),
            case(
                "article 1383 du code civil en vigueur au 1er janvier 2015",
                Verified,
                past_note("2015-01-01", "MODIFIE", "2016-10-01"),
                1,
            ),
            case(
                "article L. 1221-1 du code du travail en vigueur au 1er janvier 2008",
                Verified,
                past_note("2008-01-01", "ABROGE", "2008-05-01"),
                1,
            ),
            case(
                "article 1er du code civil en vigueur au 1er janvier 1990",
                Verified,
                past_note("1990-01-01", "MODIFIE", "2016-02-10"),
                2,
            ),
            // 10-12 : codes, absents à la date citée (1 `find_article`, 0 sommaire pour un code).
            case(
                "article 9990 du code civil en vigueur au 1er janvier 1990",
                NotFound,
                absent_note("Code civil", "9990", "1990-01-01", TRY_OTHER),
                1,
            ),
            case(
                "article 9991 du code civil en vigueur au 1er janvier 1990",
                NotFound,
                absent_note("Code civil", "9991", "1990-01-01", TRY_OTHER),
                1,
            ),
            case(
                "article L. 9992-1 du code du travail en vigueur au 1er janvier 2008",
                NotFound,
                absent_note("Code du travail", "L9992-1", "2008-01-01", TRY_OTHER),
                1,
            ),
            // 13-14 : codes, absents du jour.
            case(
                "article 9993 du code civil",
                NotFound,
                absent_note("Code civil", "9993", TODAY, RETRY),
                1,
            ),
            case(
                "article L. 9994-1 du code du travail",
                NotFound,
                absent_note("Code du travail", "L9994-1", TODAY, RETRY),
                1,
            ),
            // 15 : l'amont échoue sur cette seule citation.
            case("article 1244 du code civil", Unresolvable, "Not checked: upstream error.", 1),
            // 16-26 : la loi (U10) et des lois inconnues. Chaque citation relit l'index du texte
            // (1 `find_by_number`) avant son article.
            // 16 : « 1er » (2 `find_article`).
            case(
                "article 1er de la loi n° 2016-1321 du 7 octobre 2016",
                Verified,
                since("2016-10-09"),
                1 + 2,
            ),
            // 17-18 : absents du sommaire. Le sommaire du jour est lu une fois pour tout le texte
            // (1 `get_act`, attribué à la citation 17 ; la lecture est partagée).
            case(
                "article 150 de la loi n° 2016-1321 du 7 octobre 2016",
                NotFound,
                absent_note(LAW_TITLE, "150", TODAY, RETRY),
                1 + 1 + 1,
            ),
            case(
                "article 151 de la loi n° 2016-1321 du 7 octobre 2016",
                NotFound,
                absent_note(LAW_TITLE, "151", TODAY, RETRY),
                1 + 1,
            ),
            // 19-21 : modificatifs (1 `get_article` chacun).
            case(
                "article 36 de la loi n° 2016-1321 du 7 octobre 2016",
                Verified,
                amending_note(TODAY, "36"),
                1 + 1 + 1,
            ),
            case(
                "article 37 de la loi n° 2016-1321 du 7 octobre 2016",
                Verified,
                amending_note(TODAY, "37"),
                1 + 1 + 1,
            ),
            case(
                "article 38 de la loi n° 2016-1321 du 7 octobre 2016",
                Verified,
                amending_note(TODAY, "38"),
                1 + 1 + 1,
            ),
            // 22 : daté du 2017-01-01 : pas de sommaire ce jour-là (1 `get_act`), repli sur celui
            // du jour (déjà lu, 0).
            case(
                "article 153 au 1er janvier 2017 de la loi n° 2016-1321 du 7 octobre 2016",
                NotFound,
                absent_note(LAW_TITLE, "153", "2017-01-01", TRY_OTHER),
                1 + 1 + 1,
            ),
            // 23 : daté du 2018-06-01 : sommaire de ce jour (1 `get_act`).
            case(
                "article 154 au 1er juin 2018 de la loi n° 2016-1321 du 7 octobre 2016",
                NotFound,
                absent_note(LAW_TITLE, "154", "2018-06-01", TRY_OTHER),
                1 + 1 + 1,
            ),
            // 24 : modificatif daté du 2017-01-01 (sommaires déjà lus, 1 `get_article`).
            case(
                "article 39 au 1er janvier 2017 de la loi n° 2016-1321 du 7 octobre 2016",
                Verified,
                amending_note("2017-01-01", "39"),
                1 + 1 + 1,
            ),
            // 25-26 : lois que l'index ne connaît pas.
            case(
                "article 5 de la loi n° 2015-9999 du 3 mars 2015",
                Unresolvable,
                NOT_CONFIRMED_LODA,
                1,
            ),
            case(
                "article 6 de la loi n° 2014-9998 du 4 avril 2014",
                Unresolvable,
                NOT_CONFIRMED_LODA,
                1,
            ),
            // 27-29 : CAA présentes (1 recherche, 1 consultation).
            case("CAA Marseille, 2 octobre 2026, n° 25MA02308", Verified, "", 2),
            case("CAA Marseille, 2 octobre 2026, n° 25MA02309", Verified, "", 2),
            case("CAA Marseille, 2 octobre 2026, n° 25MA02310", Verified, "", 2),
            // 30-31 : CAA absentes (1 recherche).
            case("CAA Lyon, 4 mars 2025, n° 24LY01111", Unresolvable, CAA_COVERAGE, 1),
            case("CAA Paris, 4 mars 2025, n° 24PA01112", Unresolvable, CAA_COVERAGE, 1),
            // 32 : siège et date en écart.
            case(
                "CAA Lyon, 3 octobre 2026, n° 25MA02311",
                Invalid,
                "Cited date 2026-10-03 differs from the official record (2026-10-02); seat Lyon \
                 differs (Marseille).",
                2,
            ),
            // 33-34 : requêtes jointes. 33 : le second numéro trouve (2 recherches exactes, 1
            // consultation). 34 : aucun n'est exact (2), le repli plein texte (1) trouve, 1
            // consultation.
            case(
                "CE, Ass., 5 mars 1999, Rouquette, nos 194658 et 196116",
                Verified,
                "Only one of the case numbers was checked.",
                3,
            ),
            case(
                "CE, Ass., 30 octobre 1998, Sarran, nos 200286 et 200287",
                Verified,
                "Only one of the case numbers was checked.",
                4,
            ),
            // 35 : trois numéros introuvables (3 recherches exactes + le repli). 36 : un seul
            // numéro (1 + le repli).
            case(
                "CE, 5 mars 1999, nos 410000, 410001 et 410002",
                Unresolvable,
                NOT_CONFIRMED_CETAT,
                4,
            ),
            case("CE, 5 mars 1999, n° 410100", Unresolvable, NOT_CONFIRMED_CETAT, 2),
            // 37-38 : U08, la seconde décision citée à la suite (1 recherche, 1 consultation).
            led(CE_1966, case("CE, 23 juillet 2010, Soc. Y, n° 328757", Verified, "", 2)),
            led(CE_1966, case("CE, 24 juillet 2010, Soc. Z, n° 328758", Verified, "", 2)),
            // 39-41 : CNIL.
            case("Délibération SAN-2026-002 du 8 janvier 2026", Verified, "", 2),
            case(
                "décision DF-2017-1448 du 5 avril 2018",
                Verified,
                "Date not checked: the record has no reliable date.",
                2,
            ),
            case("Délibération SAN-2020-099 du 1er mars 2020", Unresolvable, NOT_CONFIRMED_CNIL, 1),
            // 42-45 : Cour de cassation (1 recherche, 1 consultation).
            case(
                "Cass. civ. 1re, 25 novembre 2015, n° 14-24.444",
                Invalid,
                "Cited chamber CIV1 differs from the official record (SOC).",
                2,
            ),
            led("Cass. soc., 25/11/15, ", case("n° 14-24.445", Verified, HEAD_NOT_READ, 2)),
            case("Cass. soc. 25-11-2015, n° 14-24.446", Verified, "", 2),
            case(
                "Cass. soc., 25 novembre 2015, n° 14-24.447 et 14-24.448",
                Verified,
                "Only the first case number was checked.",
                2,
            ),
            // 46-48 : hors ligne.
            case(
                "Cass. soc., 12 mars 2031, n° 14-24.449",
                Invalid,
                "The year is in the future.",
                0,
            ),
            case(
                "art. L. 32 CPCE",
                Ambiguous,
                "'CPCE' is ambiguous (Code des postes et des communications électroniques, Code \
                 des procédures civiles d'exécution); name the code in full.",
                0,
            ),
            case(
                "directive 85/374/CEE du Conseil du 25 juillet 1985",
                Unresolvable,
                "EU law is not on Légifrance (see EUR-Lex / HUDOC).",
                0,
            ),
            // 49-50 : deux répétitions (les citations 1 et 33), sans appel.
            case("article 1240 du code civil", Verified, since("2016-10-01"), 0),
            case(
                "CE, Ass., 5 mars 1999, Rouquette, nos 194658 et 196116",
                Verified,
                "Only one of the case numbers was checked.",
                0,
            ),
        ]
    }

    /// Le texte soumis : les citations de `cases()`, chacune précédée de son `lead`, séparées par
    /// « ; ».
    fn text_of(cases: &[Case]) -> String {
        cases
            .iter()
            .map(|case| format!("{}{}", case.lead, case.raw))
            .collect::<Vec<_>>()
            .join(" ; ")
    }

    // ---- l'exécution ---------------------------------------------------------------------------

    /// Ce qu'un appel de l'outil a produit, avec les mesures prises autour.
    struct Observed {
        output: VerifyCitationsOutput,
        elapsed: Duration,
        corpus: Arc<ScriptedCorpus>,
        audit: Arc<MemoryAuditLog>,
    }

    /// Appelle `LegalService::verify_citations` sur `text` avec un corpus qui répond après `delay`.
    async fn call_tool(text: &str, delay: Duration) -> Observed {
        let corpus = Arc::new(world(delay));
        let audit = Arc::new(MemoryAuditLog::new());
        let settings = ServiceSettings {
            policy: AuditPolicy {
                mode: AuditMode::Strict,
                deadline: DEADLINE,
                env: PisteEnvironment::Production,
                server_version: "9.9.9",
            },
            configuration: ConfigurationState::default(),
            budget: ResponseBudget,
        };
        let clock = FixedClock::new(
            Utc.with_ymd_and_hms(2026, 10, 29, 8, 30, 0).unwrap(),
            civil(2026, 10, 29),
        );
        let service = LegalService::new(
            Shared(Arc::clone(&corpus)),
            SharedLog(Arc::clone(&audit)),
            clock,
            settings,
        );
        let input = VerifyCitationsInput { text: Some(text.to_owned()), max_citations: None };
        let call =
            ToolCall { input: Ok(input), canonical: CanonicalInput::new("{\"text\":\"…\"}") };
        let begin = Instant::now();
        let output = service.verify_citations(call, pending()).await.unwrap();
        Observed { output, elapsed: begin.elapsed(), corpus, audit }
    }

    fn note_of(check: &CitationCheck) -> String {
        check.note.as_ref().map(Note::text).unwrap_or_default()
    }

    /// Les compteurs de la réponse et la forme de chaque citation, quel qu'ait été le verdict.
    fn assert_shape(observed: &Observed, cases: &[Case]) {
        let output = &observed.output;
        assert_eq!(output.found, 50);
        assert_eq!(output.checked, 50);
        assert!(!output.truncated);
        assert_eq!(output.note, None, "ni article illisible, ni extrait détaché");
        assert_eq!(output.citations.len(), cases.len());
        for (rank, (check, case)) in output.citations.iter().zip(cases).enumerate() {
            assert_eq!(check.raw, case.raw, "citation {}", rank + 1);
        }
        // Les spans sont croissants et sans recouvrement (l'ordre du texte).
        for pair in output.citations.windows(2) {
            assert!(pair[0].span.end <= pair[1].span.start, "{pair:?}");
        }
        // Chaque note tient dans la borne de 200 caractères (R1-17).
        for check in &output.citations {
            let note = note_of(check);
            assert!(note.chars().count() <= NOTE_MAX_CHARS, "{}: {note}", check.raw);
        }
    }

    fn audited_once_ok(observed: &Observed) {
        let records = observed.audit.records();
        assert_eq!(records.len(), 1, "une seule ligne par appel");
        assert_eq!(records[0].tool, ToolName::FR_VERIFY_CITATIONS);
        assert_eq!(records[0].status, AuditStatus::Ok);
        assert_eq!(records[0].output_count_or_size, 50, "le nombre de citations contrôlées");
        assert_eq!(records[0].error_code, None);
    }

    // ---- les tests -----------------------------------------------------------------------------

    /// Le pire cas, sans lenteur : tous les verdicts, tous les appels, la durée.
    #[tokio::test(start_paused = true)]
    async fn fifty_mixed_citations_cost_the_exact_calls_and_every_verdict_is_expected() {
        let cases = cases();
        let text = text_of(&cases);
        assert_eq!(cases.len(), 50);
        let delay = Duration::from_millis(100);
        let observed = call_tool(&text, delay).await;
        assert_shape(&observed, &cases);

        // Les 50 verdicts, exacts.
        let verdicts: Vec<(CheckStatus, String)> =
            observed.output.citations.iter().map(|c| (c.status, note_of(c))).collect();
        let expected: Vec<(CheckStatus, String)> =
            cases.iter().map(|c| (c.status, c.note.clone())).collect();
        assert_eq!(verdicts, expected);
        let count = |wanted: CheckStatus| verdicts.iter().filter(|(s, _)| *s == wanted).count();
        assert_eq!(
            [
                count(CheckStatus::Verified),
                count(CheckStatus::NotFound),
                count(CheckStatus::Ambiguous),
                count(CheckStatus::Unresolvable),
                count(CheckStatus::Invalid),
            ],
            [28, 9, 1, 9, 3],
            "verified, not_found, ambiguous, unresolvable, invalid"
        );
        // Les répétitions rendent exactement le verdict de leur première occurrence.
        for (again, first) in REPEATS {
            let (first, again) =
                (&observed.output.citations[first], &observed.output.citations[again]);
            assert_eq!((first.status, note_of(first)), (again.status, note_of(again)));
            assert_eq!(first.resolved, again.resolved);
        }

        // Les appels du port : le total exact, méthode par méthode, et la somme des cases.
        let corpus = &observed.corpus;
        assert_eq!(corpus.unscripted(), Vec::<String>::new(), "tout appel a une réponse écrite");
        for (method, expected) in EXPECTED_CALLS {
            assert_eq!(corpus.calls_of(method), expected, "appels de {method}");
        }
        let total = corpus.calls().len();
        assert_eq!(total, EXPECTED_TOTAL);
        assert_eq!(EXPECTED_CALLS.iter().map(|(_, n)| n).sum::<usize>(), EXPECTED_TOTAL);
        assert_eq!(1 + cases.iter().map(|c| c.calls).sum::<usize>(), EXPECTED_TOTAL);
        // ... dans la borne documentée.
        assert!(
            total <= 1 + 50 * PORT_CALLS_PER_CITATION_MAX,
            "{total} appels pour 50 citations (borne {})",
            1 + 50 * PORT_CALLS_PER_CITATION_MAX
        );
        for case in &cases {
            assert!(case.calls <= PORT_CALLS_PER_CITATION_MAX, "{}", case.raw);
        }
        // Le sommaire d'une même loi à un même jour n'est lu qu'une fois (U10, mémo d'exécution) :
        // trois jours distincts, trois lectures, malgré onze citations de la loi.
        let mut acts: Vec<String> =
            corpus.calls().into_iter().filter(|c| c.method == "get_act").map(|c| c.key).collect();
        acts.sort();
        assert_eq!(
            acts,
            [format!("{LAW}|2017-01-01"), format!("{LAW}|2018-06-01"), format!("{LAW}|{TODAY}")]
        );
        // Aucun sommaire pour un code (U10, « jamais pour un code ») : tous ceux qui sont lus sont
        // ceux de la loi.
        assert!(
            corpus.calls().iter().filter(|c| c.method == "get_act").all(|c| c.key.starts_with(LAW))
        );

        // La concurrence : au plus quatre résolutions en vol, et la liste des codes se lit seule,
        // avant elles.
        assert!(corpus.max_concurrency() <= CONCURRENCY, "{}", corpus.max_concurrency());
        assert!(corpus.max_concurrency() > 1, "le test doit exercer la concurrence");
        let calls = corpus.calls();
        assert_eq!(calls[0].method, "list_codes");
        assert!(calls[1..].iter().all(|call| call.started >= delay));

        // La durée (virtuelle) : entre le travail réparti sur quatre voies et le travail en série,
        // et sous le budget de 80 % du délai de l'outil.
        let work = |n: usize| delay * u32::try_from(n).unwrap();
        assert!(
            observed.elapsed >= delay + work((EXPECTED_TOTAL - 1).div_ceil(CONCURRENCY)),
            "{:?}",
            observed.elapsed
        );
        assert!(observed.elapsed <= work(EXPECTED_TOTAL), "{:?}", observed.elapsed);
        assert!(observed.elapsed < BUDGET, "{:?}", observed.elapsed);

        audited_once_ok(&observed);
    }

    /// Chaque appel dure 4 s : 85 appels sur quatre voies ne tiennent pas dans le budget de 48 s
    /// (80 % de 60 s). Les citations que le budget coupe sont `unresolvable` avec leur raison, les
    /// autres gardent leur verdict exact, aucun appel ne part après le budget.
    #[tokio::test(start_paused = true)]
    async fn a_slow_upstream_hits_the_time_budget_and_degrades_the_unfinished_citations() {
        let cases = cases();
        let text = text_of(&cases);
        let delay = Duration::from_secs(4);
        let observed = call_tool(&text, delay).await;
        assert_shape(&observed, &cases);

        // Ni erreur d'échéance (la réponse est un succès), ni dépassement du budget.
        assert!(observed.elapsed <= BUDGET, "{:?}", observed.elapsed);
        assert!(observed.elapsed > BUDGET.saturating_sub(delay), "le budget a bien été atteint");
        audited_once_ok(&observed);

        // Verdict exact pour les citations finies, « time budget » pour les autres.
        let budget_note = "Not checked: time budget.";
        let mut cut = Vec::new();
        for (rank, (check, case)) in observed.output.citations.iter().zip(&cases).enumerate() {
            let note = note_of(check);
            if (check.status, note.as_str()) == (case.status, case.note.as_str()) {
                continue;
            }
            assert_eq!(
                (check.status, note.as_str()),
                (CheckStatus::Unresolvable, budget_note),
                "citation {} : ni son verdict ni la coupure du budget",
                rank + 1
            );
            assert_eq!(check.resolved, None);
            assert_eq!(check.quote, None);
            cut.push(rank);
        }
        assert!(!cut.is_empty(), "le budget doit couper des citations");
        assert!(cut.len() < cases.len(), "le budget ne coupe pas tout");
        // Une citation qui n'appelle pas le corpus n'est jamais coupée (sauf une répétition, qui
        // suit le sort de sa première occurrence, vu plus bas).
        for &rank in cut.iter().filter(|rank| REPEATS.iter().all(|(again, _)| again != *rank)) {
            assert!(cases[rank].calls > 0, "citation {} ne coûte aucun appel", rank + 1);
        }
        // La première citation (1 appel, partie dès la fin de la liste des codes) est finie.
        assert!(!cut.contains(&0));
        // Une répétition partage le sort de sa première occurrence.
        for (again, first) in REPEATS {
            assert_eq!(cut.contains(&again), cut.contains(&first));
        }

        // Aucun appel ne part après le budget, et chaque voie en fait au plus budget / délai.
        let corpus = &observed.corpus;
        assert_eq!(corpus.unscripted(), Vec::<String>::new());
        let calls = corpus.calls();
        // (un appel qui part à l'instant exact du budget est abandonné aussitôt : `timeout_at`
        // sonde d'abord le futur, puis l'échéance.)
        assert!(calls.iter().all(|call| call.started <= BUDGET), "{calls:?}");
        let per_lane = usize::try_from(BUDGET.as_secs() / delay.as_secs()).unwrap();
        assert!(
            calls.len() < EXPECTED_TOTAL && calls.len() <= 1 + CONCURRENCY * per_lane,
            "{} appels",
            calls.len()
        );
        assert!(corpus.max_concurrency() <= CONCURRENCY);
    }

    /// Au-delà de `max_citations` (50 par défaut) : `found` compte tout, `checked` s'arrête à 50,
    /// `truncated` le dit, et les citations en trop ne coûtent aucun appel.
    #[tokio::test(start_paused = true)]
    async fn fifty_five_citations_are_cut_at_fifty_without_extra_calls() {
        let cases = cases();
        let extra =
            ["2001", "2002", "2003", "2004", "2005"].map(|n| format!("article {n} du code civil"));
        let text = format!("{} ; {}", text_of(&cases), extra.join(" ; "));
        let observed = call_tool(&text, Duration::from_millis(100)).await;

        let output = &observed.output;
        assert_eq!((output.found, output.checked, output.truncated), (55, 50, true));
        assert_eq!(output.note, None);
        let verdicts: Vec<(CheckStatus, String)> =
            output.citations.iter().map(|c| (c.status, note_of(c))).collect();
        let expected: Vec<(CheckStatus, String)> =
            cases.iter().map(|c| (c.status, c.note.clone())).collect();
        assert_eq!(verdicts, expected);
        // Les mêmes 85 appels : les cinq articles en trop n'ont rien coûté.
        assert_eq!(observed.corpus.calls().len(), EXPECTED_TOTAL);
        assert_eq!(observed.corpus.unscripted(), Vec::<String>::new());
        assert!(
            observed.corpus.calls().iter().all(|call| ["2001", "2002", "2003", "2004", "2005"]
                .iter()
                .all(|n| !call.key.starts_with(&format!("{CIVIL}|{n}|")))),
            "aucun des articles 2001 à 2005 n'est cherché"
        );
        let records = observed.audit.records();
        assert_eq!(records[0].output_count_or_size, 50);
    }
}
