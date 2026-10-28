//! Capture des journaux `tracing` pour les tests du crate (`cfg(test)` seulement).
//!
//! Partagée par `audited` (span `tool_call`, événements de fin) et `failure` (événement d'échec
//! du port) : `tracing-subscriber` n'est qu'une dépendance de développement (R6), ce module
//! n'entre donc ni dans la feature `test-support` ni dans un binaire.

use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex},
};

use tracing::{
    Level, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
    subscriber::{DefaultGuard, Interest},
};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

/// Événement capturé : niveau, message et champ `kind` s'il existe.
pub(crate) type Event = (Level, String, Option<String>);

/// Champs d'un événement ou d'un span, par nom ; une valeur non textuelle est rendue par `Debug`.
pub(crate) type FieldMap = BTreeMap<&'static str, String>;

#[derive(Default)]
struct Captured {
    /// Niveau le plus verbeux retenu ; `None` : tout.
    max_level: Option<Level>,
    events: Vec<(Level, FieldMap)>,
    spans: Vec<(Id, &'static str, FieldMap)>,
}

/// Couche `tracing` qui garde les événements et les champs des spans.
#[derive(Clone, Default)]
pub(crate) struct Capture(Arc<Mutex<Captured>>);

impl Capture {
    /// Installe la capture comme abonné du fil courant (runtime `current_thread` des tests).
    pub(crate) fn install() -> (Self, DefaultGuard) {
        let capture = Self::default();
        let subscriber = tracing_subscriber::registry().with(capture.clone());
        (capture, tracing::subscriber::set_default(subscriber))
    }

    /// Comme [`Capture::install`], mais n'active que les spans et événements de niveau
    /// `max_level` ou moins verbeux.
    pub(crate) fn install_at(max_level: Level) -> (Self, DefaultGuard) {
        let (capture, guard) = Self::install();
        capture.0.lock().unwrap().max_level = Some(max_level);
        (capture, guard)
    }

    /// Niveau, message et champ `kind` de chaque événement, dans l'ordre.
    pub(crate) fn events(&self) -> Vec<Event> {
        let captured = self.0.lock().unwrap();
        captured
            .events
            .iter()
            .map(|(level, fields)| (*level, fields["message"].clone(), fields.get("kind").cloned()))
            .collect()
    }

    /// Niveau et champs complets de chaque événement, dans l'ordre.
    pub(crate) fn event_fields(&self) -> Vec<(Level, FieldMap)> {
        self.0.lock().unwrap().events.clone()
    }

    /// Niveau de chaque événement, dans l'ordre.
    pub(crate) fn levels(&self) -> Vec<Level> {
        self.0.lock().unwrap().events.iter().map(|(level, _)| *level).collect()
    }

    /// Champs du dernier span nommé `name`.
    pub(crate) fn span(&self, name: &str) -> FieldMap {
        let captured = self.0.lock().unwrap();
        captured.spans.iter().rev().find(|(_, n, _)| *n == name).unwrap().2.clone()
    }

    /// Toutes les valeurs journalisées, événements et spans confondus.
    pub(crate) fn all_values(&self) -> Vec<String> {
        let captured = self.0.lock().unwrap();
        let events = captured.events.iter().flat_map(|(_, fields)| fields.values());
        let spans = captured.spans.iter().flat_map(|(_, _, fields)| fields.values());
        events.chain(spans).cloned().collect()
    }
}

struct Fields<'a>(&'a mut FieldMap);

impl Visit for Fields<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name(), value.to_owned());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0.insert(field.name(), format!("{value:?}"));
    }
}

impl<S: Subscriber> Layer<S> for Capture {
    /// Toujours « parfois » : le filtre est décidé à chaque appel par `enabled`, et aucun
    /// intérêt `never` n'est mis en cache pour les autres tests du processus.
    fn register_callsite(&self, _: &'static Metadata<'static>) -> Interest {
        Interest::sometimes()
    }

    fn enabled(&self, metadata: &Metadata<'_>, _: Context<'_, S>) -> bool {
        let max_level = self.0.lock().unwrap().max_level;
        max_level.is_none_or(|max| *metadata.level() <= max)
    }

    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, _: Context<'_, S>) {
        let mut fields = BTreeMap::new();
        attrs.record(&mut Fields(&mut fields));
        self.0.lock().unwrap().spans.push((id.clone(), attrs.metadata().name(), fields));
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, _: Context<'_, S>) {
        let mut captured = self.0.lock().unwrap();
        let span = captured.spans.iter_mut().rev().find(|(known, ..)| known == id).unwrap();
        values.record(&mut Fields(&mut span.2));
    }

    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        let mut fields = BTreeMap::new();
        event.record(&mut Fields(&mut fields));
        self.0.lock().unwrap().events.push((*event.metadata().level(), fields));
    }
}
