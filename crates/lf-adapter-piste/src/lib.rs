//! Adapter PISTE du serveur MCP Légifrance (04, 02 §3.3).
//!
//! Rôle : `PisteCorpus`, seule implémentation du port `LegalCorpus` jusqu'en M9. Il parle
//! HTTP à l'API Légifrance via PISTE (OAuth 2.0 `client_credentials`), avec limiteur de
//! débit et réessais, et traduit les réponses en types du domaine.
//!
//! Livré au plan 2c : le squelette, les traits de temps, la configuration, l'authentification,
//! le client HTTP, la résilience et `PisteCorpus` (assemblage, mode dégradé, appel de l'API de
//! bout en bout). Livré au plan 2d : la configuration à champs privés et les réglages validés, le
//! décodage des corps JSON sans valeur du corps, le corps d'erreur de PISTE et la classification
//! « introuvable », les DTO de réponse de `/search` et de `/consult`, le corps de la requête
//! `/search` avec le midi Europe/Paris et le mémo de `typePagination`. Livré au plan 2f : le budget
//! de décodage (200 000 éléments par document, listes de DTO bornées, `results` plafonné,
//! `typePagination` tolérant, `num` lu en chaîne ou en nombre), puis le mapping des DTO vers le
//! domaine (`map::{rules, dates, search, body, toc, act, convention, article, decision,
//! deliberation, agreement}`) et ses compteurs (`Stats::record_mapping`). Livré au plan 2g : les
//! corps des consultations (`request::consult`) et `impl LegalCorpus for PisteCorpus`
//! (`corpus::port`), qui branche le décodage et le mapping sous le permis de concurrence ; les DTO
//! ne déclarent plus que les champs que le mapping lit.
//!
//! Les modules sont privés : sont publics `PisteCorpus`, les types de configuration et
//! `StatsSnapshot` (et le module `record`, derrière sa feature).
//!
//! Modules :
//! - `auth` : jeton d'accès OAuth 2.0 `client_credentials` (`TokenManager`) : un seul jeton en
//!   mémoire, renouvelé avant son expiration par une seule requête à la fois, avec réessais.
//! - `classify` : classification d'une tentative d'envoi (`Outcome`, `Budget`, `EndpointKind`,
//!   `ErrorContext`) d'après le statut HTTP et `Retry-After` ; le corps n'est lu que pour un 400
//!   de consultation, un 404 de consultation et un 2xx vide (« introuvable », 04 §11).
//! - `config` : configuration de l'adapter (`PisteConfig`, champs privés, réglages avancés
//!   validés `PisteTuning`) et validation des URL surchargées.
//! - `corpus` : `PisteCorpus`, l'assemblage de l'adapter : mode dégradé sans E/S, puis pour chaque
//!   tentative limiteur, jeton, envoi borné, classification et réessai, enfin décodage du corps,
//!   sa lecture en DTO sous budget et son mapping, sous le permis de concurrence (`fetch`) ;
//!   `corpus::port` implémente les sept opérations de `LegalCorpus` ; `corpus::raw` (feature
//!   `record`) : `PisteCorpus::exchange`, échange brut sans classement ni réessai.
//! - `deadline` : échéance d'un appel (`CallDeadline`), durée restante lue dans `Deadline`
//!   puis posée sur l'horloge monotone de l'adapter.
//! - `dto` : structures de désérialisation des réponses de PISTE (réponse et erreur de l'endpoint
//!   token, `PisteErrorDto` pour le corps d'erreur de l'API, réponses de `/search` et de
//!   `/consult/lawDecree|kaliText|getArticle|juri|cnil|acco`), avec des lecteurs serde tolérants
//!   (`bounded_list`, `capped_list`, `opt_raw_date`…), `budget` (200 000 éléments de liste par
//!   document, armé par `decode`) et `decode`, qui lit un corps JSON sous ce budget, avec le
//!   chemin de l'erreur et sans aucune valeur du corps.
//! - `encoding` : décodage UTF-8 strict du corps, détection et réparation du mojibake latin-1.
//! - `error_body` : message d'un corps d'erreur PISTE (UTF-8, mojibake, JSON facultatif), détail
//!   nettoyé, privé des secrets puis borné, marqueur « introuvable », détection d'une page HTML.
//! - `golden` (tests seulement) : harnais du golden de parité (annexe A §4, §5) : les dix-neuf
//!   divergences volontaires (`GoldenDivergence`), dont les seules clés sont rapprochées de
//!   l'annexe A par machine, les six valeurs explicites (`OVERRIDES`, `overridden`) et la copie
//!   du golden comparée à l'octet.
//! - `http` : client HTTP unique (`reqwest`, TLS rustls), tentative d'envoi (`send_once`) et
//!   lecture bornée du corps ; aucun réessai ni jugement de statut.
//! - `integrity` (tests seulement) : intégrité des fixtures de référence et des fixtures
//!   enregistrées (04 §13.4) ; SHA-256 de `sha256` contre les `sha256_raw` des sidecars
//!   `.meta.toml`, clés et valeurs des sidecars, contenu du répertoire, absence de chaîne de
//!   forme secret (mots d'authentification, UUID, suite hexadécimale, JWT, base64 long) dans les
//!   fixtures, les snapshots et le golden (D-35).
//! - `limit` : barrière de charge sortante (`RateGate`), sémaphore de concurrence puis limiteur
//!   de débit `governor` sur l'horloge de l'adapter ; toutes les attentes bornées par le délai.
//! - `map` : mapping des DTO vers le domaine (04 §9), fonctions pures sans valeur du corps dans
//!   les erreurs. Socle : gabarits de chemin des endpoints (`endpoint`), anomalies d'un mapping
//!   comptées (`Diagnostics`, versées à `Stats::record_mapping`) et erreur d'un champ refusé
//!   (`Field`, `not_found`). Règles partagées : `map::rules` (identité, identifiant renvoyé,
//!   suffixe de version, origine, texte long) et `map::dates` (dates brutes en jours civils UTC,
//!   sentinelles de l'API écartées, horodatages hors minuit comptés). Mappers : `map::search`
//!   (page de `/search`), `map::body` et `map::toc` (métadonnées et table des matières communes
//!   aux textes consolidés), `map::act` (`/consult/lawDecree`), `map::convention`
//!   (`/consult/kaliText`), `map::article` (`/consult/getArticle`), `map::decision`
//!   (`/consult/juri`), `map::deliberation` (`/consult/cnil`), `map::agreement` (`/consult/acco`).
//!   `map::fixtures` (tests seulement) sert les seize réponses brutes de référence, les mène
//!   jusqu'à l'entité par le chemin de production et porte les aides communes des tests.
//! - `memo` : mémo de `typePagination` (`PaginationMemo`) : dernière valeur reçue par requête sans
//!   page, 1 h, 1 024 entrées ; décision pure `choose_mode` et lecture tolérante `received_mode`
//!   (04 §6.6, §12.3).
//! - `record` (feature `record`) : enregistrement de fixtures (sondes, arguments, caviardage en
//!   place, sidecar, écriture ; 04 §13.4).
//! - `request` : corps des requêtes envoyées à PISTE ; `request::search` construit celui de
//!   `/search` (`search_body`) et calcule le midi Europe/Paris du filtre `DATE_VERSION` (04 §6) ;
//!   `request::consult` construit ceux des six consultations (`Consultation`, identifiant dans le
//!   corps, jamais dans le chemin).
//! - `retry` : politique de réessai (`run`) : trois requêtes au plus, délai exponentiel avec gigue,
//!   `Retry-After`, bornée par le délai global ; boucle maison, sans `backon`.
//! - `sha256` (tests et feature `record`) : SHA-256 FIPS 180-4 sans dépendance.
//! - `stats` : compteurs en mémoire (`Stats` : mojibake et neuf diagnostics de mapping, ces
//!   derniers journalisés par `record_mapping`) et leur instantané public (`StatsSnapshot`).
//! - `testing` (tests seulement) : aides communes, `ClosedPort` (adresse locale qui refuse la
//!   connexion sur les trois systèmes de la CI).
//! - `time` : horloge monotone (`MonoClock`), attente (`Sleeper`) et course `race`, pour des
//!   tests déterministes sans geler l'horloge `tokio`.
//! - `view` (tests seulement) : projection JSON unique des entités du domaine (`view`, tous les
//!   champs, sans `..`), chaînes d'une valeur avec leur chemin (`leaf_strings`) et contrôle
//!   `no_mojibake_after_mapping` (`mojibake_paths`, D-35).

mod auth;
mod classify;
mod config;
mod corpus;
mod deadline;
mod dto;
mod encoding;
mod error_body;
#[cfg(test)]
mod golden;
mod http;
#[cfg(test)]
mod integrity;
mod limit;
mod map;
mod memo;
#[cfg(feature = "record")]
pub mod record;
mod request;
mod retry;
#[cfg(any(test, feature = "record"))]
mod sha256;
mod stats;
#[cfg(test)]
mod testing;
mod time;
#[cfg(test)]
mod view;

pub use config::{
    ConfigProblem, ConfigWarning, PisteConfig, PisteCredentials, PisteEndpoints, PisteTuning,
    RateQuota, TuningError,
};
pub use corpus::PisteCorpus;
pub use stats::StatsSnapshot;
