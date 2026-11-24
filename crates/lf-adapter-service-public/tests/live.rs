//! Test en direct contre le serveur de la DILA (archives du jour de Service-Public, T-MS-24) : le
//! test qui appelle le serveur porte `#[ignore = "live: calls lecomarquage.service-public.gouv.fr"]`
//! et son nom commence par `live_service_public_` (profil nextest `live`).
//!
//! ```text
//! cargo nextest run -p lf-adapter-service-public --test live --profile live --run-ignored only
//! ```
//!
//! Aucun secret : les archives sont ouvertes, sans compte ni clé. Deux requêtes par passage, une
//! par public : un seul test, une seule source, qui garde les archives en mémoire comme le binaire
//! (recherches et lectures n'envoient rien). Un refus 429 n'est pas un défaut de la source : le
//! test le dit sur stderr et s'arrête sans échouer, et rien n'est réessayé.
//!
//! `live_service_public_job_has_no_secret` n'appelle rien : il relit `live.yml` et tourne avec les
//! tests ordinaires (il n'est pas ignoré).
//!
//! Tout le fichier est sous `cfg(test)` : les aides partagent les règles de lint des tests
//! (`unwrap` permis, `clippy.toml`).

#[cfg(test)]
mod live {
    use std::{fs, num::NonZeroU32, path::Path, time::Duration};

    use lf_adapter_service_public::{ServicePublicConfig, ServicePublicGuidance};
    use lf_application::ports::{GuidanceError, PublicGuidance};
    use lf_domain::{
        guidance::{
            ARCHIVE_NAME, Audience, AudienceFilter, GuidanceKind, GuidanceQuery, GuidanceTerms,
            SheetId, archive_url,
        },
        search::PageSize,
    };

    /// Fiche stable de Particuliers : le solde bancaire insaisissable.
    const SHEET: &str = "F1437";
    /// Fiche commune aux deux publics : frais professionnels et avantages en nature.
    const COMMON: &str = "F2408";
    /// Numéro bien formé qu'aucune archive ne porte.
    const UNKNOWN: &str = "F9999999";

    /// `User-Agent` du binaire.
    fn agent() -> String {
        format!(
            "mcp-legifrance/{} (+https://github.com/muchiny/mcp-legifrance)",
            env!("CARGO_PKG_VERSION")
        )
    }

    fn query(words: &str, audience: AudienceFilter) -> GuidanceQuery {
        GuidanceQuery {
            terms: GuidanceTerms::parse(words).unwrap(),
            audience,
            page: NonZeroU32::MIN,
            page_size: PageSize::new(5).unwrap(),
        }
    }

    fn id(raw: &str) -> SheetId {
        SheetId::parse(raw).unwrap()
    }

    /// Vrai si le serveur a refusé par quota (429) : le test s'arrête là, sans échouer.
    #[expect(
        clippy::print_stderr,
        reason = "test en direct : diagnostic sur stderr, sans aucune valeur de la requête"
    )]
    fn limited<R>(result: &Result<R, GuidanceError>) -> bool {
        let refused = matches!(result, Err(GuidanceError::RateLimited { .. }));
        if refused {
            eprintln!("live_service_public: skipped, the DILA server answered 429");
        }
        refused
    }

    #[tokio::test]
    #[ignore = "live: calls lecomarquage.service-public.gouv.fr"]
    async fn live_service_public_reads_and_searches_both_archives() {
        let config = ServicePublicConfig::new(agent(), Duration::from_secs(280));
        let guidance = ServicePublicGuidance::new(&config);

        // Particuliers : une requête, puis tout se sert de l'archive en mémoire.
        let found = guidance
            .search(&query("solde bancaire insaisissable", AudienceFilter::Particuliers))
            .await;
        if limited(&found) {
            return;
        }
        let found = found.unwrap();
        assert!(found.page.hits.iter().any(|hit| hit.id.as_str() == SHEET), "{:?}", found.page);
        assert!(found.files[0].last_modified.is_some());
        let read = guidance.get(Audience::Particuliers, &id(SHEET)).await.unwrap();
        assert_eq!(read.doc.kind(), GuidanceKind::Sheet);
        assert_ne!(read.doc.title, "");
        assert!(!read.doc.path.is_empty(), "the menu places the sheet");
        assert!(read.doc.text().unwrap().as_str().chars().count() > 1_000);
        assert_ne!(read.doc.legal_references().unwrap().references().len(), 0);
        let absent = guidance.get(Audience::Particuliers, &id(UNKNOWN)).await;
        assert_eq!(absent.map(|read| read.doc.id), Err(GuidanceError::NotFound));

        // Entreprendre : la seconde requête.
        let common = guidance.get(Audience::Entreprendre, &id(COMMON)).await;
        if limited(&common) {
            return;
        }
        let common = common.unwrap();
        assert_eq!(common.doc.audience, Audience::Entreprendre);
        assert!(common.doc.source_url.as_str().starts_with("https://entreprendre."));
        let both =
            guidance.search(&query("frais professionnels", AudienceFilter::All)).await.unwrap();
        let audiences: Vec<Audience> = both.files.iter().map(|file| file.audience).collect();
        assert_eq!(audiences, [Audience::Particuliers, Audience::Entreprendre]);
    }

    /// Extrait les lignes d'un job de `live.yml`.
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

    /// Le job en direct de Service-Public ne reçoit aucun identifiant : ni secret, ni variable
    /// `LF_PISTE_*`, ni environnement protégé. Il joue ce fichier, tests ignorés seuls, et n'est
    /// pas bloquant. Le job `sandbox`, lui, porte bien les secrets : la lecture n'est pas aveugle.
    #[test]
    fn live_service_public_job_has_no_secret() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/live.yml");
        // Sous Windows, git rend le fichier avec des fins de ligne CRLF : lu ici en LF.
        let workflow = fs::read_to_string(path).unwrap().replace("\r\n", "\n");

        let service = job(&workflow, "service-public");
        let found = ["secrets.", "LF_PISTE_", "environment:"].map(|word| service.contains(word));
        assert_eq!(found, [false; 3]);
        assert!(service.contains("cargo nextest run -p lf-adapter-service-public --test live"));
        assert!(service.contains("--profile live --run-ignored only"));
        assert!(service.contains("continue-on-error: true"));
        assert!(service.contains("if: github.repository == 'muchiny/mcp-legifrance'"));

        // Contraste : le job de la sandbox PISTE, lui, porte ses secrets.
        let sandbox = job(&workflow, "sandbox");
        assert!(sandbox.contains("secrets.LF_PISTE_CLIENT_SECRET"));
        assert!(!sandbox.contains("lf-adapter-service-public"));

        // Les adresses appelées sont celles de la DILA, en `https`.
        for audience in Audience::ALL {
            let url = archive_url(audience);
            assert!(url.starts_with("https://lecomarquage.service-public.gouv.fr/"), "{url}");
            assert!(url.ends_with(ARCHIVE_NAME), "{url}");
        }
    }
}
