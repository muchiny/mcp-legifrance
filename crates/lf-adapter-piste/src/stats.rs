//! Compteurs en mémoire de l'adapter (04 §10.5, ADR-0005) : jamais exportés, seulement lus
//! par `PisteCorpus::stats()` sous forme d'instantané.
//!
//! Un seul compteur `mojibake_unrepaired` couvre les deux causes d'un corps gardé tel quel
//! (réparation coupée, réparation refusée par le domaine).

use std::sync::atomic::{AtomicU64, Ordering};

use crate::encoding::DecodedBody;

/// Instantané des compteurs de l'adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct StatsSnapshot {
    /// Corps de réponse dont le mojibake a été réparé.
    pub mojibake_repairs: u64,
    /// Corps de réponse où le mojibake a été détecté mais gardé tel quel.
    pub mojibake_unrepaired: u64,
}

/// Compteurs partagés de l'adapter.
#[derive(Debug, Default)]
pub(crate) struct Stats {
    mojibake_repairs: AtomicU64,
    mojibake_unrepaired: AtomicU64,
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

    /// Instantané des compteurs.
    pub(crate) fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            mojibake_repairs: self.mojibake_repairs.load(Ordering::Relaxed),
            mojibake_unrepaired: self.mojibake_unrepaired.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};

    use tracing_subscriber::fmt::MakeWriter;

    use super::*;
    use crate::encoding::decode_body;

    /// Journal capté en mémoire : sert à la fois de `io::Write` et de `MakeWriter`.
    #[derive(Clone, Default)]
    struct Capture {
        bytes: Arc<Mutex<Vec<u8>>>,
    }

    impl Capture {
        fn text(&self) -> String {
            io::Write::flush(&mut self.clone()).unwrap();
            String::from_utf8(self.bytes.lock().unwrap().clone()).unwrap()
        }
    }

    impl io::Write for Capture {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.bytes.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl MakeWriter<'_> for Capture {
        type Writer = Capture;

        fn make_writer(&self) -> Self::Writer {
            self.clone()
        }
    }

    /// Exécute `run` sous un abonné local au thread et rend le journal produit.
    fn capturing(run: impl FnOnce()) -> Capture {
        let capture = Capture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_max_level(tracing::Level::TRACE)
            .finish();
        tracing::subscriber::with_default(subscriber, run);
        capture
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

    #[test]
    fn new_stats_start_at_zero() {
        assert_eq!(
            Stats::default().snapshot(),
            StatsSnapshot { mojibake_repairs: 0, mojibake_unrepaired: 0 }
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
}
