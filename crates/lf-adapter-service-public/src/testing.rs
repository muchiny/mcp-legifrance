//! Outils des tests du crate : mini-archives réelles, faux téléchargeur aux réponses écrites
//! d'avance, retenu par une barrière quand le test veut un téléchargement en cours. Compilé sous
//! `#[cfg(test)]` seulement.

use std::{
    collections::VecDeque,
    future::Future,
    sync::{Arc, Mutex},
};

use lf_application::ports::GuidanceError;
use lf_domain::guidance::Audience;
use tokio::sync::Semaphore;

use crate::{
    client::{Download, Fetched},
    store::Fetch,
};

/// Mini-archive de Particuliers (T-MS-02) : dix pages et le menu.
pub(crate) const PART: &[u8] = include_bytes!("../tests/fixtures/archives/part-vosdroits-mini.zip");

/// Mini-archive d'Entreprendre (T-MS-02) : deux pages et le menu.
pub(crate) const PRO: &[u8] = include_bytes!("../tests/fixtures/archives/pro-vosdroits-mini.zip");

pub(crate) type Reply = Result<Fetched, GuidanceError>;

/// Faux téléchargeur : réponses dans l'ordre, appels notés, barrière avant chaque réponse.
pub(crate) struct Fake {
    replies: Mutex<VecDeque<Reply>>,
    calls: Mutex<Vec<(Audience, Option<String>)>>,
    gate: Semaphore,
}

impl Fake {
    /// Répond sans attendre.
    pub(crate) fn open(replies: impl IntoIterator<Item = Reply>) -> Arc<Self> {
        Self::with(replies, Semaphore::MAX_PERMITS)
    }

    /// Retient chaque réponse jusqu'à [`Fake::release`].
    pub(crate) fn held(replies: impl IntoIterator<Item = Reply>) -> Arc<Self> {
        Self::with(replies, 0)
    }

    fn with(replies: impl IntoIterator<Item = Reply>, permits: usize) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.into_iter().collect()),
            calls: Mutex::new(Vec::new()),
            gate: Semaphore::new(permits),
        })
    }

    /// Laisse partir une réponse retenue.
    pub(crate) fn release(&self) {
        self.gate.add_permits(1);
    }

    pub(crate) fn calls(&self) -> Vec<(Audience, Option<String>)> {
        self.calls.lock().unwrap().clone()
    }
}

impl Fetch for Arc<Fake> {
    fn fetch(
        &self,
        audience: Audience,
        etag: Option<&str>,
    ) -> impl Future<Output = Result<Fetched, GuidanceError>> + Send {
        let fake = Arc::clone(self);
        fake.calls.lock().unwrap().push((audience, etag.map(str::to_owned)));
        async move {
            fake.gate.acquire().await.unwrap().forget();
            fake.replies.lock().unwrap().pop_front().unwrap()
        }
    }
}

pub(crate) fn fresh(body: &[u8], etag: &str) -> Fetched {
    Fetched::Fresh(Download {
        body: body.to_vec(),
        etag: Some(etag.to_owned()),
        last_modified: None,
    })
}

/// Panne 500 du serveur, telle que le client la rend.
pub(crate) fn server_error() -> GuidanceError {
    GuidanceError::Upstream {
        status: Some(500),
        retryable: true,
        detail: "server error".to_owned(),
    }
}

/// Un appel noté par le faux téléchargeur.
pub(crate) fn call(audience: Audience, etag: Option<&str>) -> (Audience, Option<String>) {
    (audience, etag.map(str::to_owned))
}
