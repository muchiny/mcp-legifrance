//! Compteurs en mémoire de l'adapter (04 §9.3, §10.5, ADR-0005) : jamais exportés, seulement lus
//! par `PisteCorpus::stats()` sous forme d'instantané.
//!
//! Un seul compteur `mojibake_unrepaired` couvre les deux causes d'un corps gardé tel quel
//! (réparation coupée, réparation refusée par le domaine). Les neuf compteurs de mapping
//! reçoivent les [`Diagnostics`] d'un mapping par `record_mapping`, qui journalise une ligne par
//! compteur non nul : l'endpoint et le nombre, jamais un identifiant ni une valeur du corps.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::encoding::DecodedBody;
use crate::map::Diagnostics;

/// Instantané des compteurs de l'adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct StatsSnapshot {
    /// Corps de réponse dont le mojibake a été réparé.
    pub mojibake_repairs: u64,
    /// Corps de réponse où le mojibake a été détecté mais gardé tel quel.
    pub mojibake_unrepaired: u64,
    /// Résultats de recherche écartés : identifiant invalide ou d'un autre fond.
    pub dropped_hits: u64,
    /// Réponses dont l'identifiant renvoyé manque ou diffère de celui demandé.
    pub id_mismatch: u64,
    /// Horodatages qui ne tombent pas à minuit UTC, lus comme jours civils UTC.
    pub non_midnight_timestamps: u64,
    /// Suffixes de version retirés d'identifiants autres que `LEGITEXT`.
    pub upstream_suffix_stripped: u64,
    /// Champs d'origine en désaccord avec la famille de l'identifiant.
    pub origin_mismatch: u64,
    /// ECLI natifs refusés et omis.
    pub invalid_native_ecli: u64,
    /// NOR refusés et omis.
    pub invalid_nor: u64,
    /// Entrées de table des matières écartées : identifiant invalide ou en double.
    pub toc_dropped: u64,
    /// Caractères invisibles ou bidirectionnels retirés des textes.
    pub invisible_chars_removed: u64,
}

/// Compteurs partagés de l'adapter.
#[derive(Debug, Default)]
pub(crate) struct Stats {
    mojibake_repairs: AtomicU64,
    mojibake_unrepaired: AtomicU64,
    dropped_hits: AtomicU64,
    id_mismatch: AtomicU64,
    non_midnight_timestamps: AtomicU64,
    upstream_suffix_stripped: AtomicU64,
    origin_mismatch: AtomicU64,
    invalid_native_ecli: AtomicU64,
    invalid_nor: AtomicU64,
    toc_dropped: AtomicU64,
    invisible_chars_removed: AtomicU64,
}

impl Stats {
    /// Compte et journalise un corps décodé.
    ///
    /// Le journal ne reçoit que le gabarit de chemin et la taille en octets du texte rendu,
    /// jamais le texte.
    pub(crate) fn record_decode(&self, endpoint: &str, body: &DecodedBody) {
        if body.repaired {
            self.mojibake_repairs.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                endpoint,
                bytes = body.text.len(),
                "latin1 mojibake repaired in PISTE response"
            );
        } else if body.detected {
            self.mojibake_unrepaired.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                endpoint,
                bytes = body.text.len(),
                "latin1 mojibake detected but left as is in PISTE response"
            );
        }
    }

    /// Ajoute les diagnostics d'un mapping de `endpoint` à leurs compteurs, puis journalise une
    /// ligne par compteur non nul (`log_mapping`).
    pub(crate) fn record_mapping(&self, endpoint: &str, diags: &Diagnostics) {
        add(&self.dropped_hits, diags.dropped_hits);
        add(&self.id_mismatch, diags.id_mismatch);
        add(&self.non_midnight_timestamps, diags.non_midnight_timestamps);
        add(&self.upstream_suffix_stripped, diags.upstream_suffix_stripped);
        add(&self.origin_mismatch, diags.origin_mismatch);
        add(&self.invalid_native_ecli, diags.invalid_native_ecli);
        add(&self.invalid_nor, diags.invalid_nor);
        add(&self.toc_dropped, diags.toc_dropped);
        add(&self.invisible_chars_removed, diags.invisible_chars_removed);
        log_mapping(endpoint, diags);
    }

    /// Instantané des compteurs.
    pub(crate) fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            mojibake_repairs: self.mojibake_repairs.load(Ordering::Relaxed),
            mojibake_unrepaired: self.mojibake_unrepaired.load(Ordering::Relaxed),
            dropped_hits: self.dropped_hits.load(Ordering::Relaxed),
            id_mismatch: self.id_mismatch.load(Ordering::Relaxed),
            non_midnight_timestamps: self.non_midnight_timestamps.load(Ordering::Relaxed),
            upstream_suffix_stripped: self.upstream_suffix_stripped.load(Ordering::Relaxed),
            origin_mismatch: self.origin_mismatch.load(Ordering::Relaxed),
            invalid_native_ecli: self.invalid_native_ecli.load(Ordering::Relaxed),
            invalid_nor: self.invalid_nor.load(Ordering::Relaxed),
            toc_dropped: self.toc_dropped.load(Ordering::Relaxed),
            invisible_chars_removed: self.invisible_chars_removed.load(Ordering::Relaxed),
        }
    }
}

/// Ajoute `n` au compteur.
fn add(counter: &AtomicU64, n: u32) {
    counter.fetch_add(u64::from(n), Ordering::Relaxed);
}

/// Journalise une ligne par compteur non nul d'un mapping de `endpoint` : `warn` pour un
/// identifiant incohérent, un horodatage hors minuit ou une origine en désaccord, `debug` sinon.
///
/// Chaque ligne ne porte que le gabarit de chemin et le nombre, jamais un identifiant ni une
/// valeur du corps.
fn log_mapping(endpoint: &str, diags: &Diagnostics) {
    if diags.dropped_hits > 0 {
        tracing::debug!(
            endpoint,
            count = diags.dropped_hits,
            "search hits dropped: invalid identifier or identifier of another fond"
        );
    }
    if diags.id_mismatch > 0 {
        tracing::warn!(
            endpoint,
            count = diags.id_mismatch,
            "returned identifier is missing or differs from the requested one"
        );
    }
    if diags.non_midnight_timestamps > 0 {
        tracing::warn!(
            endpoint,
            count = diags.non_midnight_timestamps,
            "timestamps not at midnight UTC were read as UTC calendar days"
        );
    }
    if diags.upstream_suffix_stripped > 0 {
        tracing::debug!(
            endpoint,
            count = diags.upstream_suffix_stripped,
            "version suffix stripped from identifiers other than LEGITEXT"
        );
    }
    if diags.origin_mismatch > 0 {
        tracing::warn!(
            endpoint,
            count = diags.origin_mismatch,
            "origin field disagrees with the identifier family"
        );
    }
    if diags.invalid_native_ecli > 0 {
        tracing::debug!(
            endpoint,
            count = diags.invalid_native_ecli,
            "native ECLI rejected and left out"
        );
    }
    if diags.invalid_nor > 0 {
        tracing::debug!(endpoint, count = diags.invalid_nor, "NOR rejected and left out");
    }
    if diags.toc_dropped > 0 {
        tracing::debug!(
            endpoint,
            count = diags.toc_dropped,
            "table of contents entries dropped: invalid or duplicate identifier"
        );
    }
    if diags.invisible_chars_removed > 0 {
        tracing::debug!(
            endpoint,
            count = diags.invisible_chars_removed,
            "invisible or bidirectional characters removed from text"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::io;
    use std::sync::Once;

    use tracing_subscriber::fmt::MakeWriter;

    use super::*;
    use crate::encoding::decode_body;

    thread_local! {
        /// Journal du thread courant : chaque test tourne sur son propre thread.
        static JOURNAL: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    }

    /// Écrit dans le journal du thread courant : sert à la fois de `io::Write` et de `MakeWriter`.
    #[derive(Clone, Copy)]
    struct ThreadLog;

    impl io::Write for ThreadLog {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            JOURNAL.with_borrow_mut(|journal| journal.extend_from_slice(buf));
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl MakeWriter<'_> for ThreadLog {
        type Writer = ThreadLog;

        fn make_writer(&self) -> Self::Writer {
            *self
        }
    }

    /// Journal capté pendant un appel à `capturing`.
    struct Capture {
        text: String,
    }

    impl Capture {
        fn text(&self) -> String {
            self.text.clone()
        }
    }

    /// Exécute `run` et rend le journal que ce thread a produit.
    ///
    /// L'abonné est global et installé une seule fois : un abonné local (`with_default`) change
    /// l'intérêt mis en cache par chaque point d'instrumentation, et un test voisin qui passe
    /// par le même point sans abonné pouvait le figer sur « jamais », d'où des lignes perdues.
    fn capturing(run: impl FnOnce()) -> Capture {
        static INSTALL: Once = Once::new();
        INSTALL.call_once(|| {
            let subscriber = tracing_subscriber::fmt()
                .with_writer(ThreadLog)
                .with_max_level(tracing::Level::TRACE)
                .finish();
            tracing::subscriber::set_global_default(subscriber).unwrap();
        });
        JOURNAL.with_borrow_mut(Vec::clear);
        run();
        io::Write::flush(&mut ThreadLog).unwrap();
        let bytes = JOURNAL.with_borrow_mut(std::mem::take);
        Capture { text: String::from_utf8(bytes).unwrap() }
    }

    /// Corps JSON dont le texte est du mojibake synthétique (octets UTF-8 relus en latin-1).
    fn mojibake_json(texte: &str) -> String {
        let mojibake: String = texte.bytes().map(char::from).collect();
        format!(r#"{{"texte":"{mojibake}"}}"#)
    }

    fn counters(stats: &Stats) -> (u64, u64) {
        let snapshot = stats.snapshot();
        (snapshot.mojibake_repairs, snapshot.mojibake_unrepaired)
    }

    /// Diagnostics aux neuf compteurs non nuls et distincts.
    fn primes() -> Diagnostics {
        Diagnostics {
            dropped_hits: 2,
            id_mismatch: 3,
            non_midnight_timestamps: 5,
            upstream_suffix_stripped: 7,
            origin_mismatch: 11,
            invalid_native_ecli: 13,
            invalid_nor: 17,
            toc_dropped: 19,
            invisible_chars_removed: 23,
        }
    }

    /// Instantané attendu après `times` appels de `record_mapping` avec [`primes`].
    fn primes_times(times: u64) -> StatsSnapshot {
        StatsSnapshot {
            mojibake_repairs: 0,
            mojibake_unrepaired: 0,
            dropped_hits: 2 * times,
            id_mismatch: 3 * times,
            non_midnight_timestamps: 5 * times,
            upstream_suffix_stripped: 7 * times,
            origin_mismatch: 11 * times,
            invalid_native_ecli: 13 * times,
            invalid_nor: 17 * times,
            toc_dropped: 19 * times,
            invisible_chars_removed: 23 * times,
        }
    }

    #[test]
    fn new_stats_start_at_zero() {
        assert_eq!(
            Stats::default().snapshot(),
            StatsSnapshot {
                mojibake_repairs: 0,
                mojibake_unrepaired: 0,
                dropped_hits: 0,
                id_mismatch: 0,
                non_midnight_timestamps: 0,
                upstream_suffix_stripped: 0,
                origin_mismatch: 0,
                invalid_native_ecli: 0,
                invalid_nor: 0,
                toc_dropped: 0,
                invisible_chars_removed: 0,
            }
        );
    }

    #[test]
    fn mojibake_body_is_repaired_and_counted() {
        let stats = Stats::default();
        let body = mojibake_json("PREMIÈRE CHAMBRE CIVILE");
        let decoded = decode_body(body.as_bytes(), "/consult/juri", true).unwrap();
        let capture = capturing(|| stats.record_decode("/consult/juri", &decoded));
        assert_eq!(decoded.text, r#"{"texte":"PREMIÈRE CHAMBRE CIVILE"}"#);
        assert!(decoded.repaired);
        assert_eq!(counters(&stats), (1, 0));
        let log = capture.text();
        assert!(log.contains("WARN"), "{log}");
        assert!(log.contains(r#"endpoint="/consult/juri""#), "{log}");
        assert!(log.contains(&format!("bytes={}", decoded.text.len())), "{log}");
    }

    #[test]
    fn every_repaired_body_is_counted() {
        let stats = Stats::default();
        let decoded = DecodedBody { text: "x".into(), detected: true, repaired: true };
        let capture = capturing(|| {
            stats.record_decode("/search", &decoded);
            stats.record_decode("/search", &decoded);
        });
        assert_eq!(counters(&stats), (2, 0));
        assert_eq!(capture.text().matches("WARN").count(), 2);
    }

    #[test]
    fn clean_body_is_not_counted_nor_logged() {
        let stats = Stats::default();
        let decoded = DecodedBody { text: "x".into(), detected: false, repaired: false };
        let capture = capturing(|| stats.record_decode("/search", &decoded));
        assert_eq!(counters(&stats), (0, 0));
        assert_eq!(capture.text(), "");
    }

    #[test]
    fn warn_never_contains_body_text() {
        let stats = Stats::default();
        let decoded =
            DecodedBody { text: "CORPS-CONFIDENTIEL-È".into(), detected: true, repaired: true };
        let capture = capturing(|| stats.record_decode("/search", &decoded));
        let log = capture.text();
        assert!(log.contains("WARN"), "{log}");
        assert!(!log.contains("CONFIDENTIEL"), "{log}");
        assert!(!log.contains('È'), "{log}");
    }

    #[test]
    fn repair_toggle_off_keeps_detection() {
        let stats = Stats::default();
        let body = mojibake_json("PREMIÈRE CHAMBRE CIVILE");
        let decoded = decode_body(body.as_bytes(), "/consult/juri", false).unwrap();
        let capture = capturing(|| stats.record_decode("/consult/juri", &decoded));
        assert_eq!(decoded.text, body);
        assert_eq!(counters(&stats), (0, 1));
        let log = capture.text();
        assert!(log.contains("WARN"), "{log}");
        assert!(log.contains(r#"endpoint="/consult/juri""#), "{log}");
        assert!(!log.contains("PREMI"), "{log}");
    }

    #[test]
    fn unrepairable_mojibake_is_counted_and_body_kept() {
        let stats = Stats::default();
        let body = "{\"texte\":\"CONFIDENTIEL R\u{c3}\u{a9}publique \u{20ac}\"}";
        let decoded = decode_body(body.as_bytes(), "/consult/juri", true).unwrap();
        let capture = capturing(|| stats.record_decode("/consult/juri", &decoded));
        assert_eq!(decoded.text, body);
        assert_eq!(counters(&stats), (0, 1));
        let log = capture.text();
        assert!(log.contains("WARN"), "{log}");
        assert!(!log.contains("CONFIDENTIEL"), "{log}");
    }

    #[test]
    fn repaired_and_unrepaired_counters_are_independent() {
        let stats = Stats::default();
        let repaired = DecodedBody { text: "x".into(), detected: true, repaired: true };
        let kept = DecodedBody { text: "x".into(), detected: true, repaired: false };
        let capture = capturing(|| {
            stats.record_decode("/search", &repaired);
            stats.record_decode("/search", &kept);
            stats.record_decode("/search", &kept);
        });
        assert_eq!(counters(&stats), (1, 2));
        assert_eq!(capture.text().matches("WARN").count(), 3);
    }

    #[test]
    fn record_mapping_adds_every_counter_to_its_own_field() {
        let stats = Stats::default();
        stats.record_mapping("/consult/juri", &primes());
        assert_eq!(stats.snapshot(), primes_times(1));
        stats.record_mapping("/consult/juri", &primes());
        assert_eq!(stats.snapshot(), primes_times(2));
    }

    #[test]
    fn record_mapping_logs_each_diagnostic_at_its_level() {
        let zero = Diagnostics::default();
        let cases = [
            (
                Diagnostics { dropped_hits: 42, ..zero },
                "DEBUG",
                "search hits dropped: invalid identifier or identifier of another fond",
            ),
            (
                Diagnostics { id_mismatch: 42, ..zero },
                "WARN",
                "returned identifier is missing or differs from the requested one",
            ),
            (
                Diagnostics { non_midnight_timestamps: 42, ..zero },
                "WARN",
                "timestamps not at midnight UTC were read as UTC calendar days",
            ),
            (
                Diagnostics { upstream_suffix_stripped: 42, ..zero },
                "DEBUG",
                "version suffix stripped from identifiers other than LEGITEXT",
            ),
            (
                Diagnostics { origin_mismatch: 42, ..zero },
                "WARN",
                "origin field disagrees with the identifier family",
            ),
            (
                Diagnostics { invalid_native_ecli: 42, ..zero },
                "DEBUG",
                "native ECLI rejected and left out",
            ),
            (Diagnostics { invalid_nor: 42, ..zero }, "DEBUG", "NOR rejected and left out"),
            (
                Diagnostics { toc_dropped: 42, ..zero },
                "DEBUG",
                "table of contents entries dropped: invalid or duplicate identifier",
            ),
            (
                Diagnostics { invisible_chars_removed: 42, ..zero },
                "DEBUG",
                "invisible or bidirectional characters removed from text",
            ),
        ];
        for (diags, level, message) in cases {
            let stats = Stats::default();
            let log = capturing(|| stats.record_mapping("/consult/juri", &diags)).text();
            assert_eq!(log.lines().count(), 1, "{log}");
            assert!(log.contains(level), "{level}: {log}");
            assert!(log.contains(message), "{message}: {log}");
            assert!(log.contains("count=42"), "{log}");
            assert!(log.contains(r#"endpoint="/consult/juri""#), "{log}");
        }
    }

    #[test]
    fn record_mapping_is_silent_without_diagnostics() {
        let stats = Stats::default();
        let capture = capturing(|| stats.record_mapping("/consult/juri", &Diagnostics::default()));
        assert_eq!(capture.text(), "");
        assert_eq!(stats.snapshot(), StatsSnapshot::default());
    }

    #[test]
    fn mapping_log_carries_no_identifier_or_body_text() {
        let stats = Stats::default();
        let log = capturing(|| stats.record_mapping("/consult/juri", &primes())).text();
        assert_eq!(log.lines().count(), 9, "{log}");
        assert_eq!(log.matches('"').count(), 18, "{log}");
        // Le message fixe de `upstream_suffix_stripped` nomme la famille `LEGITEXT` : c'est la
        // seule occurrence admise, tout identifiant `LEGITEXT` journalisé en ajouterait une.
        assert_eq!(log.matches("LEGITEXT").count(), 1, "{log}");
        assert!(!log.contains("JURITEXT"), "{log}");
    }
}
