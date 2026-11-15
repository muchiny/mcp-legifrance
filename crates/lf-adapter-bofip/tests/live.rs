//! Tests en direct contre le portail ouvert de la DGFiP (jeu `bofip-vigueur`, T-MB-24) : chaque
//! test qui appelle le portail porte `#[ignore = "live: calls data.economie.gouv.fr"]` et son nom
//! commence par `live_bofip_` (profil nextest `live`).
//!
//! ```text
//! cargo nextest run -p lf-adapter-bofip --test live --profile live --run-ignored only
//! ```
//!
//! Aucun secret : le portail est ouvert, sans compte ni clé. Cinq requêtes par passage, une par
//! seconde au plus (une pause d'une seconde avant chaque appel ; le profil `live` joue un test à
//! la fois). Un refus 429 n'est pas un défaut du client : le test le dit sur stderr et s'arrête
//! sans échouer, et rien n'est réessayé.
//!
//! Deux tests gardent la lecture du champ HTML (ADR-0030, V-226) : une publication depuis 2024
//! se lit en plusieurs paragraphes, et le portail double toujours chaque guillemet double du
//! champ `contenu_html`. Le second lit la réponse brute : c'est la seule requête de ce fichier
//! qui ne passe pas par le client de production. Son client suit les mêmes règles : `https`
//! seul, aucune redirection suivie, corps plafonné à 4 Mio.
//!
//! `live_job_has_no_piste_secret` n'appelle rien : il relit `live.yml` et tourne avec les tests
//! ordinaires (il n'est pas ignoré).
//!
//! Tout le fichier est sous `cfg(test)` : les aides partagent les règles de lint des tests
//! (`unwrap` permis, `clippy.toml`).

#[cfg(test)]
mod live {
    use std::{fs, num::NonZeroU32, path::Path, time::Duration};

    use chrono::DateTime;
    use lf_adapter_bofip::{BofipConfig, BofipDoctrine, RECORDS_URL};
    use lf_application::{
        ports::{DoctrineError, TaxDoctrine},
        testing::FixedClock,
    };
    use lf_domain::{
        date::CivilDate,
        doctrine::{BoiId, DoctrineKind, DoctrineQuery, DoctrineTerms, KindFilter},
        search::PageSize,
    };

    /// Publication stable du jeu : les réductions de droits de donation.
    const KNOWN: &str = "BOI-ENR-DMTG-10-50-20";
    /// Identifiant bien formé qui ne désigne rien.
    const UNKNOWN: &str = "BOI-ZZ-INCONNU-99";
    /// Publication depuis 2024, dont le champ brut n'a aucun saut de ligne : le barème de l'impôt
    /// sur le revenu, avec ses numéros de paragraphe et son tableau.
    const RECENT: &str = "BOI-IR-LIQ-20-10";
    /// Plafond du corps d'une lecture, celui du client de production (4 Mio).
    const BODY_MAX_BYTES: usize = 4 * 1024 * 1024;

    /// `User-Agent` du binaire.
    fn agent() -> String {
        format!(
            "mcp-legifrance/{} (+https://github.com/muchiny/mcp-legifrance)",
            env!("CARGO_PKG_VERSION")
        )
    }

    /// Client de production, tel que le binaire le compose. L'horloge ne sert qu'au jour UTC du
    /// budget de requêtes : une horloge fixe suffit à quelques appels.
    fn doctrine() -> BofipDoctrine<FixedClock> {
        let config = BofipConfig::new(agent(), Duration::from_secs(20), Duration::from_secs(60));
        let clock = FixedClock::new(DateTime::UNIX_EPOCH, CivilDate::new(1970, 1, 1).unwrap());
        BofipDoctrine::new(config, clock)
    }

    /// Une seconde avant chaque appel : un client neuf a un débit neuf, la pause tient la suite
    /// à une requête par seconde.
    async fn pause() {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    /// La valeur d'un appel, ou `None` si le portail a refusé par quota (429).
    #[expect(
        clippy::print_stderr,
        reason = "test en direct : diagnostic sur stderr, sans aucune valeur de la requête"
    )]
    fn unless_limited<R>(test: &str, result: Result<R, DoctrineError>) -> Option<R> {
        if let Err(DoctrineError::RateLimited { retry_after }) = result {
            eprintln!("{test}: skipped, the portal answered 429 (rest: {retry_after:?})");
            return None;
        }
        Some(result.unwrap())
    }

    #[tokio::test]
    #[ignore = "live: calls data.economie.gouv.fr"]
    async fn live_bofip_search_returns_doctrine_only() {
        let query = DoctrineQuery::new(
            DoctrineTerms::parse("donation abattement").unwrap(),
            None,
            KindFilter::Doctrine,
            NonZeroU32::MIN,
            PageSize::new(5).unwrap(),
        )
        .unwrap();
        pause().await;
        let result = doctrine().search(&query).await;
        let Some(page) = unless_limited("live_bofip_search_returns_doctrine_only", result) else {
            return;
        };
        assert!(page.total >= 100, "total: {}", page.total);
        assert_eq!(page.hits.len(), 5);
        for hit in &page.hits {
            assert_eq!(hit.kind, DoctrineKind::Doctrine, "{}", hit.id.as_str());
            assert!(hit.id.as_str().starts_with("BOI-"), "{}", hit.id.as_str());
            assert!(!hit.title.is_empty(), "{}", hit.id.as_str());
        }
    }

    #[tokio::test]
    #[ignore = "live: calls data.economie.gouv.fr"]
    async fn live_bofip_get_returns_the_requested_publication() {
        let id = BoiId::parse(KNOWN).unwrap();
        pause().await;
        let result = doctrine().get(&id).await;
        let Some(docs) = unless_limited("live_bofip_get_returns_the_requested_publication", result)
        else {
            return;
        };
        assert_ne!(docs.len(), 0);
        for doc in &docs {
            assert_eq!(doc.id, id);
            assert_eq!(doc.kind, DoctrineKind::Doctrine);
            assert!(doc.source_url.is_some());
            assert!(doc.text.as_str().chars().count() > 1_000);
        }
    }

    #[tokio::test]
    #[ignore = "live: calls data.economie.gouv.fr"]
    async fn live_bofip_unknown_identifier_is_not_found() {
        let id = BoiId::parse(UNKNOWN).unwrap();
        pause().await;
        let result = doctrine().get(&id).await;
        if let Err(DoctrineError::RateLimited { .. }) = result {
            let _ = unless_limited("live_bofip_unknown_identifier_is_not_found", result);
            return;
        }
        assert_eq!(result, Err(DoctrineError::NotFound));
    }

    // ---- le texte lu dans le champ HTML (ADR-0030) ----------------------------------------------

    /// Une publication depuis 2024 se lit en plusieurs paragraphes : ses numéros de paragraphe
    /// sont seuls dans leur paragraphe et son tableau se lit ligne par ligne. Lue dans le champ
    /// brut (champ HTML retiré du jeu, ou écarté par la garde), elle tiendrait en un paragraphe.
    #[tokio::test]
    #[ignore = "live: calls data.economie.gouv.fr"]
    async fn live_bofip_recent_publication_reads_in_paragraphs() {
        let id = BoiId::parse(RECENT).unwrap();
        pause().await;
        let result = doctrine().get(&id).await;
        let Some(docs) =
            unless_limited("live_bofip_recent_publication_reads_in_paragraphs", result)
        else {
            return;
        };
        assert_ne!(docs.len(), 0);
        for doc in &docs {
            let paragraphs: Vec<&str> =
                doc.text.paragraphs().map(|(_, paragraph)| paragraph).collect();
            assert!(paragraphs.len() >= 10, "paragraphs: {}", paragraphs.len());
            let numbered = |paragraph: &&str| paragraph.chars().all(|c| c.is_ascii_digit());
            assert!(paragraphs.iter().any(numbered), "no paragraph number on its own");
            assert!(paragraphs.iter().any(|paragraph| paragraph.contains(" | ")), "no table row");
            let text = doc.text.as_str();
            for markup in ["[node:", "class=", "<p", "</"] {
                assert!(!text.contains(markup), "{markup} in the text");
            }
        }
    }

    /// V-226 : le portail double toujours chaque guillemet double du champ `contenu_html`
    /// (`class=""x""`). La lecture réduit ces suites tant qu'elles sont toutes paires ; le jour
    /// où ce test échoue, la règle des guillemets d'ADR-0030 est à relire.
    #[tokio::test]
    #[ignore = "live: calls data.economie.gouv.fr"]
    async fn live_bofip_html_field_still_doubles_its_quotes() {
        // Adresse fixe, sans aucune valeur d'appelant : le champ HTML de `RECENT`, seul.
        let url = format!(
            "{RECORDS_URL}?select=contenu_html\
             &where=identifiant_juridique%20%3D%20%22{RECENT}%22&limit=1"
        );
        let client = reqwest::Client::builder()
            .user_agent(agent())
            .timeout(Duration::from_secs(20))
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        pause().await;
        let mut response = client.get(url).send().await.unwrap();
        let status = response.status().as_u16();
        let limited = if status == 429 {
            Err(DoctrineError::RateLimited { retry_after: None })
        } else {
            Ok(())
        };
        let Some(()) = unless_limited("live_bofip_html_field_still_doubles_its_quotes", limited)
        else {
            return;
        };
        assert_eq!(status, 200);
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.unwrap() {
            body.extend_from_slice(&chunk);
            assert!(body.len() <= BODY_MAX_BYTES, "answer over {BODY_MAX_BYTES} bytes");
        }
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let html = body["results"][0]["contenu_html"]
            .as_str()
            .expect("no `contenu_html` text in the answer: the portal changed");
        let runs: Vec<usize> =
            html.split(|c| c != '"').map(str::len).filter(|length| *length > 0).collect();
        assert!(runs.len() >= 10, "quote runs in the HTML field: {}", runs.len());
        let odd = runs.iter().filter(|length| !length.is_multiple_of(2)).count();
        assert_eq!(odd, 0, "odd runs of quotes: the portal no longer doubles every quote");
        assert!(html.contains("class=\"\""), "no doubled quote after an attribute name");
    }

    // ---- le job de la CI -------------------------------------------------------------------------

    /// Lignes du job `name` de `workflow` : de son en-tête au job suivant.
    fn job(workflow: &str, name: &str) -> String {
        let header = format!("  {name}:");
        let lines: Vec<&str> = workflow
            .lines()
            .skip_while(|line| *line != header)
            .skip(1)
            .take_while(|line| {
                line.is_empty() || line.starts_with("   ") || line.starts_with("  #")
            })
            .collect();
        lines.join("\n")
    }

    /// Le job en direct du BOFiP ne reçoit aucun identifiant PISTE : ni secret, ni variable
    /// `LF_PISTE_*`, ni environnement protégé. Il joue ce fichier, tests ignorés seuls, et n'est
    /// pas bloquant. Le job `sandbox`, lui, porte bien les secrets : la lecture n'est pas aveugle.
    #[test]
    fn live_job_has_no_piste_secret() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/live.yml");
        // Sous Windows, git rend le fichier avec des fins de ligne CRLF : lu ici en LF.
        let workflow = fs::read_to_string(path).unwrap().replace("\r\n", "\n");

        let bofip = job(&workflow, "bofip");
        let found = ["secrets.", "LF_PISTE_", "environment:"].map(|word| bofip.contains(word));
        assert_eq!(found, [false; 3]);
        assert!(bofip.contains("cargo nextest run -p lf-adapter-bofip --test live"));
        assert!(bofip.contains("--profile live --run-ignored only"));
        assert!(bofip.contains("continue-on-error: true"));
        assert!(bofip.contains("if: github.repository == 'muchiny/mcp-legifrance'"));

        // Rien au niveau du workflow ne les lui donnerait.
        let (top, _) = workflow.split_once("\njobs:\n").unwrap();
        let found = ["secrets.", "LF_PISTE_"].map(|word| top.contains(word));
        assert_eq!(found, [false; 2]);

        // Contraste : le job de la sandbox PISTE, lui, les porte.
        let sandbox = job(&workflow, "sandbox");
        let found = ["secrets.LF_PISTE_CLIENT_SECRET", "environment: piste-sandbox"]
            .map(|word| sandbox.contains(word));
        assert_eq!(found, [true; 2]);
        assert!(!sandbox.contains("lf-adapter-bofip"));

        // L'adresse appelée est celle du jeu, en `https`.
        assert!(RECORDS_URL.starts_with("https://data.economie.gouv.fr/"));
    }
}
