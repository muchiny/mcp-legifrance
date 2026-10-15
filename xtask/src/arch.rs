//! Règle de dépendance de l'architecture hexagonale (00-decisions.md §2).
//!
//! `lf-domain` n'a pour dépendance normale que `thiserror` ; `lf-application`
//! ne dépend d'aucun adapter ; un adapter ne dépend d'aucun autre adapter ;
//! seul `lf-app` connaît tout. Seules les dépendances normales comptent (pas
//! `dev` ni `build`).
//!
//! La liste blanche de `lf-domain` outille la Global Constraint « dépendance
//! normale unique `thiserror` », plus stricte que la formulation du §2 (qui
//! interdit seulement l'I/O et les crates `lf-*`). La porte échoue fermée :
//! un `cargo metadata` sans membre, sans nom ou sans `dependencies`, ou un
//! graphe sans `lf-domain`, est une erreur et non un succès à vide.

use serde_json::Value;

/// Crate du workspace et ses dépendances normales directes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// Nom du crate.
    pub name: String,
    /// Noms des dépendances normales.
    pub deps: Vec<String>,
}

/// Seules dépendances normales admises dans `lf-domain`.
const DOMAIN_ALLOWED: [&str; 1] = ["thiserror"];

fn is_adapter(name: &str) -> bool {
    name.starts_with("lf-adapter-")
}

fn forbidden(package: &str, dep: &str) -> bool {
    match package {
        "lf-domain" => !DOMAIN_ALLOWED.contains(&dep),
        "lf-application" => is_adapter(dep) || dep == "lf-app",
        p if is_adapter(p) => is_adapter(dep) || dep == "lf-app",
        _ => false,
    }
}

/// Liste des violations, une ligne lisible par violation.
pub fn violations(packages: &[Package]) -> Vec<String> {
    let missing = (!packages.iter().any(|p| p.name == "lf-domain"))
        .then(|| "lf-domain missing from workspace metadata".to_owned());
    missing
        .into_iter()
        .chain(packages.iter().flat_map(|p| {
            p.deps
                .iter()
                .filter(|dep| forbidden(&p.name, dep))
                .map(move |dep| format!("{} must not depend on {dep}", p.name))
        }))
        .collect()
}

/// Extrait les membres du workspace de la sortie de `cargo metadata --format-version 1`.
///
/// # Errors
///
/// Échoue fermé : aucun membre trouvé, ou un membre sans `name` chaîne ou sans
/// tableau `dependencies`.
pub fn workspace_packages(metadata: &Value) -> Result<Vec<Package>, String> {
    let members: Vec<&str> = metadata["workspace_members"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let packages = metadata["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["id"].as_str().is_some_and(|id| members.contains(&id)))
        .map(read_package)
        .collect::<Result<Vec<_>, _>>()?;
    if packages.is_empty() {
        return Err("cargo metadata: no workspace member found".to_owned());
    }
    Ok(packages)
}

fn read_package(p: &Value) -> Result<Package, String> {
    let name = p["name"].as_str().ok_or("cargo metadata: workspace member without a name")?;
    let dependencies = p["dependencies"]
        .as_array()
        .ok_or_else(|| format!("cargo metadata: {name} has no dependencies array"))?;
    Ok(Package {
        name: name.to_owned(),
        deps: dependencies
            .iter()
            .filter(|d| d["kind"].is_null())
            .filter_map(|d| d["name"].as_str().map(str::to_owned))
            .collect(),
    })
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
    fn domain_only_allows_thiserror() {
        let graph = [pkg(
            "lf-domain",
            &["lf-application", "tokio", "regex", "chrono", "futures", "thiserror", "serde_json"],
        )];
        assert_eq!(
            violations(&graph),
            [
                "lf-domain must not depend on lf-application",
                "lf-domain must not depend on tokio",
                "lf-domain must not depend on regex",
                "lf-domain must not depend on chrono",
                "lf-domain must not depend on futures",
                "lf-domain must not depend on serde_json",
            ]
        );
    }

    #[test]
    fn missing_domain_is_a_violation() {
        let graph = [pkg("lf-application", &["thiserror"]), pkg("xtask", &[])];
        assert_eq!(violations(&graph), ["lf-domain missing from workspace metadata"]);
        assert_eq!(violations(&[]), ["lf-domain missing from workspace metadata"]);
    }

    #[test]
    fn application_and_adapters_respect_boundaries() {
        let graph = [
            pkg("lf-domain", &["thiserror"]),
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
        assert_eq!(workspace_packages(&metadata), Ok(vec![pkg("lf-domain", &["thiserror"])]));
    }

    #[test]
    fn fails_closed_without_members() {
        let error = Err("cargo metadata: no workspace member found".to_owned());
        assert_eq!(workspace_packages(&json!({})), error);
        let metadata = json!({ "workspace_members": ["b"], "packages": [{ "id": "a" }] });
        assert_eq!(workspace_packages(&metadata), error);
    }

    #[test]
    fn fails_closed_on_member_without_name() {
        let metadata = json!({
            "workspace_members": ["a"],
            "packages": [{ "id": "a", "dependencies": [] }]
        });
        assert_eq!(
            workspace_packages(&metadata),
            Err("cargo metadata: workspace member without a name".to_owned())
        );
    }

    #[test]
    fn fails_closed_on_member_without_dependencies() {
        let metadata = json!({
            "workspace_members": ["a"],
            "packages": [{ "id": "a", "name": "lf-domain" }]
        });
        assert_eq!(
            workspace_packages(&metadata),
            Err("cargo metadata: lf-domain has no dependencies array".to_owned())
        );
    }
}
