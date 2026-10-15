//! Règle de dépendance de l'architecture hexagonale (00-decisions.md §2).
//!
//! `lf-domain` ne dépend d'aucun crate `lf-*` ni d'I/O ; `lf-application` ne
//! dépend d'aucun adapter ; un adapter ne dépend d'aucun autre adapter ; seul
//! `lf-app` connaît tout. Seules les dépendances normales comptent (pas `dev`
//! ni `build`).

use serde_json::Value;

/// Crate du workspace et ses dépendances normales directes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// Nom du crate.
    pub name: String,
    /// Noms des dépendances normales.
    pub deps: Vec<String>,
}

/// Crates d'I/O ou de protocole interdits dans `lf-domain`.
const DOMAIN_FORBIDDEN: [&str; 6] = ["tokio", "reqwest", "serde_json", "rmcp", "hyper", "moka"];

fn is_adapter(name: &str) -> bool {
    name.starts_with("lf-adapter-")
}

fn forbidden(package: &str, dep: &str) -> bool {
    match package {
        "lf-domain" => dep.starts_with("lf-") || DOMAIN_FORBIDDEN.contains(&dep),
        "lf-application" => is_adapter(dep) || dep == "lf-app",
        p if is_adapter(p) => is_adapter(dep) || dep == "lf-app",
        _ => false,
    }
}

/// Liste des violations, une ligne lisible par violation.
pub fn violations(packages: &[Package]) -> Vec<String> {
    packages
        .iter()
        .flat_map(|p| {
            p.deps
                .iter()
                .filter(|dep| forbidden(&p.name, dep))
                .map(move |dep| format!("{} must not depend on {dep}", p.name))
        })
        .collect()
}

/// Extrait les membres du workspace de la sortie de `cargo metadata --format-version 1`.
pub fn workspace_packages(metadata: &Value) -> Vec<Package> {
    let members: Vec<&str> = metadata["workspace_members"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    metadata["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["id"].as_str().is_some_and(|id| members.contains(&id)))
        .map(|p| Package {
            name: p["name"].as_str().unwrap_or_default().to_owned(),
            deps: p["dependencies"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|d| d["kind"].is_null())
                .filter_map(|d| d["name"].as_str().map(str::to_owned))
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn pkg(name: &str, deps: &[&str]) -> Package {
        Package { name: name.into(), deps: deps.iter().map(|d| (*d).to_owned()).collect() }
    }

    #[test]
    fn clean_graph_has_no_violation() {
        let graph = [
            pkg("lf-domain", &["thiserror"]),
            pkg("lf-application", &["lf-domain", "tokio"]),
            pkg("lf-adapter-piste", &["lf-application", "lf-domain", "reqwest"]),
            pkg("lf-app", &["lf-adapter-piste", "lf-adapter-mcp", "lf-application"]),
            pkg("xtask", &["serde_json"]),
        ];
        assert_eq!(violations(&graph), Vec::<String>::new());
    }

    #[test]
    fn domain_stays_pure() {
        let graph = [pkg(
            "lf-domain",
            &[
                "lf-application",
                "tokio",
                "reqwest",
                "serde_json",
                "rmcp",
                "hyper",
                "moka",
                "thiserror",
            ],
        )];
        assert_eq!(
            violations(&graph),
            [
                "lf-domain must not depend on lf-application",
                "lf-domain must not depend on tokio",
                "lf-domain must not depend on reqwest",
                "lf-domain must not depend on serde_json",
                "lf-domain must not depend on rmcp",
                "lf-domain must not depend on hyper",
                "lf-domain must not depend on moka",
            ]
        );
    }

    #[test]
    fn application_and_adapters_respect_boundaries() {
        let graph = [
            pkg("lf-application", &["lf-adapter-piste", "lf-app"]),
            pkg("lf-adapter-cache", &["lf-adapter-piste", "lf-app", "lf-application"]),
        ];
        assert_eq!(
            violations(&graph),
            [
                "lf-application must not depend on lf-adapter-piste",
                "lf-application must not depend on lf-app",
                "lf-adapter-cache must not depend on lf-adapter-piste",
                "lf-adapter-cache must not depend on lf-app",
            ]
        );
    }

    #[test]
    fn reads_cargo_metadata_members_and_normal_deps() {
        let metadata = json!({
            "workspace_members": ["path+file:///w/crates/lf-domain#0.0.0"],
            "packages": [
                {
                    "id": "path+file:///w/crates/lf-domain#0.0.0",
                    "name": "lf-domain",
                    "dependencies": [
                        { "name": "thiserror", "kind": null },
                        { "name": "proptest", "kind": "dev" },
                        { "name": "cc", "kind": "build" }
                    ]
                },
                { "id": "registry+https://x#thiserror@2.0.21", "name": "thiserror", "dependencies": [] }
            ]
        });
        assert_eq!(workspace_packages(&metadata), [pkg("lf-domain", &["thiserror"])]);
    }

    #[test]
    fn tolerates_missing_fields() {
        assert_eq!(workspace_packages(&json!({})), Vec::<Package>::new());
        let metadata = json!({ "workspace_members": ["a"], "packages": [{ "id": "a" }] });
        assert_eq!(workspace_packages(&metadata), [pkg("", &[])]);
    }
}
