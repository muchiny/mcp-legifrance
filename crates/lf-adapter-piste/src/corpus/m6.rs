//! Opérations de M6 de `PisteCorpus` : `find_article`, `find_by_number` et `list_codes`
//! (04 §7.1 et §7.2, annexe B §6.6). `corpus::port` y délègue.
//!
//! - **`find_article`** : [`ArticleAt::Today`] interroge `getArticleWithIdAndNum`, qui ne rend que
//!   la version en vigueur ; sur `NotFound` (article sans version en vigueur, ou à version
//!   différée), comme pour toute autre date, la recherche exacte `CODE_DATE` sur `NUM_ARTICLE` et
//!   `DATE_VERSION` prend le relais, et l'article trouvé est lu par `getArticle`. L'application a
//!   déjà résolu le code en `LEGITEXT` par `list_codes` (D5) : l'adapter ne voit que le texte.
//! - **`find_by_number`** : `/search` en `EXACTE` pour les numéros d'article, de texte, d'affaire,
//!   de décision, de délibération et l'IDCC des accords d'entreprise ; la facette `ECLI` pour
//!   JURI et CETAT ; trois consultations (NOR, IDCC des conventions, ELI cité) qui rendent au plus
//!   un hit et dont l'absence est une liste vide (l'IDCC retombe sur la recherche si la
//!   consultation échoue sur un identifiant inconnu : l'API répond alors 500).
//! - **`list_codes`** : `/list/code` paginé par 100, chargé paresseusement dans la table unique
//!   (`CodeTable`, 7 jours), sans doublon ; un parcours dont les pages se recoupent est repris, au
//!   plus trois fois, et les parcours s'ajoutent (pagination instable de la sandbox).
//!
//! Le corps d'une recherche est construit, et sa requête de cadrage assemblée, **avant** tout
//! appel : une garde défensive franchie ne coûte ni requête ni jeton (`InvalidRequest`).

use std::collections::HashSet;
use std::num::NonZeroU32;
use std::pin::pin;

use lf_application::ports::{ArticleAt, CorpusError, LegalCorpus};
use lf_domain::{
    date::VersionDate,
    entity::{ArticleText, CodeListing, SearchHit, SearchPage},
    fond::Fond,
    id::LegiTextId,
    search::{NumberKind, NumberQuery, NumberValue, has_more},
};

use super::PisteCorpus;
use crate::classify::EndpointKind;
use crate::deadline::CallDeadline;
use crate::dto::article::GetArticleResponseDto;
use crate::dto::m6::{CodeListResponseDto, ConsultJorfDto, KaliContDto};
use crate::map::{endpoint, m6 as map, not_found};
use crate::request::m6 as request;
use crate::time::{MonoClock, Sleeper};

/// Nombre maximal de pages de la recherche d'un article par numéro : une page de 100 codes en
/// couvre déjà la quasi-totalité (76 codes en vigueur, 108 en tout).
const MAX_ARTICLE_PAGES: u32 = 3;

/// Nombre maximal de pages de `/list/code` (04 §7.3) : 1 000 codes, dix fois ce qui existe.
const MAX_CODE_PAGES: u32 = 10;

/// Nombre maximal de parcours de `/list/code` quand les pages d'un parcours se recoupent : trois
/// parcours de deux pages tiennent en six requêtes, soit six secondes au quota de la sandbox.
const MAX_CODE_PASSES: u32 = 3;

impl<K: MonoClock + Clone, S: Sleeper + Clone> PisteCorpus<K, S> {
    /// Article d'un texte par son numéro : `getArticleWithIdAndNum` pour [`ArticleAt::Today`],
    /// recherche exacte sur `NUM_ARTICLE` et `DATE_VERSION` pour [`ArticleAt::On`] et en repli de
    /// `Today` (04 §7.2 étapes 2 et 3).
    ///
    /// # Errors
    ///
    /// `InvalidRequest { status: None }` si `num` est vide ou hors du jeu de caractères d'un
    /// numéro (sans appel réseau) ; [`CorpusError::NotFound`] si aucune version ne porte ce
    /// numéro à cette date ; sinon les erreurs de `fetch` et de `getArticle`.
    pub(super) async fn find_article_impl(
        &self,
        text: &LegiTextId,
        num: &str,
        at: ArticleAt,
    ) -> Result<ArticleText, CorpusError> {
        let number = NumberValue::parse(num).map_err(|_| request::invalid_article_number())?;
        if matches!(at, ArticleAt::Today(_)) {
            let current = self.article_by_number(text, &number).await;
            if !matches!(current, Err(CorpusError::NotFound { .. })) {
                return current;
            }
        }
        self.article_on(text, number, at.date()).await
    }

    /// `getArticleWithIdAndNum` : la version en vigueur de l'article `number` du texte `text`.
    async fn article_by_number(
        &self,
        text: &LegiTextId,
        number: &NumberValue,
    ) -> Result<ArticleText, CorpusError> {
        let endpoint = request::GET_ARTICLE_WITH_ID_AND_NUM;
        let body = request::article_by_number_body(text, number.as_str());
        self.fetch(EndpointKind::Consult, endpoint, &body, |dto: GetArticleResponseDto, diags| {
            map::found_article(dto, endpoint, diags)
        })
        .await
    }

    /// Recherche exacte de l'article `number` à la date `date` dans les codes, puis lecture de la
    /// version trouvée pour `text` (04 §7.2 étape 3).
    async fn article_on(
        &self,
        text: &LegiTextId,
        number: NumberValue,
        date: VersionDate,
    ) -> Result<ArticleText, CorpusError> {
        let num = number.as_str().to_owned();
        let query = NumberQuery {
            fond: Fond::Code,
            kind: NumberKind::Article,
            value: number,
            within: None,
            version_date: Some(date),
        };
        let mut page = NonZeroU32::MIN;
        loop {
            let (found, more) = self.search_numbers(&query, page).await?;
            if let Some(id) = map::article_of_text(&found, text, &num) {
                return self.get_article(&id).await;
            }
            if !more || page.get() >= MAX_ARTICLE_PAGES {
                return Err(not_found(endpoint::SEARCH));
            }
            page = page.saturating_add(1);
        }
    }

    /// Une page de `/search` pour `query` et s'il en reste une suivante.
    ///
    /// # Errors
    ///
    /// `InvalidRequest { status: None }` d'une garde défensive, avant tout appel ; sinon celles de
    /// `fetch` et du mapping de la page.
    async fn search_numbers(
        &self,
        query: &NumberQuery,
        page: NonZeroU32,
    ) -> Result<(SearchPage, bool), CorpusError> {
        let body = request::number_search_body(query, page.get())?;
        let frame = request::result_frame(query)?;
        let size = frame.page_size();
        let found = self
            .fetch(EndpointKind::Search, endpoint::SEARCH, &body, |dto, diags| {
                map::number_page(dto, &frame, diags)
            })
            .await?;
        let more = has_more(page, size, found.total);
        Ok((found, more))
    }

    /// Recherche par numéro (affaire, décision, délibération, IDCC, NOR, ECLI, ELI, numéro
    /// d'article ou de texte).
    ///
    /// # Errors
    ///
    /// `InvalidRequest { status: None }` si le couple (fond, nature) est hors de l'annexe B §6.6
    /// ou si la requête est incohérente (sans appel réseau) ; sinon les erreurs de `fetch` et du
    /// mapping. Un NOR, un ELI ou un IDCC inconnu rend une liste vide, pas `NotFound`.
    pub(super) async fn find_by_number_impl(
        &self,
        query: &NumberQuery,
    ) -> Result<Vec<SearchHit>, CorpusError> {
        request::ensure_applicable(query)?;
        match query.kind {
            NumberKind::Nor => self.hits_of_nor(&query.value).await,
            NumberKind::Eli => self.hits_of_eli(&query.value).await,
            NumberKind::Idcc if query.fond == Fond::Kali => self.hits_of_idcc(query).await,
            _ => self.search_hits(query).await,
        }
    }

    /// Les hits de la première page de `/search` pour `query`.
    async fn search_hits(&self, query: &NumberQuery) -> Result<Vec<SearchHit>, CorpusError> {
        let (page, _more) = self.search_numbers(query, NonZeroU32::MIN).await?;
        Ok(page.hits)
    }

    /// `getJoWithNor` : au plus un hit LODA.
    async fn hits_of_nor(&self, nor: &NumberValue) -> Result<Vec<SearchHit>, CorpusError> {
        let hits = self
            .fetch(
                EndpointKind::Consult,
                request::GET_JO_WITH_NOR,
                &request::nor_body(nor),
                |dto: ConsultJorfDto, diags| Ok(map::jorf_hits(&dto, diags)),
            )
            .await;
        no_hit_when_unknown(hits)
    }

    /// L'endpoint ELI de la nature du chemin cité : au plus un hit LODA.
    async fn hits_of_eli(&self, eli: &NumberValue) -> Result<Vec<SearchHit>, CorpusError> {
        let target = request::EliTarget::of(eli);
        let (endpoint, body) = (target.endpoint(), request::eli_body(eli));
        let hits = match target {
            request::EliTarget::Article => {
                self.fetch(
                    EndpointKind::Consult,
                    endpoint,
                    &body,
                    |dto: GetArticleResponseDto, diags| {
                        map::found_article(dto, endpoint, diags).map(map::eli_article_hits)
                    },
                )
                .await
            }
            request::EliTarget::Text => {
                self.fetch(EndpointKind::Consult, endpoint, &body, |dto: ConsultJorfDto, diags| {
                    Ok(map::jorf_hits(&dto, diags))
                })
                .await
            }
        };
        no_hit_when_unknown(hits)
    }

    /// `kaliContIdcc` : au plus un hit KALI. Sur un IDCC inconnu (l'API répond 500, relevé en
    /// sandbox), sur une réponse sans texte de base ou sur un `NotFound`, `/search` KALI `IDCC`
    /// prend le relais (04 §7.2).
    async fn hits_of_idcc(&self, query: &NumberQuery) -> Result<Vec<SearchHit>, CorpusError> {
        let consulted = self
            .fetch(
                EndpointKind::Consult,
                request::KALI_CONT_IDCC,
                &request::idcc_body(&query.value),
                |dto: KaliContDto, diags| Ok(map::kali_cont_hits(&dto, diags)),
            )
            .await;
        match consulted {
            Ok(hits) if !hits.is_empty() => Ok(hits),
            Ok(_)
            | Err(CorpusError::NotFound { .. } | CorpusError::Upstream { status: Some(500), .. }) => {
                self.search_hits(query).await
            }
            Err(other) => Err(other),
        }
    }

    /// Liste des codes (`/list/code`), abrogés compris, d'après la table des codes : chargée au
    /// premier appel, jamais avant, puis gardée 7 jours (04 §12.3).
    ///
    /// # Errors
    ///
    /// [`CorpusError::Timeout`] si l'attente derrière un chargement en cours dépasse l'échéance ;
    /// sinon l'erreur du chargement quand aucune table de moins de 14 jours ne peut la remplacer.
    pub(super) async fn list_codes_impl(&self) -> Result<Vec<CodeListing>, CorpusError> {
        let inner = &*self.inner;
        let deadline =
            CallDeadline::from_task_local(&inner.clock, inner.config.tuning().tool_deadline);
        let mut wait = pin!(inner.sleeper.sleep(deadline.remaining(&inner.clock)));
        let table = inner.codes.get_or_load(wait.as_mut(), || self.load_codes()).await?;
        Ok(table.to_vec())
    }

    /// Table des codes de `/list/code`, sans doublon, dans l'ordre où les codes arrivent.
    ///
    /// La pagination de la sandbox n'est pas stable d'un appel à l'autre (mesure du 2026-10-29,
    /// F2 : deux pages de 100 et de 8 pour 108 codes ne rendaient que 104 codes distincts, le Code
    /// civil manquait, et `sort: TITLE_ASC` n'y ordonne rien). Un parcours dont une page répète un
    /// code déjà reçu est donc incomplet : l'union des parcours le complète, au plus
    /// [`MAX_CODE_PASSES`] fois. Un parcours sans répétition qui atteint le total annoncé a toutes
    /// ses entrées distinctes, il est complet et rien n'est rejoué.
    ///
    /// # Errors
    ///
    /// Celles de `fetch` : une page qui échoue fait échouer le chargement, rien n'est gardé.
    async fn load_codes(&self) -> Result<Vec<CodeListing>, CorpusError> {
        let mut codes = Vec::new();
        for _ in 0..MAX_CODE_PASSES {
            if !self.load_code_pass(&mut codes).await? {
                return Ok(codes);
            }
        }
        tracing::warn!(
            passes = MAX_CODE_PASSES,
            "list/code pages keep repeating codes: serving the union of the traversals"
        );
        Ok(codes)
    }

    /// Un parcours de `/list/code` jusqu'à `totalResultNumber` (au plus [`MAX_CODE_PAGES`]
    /// pages) : ajoute à `codes` les codes qu'il ne contient pas encore. Rend `true` si une page
    /// de ce parcours a répété un code déjà reçu pendant ce même parcours.
    ///
    /// # Errors
    ///
    /// Celles de `fetch`.
    async fn load_code_pass(&self, codes: &mut Vec<CodeListing>) -> Result<bool, CorpusError> {
        let mut in_pass = HashSet::new();
        let mut repeated = false;
        let mut received = 0_usize;
        for page in 1..=MAX_CODE_PAGES {
            let loaded = self
                .fetch(
                    EndpointKind::Search,
                    request::LIST_CODE,
                    &request::list_code_body(page),
                    |dto: CodeListResponseDto, diags| Ok(map::code_page(&dto, diags)),
                )
                .await?;
            received = received.saturating_add(loaded.received);
            for listing in loaded.listings {
                repeated |= !in_pass.insert(listing.id.clone());
                if !codes.iter().any(|known| known.id == listing.id) {
                    codes.push(listing);
                }
            }
            let full = loaded.received >= request::CODE_PAGE_SIZE as usize;
            let more = loaded.total.is_none_or(|total| (received as u64) < total);
            if !(full && more) {
                break;
            }
        }
        Ok(repeated)
    }
}

/// Un identifiant inconnu d'une consultation n'est pas une erreur de recherche : aucun hit.
fn no_hit_when_unknown(
    result: Result<Vec<SearchHit>, CorpusError>,
) -> Result<Vec<SearchHit>, CorpusError> {
    match result {
        Err(CorpusError::NotFound { .. }) => Ok(Vec::new()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use lf_application::ports::LegalCorpus;
    use lf_domain::date::{DateRange, VersionDate};
    use lf_domain::status::LegalStatus;
    use serde_json::{Value, json};
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, ResponseTemplate};

    use super::*;
    use crate::corpus::tests::{API_BASE, Rig, json_reply, never, requests_to, token_mock};

    const PAGE_1: &str =
        include_str!("../../tests/fixtures/recorded/list/code/m6/list-code-page1.json");
    const PAGE_2: &str =
        include_str!("../../tests/fixtures/recorded/list/code/m6/list-code-page2.json");
    const CIVIL_9: &str = include_str!(
        "../../tests/fixtures/recorded/consult/getArticleWithIdAndNum/m6/getArticleWithIdAndNum-code-civil-9.json"
    );
    const UNKNOWN_NUMBER: &str = include_str!(
        "../../tests/fixtures/recorded/consult/getArticleWithIdAndNum/m6/getArticleWithIdAndNum-numero-inconnu.json"
    );
    const EXACT_9: &str =
        include_str!("../../tests/fixtures/recorded/search/m6/search-exacte-num-article-9.json");
    const ECLI_JURI: &str = include_str!(
        "../../tests/fixtures/recorded/search/m6/search-ecli-JURI-ECLI-FR-CCASS-2025-C100399.json"
    );
    const ECLI_UNKNOWN: &str =
        include_str!("../../tests/fixtures/recorded/search/m6/search-ecli-JURI-inconnu.json");
    const IDCC_2121: &str = include_str!(
        "../../tests/fixtures/recorded/consult/kaliContIdcc/m6/kaliContIdcc-2121.json"
    );
    const IDCC_UNKNOWN: &str = include_str!(
        "../../tests/fixtures/recorded/consult/kaliContIdcc/m6/kaliContIdcc-inconnu.json"
    );
    const NOR: &str = include_str!(
        "../../tests/fixtures/recorded/consult/getJoWithNor/m6/getJoWithNor-AGRX2502679L.json"
    );
    const NOR_UNKNOWN: &str = include_str!(
        "../../tests/fixtures/recorded/consult/getJoWithNor/m6/getJoWithNor-inconnu.json"
    );

    /// Midi Europe/Paris du 28 octobre 2026, en millisecondes.
    const NOON_2026_10_28: i64 = 1_793_185_200_000;

    fn route(endpoint: &str) -> String {
        format!("{API_BASE}{endpoint}")
    }

    /// Nom de la variante d'une erreur, sans son détail.
    fn variant(error: &CorpusError) -> String {
        let shown = format!("{error:?}");
        shown.split([' ', '{', '(']).next().unwrap_or_default().to_owned()
    }

    fn day(text: &str) -> VersionDate {
        VersionDate::parse(text).unwrap()
    }

    fn civil_code() -> LegiTextId {
        LegiTextId::parse("LEGITEXT000006070721").unwrap()
    }

    fn value(raw: &str) -> NumberValue {
        NumberValue::parse(raw).unwrap()
    }

    fn query(fond: Fond, kind: NumberKind, raw: &str) -> NumberQuery {
        NumberQuery::new(fond, kind, value(raw), day("2026-10-28")).unwrap()
    }

    /// `body` à `endpoint` pour la seule requête `request`, `times` fois exactement.
    async fn serve(
        rig: &Rig,
        endpoint: &str,
        request: &Value,
        reply: ResponseTemplate,
        times: u64,
    ) {
        Mock::given(method("POST"))
            .and(path(route(endpoint)))
            .and(body_json(request))
            .respond_with(reply)
            .up_to_n_times(times)
            .expect(times)
            .mount(&rig.server)
            .await;
    }

    /// Corps de toutes les requêtes reçues sur `endpoint`.
    async fn bodies(rig: &Rig, endpoint: &str) -> Vec<Value> {
        let route = route(endpoint);
        let requests = rig.server.received_requests().await.unwrap();
        requests
            .iter()
            .filter(|request| request.url.path() == route)
            .map(|request| serde_json::from_slice(&request.body).unwrap())
            .collect()
    }

    /// Requête `/search` attendue pour la recherche exacte de l'article `num` à `date`.
    fn exact_article_request(num: &str, page: u32) -> Value {
        json!({
            "fond": "CODE_DATE",
            "recherche": {
                "champs": [{
                    "typeChamp": "NUM_ARTICLE",
                    "criteres": [{"typeRecherche": "EXACTE", "valeur": num, "operateur": "ET"}],
                    "operateur": "ET",
                }],
                "filtres": [{"facette": "DATE_VERSION", "singleDate": NOON_2026_10_28}],
                "pageNumber": page,
                "pageSize": 100,
                "operateur": "ET",
                "sort": "PERTINENCE",
                "typePagination": "ARTICLE",
            },
        })
    }

    // ---- find_article ----------------------------------------------------------------------

    #[tokio::test]
    async fn find_article_today_uses_get_article_with_id_and_num() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let request = json!({"id": "LEGITEXT000006070721", "num": "9"});
        serve(&rig, "/consult/getArticleWithIdAndNum", &request, json_reply(200, CIVIL_9), 1).await;
        never(&rig.server, &route("/search")).await;
        never(&rig.server, &route("/consult/getArticle")).await;

        let found = rig
            .corpus
            .find_article(&civil_code(), "9", ArticleAt::Today(day("2026-10-28")))
            .await
            .unwrap();

        assert_eq!(found.article_id.as_str(), "LEGIARTI000006419288");
        assert_eq!(found.num.as_deref(), Some("9"));
        assert_eq!(found.etat, Some(LegalStatus::Vigueur));
        assert_eq!(found.code_title.as_deref(), Some("Code civil"));
        assert!(found.text.as_str().starts_with("Chacun a droit au respect de sa vie privée."));
        // Le chemin ne contient ni identifiant ni numéro : tout va dans le corps (04 §7.1).
        assert_eq!(rig.corpus.stats().id_mismatch, 0);
    }

    #[tokio::test]
    async fn find_article_on_date_uses_search_exacte() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        serve(&rig, "/search", &exact_article_request("9", 1), json_reply(200, EXACT_9), 1).await;
        // Le corps de `getArticle` est celui de `getArticleWithIdAndNum` (même enveloppe).
        let by_id = json!({"id": "LEGIARTI000006419288"});
        serve(&rig, "/consult/getArticle", &by_id, json_reply(200, CIVIL_9), 1).await;
        never(&rig.server, &route("/consult/getArticleWithIdAndNum")).await;

        let found = rig
            .corpus
            .find_article(&civil_code(), "9", ArticleAt::On(day("2026-10-28")))
            .await
            .unwrap();

        assert_eq!(found.article_id.as_str(), "LEGIARTI000006419288");
        assert_eq!(found.code_title.as_deref(), Some("Code civil"));
        let search = bodies(&rig, "/search").await;
        assert_eq!(search.len(), 1);
        let champ = &search[0]["recherche"]["champs"][0];
        assert_eq!(champ["typeChamp"], "NUM_ARTICLE");
        assert_eq!(champ["criteres"][0]["typeRecherche"], "EXACTE");
        assert_eq!(search[0]["recherche"]["filtres"][0]["facette"], "DATE_VERSION");
        assert_eq!(search[0]["recherche"]["typePagination"], "ARTICLE");
    }

    #[tokio::test]
    async fn find_article_picks_the_requested_code_among_those_that_carry_the_number() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        serve(&rig, "/search", &exact_article_request("9", 1), json_reply(200, EXACT_9), 1).await;
        // Code de procédure civile (LEGITEXT000006070716) : son article 9.
        let by_id = json!({"id": "LEGIARTI000006410102"});
        serve(&rig, "/consult/getArticle", &by_id, json_reply(200, CIVIL_9), 1).await;

        let procedure = LegiTextId::parse("LEGITEXT000006070716").unwrap();
        let found =
            rig.corpus.find_article(&procedure, "9", ArticleAt::On(day("2026-10-28"))).await;

        // Le corps servi pour `getArticle` est celui de l'article du Code civil : la discordance
        // d'identifiant est comptée, c'est la preuve que l'identifiant demandé est bien celui du
        // Code de procédure civile.
        assert!(found.is_ok());
        assert_eq!(rig.corpus.stats().id_mismatch, 1);
    }

    #[tokio::test]
    async fn find_article_today_falls_back_to_the_search_when_the_article_is_not_in_force() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let by_number = json!({"id": "LEGITEXT000006070721", "num": "9"});
        serve(
            &rig,
            "/consult/getArticleWithIdAndNum",
            &by_number,
            json_reply(200, UNKNOWN_NUMBER),
            1,
        )
        .await;
        serve(&rig, "/search", &exact_article_request("9", 1), json_reply(200, EXACT_9), 1).await;
        let by_id = json!({"id": "LEGIARTI000006419288"});
        serve(&rig, "/consult/getArticle", &by_id, json_reply(200, CIVIL_9), 1).await;

        let found = rig
            .corpus
            .find_article(&civil_code(), "9", ArticleAt::Today(day("2026-10-28")))
            .await
            .unwrap();
        assert_eq!(found.article_id.as_str(), "LEGIARTI000006419288");
    }

    #[tokio::test]
    async fn find_article_unknown_everywhere_is_not_found() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let by_number = json!({"id": "LEGITEXT000006070721", "num": "99999"});
        serve(
            &rig,
            "/consult/getArticleWithIdAndNum",
            &by_number,
            json_reply(200, UNKNOWN_NUMBER),
            1,
        )
        .await;
        serve(&rig, "/search", &exact_article_request("99999", 1), json_reply(200, EXACT_9), 1)
            .await;
        never(&rig.server, &route("/consult/getArticle")).await;

        let result = rig
            .corpus
            .find_article(&civil_code(), "99999", ArticleAt::Today(day("2026-10-28")))
            .await;
        assert_eq!(result.unwrap_err(), CorpusError::NotFound { what: "/search".to_owned() });
    }

    #[tokio::test]
    async fn find_article_on_a_date_reports_a_failed_search() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        serve(&rig, "/search", &exact_article_request("9", 1), json_reply(200, "{"), 1).await;
        never(&rig.server, &route("/consult/getArticle")).await;

        let error = rig
            .corpus
            .find_article(&civil_code(), "9", ArticleAt::On(day("2026-10-28")))
            .await
            .unwrap_err();
        assert_eq!(variant(&error), "Decode");
    }

    #[tokio::test]
    async fn find_article_today_does_not_search_after_another_error() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let by_number = json!({"id": "LEGITEXT000006070721", "num": "9"});
        serve(&rig, "/consult/getArticleWithIdAndNum", &by_number, json_reply(200, "{"), 1).await;
        never(&rig.server, &route("/search")).await;

        let error = rig
            .corpus
            .find_article(&civil_code(), "9", ArticleAt::Today(day("2026-10-28")))
            .await
            .unwrap_err();
        assert_eq!(variant(&error), "Decode");
    }

    /// Une page de recherche de `count` codes porteurs de l'article `num`, avec `total` résultats.
    fn search_page(first_code: u64, count: u64, num: &str, total: u64) -> String {
        let results: Vec<Value> = (first_code..first_code + count)
            .map(|n| {
                let code = format!("LEGITEXT{n:012}");
                json!({
                    "titles": [{"id": code, "cid": code, "title": format!("Code {n}")}],
                    "nature": "CODE",
                    "sections": [{"extracts": [{
                        "id": format!("LEGIARTI{n:012}"), "num": num, "values": ["texte"],
                    }]}],
                })
            })
            .collect();
        json!({"totalResultNumber": total, "typePagination": "ARTICLE", "results": results})
            .to_string()
    }

    #[tokio::test]
    async fn find_article_reads_the_next_page_when_the_code_is_not_on_the_first() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let first = search_page(1_000_000, 100, "9", 150);
        let second = search_page(2_000_000, 50, "9", 150);
        serve(&rig, "/search", &exact_article_request("9", 1), json_reply(200, &first), 1).await;
        serve(&rig, "/search", &exact_article_request("9", 2), json_reply(200, &second), 1).await;
        let by_id = json!({"id": "LEGIARTI000002000010"});
        serve(&rig, "/consult/getArticle", &by_id, json_reply(200, CIVIL_9), 1).await;

        let wanted = LegiTextId::parse("LEGITEXT000002000010").unwrap();
        let found = rig.corpus.find_article(&wanted, "9", ArticleAt::On(day("2026-10-28"))).await;
        assert!(found.is_ok(), "{found:?}");
        assert_eq!(bodies(&rig, "/search").await.len(), 2);
    }

    #[tokio::test]
    async fn find_article_stops_after_three_pages() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        for page in 1..=3 {
            let reply = search_page(1_000_000 + 100 * u64::from(page), 100, "9", 1_000);
            serve(&rig, "/search", &exact_article_request("9", page), json_reply(200, &reply), 1)
                .await;
        }
        never(&rig.server, &route("/consult/getArticle")).await;

        let result =
            rig.corpus.find_article(&civil_code(), "9", ArticleAt::On(day("2026-10-28"))).await;
        assert_eq!(result.unwrap_err(), CorpusError::NotFound { what: "/search".to_owned() });
        assert_eq!(bodies(&rig, "/search").await.len(), 3);
    }

    #[tokio::test]
    async fn find_article_refuses_a_bad_number_before_any_request() {
        let rig = Rig::new().await;
        let too_long = "9".repeat(129);
        for num in ["", "L. 1242–2", "9\n", too_long.as_str()] {
            let error = rig
                .corpus
                .find_article(&civil_code(), num, ArticleAt::Today(day("2026-10-28")))
                .await
                .unwrap_err();
            assert_eq!(error, request::invalid_article_number(), "{num:?}");
        }
        assert!(rig.server.received_requests().await.unwrap().is_empty());
    }

    // ---- find_by_number : /search -----------------------------------------------------------

    #[tokio::test]
    async fn find_by_number_couples_of_annex_b_6_6() {
        // (fond, nature, valeur, fond de l'API, typeChamp attendu, typePagination, taille)
        let table = [
            (Fond::Code, NumberKind::Article, "9", "CODE_DATE", "NUM_ARTICLE", "ARTICLE", 100),
            (Fond::Loda, NumberKind::Article, "9", "LODA_DATE", "NUM_ARTICLE", "ARTICLE", 100),
            (Fond::Loda, NumberKind::Text, "2016-1321", "LODA_DATE", "NUM", "DEFAUT", 20),
            (Fond::Juri, NumberKind::Case, "24-13.470", "JURI", "NUM_AFFAIRE", "DEFAUT", 20),
            (
                Fond::Constit,
                NumberKind::Decision,
                "2025-1173 QPC",
                "CONSTIT",
                "NUM_DEC",
                "DEFAUT",
                20,
            ),
            (Fond::Cetat, NumberKind::Decision, "506507", "CETAT", "NUM_DEC", "DEFAUT", 20),
            (
                Fond::Cnil,
                NumberKind::Deliberation,
                "SAN-2026-002",
                "CNIL",
                "NUM_DELIB",
                "DEFAUT",
                20,
            ),
            (Fond::Acco, NumberKind::Idcc, "2121", "ACCO", "IDCC", "DEFAUT", 20),
        ];
        for (fond, kind, raw, api, field, pagination, size) in table {
            let rig = Rig::new().await;
            token_mock(&rig.server, "tok-1", 1).await;
            Mock::given(method("POST"))
                .and(path(route("/search")))
                .respond_with(json_reply(200, r#"{"totalResultNumber":0,"results":[]}"#))
                .expect(1)
                .mount(&rig.server)
                .await;

            let hits = rig.corpus.find_by_number(&query(fond, kind, raw)).await.unwrap();

            assert_eq!(hits, Vec::new(), "{fond:?} {kind:?}");
            let all = bodies(&rig, "/search").await;
            let sent = &all[0];
            let search = &sent["recherche"];
            let criterion = &search["champs"][0]["criteres"][0];
            assert_eq!(sent["fond"], api, "{fond:?} {kind:?}");
            assert_eq!(search["champs"][0]["typeChamp"], field, "{fond:?} {kind:?}");
            assert_eq!(
                (&criterion["typeRecherche"], &criterion["valeur"]),
                (&json!("EXACTE"), &json!(raw))
            );
            assert_eq!(
                (&search["typePagination"], &search["pageSize"], &search["pageNumber"]),
                (&json!(pagination), &json!(size), &json!(1)),
                "{fond:?} {kind:?}"
            );
        }
    }

    #[tokio::test]
    async fn find_by_number_article_lists_the_codes_that_carry_the_number() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        serve(&rig, "/search", &exact_article_request("9", 1), json_reply(200, EXACT_9), 1).await;

        let hits =
            rig.corpus.find_by_number(&query(Fond::Code, NumberKind::Article, "9")).await.unwrap();

        assert_eq!(hits.len(), 7);
        assert!(hits.iter().all(|hit| hit.fond == Fond::Code && hit.articles.len() == 1));
        assert!(hits.iter().any(|hit| hit.id.as_str() == "LEGITEXT000006070721"));
    }

    #[tokio::test]
    async fn find_by_number_sends_the_period_on_the_date_facet_of_the_fond() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        Mock::given(method("POST"))
            .and(path(route("/search")))
            .respond_with(json_reply(200, r#"{"results":[]}"#))
            .expect(1)
            .mount(&rig.server)
            .await;
        let range =
            DateRange::new(Some(day("2025-01-01")), Some(day("2025-12-31"))).unwrap().unwrap();

        rig.corpus
            .find_by_number(&query(Fond::Juri, NumberKind::Case, "24-13.470").with_within(range))
            .await
            .unwrap();

        let all = bodies(&rig, "/search").await;
        assert_eq!(
            all[0]["recherche"]["filtres"],
            json!([{"facette": "DATE_DECISION", "dates": {"start": "2025-01-01", "end": "2025-12-31"}}])
        );
    }

    // ---- find_by_number : ECLI ---------------------------------------------------------------

    #[tokio::test]
    async fn ecli_facet_filter() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let ecli = "ECLI:FR:CCASS:2025:C100399";
        let request = json!({
            "fond": "JURI",
            "recherche": {
                "champs": [{
                    "typeChamp": "ALL",
                    "criteres": [{"typeRecherche": "UN_DES_MOTS", "valeur": "*", "operateur": "ET"}],
                    "operateur": "ET",
                }],
                "filtres": [{"facette": "ECLI", "valeurs": [ecli]}],
                "pageNumber": 1,
                "pageSize": 10,
                "operateur": "ET",
                "sort": "PERTINENCE",
                "typePagination": "DEFAUT",
            },
        });
        serve(&rig, "/search", &request, json_reply(200, ECLI_JURI), 1).await;

        let hits =
            rig.corpus.find_by_number(&query(Fond::Juri, NumberKind::Ecli, ecli)).await.unwrap();

        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].fond, hits[0].id.as_str()), (Fond::Juri, "JURITEXT000051743650"));
    }

    #[tokio::test]
    async fn an_unknown_ecli_gives_no_hit() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        Mock::given(method("POST"))
            .and(path(route("/search")))
            .respond_with(json_reply(200, ECLI_UNKNOWN))
            .expect(1)
            .mount(&rig.server)
            .await;
        let unknown = query(Fond::Juri, NumberKind::Ecli, "ECLI:FR:CCASS:1900:C000000");
        assert_eq!(rig.corpus.find_by_number(&unknown).await.unwrap(), Vec::new());
    }

    // ---- find_by_number : consultations -----------------------------------------------------

    #[tokio::test]
    async fn a_nor_is_looked_up_by_get_jo_with_nor_and_a_journal_only_text_gives_no_hit() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let request = json!({"nor": "AGRX2502679L"});
        serve(&rig, "/consult/getJoWithNor", &request, json_reply(200, NOR), 1).await;
        never(&rig.server, &route("/search")).await;

        let hits = rig
            .corpus
            .find_by_number(&query(Fond::Loda, NumberKind::Nor, "AGRX2502679L"))
            .await
            .unwrap();

        // `JORFTEXT…` : pas encore de surface JORF (M7), donc pas de hit LODA possible.
        assert_eq!(hits, Vec::new());
    }

    #[tokio::test]
    async fn an_unknown_nor_gives_no_hit() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let request = json!({"nor": "ZZZZ0000000Z"});
        serve(&rig, "/consult/getJoWithNor", &request, json_reply(400, NOR_UNKNOWN), 1).await;

        let nor = query(Fond::Loda, NumberKind::Nor, "ZZZZ0000000Z");
        assert_eq!(rig.corpus.find_by_number(&nor).await.unwrap(), Vec::new());
    }

    #[tokio::test]
    async fn a_consolidated_text_found_by_nor_gives_a_loda_hit() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let reply = r#"{"id":"LEGITEXT000033205014","cid":"LEGITEXT000033205014","title":"LOI pour une République numérique","nature":"LOI"}"#;
        let request = json!({"nor": "ECFI1524250L"});
        serve(&rig, "/consult/getJoWithNor", &request, json_reply(200, reply), 1).await;

        let hits = rig
            .corpus
            .find_by_number(&query(Fond::Loda, NumberKind::Nor, "ECFI1524250L"))
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].fond, hits[0].id.as_str()), (Fond::Loda, "LEGITEXT000033205014"));
        assert_eq!(hits[0].nature.as_deref(), Some("LOI"));
    }

    #[tokio::test]
    async fn an_idcc_is_looked_up_by_kali_cont_idcc() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let request = json!({"id": "2121"});
        serve(&rig, "/consult/kaliContIdcc", &request, json_reply(200, IDCC_2121), 1).await;
        never(&rig.server, &route("/search")).await;

        let hits =
            rig.corpus.find_by_number(&query(Fond::Kali, NumberKind::Idcc, "2121")).await.unwrap();

        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].fond, hits[0].id.as_str()), (Fond::Kali, "KALITEXT000005654011"));
        assert_eq!(hits[0].idcc.map(lf_domain::contract::Idcc::get), Some(2121));
    }

    #[tokio::test]
    async fn an_unknown_idcc_falls_back_to_the_kali_search() {
        // L'API répond 500 à un IDCC inconnu (relevé en sandbox) : réessayé, puis la recherche.
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        Mock::given(method("POST"))
            .and(path(route("/consult/kaliContIdcc")))
            .respond_with(json_reply(500, IDCC_UNKNOWN))
            .mount(&rig.server)
            .await;
        Mock::given(method("POST"))
            .and(path(route("/search")))
            .respond_with(json_reply(200, r#"{"totalResultNumber":0,"results":[]}"#))
            .expect(1)
            .mount(&rig.server)
            .await;

        let hits =
            rig.corpus.find_by_number(&query(Fond::Kali, NumberKind::Idcc, "99999")).await.unwrap();

        assert_eq!(hits, Vec::new());
        let all = bodies(&rig, "/search").await;
        let search = &all[0];
        assert_eq!(search["fond"], "KALI");
        assert_eq!(search["recherche"]["champs"][0]["typeChamp"], "IDCC");
        assert_eq!(search["recherche"]["champs"][0]["criteres"][0]["valeur"], "99999");
    }

    #[tokio::test]
    async fn a_container_without_base_text_falls_back_to_the_kali_search_and_not_found_too() {
        // 400 « L'expression à valider est fausse. » : le marqueur d'un identifiant inconnu.
        for consult in
            [json_reply(200, r#"{"id":"KALICONT000005635096"}"#), json_reply(400, NOR_UNKNOWN)]
        {
            let rig = Rig::new().await;
            token_mock(&rig.server, "tok-1", 1).await;
            Mock::given(method("POST"))
                .and(path(route("/consult/kaliContIdcc")))
                .respond_with(consult)
                .mount(&rig.server)
                .await;
            Mock::given(method("POST"))
                .and(path(route("/search")))
                .respond_with(json_reply(200, r#"{"totalResultNumber":0,"results":[]}"#))
                .expect(1)
                .mount(&rig.server)
                .await;
            let kali = query(Fond::Kali, NumberKind::Idcc, "2121");
            assert_eq!(rig.corpus.find_by_number(&kali).await.unwrap(), Vec::new());
        }
    }

    #[tokio::test]
    async fn another_error_on_the_idcc_consultation_is_not_hidden_by_the_search() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let request = json!({"id": "2121"});
        serve(&rig, "/consult/kaliContIdcc", &request, json_reply(200, "{"), 1).await;
        never(&rig.server, &route("/search")).await;

        let error = rig
            .corpus
            .find_by_number(&query(Fond::Kali, NumberKind::Idcc, "2121"))
            .await
            .unwrap_err();
        assert_eq!(variant(&error), "Decode");
    }

    #[tokio::test]
    async fn an_eli_article_is_resolved_to_its_text_and_an_eli_text_by_the_redirection() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let article = "/eli/loi/2016/10/7/ECFI1524250L/jo/article_9";
        let text = "/eli/loi/2016/10/7/ECFI1524250L/jo/texte";
        serve(
            &rig,
            "/consult/getArticleWithIdEliOrAlias",
            &json!({"idEliOrAlias": article}),
            json_reply(200, CIVIL_9),
            1,
        )
        .await;
        let redirect = r#"{"cid":"LEGITEXT000033205014","title":"LOI numérique","nature":"LOI"}"#;
        serve(
            &rig,
            "/consult/eliAndAliasRedirectionTexte",
            &json!({"idEliOrAlias": text}),
            json_reply(200, redirect),
            1,
        )
        .await;
        never(&rig.server, &route("/search")).await;

        let by_article =
            rig.corpus.find_by_number(&query(Fond::Loda, NumberKind::Eli, article)).await.unwrap();
        assert_eq!(by_article.len(), 1);
        assert_eq!(by_article[0].id.as_str(), "LEGITEXT000006070721");
        assert_eq!(by_article[0].articles[0].article_id.as_str(), "LEGIARTI000006419288");

        let by_text =
            rig.corpus.find_by_number(&query(Fond::Loda, NumberKind::Eli, text)).await.unwrap();
        assert_eq!(by_text.len(), 1);
        assert_eq!(by_text[0].id.as_str(), "LEGITEXT000033205014");
    }

    #[tokio::test]
    async fn an_unknown_eli_gives_no_hit() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let article = "/eli/loi/1900/1/1/ZZZZ0000000Z/jo/article_1";
        let empty = r#"{"executionTime":0,"dereferenced":false,"article":null}"#;
        serve(
            &rig,
            "/consult/getArticleWithIdEliOrAlias",
            &json!({"idEliOrAlias": article}),
            json_reply(200, empty),
            1,
        )
        .await;
        let hits =
            rig.corpus.find_by_number(&query(Fond::Loda, NumberKind::Eli, article)).await.unwrap();
        assert_eq!(hits, Vec::new());
    }

    // ---- gardes défensives -------------------------------------------------------------------

    #[tokio::test]
    async fn number_kind_not_applicable() {
        let rig = Rig::new().await;
        let mut query = query(Fond::Juri, NumberKind::Case, "24-13.470");
        query.fond = Fond::Cnil;

        let error = rig.corpus.find_by_number(&query).await.unwrap_err();

        assert_eq!(
            error,
            CorpusError::InvalidRequest {
                status: None,
                detail: "number kind case does not apply to fond CNIL".to_owned()
            }
        );
        assert!(rig.server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_inconsistent_query_never_reaches_the_network() {
        let rig = Rig::new().await;
        let mut missing = query(Fond::Code, NumberKind::Article, "9");
        missing.version_date = None;
        let blank = query(Fond::Juri, NumberKind::Case, "   ");
        for query in [missing, blank] {
            let error = rig.corpus.find_by_number(&query).await.unwrap_err();
            assert_eq!(variant(&error), "InvalidRequest");
        }
        assert!(rig.server.received_requests().await.unwrap().is_empty());
    }

    // ---- list_codes ------------------------------------------------------------------------

    fn page_request(page: u32) -> Value {
        json!({"pageNumber": page, "pageSize": 100, "sort": "TITLE_ASC"})
    }

    #[tokio::test]
    async fn list_codes_lazy_single_table() {
        let rig = Rig::new().await;
        // Paresseux : la construction n'envoie rien.
        assert!(rig.server.received_requests().await.unwrap().is_empty());
        token_mock(&rig.server, "tok-1", 1).await;
        serve(&rig, "/list/code", &page_request(1), json_reply(200, PAGE_1), 1).await;
        serve(&rig, "/list/code", &page_request(2), json_reply(200, PAGE_2), 1).await;

        // Le premier appel charge les deux pages, les suivants servent la table (la mono-vol des
        // appels simultanés est vérifiée sur la table seule, `codes::tests` : l'attente
        // instantanée de ce banc d'essai ferait expirer le second appelant).
        let first = rig.corpus.list_codes().await.unwrap();
        let second = rig.corpus.list_codes().await.unwrap();
        let third = rig.corpus.list_codes().await.unwrap();

        assert_eq!(first.len(), 108);
        assert_eq!((&first, &second), (&third, &third));
        assert_eq!(requests_to(&rig.server, &route("/list/code")).await, 2);
        let civil = first.iter().find(|code| code.id == civil_code()).unwrap();
        assert_eq!(
            (civil.title.as_str(), civil.etat.clone()),
            ("Code civil", Some(LegalStatus::Vigueur))
        );
        let repealed = first.iter().filter(|code| code.etat == Some(LegalStatus::Abroge)).count();
        assert_eq!(repealed, 32);
    }

    #[tokio::test]
    async fn list_codes_stops_at_the_announced_total() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        // Une seule page de 100 entrées pour un total de 100 : pas de seconde page.
        let entries: Vec<Value> = (1..=100_u64)
            .map(|n| json!({"id": format!("LEGITEXT{n:012}"), "titre": format!("Code {n}")}))
            .collect();
        let page = json!({"totalResultNumber": 100, "results": entries}).to_string();
        serve(&rig, "/list/code", &page_request(1), json_reply(200, &page), 1).await;

        assert_eq!(rig.corpus.list_codes().await.unwrap().len(), 100);
        assert_eq!(bodies(&rig, "/list/code").await.len(), 1);
    }

    /// Page de 100 codes distincts : les numéros `first` à `first + 99`.
    fn numbered_page(first: u64, total: u64) -> String {
        let entries: Vec<Value> = (first..first + 100)
            .map(|n| json!({"id": format!("LEGITEXT{n:012}"), "titre": format!("Code {n}")}))
            .collect();
        json!({"totalResultNumber": total, "results": entries}).to_string()
    }

    #[tokio::test]
    async fn list_codes_never_asks_for_more_than_ten_pages() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        // Dix pages de codes tous distincts pour un total annoncé de 5 000 : le plafond de pages
        // s'applique, et aucune répétition ne déclenche de second parcours.
        for page in 1..=10_u32 {
            let first = u64::from(page) * 100 - 99;
            let reply = json_reply(200, &numbered_page(first, 5_000));
            serve(&rig, "/list/code", &page_request(page), reply, 1).await;
        }

        assert_eq!(rig.corpus.list_codes().await.unwrap().len(), 1_000);
        assert_eq!(bodies(&rig, "/list/code").await.len(), 10);
    }

    /// Deuxième page de `PAGE_2` dont les quatre premiers codes sont ceux de la première page :
    /// ce que la sandbox a rendu le 2026-10-29 (104 codes distincts pour 108 annoncés, le Code
    /// civil manquait).
    fn overlapping_page_2() -> String {
        let first: Value = serde_json::from_str(PAGE_1).unwrap();
        let mut second: Value = serde_json::from_str(PAGE_2).unwrap();
        for index in 0..4 {
            second["results"][index] = first["results"][index].clone();
        }
        second.to_string()
    }

    #[tokio::test]
    async fn list_codes_completes_a_traversal_whose_pages_overlap() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        // Premier parcours : la seconde page répète quatre codes de la première (104 distincts).
        serve(&rig, "/list/code", &page_request(1), json_reply(200, PAGE_1), 1).await;
        serve(&rig, "/list/code", &page_request(2), json_reply(200, &overlapping_page_2()), 1)
            .await;
        // Second parcours : les pages sont disjointes, les 108 codes y sont.
        serve(&rig, "/list/code", &page_request(1), json_reply(200, PAGE_1), 1).await;
        serve(&rig, "/list/code", &page_request(2), json_reply(200, PAGE_2), 1).await;

        let codes = rig.corpus.list_codes().await.unwrap();

        assert_eq!(codes.len(), 108);
        let distinct: HashSet<&LegiTextId> = codes.iter().map(|code| &code.id).collect();
        assert_eq!(distinct.len(), 108);
        assert!(distinct.contains(&civil_code()));
        // Deux parcours de deux pages, pas un de plus : le second est sans répétition.
        assert_eq!(bodies(&rig, "/list/code").await.len(), 4);
    }

    #[tokio::test]
    async fn list_codes_gives_up_after_three_traversals_and_serves_their_union() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        // Chaque parcours rend 100 codes puis 4 nouveaux et 4 répétés : 104 distincts pour 108.
        let first_four = |page: String| -> Vec<Value> {
            let page: Value = serde_json::from_str(&page).unwrap();
            page["results"].as_array().unwrap().iter().take(4).cloned().collect()
        };
        let mut results = first_four(numbered_page(101, 108));
        results.extend(first_four(numbered_page(1, 108)));
        let page_2 = json!({"totalResultNumber": 108, "results": results}).to_string();
        serve(&rig, "/list/code", &page_request(1), json_reply(200, &numbered_page(1, 108)), 3)
            .await;
        serve(&rig, "/list/code", &page_request(2), json_reply(200, &page_2), 3).await;

        let codes = rig.corpus.list_codes().await.unwrap();

        assert_eq!(codes.len(), 104);
        // Trois parcours de deux pages, puis la table est servie telle quelle (et gardée).
        assert_eq!(bodies(&rig, "/list/code").await.len(), 6);
        assert_eq!(rig.corpus.list_codes().await.unwrap(), codes);
        assert_eq!(bodies(&rig, "/list/code").await.len(), 6);
    }

    #[tokio::test]
    async fn list_codes_without_a_total_stops_on_a_short_page() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let page = json!({"results": [{"id": "LEGITEXT000006070721", "titre": "Code civil"}]});
        serve(&rig, "/list/code", &page_request(1), json_reply(200, &page.to_string()), 1).await;

        assert_eq!(rig.corpus.list_codes().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_failed_load_keeps_nothing_and_the_next_call_loads_again() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let single = json!({"results": [{"id": "LEGITEXT000006070721", "titre": "Code civil"}]});
        // Première tentative : réponse illisible ; seconde : la bonne.
        Mock::given(method("POST"))
            .and(path(route("/list/code")))
            .respond_with(json_reply(200, "{"))
            .up_to_n_times(1)
            .mount(&rig.server)
            .await;
        Mock::given(method("POST"))
            .and(path(route("/list/code")))
            .respond_with(json_reply(200, &single.to_string()))
            .mount(&rig.server)
            .await;

        let error = rig.corpus.list_codes().await.unwrap_err();
        assert_eq!(variant(&error), "Decode");
        assert_eq!(rig.corpus.list_codes().await.unwrap().len(), 1);
        assert_eq!(requests_to(&rig.server, &route("/list/code")).await, 2);
    }
}
