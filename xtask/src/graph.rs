//! Graphe résolu de `cargo metadata` : fermeture transitive des dépendances
//! interdites (02 §4.2, étape 4) et doublons de versions des crates critiques
//! (R9, 06 §9.3).
//!
//! La fermeture suit les arêtes normales et de build seulement (les
//! dev-dependencies n'entrent pas dans le binaire livré) ; R9 regarde tout le
//! graphe résolu. Le graphe se lit avec `--all-features`, pour le pire cas, et
//! sans `--no-deps`. La porte échoue fermée : des métadonnées sans graphe
//! résolu sont une erreur et non un succès à vide.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::Value;

/// Crates qu'un membre ne doit atteindre par aucun chemin (02 §4.2, étape 4).
const TRANSITIVE_FORBIDDEN: [(&str, &[&str]); 2] = [
    ("lf-domain", &["tokio", "reqwest", "serde_json", "rmcp"]),
    ("lf-application", &["rmcp", "reqwest", "hyper", "moka"]),
];

/// Crates dont le graphe résolu ne doit contenir qu'une version (R9, 06 §9.3).
const CRITICAL: [&str; 9] =
    ["tokio", "rustls", "reqwest", "hyper", "http", "serde", "schemars", "rmcp", "chrono"];

/// Graphe résolu de production lu dans `cargo metadata`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Resolved {
    /// Paquets du graphe : identifiant vers (nom, version).
    pub packages: BTreeMap<String, (String, String)>,
    /// Arêtes de production (normales et de build) : identifiant vers
    /// identifiants des dépendances.
    pub edges: BTreeMap<String, Vec<String>>,
    /// Identifiants des membres du workspace.
    pub members: Vec<String>,
}

/// Lit le graphe résolu de `cargo metadata` (sans `--no-deps`).
///
/// Une arête compte si un de ses `dep_kinds` est normal ou de build ; sa cible
/// se lit dans `pkg` (le `name` d'une arête est le nom de crate souligné).
/// Un paquet sans `id`, `name` ou `version` est ignoré.
///
/// # Errors
///
/// Échoue fermé si `resolve.nodes` n'est pas un tableau.
#[expect(
    clippy::indexing_slicing,
    reason = "indexation de `serde_json::Value` : un champ absent donne `Null`, sans panique"
)]
pub fn resolved_graph(metadata: &Value) -> Result<Resolved, String> {
    let nodes = metadata["resolve"]["nodes"]
        .as_array()
        .ok_or("cargo metadata: no resolve graph (run without --no-deps)")?;
    let packages = metadata["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some((
                p["id"].as_str()?.to_owned(),
                (p["name"].as_str()?.to_owned(), p["version"].as_str()?.to_owned()),
            ))
        })
        .collect();
    let edges = nodes
        .iter()
        .filter_map(|node| {
            let deps = node["deps"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|dep| is_production(dep))
                .filter_map(|dep| dep["pkg"].as_str().map(str::to_owned))
                .collect();
            Some((node["id"].as_str()?.to_owned(), deps))
        })
        .collect();
    let members = metadata["workspace_members"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m.as_str().map(str::to_owned))
        .collect();
    Ok(Resolved { packages, edges, members })
}

/// Une arête est de production si un de ses genres est normal (`kind` nul) ou
/// de build.
fn is_production(dep: &Value) -> bool {
    dep["dep_kinds"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|kind| kind["kind"].is_null() || kind["kind"].as_str() == Some("build"))
}

/// Violations du graphe résolu : fermeture transitive de chaque membre soumis à
/// `TRANSITIVE_FORBIDDEN` (dans l'ordre de `members`), puis doublons de
/// versions des crates de `CRITICAL`.
#[must_use]
pub fn graph_violations(graph: &Resolved) -> Vec<String> {
    let mut found: Vec<String> =
        graph.members.iter().flat_map(|member| closure_violations(graph, member)).collect();
    found.extend(duplicate_violations(graph));
    found
}

/// Nom du paquet, ou l'identifiant s'il est inconnu.
fn name_of<'a>(graph: &'a Resolved, id: &'a str) -> &'a str {
    graph.packages.get(id).map_or(id, |(name, _)| name.as_str())
}

/// Parcours en largeur depuis `member` : chaque crate interdit atteint est
/// signalé une fois, par son plus court chemin. `seen` contient le membre dès
/// le départ et `seen.insert` décide seul de la visite, ce qui borne le
/// parcours sur un cycle.
fn closure_violations(graph: &Resolved, member: &str) -> Vec<String> {
    let start = name_of(graph, member);
    let Some((_, forbidden)) = TRANSITIVE_FORBIDDEN.iter().find(|(name, _)| *name == start) else {
        return Vec::new();
    };
    let mut seen = BTreeSet::from([member]);
    let mut parent: BTreeMap<&str, &str> = BTreeMap::new();
    let mut queue = VecDeque::from([member]);
    let mut found = Vec::new();
    while let Some(id) = queue.pop_front() {
        for next in graph.edges.get(id).into_iter().flatten() {
            if !seen.insert(next) {
                continue;
            }
            parent.insert(next, id);
            queue.push_back(next);
            let name = name_of(graph, next);
            if forbidden.contains(&name) {
                found.push(format!(
                    "{start} must not reach {name} ({})",
                    path(graph, &parent, next)
                ));
            }
        }
    }
    found
}

/// Chemin `a -> b -> c` du membre à `target`, reconstruit par la table `parent`.
fn path(graph: &Resolved, parent: &BTreeMap<&str, &str>, target: &str) -> String {
    let mut names = vec![name_of(graph, target)];
    let mut current = target;
    while let Some(&up) = parent.get(current) {
        names.push(name_of(graph, up));
        current = up;
    }
    names.reverse();
    names.join(" -> ")
}

/// R9 : une violation par crate critique présent en plus d'une version.
fn duplicate_violations(graph: &Resolved) -> Vec<String> {
    let mut versions: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (name, version) in graph.packages.values() {
        if CRITICAL.contains(&name.as_str()) {
            versions.entry(name).or_default().insert(version);
        }
    }
    versions
        .into_iter()
        .filter(|(_, set)| set.len() > 1)
        .map(|(name, set)| {
            let list = set.into_iter().collect::<Vec<_>>().join(", ");
            format!("duplicate versions of {name} in the resolved graph: {list}")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    /// Graphe de test dont les identifiants sont `nom@version` ; `extra` ajoute
    /// des paquets sans arête.
    fn graph(members: &[&str], edges: &[(&str, &[&str])], extra: &[&str]) -> Resolved {
        let mut ids: Vec<&str> = members.to_vec();
        ids.extend_from_slice(extra);
        for (from, tos) in edges {
            ids.push(from);
            ids.extend_from_slice(tos);
        }
        let packages = ids
            .into_iter()
            .map(|id| {
                let (name, version) = id.split_once('@').expect("id is name@version");
                (id.to_owned(), (name.to_owned(), version.to_owned()))
            })
            .collect();
        Resolved {
            packages,
            edges: edges
                .iter()
                .map(|(from, tos)| {
                    ((*from).to_owned(), tos.iter().map(|t| (*t).to_owned()).collect())
                })
                .collect(),
            members: members.iter().map(|m| (*m).to_owned()).collect(),
        }
    }

    #[test]
    fn domain_must_not_reach_io_crates() {
        let g = graph(
            &["lf-domain@0.0.0"],
            &[
                ("lf-domain@0.0.0", &["thiserror@2.0.21", "x@1.0.0"]),
                ("x@1.0.0", &["y@1.0.0", "serde_json@1.0.154"]),
                ("y@1.0.0", &["tokio@1.53.2"]),
            ],
            &[],
        );
        assert_eq!(
            graph_violations(&g),
            [
                "lf-domain must not reach serde_json (lf-domain -> x -> serde_json)",
                "lf-domain must not reach tokio (lf-domain -> x -> y -> tokio)",
            ]
        );
    }

    #[test]
    fn application_must_not_reach_network_or_mcp_crates() {
        let g = graph(
            &["lf-application@0.0.0"],
            &[
                ("lf-application@0.0.0", &["tokio@1.53.2", "z@1.0.0"]),
                ("z@1.0.0", &["hyper@1.7.0", "rmcp@3.5.1", "moka@0.12.16", "reqwest@0.13.5"]),
            ],
            &[],
        );
        let expected: Vec<String> = ["hyper", "rmcp", "moka", "reqwest"]
            .iter()
            .map(|n| format!("lf-application must not reach {n} (lf-application -> z -> {n})"))
            .collect();
        assert_eq!(graph_violations(&g), expected);
    }

    #[test]
    fn other_members_reach_anything() {
        let g = graph(
            &["xtask@0.0.0", "lf-adapter-piste@0.0.0"],
            &[
                ("xtask@0.0.0", &["serde_json@1.0.154"]),
                ("lf-adapter-piste@0.0.0", &["reqwest@0.13.5", "tokio@1.53.2"]),
            ],
            &[],
        );
        assert_eq!(graph_violations(&g), Vec::<String>::new());
    }

    #[test]
    fn each_forbidden_crate_is_reported_once_by_its_shortest_path() {
        let g = graph(
            &["lf-domain@0.0.0"],
            &[
                ("lf-domain@0.0.0", &["a@1.0.0", "b@1.0.0"]),
                ("a@1.0.0", &["c@1.0.0", "lf-domain@0.0.0"]),
                ("b@1.0.0", &["tokio@1.53.2"]),
                ("c@1.0.0", &["tokio@1.53.2", "a@1.0.0"]),
            ],
            &[],
        );
        assert_eq!(
            graph_violations(&g),
            ["lf-domain must not reach tokio (lf-domain -> b -> tokio)"]
        );
    }

    #[test]
    fn critical_crates_have_a_single_version() {
        let g = graph(
            &["xtask@0.0.0"],
            &[],
            &["tokio@1.53.2", "tokio@1.54.0", "chrono@0.4.45", "rand@0.8.5", "rand@0.9.2"],
        );
        assert_eq!(
            graph_violations(&g),
            ["duplicate versions of tokio in the resolved graph: 1.53.2, 1.54.0"]
        );
    }

    #[test]
    fn reads_production_edges_of_the_resolve_graph() {
        let normal = json!([{ "kind": null, "target": null }]);
        let dev = json!([{ "kind": "dev", "target": null }]);
        let build = json!([{ "kind": "build", "target": null }]);
        let mixed = json!([
            { "kind": "dev", "target": null },
            { "kind": null, "target": "cfg(unix)" },
        ]);
        let metadata = json!({
            "workspace_members": ["d"],
            "packages": [
                { "id": "d", "name": "lf-domain", "version": "0.0.0" },
                { "id": "t", "name": "thiserror", "version": "2.0.21" },
                { "id": "p", "name": "pretty_assertions", "version": "1.4.1" },
                { "id": "c", "name": "cc", "version": "1.2.0" },
                { "id": "s", "name": "serde", "version": "1.0.228" },
                { "id": "anonymous" },
                { "id": "unversioned", "name": "x" },
                { "name": "no-id", "version": "1.0.0" },
            ],
            "resolve": { "nodes": [
                { "id": "d", "deps": [
                    { "name": "thiserror", "pkg": "t", "dep_kinds": normal },
                    { "name": "pretty_assertions", "pkg": "p", "dep_kinds": dev },
                    { "name": "cc", "pkg": "c", "dep_kinds": build },
                    { "name": "serde", "pkg": "s", "dep_kinds": mixed },
                ] },
                { "id": "t", "deps": [] },
                { "deps": [{ "name": "ghost", "pkg": "g", "dep_kinds": normal }] },
            ] },
        });
        let owned = |id: &str, name: &str, version: &str| {
            (id.to_owned(), (name.to_owned(), version.to_owned()))
        };
        assert_eq!(
            resolved_graph(&metadata),
            Ok(Resolved {
                packages: BTreeMap::from([
                    owned("d", "lf-domain", "0.0.0"),
                    owned("t", "thiserror", "2.0.21"),
                    owned("p", "pretty_assertions", "1.4.1"),
                    owned("c", "cc", "1.2.0"),
                    owned("s", "serde", "1.0.228"),
                ]),
                edges: BTreeMap::from([
                    ("d".to_owned(), vec!["t".to_owned(), "c".to_owned(), "s".to_owned()]),
                    ("t".to_owned(), Vec::new()),
                ]),
                members: vec!["d".to_owned()],
            })
        );
    }

    #[test]
    fn fails_closed_without_resolve_graph() {
        let expected = Err("cargo metadata: no resolve graph (run without --no-deps)".to_owned());
        assert_eq!(resolved_graph(&json!({ "resolve": null })), expected);
        assert_eq!(resolved_graph(&json!({ "resolve": { "nodes": null } })), expected);
    }
}
