//! Opérations de M6 de `PisteCorpus` : `find_article`, `find_by_number` et `list_codes`
//! (04 §7.1 et §7.2, annexe B §6.6). `corpus::port` y délègue.
//!
//! - **`find_article`** : [`ArticleAt::Today`] interroge `getArticleWithIdAndNum`, qui ne rend que
//!   la version en vigueur ; sur `NotFound` (article sans version en vigueur, ou à version
//!   différée), comme pour toute autre date, la recherche exacte `CODE_DATE` sur `NUM_ARTICLE` et
//!   `DATE_VERSION` prend le relais, et l'article trouvé est lu par `getArticle`. L'application a
//!   déjà résolu le code en `LEGITEXT` par `list_codes` (D5) : l'adapter ne voit que le texte.
//!   Pour [`ArticleAt::On`], quand cette recherche ne trouve rien (l'index daté ne contient ni les
//!   LODA ni les articles de code d'avant ~1994-1999), repli de U01 : la version courante, puis les
//!   versions de l'article (`canonicalArticleVersion`, règle `in_force_on`), puis, pour un texte
//!   non codifié, sa table des matières à la date. Jamais pour `Today`, jamais après une erreur.
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
    entity::{
        ArticleText, ArticleVersion, CodeListing, SearchHit, SearchPage, TocEntryRef, in_force_on,
    },
    fond::Fond,
    id::{ArticleId, LegiTextId},
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
        if let Some(article) = self.article_on(text, number.clone(), at.date()).await? {
            return Ok(article);
        }
        match at {
            ArticleAt::On(date) => self.article_by_versions(text, &number, date).await,
            ArticleAt::Today(_) => Err(not_found(endpoint::SEARCH)),
        }
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
    /// version trouvée pour `text` (04 §7.2 étape 3). `None` : la recherche n'a pas trouvé
    /// l'article (index daté sans ce texte, lacunaire avant ~1994-1999, jamais LODA).
    async fn article_on(
        &self,
        text: &LegiTextId,
        number: NumberValue,
        date: VersionDate,
    ) -> Result<Option<ArticleText>, CorpusError> {
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
                return self.get_article(&id).await.map(Some);
            }
            if !more || page.get() >= MAX_ARTICLE_PAGES {
                return Ok(None);
            }
            page = page.saturating_add(1);
        }
    }

    /// Repli d'une recherche datée sans résultat (U01) : l'index daté `CODE_DATE` ne contient ni
    /// les LODA ni les articles de code d'avant ~1994-1999. Étage 1 : la version courante
    /// (`getArticleWithIdAndNum`) puis, si elle ne couvre pas la date, les versions de l'article
    /// (`canonicalArticleVersion`) et la règle [`in_force_on`] ; étage 2, pour un texte non
    /// codifié seulement : sa table des matières à la date. Une erreur de transport ou
    /// d'authentification est rendue ; l'absence reste le `NotFound` de la recherche, comme en
    /// 1.1.0.
    async fn article_by_versions(
        &self,
        text: &LegiTextId,
        number: &NumberValue,
        date: VersionDate,
    ) -> Result<ArticleText, CorpusError> {
        let current = match self.article_by_number(text, number).await {
            Ok(current) => Some(current),
            Err(CorpusError::NotFound { .. }) => None,
            Err(error) => return Err(error),
        };
        if let Some(current) = current
            && let Some(article) = self.version_in_force(current, number, date).await?
        {
            return Ok(article);
        }
        self.article_in_dated_toc(text, number, date).await
    }

    /// La version de l'article `current` en vigueur à `date` : `current` elle-même si elle la
    /// couvre, sinon celle que désignent les versions de l'article ; `None` si aucune ne convient
    /// (liste illisible ou vide comprise, numéro différent).
    async fn version_in_force(
        &self,
        current: ArticleText,
        number: &NumberValue,
        date: VersionDate,
    ) -> Result<Option<ArticleText>, CorpusError> {
        let ArticleId::Legi(current_id) = current.article_id.clone() else {
            return Ok(None);
        };
        let here = ArticleVersion {
            id: current_id.clone(),
            num: current.num.clone(),
            etat: current.etat.clone(),
            date_debut: current.date_debut,
            date_fin: current.date_fin,
        };
        if in_force_on(std::slice::from_ref(&here), date).is_some() {
            return Ok(Some(current));
        }
        let versions = match self.article_versions_impl(&ArticleId::Legi(current_id)).await {
            Ok(versions) => versions,
            Err(
                CorpusError::NotFound { .. }
                | CorpusError::Decode { .. }
                | CorpusError::Upstream { .. },
            ) => return Ok(None),
            Err(error) => return Err(error),
        };
        let Some(chosen) = in_force_on(&versions, date) else {
            return Ok(None);
        };
        if chosen.id == here.id {
            return Ok(Some(current));
        }
        if chosen.num.as_deref().is_some_and(|n| !same_number(n, number.as_str())) {
            return Ok(None);
        }
        let read = match self.get_article(&ArticleId::Legi(chosen.id.clone())).await {
            Ok(read) => read,
            Err(CorpusError::NotFound { .. }) => return Ok(None),
            Err(error) => return Err(error),
        };
        // `numero` est nul dans la liste canonique : le numéro ne se vérifie qu'à la lecture.
        let same = read.num.as_deref().is_none_or(|n| same_number(n, number.as_str()));
        Ok(same.then_some(read))
    }

    /// Étage 2 : l'article de `number` dans la table des matières de `text` à `date`, pour un
    /// texte non codifié (un code garde le résultat de l'index daté). Rend le `NotFound` de la
    /// recherche quand la table n'a pas l'article, est trop grande ou illisible.
    async fn article_in_dated_toc(
        &self,
        text: &LegiTextId,
        number: &NumberValue,
        date: VersionDate,
    ) -> Result<ArticleText, CorpusError> {
        let absent = || not_found(endpoint::SEARCH);
        match self.list_codes_impl().await {
            Ok(codes) if codes.iter().any(|code| code.id == *text) => return Err(absent()),
            Ok(_) => {}
            Err(error @ CorpusError::Auth { .. }) => return Err(error),
            Err(_) => return Err(absent()),
        }
        let act = match self.get_act(text, date).await {
            Ok(act) => act,
            Err(
                CorpusError::NotFound { .. }
                | CorpusError::TooLarge { .. }
                | CorpusError::Decode { .. },
            ) => return Err(absent()),
            Err(error) => return Err(error),
        };
        let Ok(page) = act.body.toc.page(0, NonZeroU32::MAX) else {
            return Err(absent());
        };
        let entry = page.entries.iter().find_map(|entry| match entry.node {
            TocEntryRef::Article(article)
                if article.num.as_deref().is_some_and(|n| same_number(n, number.as_str())) =>
            {
                Some(article.article_id.clone())
            }
            _ => None,
        });
        let Some(entry) = entry else {
            return Err(absent());
        };
        match self.get_article(&entry).await {
            Err(CorpusError::NotFound { .. }) => Err(absent()),
            other => other,
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
            let full =
                loaded.received >= usize::try_from(request::CODE_PAGE_SIZE).unwrap_or(usize::MAX);
            let more = loaded.total.is_none_or(|total| {
                u64::try_from(received).map_or(true, |received| received < total)
            });
            if !(full && more) {
                break;
            }
        }
        Ok(repeated)
    }
}

/// Forme comparable d'un numéro d'article : lettres et chiffres repliés en minuscules, rien
/// d'autre, et « 1er » vaut « 1 ».
fn number_key(raw: &str) -> String {
    let key: String =
        raw.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
    match key.strip_suffix("er") {
        Some(digits) if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) => {
            digits.to_owned()
        }
        _ => key,
    }
}

/// Deux numéros désignent-ils le même article (espaces, points, casse, « 1er » comme « 1 ») ?
fn same_number(left: &str, right: &str) -> bool {
    number_key(left) == number_key(right)
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

#[expect(
    clippy::arithmetic_side_effects,
    reason = "tests : valeurs bornées par les données du test"
)]
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
        // U01-R : après l'épuisement des trois pages, le repli par les versions interroge la
        // version courante (« numéro inconnu » : fin) ; garde sans compte d'appels.
        by_number(&rig, "LEGITEXT000006070721", "9", UNKNOWN_NUMBER, 0).await;
        never(&rig.server, &route("/search/canonicalArticleVersion")).await;

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

    // ---- U01-R : repli par les versions et table des matières datée ---------------------------

    /// Fixtures `derivee` de U01 : aucun relevé sandbox (D1) n'existe. Les formes sont celles des
    /// relevés `getArticleWithIdAndNum-code-civil-9` (enveloppe d'un article) et
    /// `canonicalArticleVersion-LEGIARTI000006419288` (liste de versions) ; les identifiants et
    /// les dates de l'article 17 de la loi 89-462 sont ceux relevés en production le 2026-11-02
    /// (fiche U01, §2) ; le texte des articles est un marqueur, rien n'est copié d'un texte tiers.
    /// Les ids de l'art. 8 du Code civil, de l'art. 24-2 et de l'art. 1 de la loi sont synthétiques.
    const LAW: &str = "LEGITEXT000006069108";
    /// Sentinelle `2999-01-01` de l'API, en millisecondes.
    const NO_END: i64 = 32_472_144_000_000;
    const MS_1927_08_14: i64 = -1_337_558_400_000;
    const MS_1950_01_01: i64 = -631_152_000_000;
    const MS_1970_07_19: i64 = 17_193_600_000;
    const MS_1989_07_08: i64 = 615_859_200_000;
    const MS_2014_03_27: i64 = 1_395_878_400_000;
    const MS_2018_11_25: i64 = 1_543_104_000_000;
    const MS_2022_08_24: i64 = 1_661_299_200_000;
    const MS_2019_01_01: i64 = 1_546_300_800_000;
    const MS_1990_01_01: i64 = 631_152_000_000;

    fn law() -> LegiTextId {
        LegiTextId::parse(LAW).unwrap()
    }

    /// Enveloppe d'un article (`getArticle`, `getArticleWithIdAndNum`) dérivée de `CIVIL_9`.
    fn derived_article(
        id: &str,
        num: &str,
        etat: &str,
        start: i64,
        end: Option<i64>,
        text: (&str, &str, &str),
    ) -> String {
        let (text_id, title, nature) = text;
        let mut body: Value = serde_json::from_str(CIVIL_9).unwrap();
        let article = &mut body["article"];
        article["id"] = json!(id);
        article["cid"] = json!(id);
        article["num"] = json!(num);
        article["etat"] = json!(etat);
        article["dateDebut"] = json!(start);
        article["dateFin"] = json!(end.unwrap_or(NO_END));
        article["texte"] = json!("[texte dérivé : non relevé]");
        article["texteHtml"] = json!("<p>[texte dérivé : non relevé]</p>");
        article["lienModifications"] = json!([]);
        article["context"]["titreTxt"][0] = json!({"titre": title, "cid": text_id, "id": text_id});
        article["textTitles"][0]["id"] = json!(text_id);
        article["textTitles"][0]["cid"] = json!(text_id);
        article["textTitles"][0]["titre"] = json!(title);
        article["textTitles"][0]["titreLong"] = json!(title);
        article["textTitles"][0]["nature"] = json!(nature);
        body.to_string()
    }

    fn law_article(id: &str, num: &str, etat: &str, start: i64, end: Option<i64>) -> String {
        let text = (LAW, "Loi n° 89-462 du 6 juillet 1989", "LOI");
        derived_article(id, num, etat, start, end, text)
    }

    fn civil_article(id: &str, num: &str, etat: &str, start: i64, end: Option<i64>) -> String {
        derived_article(id, num, etat, start, end, ("LEGITEXT000006070721", "Code civil", "CODE"))
    }

    /// Réponse de `canonicalArticleVersion` : (id, état, début, fin), `numero` à `null`.
    fn canonical_body(versions: &[(&str, &str, i64, Option<i64>)]) -> String {
        let list: Vec<Value> = versions
            .iter()
            .map(|(id, etat, start, end)| {
                json!({"id": id, "etat": etat, "version": "1.0", "dateDebut": start,
                       "dateFin": end.unwrap_or(NO_END), "numero": null, "ordre": null})
            })
            .collect();
        json!({"articleVersions": list}).to_string()
    }

    /// Les trois versions de l'article 17 de la loi 89-462 (ids et dates relevés en production).
    fn law_17_versions() -> String {
        canonical_body(&[
            ("LEGIARTI000028806710", "MODIFIE", MS_2014_03_27, Some(MS_2018_11_25)),
            ("LEGIARTI000037670690", "MODIFIE", MS_2018_11_25, Some(MS_2022_08_24)),
            ("LEGIARTI000043977087", "VIGUEUR", MS_2022_08_24, None),
        ])
    }

    fn current_law_17() -> String {
        law_article("LEGIARTI000043977087", "17", "VIGUEUR", MS_2022_08_24, None)
    }

    /// Sommaire daté d'un texte non codifié : (id, numéro, état).
    fn toc_body(entries: &[(&str, &str, &str)]) -> String {
        let articles: Vec<Value> = entries
            .iter()
            .map(|(id, num, etat)| {
                json!({"executionTime": 0, "dereferenced": false, "id": id, "cid": id,
                       "intOrdre": 524_287, "etat": etat, "num": num, "path": null,
                       "pathTitle": [], "content": "<p>[texte dérivé]</p>", "nota": "",
                       "articleVersion": "1.0", "type": "AUTONOME"})
            })
            .collect();
        json!({"executionTime": 0, "dereferenced": false, "id": format!("{LAW}_01-01-2000"),
               "cid": null, "title": "Loi n° 89-462 du 6 juillet 1989", "nature": "LOI",
               "jurisState": "Vigueur", "dateDebutVersion": "1999-12-17",
               "dateFinVersion": "2999-01-01", "appellations": [],
               "sections": [{"executionTime": 0, "dereferenced": false,
                   "id": "LEGISCTA000006000001", "cid": "JORFSCTA000006000001",
                   "intOrdre": 524_287, "etat": "VIGUEUR", "title": "Titre Ier",
                   "sections": [], "articles": articles}],
               "articles": []})
        .to_string()
    }

    const EMPTY_SEARCH: &str = r#"{"totalResultNumber":0,"results":[]}"#;

    /// `/search` répond « aucun hit », `times` fois exactement (0 : sans exigence de compte).
    async fn empty_search(rig: &Rig) {
        Mock::given(method("POST"))
            .and(path(route("/search")))
            .respond_with(json_reply(200, EMPTY_SEARCH))
            .mount(&rig.server)
            .await;
    }

    /// Comme `serve`, mais `times` à 0 veut dire « à volonté, sans exigence de compte » : le
    /// garde ne dépend pas du nombre d'appels que la souche émet.
    async fn serve_or_lenient(
        rig: &Rig,
        endpoint: &str,
        request: &Value,
        reply: ResponseTemplate,
        times: u64,
    ) {
        if times > 0 {
            return serve(rig, endpoint, request, reply, times).await;
        }
        Mock::given(method("POST"))
            .and(path(route(endpoint)))
            .and(body_json(request))
            .respond_with(reply)
            .mount(&rig.server)
            .await;
    }

    async fn by_number(rig: &Rig, text: &str, num: &str, reply: &str, times: u64) {
        let request = json!({"id": text, "num": num});
        serve_or_lenient(
            rig,
            "/consult/getArticleWithIdAndNum",
            &request,
            json_reply(200, reply),
            times,
        )
        .await;
    }

    async fn canonical(rig: &Rig, id: &str, reply: &str, times: u64) {
        serve_or_lenient(
            rig,
            "/search/canonicalArticleVersion",
            &json!({"id": id}),
            json_reply(200, reply),
            times,
        )
        .await;
    }

    async fn read_version(rig: &Rig, id: &str, reply: &str, times: u64) {
        serve_or_lenient(
            rig,
            "/consult/getArticle",
            &json!({"id": id}),
            json_reply(200, reply),
            times,
        )
        .await;
    }

    async fn toc(rig: &Rig, date: &str, reply: &str, times: u64) {
        let request = json!({"textId": LAW, "date": date});
        serve_or_lenient(rig, "/consult/lawDecree", &request, json_reply(200, reply), times).await;
    }

    /// Charge la table des codes (2 appels `/list/code`), une fois.
    async fn load_codes(rig: &Rig) {
        serve(rig, "/list/code", &page_request(1), json_reply(200, PAGE_1), 1).await;
        serve(rig, "/list/code", &page_request(2), json_reply(200, PAGE_2), 1).await;
        rig.corpus.list_codes().await.unwrap();
    }

    /// L'article du résultat, ou un échec par assertion (jamais par `unwrap`).
    fn ok(result: Result<ArticleText, CorpusError>) -> ArticleText {
        assert!(result.is_ok(), "un article était attendu : {result:?}");
        result.unwrap()
    }

    fn on(date: &str) -> ArticleAt {
        ArticleAt::On(day(date))
    }

    fn not_found_in_search() -> CorpusError {
        CorpusError::NotFound { what: "/search".to_owned() }
    }

    async fn calls(rig: &Rig, endpoint: &str) -> usize {
        requests_to(&rig.server, &route(endpoint)).await
    }

    #[tokio::test]
    async fn find_article_on_a_date_falls_back_to_the_versions_for_a_law() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        empty_search(&rig).await;
        by_number(&rig, LAW, "17", &current_law_17(), 1).await;
        canonical(&rig, "LEGIARTI000043977087", &law_17_versions(), 1).await;
        let old = law_article(
            "LEGIARTI000037670690",
            "17",
            "MODIFIE",
            MS_2018_11_25,
            Some(MS_2022_08_24),
        );
        read_version(&rig, "LEGIARTI000037670690", &old, 1).await;
        never(&rig.server, &route("/consult/lawDecree")).await;

        let found = ok(rig.corpus.find_article(&law(), "17", on("2020-01-01")).await);

        assert_eq!(found.article_id.as_str(), "LEGIARTI000037670690");
        assert_eq!(found.num.as_deref(), Some("17"));
        assert_eq!(found.etat, Some(LegalStatus::Modifie));
    }

    #[tokio::test]
    async fn find_article_on_a_date_serves_the_current_article_of_a_single_version_law_without_the_versions()
     {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        empty_search(&rig).await;
        let single = law_article("LEGIARTI000047000001", "3", "VIGUEUR", MS_2019_01_01, None);
        by_number(&rig, LAW, "3", &single, 1).await;
        never(&rig.server, &route("/search/canonicalArticleVersion")).await;
        never(&rig.server, &route("/consult/getArticle")).await;
        never(&rig.server, &route("/consult/lawDecree")).await;

        let found = ok(rig.corpus.find_article(&law(), "3", on("2020-06-01")).await);

        assert_eq!(found.article_id.as_str(), "LEGIARTI000047000001");
    }

    #[tokio::test]
    async fn find_article_on_a_date_falls_back_to_the_versions_for_civil_code_art_9_in_1960() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        empty_search(&rig).await;
        by_number(&rig, "LEGITEXT000006070721", "9", CIVIL_9, 1).await;
        let versions = include_str!(
            "../../tests/fixtures/recorded/search/canonicalArticleVersion/m7/canonicalArticleVersion-LEGIARTI000006419288.json"
        );
        canonical(&rig, "LEGIARTI000006419288", versions, 1).await;
        let abroge = civil_article(
            "LEGIARTI000050396821",
            "9",
            "ABROGE",
            MS_1927_08_14,
            Some(MS_1970_07_19),
        );
        read_version(&rig, "LEGIARTI000050396821", &abroge, 1).await;

        let found = ok(rig.corpus.find_article(&civil_code(), "9", on("1960-01-01")).await);

        assert_eq!(found.article_id.as_str(), "LEGIARTI000050396821");
        assert_eq!(found.etat, Some(LegalStatus::Abroge));
    }

    #[tokio::test]
    async fn find_article_on_a_date_falls_back_to_the_versions_for_civil_code_art_8_in_1985() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        empty_search(&rig).await;
        let current = civil_article("LEGIARTI000006419287", "8", "VIGUEUR", MS_1990_01_01, None);
        by_number(&rig, "LEGITEXT000006070721", "8", &current, 1).await;
        let versions = canonical_body(&[
            ("LEGIARTI000999000001", "MODIFIE", MS_1950_01_01, Some(MS_1990_01_01)),
            ("LEGIARTI000006419287", "VIGUEUR", MS_1990_01_01, None),
        ]);
        canonical(&rig, "LEGIARTI000006419287", &versions, 1).await;
        let old = civil_article(
            "LEGIARTI000999000001",
            "8",
            "MODIFIE",
            MS_1950_01_01,
            Some(MS_1990_01_01),
        );
        read_version(&rig, "LEGIARTI000999000001", &old, 1).await;

        let found = ok(rig.corpus.find_article(&civil_code(), "8", on("1985-06-01")).await);

        assert_eq!(found.article_id.as_str(), "LEGIARTI000999000001");
    }

    /// Garde : une date antérieure à la première version reste introuvable, rien n'est lu.
    #[tokio::test]
    async fn find_article_on_a_date_before_the_first_version_is_not_found() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        empty_search(&rig).await;
        load_codes(&rig).await;
        by_number(&rig, "LEGITEXT000006070721", "9", CIVIL_9, 0).await;
        let versions = include_str!(
            "../../tests/fixtures/recorded/search/canonicalArticleVersion/m7/canonicalArticleVersion-LEGIARTI000006419288.json"
        );
        canonical(&rig, "LEGIARTI000006419288", versions, 0).await;
        never(&rig.server, &route("/consult/getArticle")).await;

        let result = rig.corpus.find_article(&civil_code(), "9", on("1800-01-01")).await;

        assert_eq!(result.unwrap_err(), not_found_in_search());
    }

    #[tokio::test]
    async fn find_article_on_a_date_reads_the_dated_toc_for_an_article_repealed_today() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        empty_search(&rig).await;
        load_codes(&rig).await;
        by_number(&rig, LAW, "19", UNKNOWN_NUMBER, 1).await;
        let entries = [
            ("LEGIARTI000006000010", "1", "VIGUEUR"),
            ("LEGIARTI000039337710", "19", "ABROGE"),
            ("LEGIARTI000039337711", "24-2", "ABROGE"),
        ];
        toc(&rig, "2000-01-01", &toc_body(&entries), 1).await;
        let repealed =
            law_article("LEGIARTI000039337710", "19", "ABROGE", MS_1989_07_08, Some(MS_2014_03_27));
        read_version(&rig, "LEGIARTI000039337710", &repealed, 1).await;
        never(&rig.server, &route("/search/canonicalArticleVersion")).await;

        let found = ok(rig.corpus.find_article(&law(), "19", on("2000-01-01")).await);

        assert_eq!(found.article_id.as_str(), "LEGIARTI000039337710");
        assert_eq!(found.etat, Some(LegalStatus::Abroge));
    }

    #[tokio::test]
    async fn find_article_on_a_date_reads_the_dated_toc_for_art_24_2_of_law_89_462_at_2000_01_01() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        empty_search(&rig).await;
        load_codes(&rig).await;
        by_number(&rig, LAW, "24-2", UNKNOWN_NUMBER, 1).await;
        let entries =
            [("LEGIARTI000039337710", "19", "ABROGE"), ("LEGIARTI000039337711", "24-2", "ABROGE")];
        toc(&rig, "2000-01-01", &toc_body(&entries), 1).await;
        let repealed = law_article(
            "LEGIARTI000039337711",
            "24-2",
            "ABROGE",
            MS_1989_07_08,
            Some(MS_2014_03_27),
        );
        read_version(&rig, "LEGIARTI000039337711", &repealed, 1).await;

        let found = ok(rig.corpus.find_article(&law(), "24-2", on("2000-01-01")).await);

        assert_eq!(found.article_id.as_str(), "LEGIARTI000039337711");
    }

    #[tokio::test]
    async fn gap4_2_resolve_serves_the_version_that_the_get_text_note_points_to() {
        // `fr_get_text` d'une version abrogée (LEGIARTI000039337710) renvoie vers `fr_resolve`
        // avec une date : le second maillon, l'appel daté, est servi par la table des matières.
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        empty_search(&rig).await;
        load_codes(&rig).await;
        by_number(&rig, LAW, "19", UNKNOWN_NUMBER, 1).await;
        let entries = [("LEGIARTI000039337710", "19", "ABROGE")];
        toc(&rig, "2013-12-31", &toc_body(&entries), 1).await;
        let repealed =
            law_article("LEGIARTI000039337710", "19", "ABROGE", MS_1989_07_08, Some(MS_2014_03_27));
        read_version(&rig, "LEGIARTI000039337710", &repealed, 1).await;

        let found = ok(rig.corpus.find_article(&law(), "19", on("2013-12-31")).await);

        assert_eq!(found.article_id.as_str(), "LEGIARTI000039337710");
    }

    #[tokio::test]
    async fn find_article_on_a_date_unknown_article_costs_two_extra_calls() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        empty_search(&rig).await;
        load_codes(&rig).await;
        by_number(&rig, LAW, "99", UNKNOWN_NUMBER, 1).await;
        toc(&rig, "2000-01-01", &toc_body(&[("LEGIARTI000039337710", "19", "ABROGE")]), 1).await;
        never(&rig.server, &route("/search/canonicalArticleVersion")).await;
        never(&rig.server, &route("/consult/getArticle")).await;

        let result = rig.corpus.find_article(&law(), "99", on("2000-01-01")).await;

        assert_eq!(result.unwrap_err(), not_found_in_search());
        assert_eq!(calls(&rig, "/consult/getArticleWithIdAndNum").await, 1);
        assert_eq!(calls(&rig, "/consult/lawDecree").await, 1);
    }

    #[tokio::test]
    async fn find_article_on_a_date_fallback_total_calls_are_bounded() {
        // Chemin le plus long : version courante, liste canonique vide puis `getArticleByCid`,
        // lecture d'une version dont le numéro diffère, table des matières, lecture de l'entrée :
        // exactement 6 appels de repli.
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        empty_search(&rig).await;
        load_codes(&rig).await;
        let current = law_article("LEGIARTI000043977087", "17", "VIGUEUR", MS_2022_08_24, None);
        by_number(&rig, LAW, "17", &current, 1).await;
        canonical(&rig, "LEGIARTI000043977087", r#"{"articleVersions":[]}"#, 1).await;
        let listed = json!({"listArticle": [{
            "id": "LEGIARTI000028806710", "etat": "MODIFIE",
            "dateDebut": MS_2014_03_27, "dateFin": MS_2018_11_25}]})
        .to_string();
        serve(
            &rig,
            "/consult/getArticleByCid",
            &json!({"cid": "LEGIARTI000043977087"}),
            json_reply(200, &listed),
            1,
        )
        .await;
        let other = law_article(
            "LEGIARTI000028806710",
            "18",
            "MODIFIE",
            MS_2014_03_27,
            Some(MS_2018_11_25),
        );
        read_version(&rig, "LEGIARTI000028806710", &other, 1).await;
        toc(&rig, "2015-01-01", &toc_body(&[("LEGIARTI000039337710", "17", "MODIFIE")]), 1).await;
        let found = law_article(
            "LEGIARTI000039337710",
            "17",
            "MODIFIE",
            MS_2014_03_27,
            Some(MS_2018_11_25),
        );
        read_version(&rig, "LEGIARTI000039337710", &found, 1).await;

        let article = ok(rig.corpus.find_article(&law(), "17", on("2015-01-01")).await);

        assert_eq!(article.article_id.as_str(), "LEGIARTI000039337710");
        let names = [
            "/consult/getArticleWithIdAndNum",
            "/search/canonicalArticleVersion",
            "/consult/getArticleByCid",
            "/consult/getArticle",
            "/consult/lawDecree",
        ];
        let mut total = 0;
        for name in names {
            total += calls(&rig, name).await;
        }
        assert_eq!(total, 6);
    }

    #[tokio::test]
    async fn find_article_fallback_reads_then_rejects_a_renumbered_article() {
        // Liste canonique réelle : `numero` est `null`, le numéro ne se vérifie qu'à la lecture.
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        empty_search(&rig).await;
        load_codes(&rig).await;
        by_number(&rig, "LEGITEXT000006070721", "10", CIVIL_9, 1).await;
        let versions = include_str!(
            "../../tests/fixtures/recorded/search/canonicalArticleVersion/m7/canonicalArticleVersion-LEGIARTI000006419288.json"
        );
        canonical(&rig, "LEGIARTI000006419288", versions, 1).await;
        let old = civil_article(
            "LEGIARTI000050396821",
            "9",
            "ABROGE",
            MS_1927_08_14,
            Some(MS_1970_07_19),
        );
        read_version(&rig, "LEGIARTI000050396821", &old, 1).await;

        let result = rig.corpus.find_article(&civil_code(), "10", on("1960-01-01")).await;

        assert_eq!(result.unwrap_err(), not_found_in_search());
        assert_eq!(calls(&rig, "/consult/getArticle").await, 1);
    }

    #[tokio::test]
    async fn find_article_fallback_compares_the_number_before_reading_when_by_cid_carries_it() {
        // (numéro demandé, numéro de la version listée, lue ?)
        let table =
            [("1er", "1", true), ("L324-2-2", "L. 324-2-2", true), ("L111-3", "L111-2", false)];
        for (asked, listed_num, read) in table {
            let rig = Rig::new().await;
            token_mock(&rig.server, "tok-1", 1).await;
            empty_search(&rig).await;
            load_codes(&rig).await;
            let current =
                civil_article("LEGIARTI000006419288", asked, "VIGUEUR", MS_2022_08_24, None);
            by_number(&rig, "LEGITEXT000006070721", asked, &current, 1).await;
            canonical(&rig, "LEGIARTI000006419288", CANONICAL_NO_BYTE, 1).await;
            let listed = json!({"listArticle": [{
                "id": "LEGIARTI000050396821", "num": listed_num, "etat": "ABROGE",
                "dateDebut": MS_1927_08_14, "dateFin": MS_1970_07_19}]})
            .to_string();
            serve(
                &rig,
                "/consult/getArticleByCid",
                &json!({"cid": "LEGIARTI000006419288"}),
                json_reply(200, &listed),
                1,
            )
            .await;
            let body = civil_article(
                "LEGIARTI000050396821",
                listed_num,
                "ABROGE",
                MS_1927_08_14,
                Some(MS_1970_07_19),
            );
            read_version(&rig, "LEGIARTI000050396821", &body, u64::from(read)).await;

            let result = rig.corpus.find_article(&civil_code(), asked, on("1960-01-01")).await;

            assert_eq!(result.is_ok(), read, "{asked} / {listed_num}: {result:?}");
            assert_eq!(calls(&rig, "/consult/getArticle").await, usize::from(read), "{asked}");
        }
    }

    /// Un 200 sans aucun octet : `canonicalArticleVersion` « introuvable » (fixture `inconnu`).
    const CANONICAL_NO_BYTE: &str = "";

    #[tokio::test]
    async fn find_article_on_a_date_failed_fallback_is_not_hidden() {
        // (réponse de getArticleWithIdAndNum, variante attendue)
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        empty_search(&rig).await;
        by_number(&rig, LAW, "17", "{", 1).await;
        let error = rig.corpus.find_article(&law(), "17", on("2020-01-01")).await.unwrap_err();
        assert_eq!(variant(&error), "Decode");

        // Une version choisie mais illisible n'est pas une absence non plus.
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        empty_search(&rig).await;
        by_number(&rig, LAW, "17", &current_law_17(), 1).await;
        canonical(&rig, "LEGIARTI000043977087", &law_17_versions(), 1).await;
        read_version(&rig, "LEGIARTI000037670690", "{", 1).await;
        let error = rig.corpus.find_article(&law(), "17", on("2020-01-01")).await.unwrap_err();
        assert_eq!(variant(&error), "Decode");
    }

    #[tokio::test]
    async fn two_articles_of_the_same_text_and_date_never_share_a_fallback_failure() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        empty_search(&rig).await;
        load_codes(&rig).await;
        by_number(&rig, LAW, "99", UNKNOWN_NUMBER, 1).await;
        toc(&rig, "2020-01-01", &toc_body(&[("LEGIARTI000039337710", "19", "ABROGE")]), 1).await;
        by_number(&rig, LAW, "17", &current_law_17(), 1).await;
        canonical(&rig, "LEGIARTI000043977087", &law_17_versions(), 1).await;
        let old = law_article(
            "LEGIARTI000037670690",
            "17",
            "MODIFIE",
            MS_2018_11_25,
            Some(MS_2022_08_24),
        );
        read_version(&rig, "LEGIARTI000037670690", &old, 1).await;

        let absent = rig.corpus.find_article(&law(), "99", on("2020-01-01")).await;
        let present = rig.corpus.find_article(&law(), "17", on("2020-01-01")).await;

        assert_eq!(absent.unwrap_err(), not_found_in_search());
        assert_eq!(ok(present).article_id.as_str(), "LEGIARTI000037670690");
    }

    #[tokio::test]
    async fn find_article_on_a_date_for_two_key_variants_costs_at_most_twelve_calls() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        empty_search(&rig).await;
        load_codes(&rig).await;
        by_number(&rig, LAW, "1er", UNKNOWN_NUMBER, 1).await;
        by_number(&rig, LAW, "1", UNKNOWN_NUMBER, 1).await;
        let empty_toc = toc_body(&[]);
        toc(&rig, "2000-01-01", &empty_toc, 2).await;

        let first = rig.corpus.find_article(&law(), "1er", on("2000-01-01")).await;
        let second = rig.corpus.find_article(&law(), "1", on("2000-01-01")).await;

        assert_eq!(first.unwrap_err(), not_found_in_search());
        assert_eq!(second.unwrap_err(), not_found_in_search());
        let names = [
            "/consult/getArticleWithIdAndNum",
            "/search/canonicalArticleVersion",
            "/consult/getArticleByCid",
            "/consult/getArticle",
            "/consult/lawDecree",
        ];
        let mut total = 0;
        for name in names {
            total += calls(&rig, name).await;
        }
        assert!(total <= 12, "{total}");
        assert_eq!(total, 4);
    }

    #[tokio::test]
    async fn a_dated_absence_with_a_cold_code_table_costs_two_list_code_calls_once() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        empty_search(&rig).await;
        serve(&rig, "/list/code", &page_request(1), json_reply(200, PAGE_1), 1).await;
        serve(&rig, "/list/code", &page_request(2), json_reply(200, PAGE_2), 1).await;
        by_number(&rig, LAW, "99", UNKNOWN_NUMBER, 2).await;
        toc(&rig, "2000-01-01", &toc_body(&[]), 2).await;

        let first = rig.corpus.find_article(&law(), "99", on("2000-01-01")).await;
        assert_eq!(first.unwrap_err(), not_found_in_search());
        assert_eq!(calls(&rig, "/list/code").await, 2);
        let second = rig.corpus.find_article(&law(), "99", on("2000-01-01")).await;
        assert_eq!(second.unwrap_err(), not_found_in_search());

        assert_eq!(calls(&rig, "/list/code").await, 2);
        assert_eq!(calls(&rig, "/consult/lawDecree").await, 2);
    }

    #[tokio::test]
    async fn a_failed_code_table_load_keeps_the_original_not_found() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        empty_search(&rig).await;
        serve(&rig, "/list/code", &page_request(1), json_reply(200, "{"), 1).await;
        by_number(&rig, LAW, "99", UNKNOWN_NUMBER, 1).await;
        never(&rig.server, &route("/consult/lawDecree")).await;

        let result = rig.corpus.find_article(&law(), "99", on("2000-01-01")).await;

        assert_eq!(result.unwrap_err(), not_found_in_search());
        assert_eq!(calls(&rig, "/list/code").await, 1, "le chargement a bien été tenté");
    }

    /// Garde : un code daté absent ne lit jamais de table des matières.
    #[tokio::test]
    async fn a_dated_absence_of_a_code_article_makes_no_dated_toc_read() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        empty_search(&rig).await;
        load_codes(&rig).await;
        by_number(&rig, "LEGITEXT000006070721", "99999", UNKNOWN_NUMBER, 0).await;
        never(&rig.server, &route("/consult/lawDecree")).await;

        let result = rig.corpus.find_article(&civil_code(), "99999", on("2000-01-01")).await;

        assert_eq!(result.unwrap_err(), not_found_in_search());
    }

    /// Garde : `TooLarge` sur la table des matières garde le `NotFound` d'origine.
    #[tokio::test]
    async fn find_article_on_a_date_reads_the_dated_toc_too_large_keeps_the_original_not_found() {
        let rig = Rig::tuned(|setup| setup.tuning.max_body_bytes = 100_000).await;
        token_mock(&rig.server, "tok-1", 1).await;
        empty_search(&rig).await;
        load_codes(&rig).await;
        by_number(&rig, LAW, "19", UNKNOWN_NUMBER, 0).await;
        toc(&rig, "2000-01-01", &" ".repeat(150_000), 0).await;

        let result = rig.corpus.find_article(&law(), "19", on("2000-01-01")).await;

        assert_eq!(result.unwrap_err(), not_found_in_search());
    }

    /// Garde : un échec du jour (`Today`) ne rejoue pas le repli par les versions.
    #[tokio::test]
    async fn find_article_today_miss_does_not_run_the_version_fallback() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        empty_search(&rig).await;
        by_number(&rig, LAW, "99", UNKNOWN_NUMBER, 1).await;
        never(&rig.server, &route("/search/canonicalArticleVersion")).await;
        never(&rig.server, &route("/consult/lawDecree")).await;
        never(&rig.server, &route("/consult/getArticle")).await;

        let result =
            rig.corpus.find_article(&law(), "99", ArticleAt::Today(day("2026-10-28"))).await;

        assert_eq!(result.unwrap_err(), not_found_in_search());
        assert_eq!(calls(&rig, "/consult/getArticleWithIdAndNum").await, 1);
    }

    /// Garde : un hit de la recherche dont la lecture échoue ne déclenche pas le repli.
    #[tokio::test]
    async fn find_article_search_hit_then_getarticle_not_found_does_not_run_the_fallback() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        serve(&rig, "/search", &exact_article_request("9", 1), json_reply(200, EXACT_9), 1).await;
        read_version(&rig, "LEGIARTI000006419288", UNKNOWN_NUMBER, 1).await;
        never(&rig.server, &route("/consult/getArticleWithIdAndNum")).await;
        never(&rig.server, &route("/search/canonicalArticleVersion")).await;

        let result = rig.corpus.find_article(&civil_code(), "9", on("2026-10-28")).await;

        assert!(matches!(result, Err(CorpusError::NotFound { .. })), "{result:?}");
    }

    /// Garde : version unique dont la liste canonique est illisible, avant son début : `NotFound`
    /// comme en 1.1.0, jamais une erreur nouvelle.
    #[tokio::test]
    async fn find_article_on_a_date_before_a_single_version_loda_start_stays_not_found() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        empty_search(&rig).await;
        load_codes(&rig).await;
        let single = law_article("LEGIARTI000047000001", "3", "VIGUEUR", MS_2019_01_01, None);
        by_number(&rig, LAW, "3", &single, 0).await;
        canonical(&rig, "LEGIARTI000047000001", CANONICAL_NO_BYTE, 0).await;
        serve_or_lenient(
            &rig,
            "/consult/getArticleByCid",
            &json!({"cid": "LEGIARTI000047000001"}),
            json_reply(200, r#"{"listArticle":[]}"#),
            0,
        )
        .await;
        toc(&rig, "2010-01-01", &toc_body(&[]), 0).await;

        let result = rig.corpus.find_article(&law(), "3", on("2010-01-01")).await;

        assert_eq!(result.unwrap_err(), not_found_in_search());
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
