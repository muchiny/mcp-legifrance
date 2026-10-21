//! Règles de dépendance de l'architecture hexagonale (02 §4.1, R1 à R7, et
//! étape 5 de 02 §4.2).
//!
//! Règles en données : une `CrateRule` par paquet connu (`RULES`), moteur pur.
//! Dans les règles, `lf-app` s'écrit `mcp-legifrance` (02 §3.7). Est interne un
//! nom en `lf-*`, `mcp-legifrance` ou `xtask`. Seules les dépendances normales
//! comptent pour R1 à R7. Contrôles :
//!
//! - membre inconnu : absent de la table, une seule violation, aucun autre
//!   contrôle sur ce membre ;
//! - R1 : `lf-domain` sans dépendance interne, liste blanche externe
//!   `DOMAIN_ALLOWED` (`thiserror` seul, décision C3) ;
//! - R2 : `lf-application` ne dépend en interne que de `lf-domain`, et en externe
//!   de la liste fermée `APPLICATION_ALLOWED` ; les features qu'elle active sur
//!   `tokio` et `chrono` sont bornées par `APPLICATION_FEATURES` (`default`
//!   comprise : le workspace déclare `default-features = false`) ;
//! - R3 : un adapter ne dépend d'aucun autre adapter ni de `mcp-legifrance` ;
//!   `lf-adapter-audit` dépend de `lf-application` seul ;
//! - R4 : personne ne dépend de `mcp-legifrance`, qui peut dépendre de tous les
//!   `lf-*` (permission, pas obligation) et n'a pas de cible `lib` ;
//! - R5 : `xtask` ne dépend d'aucun crate interne, et réciproquement ;
//! - R7 : aucun membre ne dépend directement de `async-trait` (ADR-0012) ;
//! - étape 5 : seuls `mcp-legifrance` et `xtask` ont une cible `bin` (5f) ;
//!   `version`, `edition`, `rust-version` et `license` égalent ceux de
//!   `lf-domain` (5a-5d : `cargo metadata` ne montre pas l'héritage de
//!   `[workspace.package]`, d'où une égalité de valeur ; sans `lf-domain`, pas
//!   de contrôle) ; toute exigence interne, de tout kind, vaut `=` + version de
//!   `lf-domain` (5h) ; chaque manifeste contient `[lints] workspace = true`
//!   (5e, lu par `inherits_workspace_lints` : `cargo metadata` ne montre pas
//!   l'héritage des lints).
//!
//! Ajouter un crate au workspace oblige à vérifier ou ajouter sa ligne dans
//! `RULES`.
//!
//! La porte échoue fermée : un `cargo metadata` sans membre, ou un membre sans
//! nom, sans `dependencies`, sans `version`, sans `edition`, sans `targets` ou
//! sans `manifest_path`, ou un graphe sans `lf-domain`, est une erreur et non un
//! succès à vide. Une dépendance sans `features` ni `uses_default_features`
//! compte comme activant `default` ; un manifeste illisible est une erreur, et
//! `lints_workspace` vaut `false` (5e en violation) tant que le manifeste n'a
//! pas été lu.

use serde_json::Value;

/// Crate du workspace et ses données lues dans `cargo metadata`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// Nom du crate.
    pub name: String,
    /// Noms des dépendances normales.
    pub deps: Vec<String>,
    /// Exigences de version (`nom`, `req`), tous kinds confondus.
    pub requirements: Vec<(String, String)>,
    /// Kinds des cibles (`lib`, `bin`, `custom-build`, ...).
    pub targets: Vec<String>,
    /// Version du paquet.
    pub version: String,
    /// Édition Rust.
    pub edition: String,
    /// Version minimale de Rust, si déclarée.
    pub rust_version: Option<String>,
    /// Licence, si déclarée.
    pub license: Option<String>,
    /// Features activées de chaque dépendance normale (`nom`, features) ;
    /// `default` y figure sauf si `uses_default_features` vaut `false`.
    pub features: Vec<(String, Vec<String>)>,
    /// Chemin du `Cargo.toml` du membre.
    pub manifest_path: String,
    /// Le manifeste contient `[lints] workspace = true` (5e) ; `false` à la
    /// lecture de `cargo metadata`, fixé ensuite par `inherits_workspace_lints`.
    pub lints_workspace: bool,
}

/// Seules dépendances normales externes admises dans `lf-domain`.
const DOMAIN_ALLOWED: [&str; 1] = ["thiserror"];

/// Dépendances normales externes admises dans `lf-application` : liste fermée
/// de R2 (00 §2).
const APPLICATION_ALLOWED: [&str; 6] =
    ["thiserror", "chrono", "tracing", "futures", "tokio", "similar"];

/// Features admises par dépendance dans `lf-application` (R2) : `rt` pour
/// `task_local!` seulement ; `chrono` sans `clock` ni `now`.
const APPLICATION_FEATURES: [(&str, &[&str]); 2] =
    [("tokio", &["time", "macros", "rt"]), ("chrono", &["std"])];

/// Règle d'un paquet connu du workspace.
struct CrateRule {
    name: &'static str,
    /// Dépendances internes admises.
    internal: &'static [&'static str],
    /// Liste fermée des dépendances externes ; `None` : pas de liste fermée.
    external: Option<&'static [&'static str]>,
    /// Features admises par dépendance ; une dépendance absente n'est pas bornée.
    features: &'static [(&'static str, &'static [&'static str])],
    /// Cible `bin` admise.
    bin: bool,
    /// Cible `lib` admise.
    lib: bool,
}

impl CrateRule {
    fn allowed_features(&self, dep: &str) -> Option<&'static [&'static str]> {
        self.features.iter().find(|(name, _)| *name == dep).map(|(_, allowed)| *allowed)
    }

    fn forbids(&self, dep: &str) -> bool {
        if is_internal(dep) {
            !self.internal.contains(&dep)
        } else {
            dep == "async-trait" || self.external.is_some_and(|allowed| !allowed.contains(&dep))
        }
    }
}

/// Gabarit : bibliothèque sans dépendance interne ni liste externe fermée.
const LIBRARY: CrateRule =
    CrateRule { name: "", internal: &[], external: None, features: &[], bin: false, lib: true };

const RULES: [CrateRule; 8] = [
    CrateRule { name: "lf-domain", external: Some(&DOMAIN_ALLOWED), ..LIBRARY },
    CrateRule {
        name: "lf-application",
        internal: &["lf-domain"],
        external: Some(&APPLICATION_ALLOWED),
        features: &APPLICATION_FEATURES,
        ..LIBRARY
    },
    CrateRule { name: "lf-adapter-piste", internal: &["lf-application", "lf-domain"], ..LIBRARY },
    CrateRule { name: "lf-adapter-cache", internal: &["lf-application", "lf-domain"], ..LIBRARY },
    CrateRule { name: "lf-adapter-audit", internal: &["lf-application"], ..LIBRARY },
    CrateRule { name: "lf-adapter-mcp", internal: &["lf-application", "lf-domain"], ..LIBRARY },
    CrateRule {
        name: "mcp-legifrance",
        internal: &[
            "lf-domain",
            "lf-application",
            "lf-adapter-piste",
            "lf-adapter-cache",
            "lf-adapter-audit",
            "lf-adapter-mcp",
        ],
        bin: true,
        lib: false,
        ..LIBRARY
    },
    CrateRule { name: "xtask", bin: true, ..LIBRARY },
];

fn is_internal(name: &str) -> bool {
    name.starts_with("lf-") || name == "mcp-legifrance" || name == "xtask"
}

/// Liste des violations, une ligne lisible par violation.
pub fn violations(packages: &[Package]) -> Vec<String> {
    let domain = packages.iter().find(|p| p.name == "lf-domain");
    let missing = domain.is_none().then(|| "lf-domain missing from workspace metadata".to_owned());
    missing
        .into_iter()
        .chain(packages.iter().flat_map(|p| match RULES.iter().find(|rule| rule.name == p.name) {
            Some(rule) => member_violations(p, rule, domain),
            None => vec![format!("unknown workspace member: {}", p.name)],
        }))
        .collect()
}

fn member_violations(p: &Package, rule: &CrateRule, domain: Option<&Package>) -> Vec<String> {
    let deps = p
        .deps
        .iter()
        .filter(|dep| rule.forbids(dep))
        .map(|dep| format!("{} must not depend on {dep}", p.name));
    let features = p.features.iter().flat_map(|(dep, enabled)| {
        let allowed = rule.allowed_features(dep);
        enabled
            .iter()
            .filter(move |feature| allowed.is_some_and(|list| !list.contains(&feature.as_str())))
            .map(move |feature| format!("{} must not enable {dep} feature {feature}", p.name))
    });
    let targets = [("bin", rule.bin), ("lib", rule.lib)]
        .into_iter()
        .filter(|(kind, allowed)| !allowed && p.targets.iter().any(|t| t == kind))
        .map(|(kind, _)| format!("{} must not have a {kind} target", p.name));
    let lints =
        (!p.lints_workspace).then(|| format!("{} must declare [lints] workspace = true", p.name));
    deps.chain(features)
        .chain(targets)
        .chain(lints)
        .chain(domain.into_iter().flat_map(|d| workspace_violations(p, d)))
        .collect()
}

/// Égalité avec `[workspace.package]` (5a-5d) et exigences internes `=` (5h).
fn workspace_violations(p: &Package, domain: &Package) -> Vec<String> {
    let fields = [
        ("version", Some(&p.version), Some(&domain.version)),
        ("edition", Some(&p.edition), Some(&domain.edition)),
        ("rust-version", p.rust_version.as_ref(), domain.rust_version.as_ref()),
        ("license", p.license.as_ref(), domain.license.as_ref()),
    ];
    let expected = format!("={}", domain.version);
    fields
        .into_iter()
        .filter(|(_, value, reference)| value != reference)
        .map(|(field, value, reference)| {
            format!(
                "{} {field} {} differs from lf-domain ({})",
                p.name,
                value.map_or("unset", String::as_str),
                reference.map_or("unset", String::as_str),
            )
        })
        .chain(
            p.requirements
                .iter()
                .filter(|(dep, req)| is_internal(dep) && *req != expected)
                .map(|(dep, req)| format!("{} requires {dep} {req}, expected {expected}", p.name)),
        )
        .collect()
}

/// Le manifeste hérite des lints du workspace (`[lints] workspace = true`, 5e),
/// quelle que soit la forme TOML (table, clé pointée, table en ligne).
///
/// # Errors
///
/// Échoue si le texte n'est pas du TOML valide.
pub fn inherits_workspace_lints(manifest: &str) -> Result<bool, String> {
    let table = manifest.parse::<toml::Table>().map_err(|e| format!("Cargo.toml: {e}"))?;
    Ok(table.get("lints").and_then(|l| l.get("workspace")) == Some(&toml::Value::Boolean(true)))
}

/// Extrait les membres du workspace de la sortie de `cargo metadata --format-version 1`.
///
/// # Errors
///
/// Échoue fermé : aucun membre trouvé, ou un membre sans `name` chaîne, sans
/// tableau `dependencies`, sans `version` ou `edition` chaînes, sans tableau
/// `targets`, ou sans `manifest_path` chaîne.
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
    let version =
        p["version"].as_str().ok_or_else(|| format!("cargo metadata: {name} has no version"))?;
    let edition =
        p["edition"].as_str().ok_or_else(|| format!("cargo metadata: {name} has no edition"))?;
    let targets = p["targets"]
        .as_array()
        .ok_or_else(|| format!("cargo metadata: {name} has no targets array"))?;
    let manifest_path = p["manifest_path"]
        .as_str()
        .ok_or_else(|| format!("cargo metadata: {name} has no manifest_path"))?;
    Ok(Package {
        name: name.to_owned(),
        deps: dependencies
            .iter()
            .filter(|d| d["kind"].is_null())
            .filter_map(|d| d["name"].as_str().map(str::to_owned))
            .collect(),
        requirements: dependencies
            .iter()
            .filter_map(|d| Some((d["name"].as_str()?.to_owned(), d["req"].as_str()?.to_owned())))
            .collect(),
        targets: targets
            .iter()
            .flat_map(|t| t["kind"].as_array().into_iter().flatten())
            .filter_map(|k| k.as_str().map(str::to_owned))
            .collect(),
        version: version.to_owned(),
        edition: edition.to_owned(),
        rust_version: p["rust_version"].as_str().map(str::to_owned),
        license: p["license"].as_str().map(str::to_owned),
        features: dependencies
            .iter()
            .filter(|d| d["kind"].is_null())
            .filter_map(read_features)
            .collect(),
        manifest_path: manifest_path.to_owned(),
        lints_workspace: false,
    })
}

/// Features activées d'une dépendance : celles déclarées, plus `default` sauf
/// `uses_default_features: false` (absent : `default`, échec fermé).
fn read_features(dependency: &Value) -> Option<(String, Vec<String>)> {
    let name = dependency["name"].as_str()?;
    let mut features: Vec<String> = dependency["features"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| f.as_str().map(str::to_owned))
        .collect();
    if dependency["uses_default_features"].as_bool() != Some(false) {
        features.push("default".to_owned());
    }
    Some((name.to_owned(), features))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    /// Membre conforme au workspace livré : cible `lib`, version 0.0.0,
    /// édition 2024, MSRV 1.88, licence Apache-2.0, aucune exigence lue.
    fn pkg(name: &str, deps: &[&str]) -> Package {
        Package {
            name: name.into(),
            deps: strings(deps),
            requirements: Vec::new(),
            targets: strings(&["lib"]),
            version: "0.0.0".into(),
            edition: "2024".into(),
            rust_version: Some("1.88".into()),
            license: Some("Apache-2.0".into()),
            features: Vec::new(),
            manifest_path: format!("/w/{name}/Cargo.toml"),
            lints_workspace: true,
        }
    }

    /// Paquet `mcp-legifrance` (répertoire `crates/lf-app`) : une cible `bin` seule.
    fn app(deps: &[&str]) -> Package {
        Package { targets: strings(&["bin"]), ..pkg("mcp-legifrance", deps) }
    }

    /// Le même membre, avec ces features activées (`dépendance`, features).
    fn enabling(package: Package, features: &[(&str, &[&str])]) -> Package {
        let features = features.iter().map(|(dep, f)| ((*dep).to_owned(), strings(f))).collect();
        Package { features, ..package }
    }

    /// Le même membre, avec ces exigences (`nom`, `req`).
    fn requiring(package: Package, requirements: &[(&str, &str)]) -> Package {
        let requirements =
            requirements.iter().map(|(n, r)| ((*n).to_owned(), (*r).to_owned())).collect();
        Package { requirements, ..package }
    }

    #[test]
    fn clean_graph_has_no_violation() {
        let graph = [
            pkg("lf-domain", &["thiserror"]),
            requiring(
                pkg("lf-application", &["lf-domain", "tokio"]),
                &[("lf-domain", "=0.0.0"), ("tokio", "^1.53.2")],
            ),
            pkg("lf-adapter-piste", &["lf-application", "lf-domain", "reqwest"]),
            pkg("lf-adapter-audit", &["lf-application", "sha2"]),
            app(&["lf-adapter-piste", "lf-adapter-mcp", "lf-application"]),
            Package { targets: strings(&["lib", "bin"]), ..pkg("xtask", &["serde_json"]) },
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
            pkg("lf-application", &["lf-adapter-piste", "mcp-legifrance"]),
            pkg("lf-adapter-cache", &["lf-adapter-piste", "mcp-legifrance", "lf-application"]),
        ];
        assert_eq!(
            violations(&graph),
            [
                "lf-application must not depend on lf-adapter-piste",
                "lf-application must not depend on mcp-legifrance",
                "lf-adapter-cache must not depend on lf-adapter-piste",
                "lf-adapter-cache must not depend on mcp-legifrance",
            ]
        );
    }

    #[test]
    fn unknown_member_is_a_violation() {
        let dila = pkg("lf-adapter-dila", &["async-trait", "mcp-legifrance"]);
        let graph = [
            pkg("lf-domain", &["thiserror"]),
            pkg("lf-app", &["lf-domain"]),
            Package { targets: strings(&["lib", "bin"]), edition: "2021".into(), ..dila },
            pkg("xtask", &[]),
        ];
        assert_eq!(
            violations(&graph),
            ["unknown workspace member: lf-app", "unknown workspace member: lf-adapter-dila"]
        );
    }

    #[test]
    fn nothing_depends_on_mcp_legifrance() {
        let graph = [
            pkg("lf-domain", &["thiserror"]),
            pkg("lf-application", &["lf-domain", "mcp-legifrance"]),
            pkg("lf-adapter-piste", &["lf-application", "mcp-legifrance"]),
            app(&[
                "lf-domain",
                "lf-application",
                "lf-adapter-piste",
                "lf-adapter-cache",
                "lf-adapter-audit",
                "lf-adapter-mcp",
            ]),
        ];
        assert_eq!(
            violations(&graph),
            [
                "lf-application must not depend on mcp-legifrance",
                "lf-adapter-piste must not depend on mcp-legifrance",
            ]
        );
    }

    #[test]
    fn mcp_legifrance_has_no_lib_target() {
        let graph = [
            pkg("lf-domain", &["thiserror"]),
            Package { targets: strings(&["lib", "bin"]), ..app(&["lf-application"]) },
        ];
        assert_eq!(violations(&graph), ["mcp-legifrance must not have a lib target"]);
    }

    #[test]
    fn xtask_and_lf_crates_ignore_each_other() {
        let graph = [
            pkg("lf-domain", &["thiserror"]),
            pkg("xtask", &["serde_json", "lf-domain", "mcp-legifrance"]),
            pkg("lf-application", &["lf-domain", "xtask"]),
            app(&["xtask"]),
        ];
        assert_eq!(
            violations(&graph),
            [
                "xtask must not depend on lf-domain",
                "xtask must not depend on mcp-legifrance",
                "lf-application must not depend on xtask",
                "mcp-legifrance must not depend on xtask",
            ]
        );
    }

    #[test]
    fn audit_adapter_reaches_domain_through_application() {
        let graph = [
            pkg("lf-domain", &["thiserror"]),
            pkg("lf-adapter-audit", &["lf-application", "lf-domain", "sha2"]),
        ];
        assert_eq!(violations(&graph), ["lf-adapter-audit must not depend on lf-domain"]);
    }

    #[test]
    fn no_member_depends_on_async_trait() {
        let graph = [
            pkg("lf-domain", &["async-trait"]),
            pkg("lf-application", &["lf-domain", "async-trait"]),
            pkg("lf-adapter-mcp", &["lf-application", "async-trait"]),
            app(&["async-trait"]),
            pkg("xtask", &["serde_json", "async-trait"]),
        ];
        assert_eq!(
            violations(&graph),
            [
                "lf-domain must not depend on async-trait",
                "lf-application must not depend on async-trait",
                "lf-adapter-mcp must not depend on async-trait",
                "mcp-legifrance must not depend on async-trait",
                "xtask must not depend on async-trait",
            ]
        );
    }

    #[test]
    fn internal_requirement_equals_workspace_version() {
        let graph = [
            pkg("lf-domain", &["thiserror"]),
            requiring(
                pkg("lf-application", &["lf-domain"]),
                &[("lf-domain", "=0.0.0"), ("thiserror", "^2.0.21")],
            ),
            requiring(
                pkg("lf-adapter-piste", &["lf-application", "lf-domain"]),
                &[("lf-application", "*"), ("lf-domain", "^0.0")],
            ),
            requiring(
                app(&["lf-application", "lf-adapter-piste"]),
                &[("lf-application", "0.0.0"), ("lf-adapter-piste", "=0.1.0")],
            ),
        ];
        assert_eq!(
            violations(&graph),
            [
                "lf-adapter-piste requires lf-application *, expected =0.0.0",
                "lf-adapter-piste requires lf-domain ^0.0, expected =0.0.0",
                "mcp-legifrance requires lf-application 0.0.0, expected =0.0.0",
                "mcp-legifrance requires lf-adapter-piste =0.1.0, expected =0.0.0",
            ]
        );
    }

    #[test]
    fn only_binaries_have_bin_targets() {
        let graph = [
            Package { targets: strings(&["lib", "bin"]), ..pkg("lf-domain", &[]) },
            Package { targets: strings(&["lib", "bin"]), ..pkg("xtask", &[]) },
            app(&[]),
            Package { targets: strings(&["lib", "example"]), ..pkg("lf-adapter-piste", &[]) },
        ];
        assert_eq!(violations(&graph), ["lf-domain must not have a bin target"]);
    }

    #[test]
    fn members_share_workspace_package() {
        let graph = [
            pkg("lf-domain", &["thiserror"]),
            Package { edition: "2021".into(), ..pkg("xtask", &[]) },
            Package { rust_version: None, ..pkg("lf-application", &[]) },
            Package { license: Some("MIT".into()), ..pkg("lf-adapter-mcp", &[]) },
            Package { version: "0.1.0".into(), ..pkg("lf-adapter-cache", &[]) },
        ];
        assert_eq!(
            violations(&graph),
            [
                "xtask edition 2021 differs from lf-domain (2024)",
                "lf-application rust-version unset differs from lf-domain (1.88)",
                "lf-adapter-mcp license MIT differs from lf-domain (Apache-2.0)",
                "lf-adapter-cache version 0.1.0 differs from lf-domain (0.0.0)",
            ]
        );
        // Sans `lf-domain`, pas de référence : aucune violation de plus.
        let orphan = [Package { edition: "2021".into(), ..pkg("xtask", &[]) }];
        assert_eq!(violations(&orphan), ["lf-domain missing from workspace metadata"]);
    }

    #[test]
    fn check_arch_detects_each_rule() {
        let domain = || pkg("lf-domain", &[]);
        let cases: Vec<(&str, Vec<Package>, &str)> = vec![
            (
                "lf-domain absent",
                vec![pkg("xtask", &[])],
                "lf-domain missing from workspace metadata",
            ),
            (
                "membre inconnu",
                vec![domain(), pkg("lf-app", &[])],
                "unknown workspace member: lf-app",
            ),
            (
                "R1",
                vec![pkg("lf-domain", &["serde_json"])],
                "lf-domain must not depend on serde_json",
            ),
            (
                "R2, frontière interne",
                vec![domain(), pkg("lf-application", &["lf-adapter-piste"])],
                "lf-application must not depend on lf-adapter-piste",
            ),
            (
                "R2, liste fermée",
                vec![domain(), pkg("lf-application", &["serde"])],
                "lf-application must not depend on serde",
            ),
            (
                "R2, features",
                vec![domain(), enabling(pkg("lf-application", &["tokio"]), &[("tokio", &["net"])])],
                "lf-application must not enable tokio feature net",
            ),
            (
                "5e",
                vec![domain(), Package { lints_workspace: false, ..pkg("lf-application", &[]) }],
                "lf-application must declare [lints] workspace = true",
            ),
            (
                "R3",
                vec![domain(), pkg("lf-adapter-cache", &["lf-adapter-piste"])],
                "lf-adapter-cache must not depend on lf-adapter-piste",
            ),
            (
                "R4, 5g",
                vec![domain(), pkg("lf-application", &["mcp-legifrance"])],
                "lf-application must not depend on mcp-legifrance",
            ),
            (
                "R4, cible lib",
                vec![domain(), Package { targets: strings(&["lib", "bin"]), ..app(&[]) }],
                "mcp-legifrance must not have a lib target",
            ),
            (
                "R5",
                vec![domain(), pkg("xtask", &["lf-domain"])],
                "xtask must not depend on lf-domain",
            ),
            (
                "R7",
                vec![domain(), pkg("xtask", &["async-trait"])],
                "xtask must not depend on async-trait",
            ),
            (
                "5a",
                vec![domain(), Package { edition: "2021".into(), ..pkg("xtask", &[]) }],
                "xtask edition 2021 differs from lf-domain (2024)",
            ),
            (
                "5b",
                vec![domain(), Package { rust_version: Some("1.85".into()), ..pkg("xtask", &[]) }],
                "xtask rust-version 1.85 differs from lf-domain (1.88)",
            ),
            (
                "5c",
                vec![domain(), Package { version: "0.1.0".into(), ..pkg("xtask", &[]) }],
                "xtask version 0.1.0 differs from lf-domain (0.0.0)",
            ),
            (
                "5d",
                vec![Package { license: None, ..domain() }, pkg("xtask", &[])],
                "xtask license Apache-2.0 differs from lf-domain (unset)",
            ),
            (
                "5f",
                vec![
                    domain(),
                    Package { targets: strings(&["lib", "bin"]), ..pkg("lf-application", &[]) },
                ],
                "lf-application must not have a bin target",
            ),
            (
                "5h",
                vec![domain(), requiring(pkg("xtask", &[]), &[("lf-domain", "^0.0")])],
                "xtask requires lf-domain ^0.0, expected =0.0.0",
            ),
        ];
        for (rule, graph, expected) in cases {
            assert_eq!(violations(&graph), [expected], "{rule}");
        }
    }

    #[test]
    fn reads_cargo_metadata_members_and_normal_deps() {
        let metadata = json!({
            "workspace_members": ["path+file:///w/crates/lf-domain#0.0.0"],
            "packages": [
                {
                    "id": "path+file:///w/crates/lf-domain#0.0.0",
                    "name": "lf-domain",
                    "version": "0.0.0",
                    "edition": "2024",
                    "manifest_path": "/w/lf-domain/Cargo.toml",
                    "rust_version": "1.88",
                    "license": "Apache-2.0",
                    "targets": [{ "kind": ["lib"] }],
                    "dependencies": [
                        { "name": "thiserror", "kind": null },
                        { "name": "proptest", "kind": "dev" },
                        { "name": "cc", "kind": "build" }
                    ]
                },
                { "id": "registry+https://x#thiserror@2.0.21", "name": "thiserror", "dependencies": [] }
            ]
        });
        let expected = Package {
            lints_workspace: false,
            ..enabling(pkg("lf-domain", &["thiserror"]), &[("thiserror", &["default"])])
        };
        assert_eq!(workspace_packages(&metadata), Ok(vec![expected]));
    }

    #[test]
    fn reads_versions_targets_and_requirements() {
        let id = "path+file:///w/crates/lf-app#mcp-legifrance@0.0.0";
        let metadata = json!({
            "workspace_members": [id],
            "packages": [{
                "id": id,
                "name": "mcp-legifrance",
                "version": "0.0.0",
                "edition": "2024",
                "manifest_path": "/w/crates/lf-app/Cargo.toml",
                "rust_version": null,
                "license": null,
                "targets": [{ "kind": ["bin"] }, { "kind": ["custom-build"] }],
                "dependencies": [
                    {
                        "name": "lf-domain", "req": "=0.0.0", "kind": null,
                        "source": null, "path": "/w/crates/lf-domain"
                    },
                    {
                        "name": "lf-application", "req": "*", "kind": "dev",
                        "source": null, "path": "/w/crates/lf-application"
                    },
                    {
                        "name": "tokio", "req": "^1.53.2", "kind": null,
                        "features": ["rt", "time"], "uses_default_features": false,
                        "source": "registry+https://github.com/rust-lang/crates.io-index"
                    },
                    // Entrée sans nom : ignorée, comme par la lecture livrée de `deps`.
                    { "req": "^1.0", "kind": null }
                ]
            }]
        });
        let expected = Package {
            name: "mcp-legifrance".into(),
            deps: strings(&["lf-domain", "tokio"]),
            requirements: vec![
                ("lf-domain".into(), "=0.0.0".into()),
                ("lf-application".into(), "*".into()),
                ("tokio".into(), "^1.53.2".into()),
            ],
            targets: strings(&["bin", "custom-build"]),
            version: "0.0.0".into(),
            edition: "2024".into(),
            rust_version: None,
            license: None,
            features: vec![
                ("lf-domain".into(), strings(&["default"])),
                ("tokio".into(), strings(&["rt", "time"])),
            ],
            manifest_path: "/w/crates/lf-app/Cargo.toml".into(),
            lints_workspace: false,
        };
        assert_eq!(workspace_packages(&metadata), Ok(vec![expected]));
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

    #[test]
    fn fails_closed_on_member_without_version() {
        let metadata = json!({
            "workspace_members": ["a"],
            "packages": [{ "id": "a", "name": "lf-domain", "dependencies": [] }]
        });
        assert_eq!(
            workspace_packages(&metadata),
            Err("cargo metadata: lf-domain has no version".to_owned())
        );
    }

    #[test]
    fn fails_closed_on_member_without_edition() {
        let metadata = json!({
            "workspace_members": ["a"],
            "packages": [{ "id": "a", "name": "lf-domain", "dependencies": [], "version": "0.0.0" }]
        });
        assert_eq!(
            workspace_packages(&metadata),
            Err("cargo metadata: lf-domain has no edition".to_owned())
        );
    }

    #[test]
    fn fails_closed_on_member_without_targets() {
        let metadata = json!({
            "workspace_members": ["a"],
            "packages": [{
                "id": "a", "name": "lf-domain", "dependencies": [],
                "version": "0.0.0", "edition": "2024"
            }]
        });
        assert_eq!(
            workspace_packages(&metadata),
            Err("cargo metadata: lf-domain has no targets array".to_owned())
        );
    }

    #[test]
    fn fails_closed_on_member_without_manifest_path() {
        let metadata = json!({
            "workspace_members": ["a"],
            "packages": [{ "id": "a", "name": "lf-domain", "dependencies": [],
                           "version": "0.0.0", "edition": "2024", "targets": [] }]
        });
        assert_eq!(
            workspace_packages(&metadata),
            Err("cargo metadata: lf-domain has no manifest_path".to_owned())
        );
    }

    #[test]
    fn application_external_list_is_closed() {
        let allowed =
            ["lf-domain", "thiserror", "chrono", "tracing", "futures", "tokio", "similar"];
        let graph = [
            pkg("lf-domain", &["thiserror"]),
            pkg("lf-application", &allowed),
            pkg("lf-adapter-piste", &["lf-application", "serde", "reqwest", "backon"]),
        ];
        assert_eq!(violations(&graph), Vec::<String>::new());
        let forbidden = ["serde", "serde_json", "rmcp", "reqwest", "moka", "governor", "backon"];
        let graph = [pkg("lf-domain", &["thiserror"]), pkg("lf-application", &forbidden)];
        let expected: Vec<String> =
            forbidden.iter().map(|d| format!("lf-application must not depend on {d}")).collect();
        assert_eq!(violations(&graph), expected);
    }

    #[test]
    fn application_features_are_bounded() {
        let bounded = enabling(
            pkg("lf-application", &["tokio", "chrono"]),
            &[("tokio", &["time", "macros", "rt"]), ("chrono", &["std"])],
        );
        let unbounded =
            enabling(pkg("lf-adapter-piste", &["tokio"]), &[("tokio", &["full", "default"])]);
        assert_eq!(violations(&[pkg("lf-domain", &[]), bounded, unbounded]), Vec::<String>::new());
        let wide = enabling(
            pkg("lf-application", &["tokio", "chrono", "thiserror"]),
            &[
                ("tokio", &["rt", "rt-multi-thread", "time"]),
                ("chrono", &["std", "clock", "default"]),
                ("thiserror", &["default"]),
            ],
        );
        assert_eq!(
            violations(&[pkg("lf-domain", &[]), wide]),
            [
                "lf-application must not enable tokio feature rt-multi-thread",
                "lf-application must not enable chrono feature clock",
                "lf-application must not enable chrono feature default",
            ]
        );
    }

    #[test]
    fn every_member_inherits_workspace_lints() {
        let graph =
            [pkg("lf-domain", &[]), Package { lints_workspace: false, ..pkg("xtask", &[]) }];
        assert_eq!(violations(&graph), ["xtask must declare [lints] workspace = true"]);
    }

    #[test]
    fn reads_workspace_lints_in_every_toml_form() {
        let cases = [
            ("[package]\nname = \"x\"\n\n[lints]\nworkspace = true\n", Ok(true)),
            ("lints.workspace = true", Ok(true)),
            ("lints = { workspace = true }", Ok(true)),
            ("[lints]\nworkspace = false", Ok(false)),
            ("[lints]\nworkspace = \"true\"", Ok(false)),
            ("[lints.clippy]\npedantic = \"warn\"", Ok(false)),
            ("[package]\nname = \"x\"", Ok(false)),
        ];
        for (manifest, expected) in cases {
            assert_eq!(inherits_workspace_lints(manifest), expected, "{manifest}");
        }
        assert!(inherits_workspace_lints("[lints").unwrap_err().starts_with("Cargo.toml: "));
    }
}
