//! [`ServicePublicGuidance`] : le port `PublicGuidance` servi par les archives du jour de la DILA
//! (ADR-0033 ; 05 §5.16 et §5.17 ; T-MS-25).
//!
//! - **Recherche** : l'index de chaque public demandé (`all` : les deux archives se chargent
//!   ensemble, et la recherche échoue si l'une manque), puis la recherche du domaine, sur un fil
//!   bloquant. Aucune requête ne porte un mot cherché : la seule requête est le téléchargement de
//!   l'archive.
//! - **Lecture** : l'entrée `{numéro}.xml` de l'archive du public, lue en mémoire sur un fil
//!   bloquant, traduite en page du domaine avec son chemin (menu du public). Entrée absente :
//!   `NotFound` (le cas d'usage nomme l'autre public) ; au-delà de 2 Mio : `EntryTooLarge` avec
//!   l'adresse publique ; entrée corrompue ou page refusée : `Decode`.

use std::{future::Future, sync::Arc, time::Duration};

use lf_application::ports::{
    ArchiveFile, Clock, GuidanceError, GuidanceFound, GuidanceRead, PublicGuidance,
};
use lf_domain::guidance::{
    Audience, GuidanceDoc, GuidanceQuery, ServicePublicUrl, SheetId, search,
};

use crate::{
    archive::{ENTRY_MAX_BYTES, EntryError},
    client::ArchiveClient,
    page,
    store::{Fetch, Loaded, Store, off_thread},
    xml,
};

/// Réglages de la source, posés par le binaire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServicePublicConfig {
    user_agent: String,
    tool_deadline: Duration,
}

impl ServicePublicConfig {
    /// `user_agent` (en-tête `User-Agent` du téléchargement) et `tool_deadline` (échéance d'un
    /// appel d'outil quand aucune n'est publiée, `LF_TOOL_DEADLINE_S`).
    pub fn new(user_agent: String, tool_deadline: Duration) -> Self {
        Self { user_agent, tool_deadline }
    }
}

/// Fiches pratiques de Service-Public.gouv.fr, lues dans l'archive du jour de chaque public.
///
/// `new` ne fait aucune E/S : rien n'est téléchargé avant le premier appel, et le client HTTP naît
/// avec le premier téléchargement. Une instance par processus : elle garde les archives.
pub struct ServicePublicGuidance {
    source: Source<ArchiveClient>,
}

impl ServicePublicGuidance {
    /// Source de production : les deux adresses fixes de la DILA, `https` seul. `clock` donne
    /// l'heure civile, qui date les archives même quand la machine a dormi.
    pub fn new(config: &ServicePublicConfig, clock: impl Clock) -> Self {
        Self {
            source: Source::new(
                ArchiveClient::production(&config.user_agent),
                config.tool_deadline,
                Arc::new(clock),
            ),
        }
    }

    /// Source d'un serveur de test, en `http`. Compilé pour les tests de ce crate seulement.
    #[cfg(test)]
    pub(crate) fn local(base: &str, tool_deadline: Duration) -> Self {
        let client = ArchiveClient::local(base, tool_deadline, crate::client::ARCHIVE_MAX_BYTES);
        let clock = Arc::new(crate::testing::TestWall::at(crate::testing::MIDDAY));
        Self { source: Source::new(client, tool_deadline, clock) }
    }
}

impl PublicGuidance for ServicePublicGuidance {
    fn search(
        &self,
        query: &GuidanceQuery,
    ) -> impl Future<Output = Result<GuidanceFound, GuidanceError>> + Send {
        self.source.search(query)
    }

    fn get(
        &self,
        audience: Audience,
        id: &SheetId,
    ) -> impl Future<Output = Result<GuidanceRead, GuidanceError>> + Send {
        self.source.get(audience, id)
    }
}

/// La source, sur n'importe quel téléchargeur : le client en production, un faux dans les tests.
pub(crate) struct Source<F> {
    store: Store<F>,
}

impl<F: Fetch> Source<F> {
    pub(crate) fn new(fetcher: F, tool_deadline: Duration, clock: Arc<dyn Clock>) -> Self {
        Self { store: Store::new(fetcher, tool_deadline, clock) }
    }

    /// Recherche dans les index des publics de la requête.
    pub(crate) async fn search(
        &self,
        query: &GuidanceQuery,
    ) -> Result<GuidanceFound, GuidanceError> {
        let audiences = query.audience.audiences();
        for audience in audiences {
            self.store.prepare(*audience);
        }
        let mut loaded: Vec<(Audience, Arc<Loaded>)> = Vec::with_capacity(audiences.len());
        for audience in audiences {
            loaded.push((*audience, self.store.current(*audience).await?));
        }
        let files = loaded.iter().map(|(audience, archive)| file(*audience, archive)).collect();
        // Sur un fil bloquant : le classement parcourt les index, et ne doit pas retenir un fil
        // du runtime.
        let query = query.clone();
        off_thread(move || {
            let indexes = loaded.iter().map(|(audience, archive)| (*audience, &archive.index));
            Ok(search(indexes, &query))
        })
        .await
        .map(|page| GuidanceFound { page, files })
    }

    /// Lecture de la page `id` dans l'archive de `audience`.
    pub(crate) async fn get(
        &self,
        audience: Audience,
        id: &SheetId,
    ) -> Result<GuidanceRead, GuidanceError> {
        let archive = self.store.current(audience).await?;
        let file = file(audience, &archive);
        let id = id.clone();
        let doc = off_thread(move || read(&archive, audience, &id)).await?;
        Ok(GuidanceRead { doc, file })
    }
}

/// L'archive d'un public, pour les mentions de la réponse.
fn file(audience: Audience, archive: &Loaded) -> ArchiveFile {
    ArchiveFile { audience, last_modified: archive.last_modified }
}

/// Lit et traduit l'entrée `{id}.xml` de l'archive.
fn read(archive: &Loaded, audience: Audience, id: &SheetId) -> Result<GuidanceDoc, GuidanceError> {
    let name = format!("{}.xml", id.as_str());
    let bytes = match archive.archive.read(&name, ENTRY_MAX_BYTES) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Err(GuidanceError::NotFound),
        Err(EntryError::TooLarge) => {
            // La grammaire de l'adresse publique ne dépend que du public et du numéro.
            let source_url = ServicePublicUrl::read(None, audience, id);
            return Err(GuidanceError::EntryTooLarge { source_url });
        }
        Err(EntryError::Corrupt) => return Err(refused("archive entry is corrupt")),
    };
    let root = xml::parse(&bytes).map_err(|error| refused(&format!("page refused: {error}")))?;
    page::page(&root, audience, id, &archive.menu)
        .map_err(|error| refused(&format!("page refused: {error}")))
}

/// Erreur de lecture au libellé fixe, sans contenu lu.
fn refused(detail: &str) -> GuidanceError {
    GuidanceError::Decode { detail: detail.to_owned() }
}

#[cfg(test)]
mod tests;
