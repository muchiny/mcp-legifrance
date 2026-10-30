//! Mesures de performance du binaire `mcp-legifrance` (06 §12, T-M5-07, V-18). Marquées
//! `#[ignore = "perf"]` : le job nocturne `extended` les lance (`--run-ignored only`, filtre
//! `test(/^perf_/)`) sur un binaire release ; elles impriment leurs mesures et n'échouent
//! qu'au-delà de trois fois la cible, pour signaler une régression grossière sans rendre la CI
//! instable (06 §12.1).
//!
//! Tout le fichier est sous `cfg(test)` : les aides partagent les règles de lint des tests
//! (`unwrap` et `expect` permis, `clippy.toml`).

#[cfg(test)]
mod common;

/// Mémoire résidente, lue dans `/proc/<pid>/status` (Linux).
#[cfg(test)]
#[cfg(target_os = "linux")]
mod memory {
    use crate::common::{FakePiste, Launch, Session, call};
    use serde_json::json;

    /// Cible de 06 §12.1 : 20 Mio de mémoire résidente au repos (proposition).
    const TARGET_KIB: u64 = 20 * 1024;

    /// Au-delà de trois fois la cible, le test échoue (06 §12.1).
    const LIMIT_KIB: u64 = 3 * TARGET_KIB;

    /// Lancements mesurés : la médiane et le maximum sont imprimés.
    const LAUNCHES: usize = 5;

    /// Valeur en kio d'un champ de `/proc/<pid>/status` (`VmRSS`, `VmHWM`).
    fn status_kib(pid: u32, field: &str) -> u64 {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).expect("/proc status");
        let line = status
            .lines()
            .find_map(|line| line.strip_prefix(field).and_then(|rest| rest.strip_prefix(':')))
            .unwrap_or_else(|| crate::common::fail(&format!("no {field} in /proc/{pid}/status")));
        line.split_whitespace().next().and_then(|kib| kib.parse().ok()).expect("a size in kB")
    }

    /// Un serveur après `server/discover`, `initialize` et `tools/list` : la mémoire résidente et
    /// son pic, en kio.
    async fn idle_memory(session: &mut Session) -> (u64, u64) {
        session.handshake().await;
        let tools = session.request(&crate::common::method_request(2, "tools/list")).await;
        assert!(tools["result"]["tools"].is_array(), "{tools:.300}");
        let coverage = session.request(&call(3, "fr_coverage", &json!({}))).await;
        assert!(coverage["result"]["structuredContent"].is_object(), "{coverage:.300}");
        let pid = session.pid();
        (status_kib(pid, "VmRSS"), status_kib(pid, "VmHWM"))
    }

    #[expect(
        clippy::print_stdout,
        reason = "mesure : les valeurs sont le produit du test (06 §12.3)"
    )]
    fn report(samples: &[(u64, u64)]) {
        let mut rss: Vec<u64> = samples.iter().map(|(rss, _)| *rss).collect();
        rss.sort_unstable();
        let peak = samples.iter().map(|(_, peak)| *peak).max().unwrap_or(0);
        println!(
            "perf_rss_idle: VmRSS min {} kB, median {} kB, max {} kB; VmHWM max {peak} kB; target {TARGET_KIB} kB, limit {LIMIT_KIB} kB ({} launches)",
            rss[0],
            rss[rss.len() / 2],
            rss[rss.len() - 1],
            samples.len()
        );
    }

    /// V-18 (mémoire) : la mémoire résidente du serveur au repos, après la poignée de main, la
    /// liste des outils et un appel local, reste sous trois fois la cible de 20 Mio. Le chiffre
    /// de référence est celui d'un binaire release (job `extended`) ; en profil de test, il est
    /// plus grand mais la borne reste celle de 06 §12.1.
    #[tokio::test]
    #[ignore = "perf"]
    async fn perf_rss_idle() {
        let mut samples = Vec::with_capacity(LAUNCHES);
        for _ in 0..LAUNCHES {
            let audit = tempfile::tempdir().expect("a temporary directory");
            let piste = FakePiste::silent().await;
            let mut session = Launch::new(audit.path()).credentials().piste(&piste).spawn();
            samples.push(idle_memory(&mut session).await);
            let finished = session.finish().await;
            assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        }
        report(&samples);
        for (rss, peak) in &samples {
            assert!(*rss <= LIMIT_KIB, "VmRSS {rss} kB is above three times the 20 MiB target");
            assert!(*peak >= *rss, "VmHWM {peak} kB below VmRSS {rss} kB");
        }
    }
}
