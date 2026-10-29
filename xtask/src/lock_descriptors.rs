//! `cargo xtask lock-descriptors` : verrou des descripteurs de la surface MCP (05 §9, 02 §3.8).
//!
//! STUB(V0) → V1-13 : seuls les modes et leur lecture sur la ligne de commande sont écrits ;
//! V1-13 écrit `run` (sessions JSON-RPC, empreintes, manifeste MCPB, version de `server.json`).

/// Mode du verrou, choisi sur la ligne de commande.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockMode {
    /// Régénère en mémoire et compare à `tests/tools.lock.json` (défaut, utilisé par la CI).
    Check,
    /// Réécrit `tests/tools.lock.json`.
    Update,
    /// Classe les différences avec le verrou de la référence git donnée (release).
    Compare(String),
}

impl LockMode {
    /// Lit les arguments qui suivent `lock-descriptors` : aucun (ou `--check`), `--update`, ou
    /// `--compare <réf-git>`.
    ///
    /// # Errors
    ///
    /// Le message d'usage si un argument est inconnu, si `--compare` n'a pas de référence, ou si
    /// plusieurs modes sont demandés.
    pub fn from_args(args: &[String]) -> Result<Self, String> {
        const USAGE: &str =
            "usage: cargo xtask lock-descriptors [--check | --update | --compare <ref-git>]";
        match args {
            [] => Ok(Self::Check),
            [flag] if flag == "--check" => Ok(Self::Check),
            [flag] if flag == "--update" => Ok(Self::Update),
            [flag, reference] if flag == "--compare" && !reference.starts_with('-') => {
                Ok(Self::Compare(reference.clone()))
            }
            _ => Err(USAGE.to_owned()),
        }
    }
}

/// Exécute le verrou dans le `mode` donné.
///
/// # Errors
///
/// Le message de l'échec. Tant que V1-13 n'a pas écrit le verrou, toujours une erreur.
pub fn run(_mode: LockMode) -> Result<(), String> {
    // STUB(V0) → V1-13 : sessions JSON-RPC sur le binaire, empreintes, comparaison au verrou.
    Err("lock-descriptors: pas encore implémenté (écrit par V1-13)".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn parse(args: &[&str]) -> Result<LockMode, String> {
        LockMode::from_args(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn check_is_the_default_mode() {
        assert_eq!(parse(&[]), Ok(LockMode::Check));
        assert_eq!(parse(&["--check"]), Ok(LockMode::Check));
    }

    #[test]
    fn update_and_compare_are_explicit() {
        assert_eq!(parse(&["--update"]), Ok(LockMode::Update));
        assert_eq!(parse(&["--compare", "v0.1.0"]), Ok(LockMode::Compare("v0.1.0".to_owned())));
    }

    #[test]
    fn unknown_missing_or_combined_arguments_give_the_usage() {
        for args in [
            &["--bogus"][..],
            &["--compare"],
            &["--compare", "--update"],
            &["--check", "--update"],
            &["--update", "--compare", "v0.1.0"],
            &["v0.1.0"],
        ] {
            let error = parse(args).unwrap_err();
            assert!(error.starts_with("usage: cargo xtask lock-descriptors"), "{args:?} : {error}");
        }
    }

    #[test]
    fn run_is_a_stub_that_fails_loudly() {
        assert!(run(LockMode::Check).unwrap_err().contains("pas encore implémenté"));
    }
}
