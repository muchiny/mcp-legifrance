//! Règles de dépendance de l'architecture hexagonale (02 §4.1, R1 à R8, et
//! étape 5 de 02 §4.2).
//!
//! Règles en données : une `CrateRule` par paquet connu (`RULES`), moteur pur.
//! Dans les règles, `lf-app` s'écrit `mcp-legifrance` (02 §3.7). Est interne un
//! nom en `lf-*`, `mcp-legifrance` ou `xtask`. Seules les dépendances normales
//! comptent pour R1 à R7 : les dépendances `build` et `dev` n'en déclenchent
//! aucune, R8 lisant les `dev` à part. Contrôles :
//!
//! - membre inconnu : absent de la table, une seule violation, aucun autre
//!   contrôle sur ce membre ;
//! - R1 : `lf-domain` sans dépendance interne, liste blanche externe
//!   `DOMAIN_ALLOWED` (`thiserror`, puis les parseurs de citations de M6 :
//!   `winnow`, `aho-corasick`, `regex`, `unicode-normalization`, ADR-0018) ;
//! - R2 : `lf-application` ne dépend en interne que de `lf-domain`, et en externe
//!   de la liste fermée `APPLICATION_ALLOWED` ; les features qu'elle active sur
//!   `tokio`, `chrono` et `tracing` sont bornées par `APPLICATION_FEATURES` (`default`
//!   comprise : le workspace déclare `default-features = false`), lues dans
//!   les déclarations de dépendance et dans la table `[features]` du membre
//!   (`dep/feat`, `dep?/feat`) ;
//! - R3 : un adapter ne dépend d'aucun autre adapter ni de `mcp-legifrance` ;
//!   `lf-adapter-audit` dépend de `lf-application` seul ;
//! - R4 : personne ne dépend de `mcp-legifrance`, qui peut dépendre de tous les
//!   `lf-*` (permission, pas obligation) et n'a pas de cible `lib` ;
//! - R5 : `xtask` ne dépend d'aucun crate interne, et réciproquement ;
//! - R6 : un crate de `CONFINED` (`rmcp`, `reqwest`, `moka`, `sha2`, `clap`...)
//!   n'est déclaré en dépendance normale que par ses portes, message `{membre}
//!   must not depend on {dep} (confined to {portes})` (portes séparées par
//!   `", "`), en plus de R2 pour `lf-application` (règles distinctes, pas de
//!   dédoublonnage) ; `NEVER_RENAMED` (`rmcp`) interdit en outre tout
//!   renommage dans le manifeste, `{membre} must not rename rmcp` ; `PINNED`
//!   (`rmcp`, `rmcp-macros`) exige l'épinglage exact `=3.5.1`, de tout kind,
//!   `{membre} must pin {dep} to {exigence} (found {lue})` (T-M4-15) ;
//! - R7 : aucun membre ne dépend directement de `async-trait` (ADR-0012) ;
//! - R8 : en dépendance de développement, `lf-domain` et `lf-application`
//!   (`dev_adapters` à `false`) ne prennent aucun adapter ni `mcp-legifrance`,
//!   message `{membre} must not depend on {dep} even in dev-dependencies` ;
//!   les autres membres (`dev_adapters` à `true`) n'ont pas cette limite ;
//! - R10 : aucun membre n'active, en dépendance
//!   normale ou par sa table `[features]`, une feature de `DEV_ONLY_FEATURES`
//!   (`lf-application/test-support`, `lf-adapter-mcp/test-support`), message `{membre} must not enable {dep}
//!   feature {feature} (dev-dependencies only)` ; en `[dev-dependencies]` elle
//!   reste permise (`cargo metadata` classe ces dépendances à part) ; chez le
//!   propriétaire, aucune autre feature (`default`, `full`...) ne la liste,
//!   message `{propriétaire} feature {nom} must not enable {feature}
//!   (dev-dependencies only)` (F-6 : sinon les features par défaut
//!   l'activeraient chez tout membre qui le prend) ;
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
    /// Noms des dépendances de développement (`kind` `dev`), lus à part : seule
    /// R8 les regarde.
    pub dev_deps: Vec<String>,
    /// Noms de paquet (pas l'alias) des dépendances normales renommées dans le
    /// manifeste (`rename` non nul).
    pub renamed: Vec<String>,
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
    /// `default` y figure sauf si `uses_default_features` vaut `false` ; celles
    /// activées par la table `[features]` du membre (`dep/feat`) s'y ajoutent.
    pub features: Vec<(String, Vec<String>)>,
    /// Table `[features]` du membre lui-même (`feature`, entrées), telle que la donne
    /// `cargo metadata` : R10 y cherche une feature qui active `test-support`.
    pub own_features: Vec<(String, Vec<String>)>,
    /// Chemin du `Cargo.toml` du membre.
    pub manifest_path: String,
    /// Le manifeste contient `[lints] workspace = true` (5e) ; `false` à la
    /// lecture de `cargo metadata`, fixé ensuite par `inherits_workspace_lints`.
    pub lints_workspace: bool,
}

/// Seules dépendances normales externes admises dans `lf-domain` : `thiserror`, puis les
/// parseurs de citations de M6 (03 §4, ADR-0018 : élargissement de 00 §11.1).
const DOMAIN_ALLOWED: [&str; 5] =
    ["thiserror", "winnow", "aho-corasick", "regex", "unicode-normalization"];

/// Dépendances normales externes admises dans `lf-application` : liste fermée
/// de R2 (00 §2).
const APPLICATION_ALLOWED: [&str; 6] =
    ["thiserror", "chrono", "tracing", "futures", "tokio", "similar"];

/// Features admises par dépendance dans `lf-application` (R2) : `rt` pour
/// `task_local!` seulement ; `chrono` sans `clock` ni `now`.
const APPLICATION_FEATURES: [(&str, &[&str]); 3] =
    [("tokio", &["time", "macros", "rt"]), ("chrono", &["std"]), ("tracing", &["std"])];

/// Feature réservée aux dépendances de développement (R10) : `(crate, feature)`. Les
/// doublures de `lf-application` (`test-support`) et le harnais client de `lf-adapter-mcp`
/// (`test-support`, qui active `rmcp/client`) ne doivent jamais entrer dans un binaire.
const DEV_ONLY_FEATURES: [(&str, &str); 2] =
    [("lf-application", "test-support"), ("lf-adapter-mcp", "test-support")];

/// Crates confinés (R6) : chacun avec les seuls membres autorisés à le déclarer
/// en dépendance normale (leurs « portes »). `backon` reste ici même si
/// `lf-adapter-piste` ne l'emploie pas : la table décrit la porte, pas l'usage.
/// `chrono-tz` n'est ouvert ni à `lf-domain` ni à `lf-application` (00 §2).
/// Les portes `lf-adapter-dila` sont actives depuis la vague 0 du plan fin-du-projet : le membre
/// figure dans `RULES` (M9, derrière la feature `dila` de `mcp-legifrance`). Dans la table,
/// `lf-app` s'écrit `mcp-legifrance`.
const CONFINED: [(&str, &[&str]); 21] = [
    ("rmcp", &["lf-adapter-mcp"]),
    ("rmcp-macros", &["lf-adapter-mcp"]),
    ("schemars", &["lf-adapter-mcp"]),
    ("axum", &["lf-adapter-mcp"]),
    ("reqwest", &["lf-adapter-piste"]),
    ("rustls", &["lf-adapter-piste"]),
    ("backon", &["lf-adapter-piste"]),
    ("governor", &["lf-adapter-piste"]),
    ("moka", &["lf-adapter-cache"]),
    ("sha2", &["lf-adapter-audit", "xtask"]),
    ("fs4", &["lf-adapter-audit", "xtask"]),
    ("tantivy", &["lf-adapter-dila"]),
    ("quick-xml", &["lf-adapter-dila"]),
    ("tar", &["lf-adapter-dila"]),
    ("flate2", &["lf-adapter-dila"]),
    ("clap", &["mcp-legifrance"]),
    ("tracing-subscriber", &["mcp-legifrance"]),
    ("dirs", &["mcp-legifrance"]),
    ("anyhow", &["mcp-legifrance", "xtask"]),
    ("secrecy", &["lf-adapter-piste", "mcp-legifrance"]),
    ("chrono-tz", &["mcp-legifrance", "lf-adapter-piste"]),
];

/// Crates que le manifeste ne renomme jamais (R6) : un alias contournerait les
/// contrôles par nom de `rmcp`.
const NEVER_RENAMED: [&str; 1] = ["rmcp"];

/// Crates épinglés exactement (R6, T-M4-15) : `(crate, exigence)`. `rmcp` déclare une exigence
/// caret sur `rmcp-macros` : sans épinglage direct des deux, un `cargo update` pourrait les
/// désassortir (02 §3.0.1). Une montée passe par un ADR et par cette table.
const PINNED: [(&str, &str); 2] = [("rmcp", "=3.5.1"), ("rmcp-macros", "=3.5.1")];

/// Portes d'un crate confiné, `None` s'il ne l'est pas.
fn doors(dep: &str) -> Option<&'static [&'static str]> {
    CONFINED.iter().find(|(name, _)| *name == dep).map(|(_, doors)| *doors)
}

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
    /// R8 : les dépendances de développement peuvent être un adapter ou
    /// `mcp-legifrance` ; `false` pour `lf-domain` et `lf-application`.
    dev_adapters: bool,
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

    /// R8 : `dep`, en dépendance de développement, est un adapter ou
    /// `mcp-legifrance` que ce crate n'a pas le droit de prendre.
    fn forbids_in_dev(&self, dep: &str) -> bool {
        !self.dev_adapters && (dep.starts_with("lf-adapter-") || dep == "mcp-legifrance")
    }
}

/// Gabarit : bibliothèque sans dépendance interne ni liste externe fermée.
const LIBRARY: CrateRule = CrateRule {
    name: "",
    internal: &[],
    external: None,
    features: &[],
    bin: false,
    lib: true,
    dev_adapters: true,
};

const RULES: [CrateRule; 9] = [
    CrateRule {
        name: "lf-domain",
        external: Some(&DOMAIN_ALLOWED),
        dev_adapters: false,
        ..LIBRARY
    },
    CrateRule {
        name: "lf-application",
        internal: &["lf-domain"],
        external: Some(&APPLICATION_ALLOWED),
        features: &APPLICATION_FEATURES,
        dev_adapters: false,
        ..LIBRARY
    },
    CrateRule { name: "lf-adapter-piste", internal: &["lf-application", "lf-domain"], ..LIBRARY },
    CrateRule { name: "lf-adapter-cache", internal: &["lf-application", "lf-domain"], ..LIBRARY },
    CrateRule { name: "lf-adapter-audit", internal: &["lf-application"], ..LIBRARY },
    CrateRule { name: "lf-adapter-mcp", internal: &["lf-application", "lf-domain"], ..LIBRARY },
    CrateRule { name: "lf-adapter-dila", internal: &["lf-application", "lf-domain"], ..LIBRARY },
    CrateRule {
        name: "mcp-legifrance",
        internal: &[
            "lf-domain",
            "lf-application",
            "lf-adapter-piste",
            "lf-adapter-cache",
            "lf-adapter-audit",
            "lf-adapter-mcp",
            "lf-adapter-dila",
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
    let confined = p.deps.iter().filter_map(|dep| {
        let members = doors(dep).filter(|members| !members.contains(&p.name.as_str()))?;
        Some(format!("{} must not depend on {dep} (confined to {})", p.name, members.join(", ")))
    });
    let renamed = p
        .renamed
        .iter()
        .filter(|name| NEVER_RENAMED.contains(&name.as_str()))
        .map(|name| format!("{} must not rename {name}", p.name));
    let pinned = p.requirements.iter().filter_map(|(dep, req)| {
        let (_, exact) = PINNED.iter().find(|(name, _)| name == dep)?;
        (req != exact).then(|| format!("{} must pin {dep} to {exact} (found {req})", p.name))
    });
    let dev = p
        .dev_deps
        .iter()
        .filter(|dep| rule.forbids_in_dev(dep))
        .map(|dep| format!("{} must not depend on {dep} even in dev-dependencies", p.name));
    let features = p.features.iter().flat_map(|(dep, enabled)| {
        let allowed = rule.allowed_features(dep);
        enabled
            .iter()
            .filter(move |feature| allowed.is_some_and(|list| !list.contains(&feature.as_str())))
            .map(move |feature| format!("{} must not enable {dep} feature {feature}", p.name))
    });
    let dev_only = DEV_ONLY_FEATURES.iter().flat_map(|(owner, feature)| {
        p.features
            .iter()
            .filter(move |(dep, enabled)| dep == owner && enabled.iter().any(|f| f == feature))
            .map(move |_| {
                format!(
                    "{} must not enable {owner} feature {feature} (dev-dependencies only)",
                    p.name
                )
            })
    });
    // Chez le propriétaire, aucune autre feature ne liste la feature réservée : `default` qui la
    // listerait l'activerait chez tout membre qui prend le crate avec ses features par défaut.
    let dev_only_in_owner = DEV_ONLY_FEATURES
        .iter()
        .filter(|(owner, _)| p.name == *owner)
        .flat_map(|(owner, feature)| {
            p.own_features
                .iter()
                .filter(move |(name, entries)| {
                    name != feature && entries.iter().any(|entry| entry == feature)
                })
                .map(move |(name, _)| {
                    format!(
                        "{owner} feature {name} must not enable {feature} (dev-dependencies only)"
                    )
                })
        });
    let targets = [("bin", rule.bin), ("lib", rule.lib)]
        .into_iter()
        .filter(|(kind, allowed)| !allowed && p.targets.iter().any(|t| t == kind))
        .map(|(kind, _)| format!("{} must not have a {kind} target", p.name));
    let lints =
        (!p.lints_workspace).then(|| format!("{} must declare [lints] workspace = true", p.name));
    deps.chain(confined)
        .chain(renamed)
        .chain(pinned)
        .chain(dev)
        .chain(features)
        .chain(dev_only)
        .chain(dev_only_in_owner)
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
        deps: dependency_names(dependencies, |d| d["kind"].is_null()),
        dev_deps: dependency_names(dependencies, |d| d["kind"].as_str() == Some("dev")),
        renamed: dependency_names(dependencies, |d| d["kind"].is_null() && !d["rename"].is_null()),
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
        features: read_all_features(p, dependencies),
        own_features: read_feature_table(p),
        manifest_path: manifest_path.to_owned(),
        lints_workspace: false,
    })
}

/// Noms de paquet des dépendances retenues par `keep` ; une entrée sans `name`
/// chaîne est ignorée.
fn dependency_names(dependencies: &[Value], keep: impl Fn(&Value) -> bool) -> Vec<String> {
    dependencies
        .iter()
        .filter(|d| keep(d))
        .filter_map(|d| d["name"].as_str().map(str::to_owned))
        .collect()
}

/// Features activées de chaque dépendance normale : celles de ses déclarations
/// (`read_features`), puis celles que la table `[features]` du membre active
/// par `dep/feat` ou `dep?/feat`.
fn read_all_features(p: &Value, dependencies: &[Value]) -> Vec<(String, Vec<String>)> {
    let normal: Vec<&Value> = dependencies.iter().filter(|d| d["kind"].is_null()).collect();
    let mut features: Vec<(String, Vec<String>)> =
        normal.iter().copied().filter_map(read_features).collect();
    let table = p["features"].as_object().into_iter().flat_map(|t| t.values());
    let enabled = table.filter_map(Value::as_array).flatten().filter_map(Value::as_str);
    for (key, feature) in enabled.filter_map(|entry| entry.split_once('/')) {
        let key = key.strip_suffix('?').unwrap_or(key);
        enable(&mut features, dependency_name(&normal, key), feature);
    }
    features
}

/// Table `[features]` du membre : chaque feature et ses entrées chaînes, dans l'ordre de
/// `cargo metadata` (clés triées) ; vide sans table.
fn read_feature_table(p: &Value) -> Vec<(String, Vec<String>)> {
    p["features"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, entries)| {
            let entries = entries.as_array().into_iter().flatten().filter_map(Value::as_str);
            (name.clone(), entries.map(str::to_owned).collect())
        })
        .collect()
}

/// Nom du paquet d'une dépendance normale désignée par sa clé de manifeste :
/// `rename` si elle est renommée, sinon la clé elle-même.
fn dependency_name<'a>(normal: &[&'a Value], key: &'a str) -> &'a str {
    normal
        .iter()
        .find(|d| d["rename"].as_str() == Some(key))
        .and_then(|d| d["name"].as_str())
        .unwrap_or(key)
}

/// Ajoute `feature` aux features activées de `dep`, sans doublon ; une
/// dépendance sans entrée en reçoit une.
fn enable(features: &mut Vec<(String, Vec<String>)>, dep: &str, feature: &str) {
    let position = features.iter().position(|(name, _)| name == dep).unwrap_or_else(|| {
        features.push((dep.to_owned(), Vec::new()));
        features.len() - 1
    });
    let enabled = &mut features[position].1;
    if !enabled.iter().any(|f| f == feature) {
        enabled.push(feature.to_owned());
    }
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
            dev_deps: Vec::new(),
            renamed: Vec::new(),
            requirements: Vec::new(),
            targets: strings(&["lib"]),
            version: "0.0.0".into(),
            edition: "2024".into(),
            rust_version: Some("1.88".into()),
            license: Some("Apache-2.0".into()),
            features: Vec::new(),
            own_features: Vec::new(),
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

    /// Le même membre, avec cette table `[features]` à lui (`feature`, entrées).
    fn declaring(package: Package, table: &[(&str, &[&str])]) -> Package {
        let own_features =
            table.iter().map(|(name, entries)| ((*name).to_owned(), strings(entries))).collect();
        Package { own_features, ..package }
    }

    /// Le même membre, avec ces dépendances de développement.
    fn developing(package: Package, dev_deps: &[&str]) -> Package {
        Package { dev_deps: strings(dev_deps), ..package }
    }

    /// Le même membre, avec ces dépendances normales renommées (noms de paquet).
    fn renaming(package: Package, renamed: &[&str]) -> Package {
        Package { renamed: strings(renamed), ..package }
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
        // `regex` est admis depuis ADR-0018 (parseurs de M6) : il n'est plus une violation.
        assert_eq!(
            violations(&graph),
            [
                "lf-domain must not depend on lf-application",
                "lf-domain must not depend on tokio",
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
        let ghost = pkg("lf-adapter-ghost", &["async-trait", "mcp-legifrance"]);
        let graph = [
            pkg("lf-domain", &["thiserror"]),
            pkg("lf-app", &["lf-domain"]),
            Package { targets: strings(&["lib", "bin"]), edition: "2021".into(), ..ghost },
            pkg("xtask", &[]),
        ];
        assert_eq!(
            violations(&graph),
            ["unknown workspace member: lf-app", "unknown workspace member: lf-adapter-ghost"]
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

    /// Chaque cas (règle, graphe, violation attendue) produit cette seule violation.
    fn assert_each_rule_is_detected(cases: Vec<(&str, Vec<Package>, &str)>) {
        for (rule, graph, expected) in cases {
            assert_eq!(violations(&graph), [expected], "{rule}");
        }
    }

    #[test]
    fn check_arch_detects_each_dependency_rule() {
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
                "R10",
                vec![
                    domain(),
                    enabling(
                        pkg("lf-adapter-mcp", &["lf-application"]),
                        &[("lf-application", &["test-support"])],
                    ),
                ],
                "lf-adapter-mcp must not enable lf-application feature test-support \
                 (dev-dependencies only)",
            ),
            (
                "R10, table du propriétaire",
                vec![
                    domain(),
                    declaring(
                        pkg("lf-application", &[]),
                        &[("test-support", &[]), ("default", &["test-support"])],
                    ),
                ],
                "lf-application feature default must not enable test-support \
                 (dev-dependencies only)",
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
        ];
        assert_each_rule_is_detected(cases);
    }

    #[test]
    fn check_arch_detects_each_workspace_rule() {
        let domain = || pkg("lf-domain", &[]);
        let cases: Vec<(&str, Vec<Package>, &str)> = vec![
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
        assert_each_rule_is_detected(cases);
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
            ..developing(
                enabling(pkg("lf-domain", &["thiserror"]), &[("thiserror", &["default"])]),
                &["proptest"],
            )
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
                        "name": "tokio", "req": "^1.53.2", "kind": null, "rename": "tk",
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
            dev_deps: strings(&["lf-application"]),
            renamed: strings(&["tokio"]),
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
            own_features: Vec::new(),
            manifest_path: "/w/crates/lf-app/Cargo.toml".into(),
            lints_workspace: false,
        };
        assert_eq!(workspace_packages(&metadata), Ok(vec![expected]));
    }

    #[test]
    fn reads_dev_dependencies_separately() {
        let id = "path+file:///w/crates/lf-adapter-mcp#0.0.0";
        let metadata = json!({
            "workspace_members": [id],
            "packages": [{
                "id": id,
                "name": "lf-adapter-mcp",
                "version": "0.0.0",
                "edition": "2024",
                "manifest_path": "/w/crates/lf-adapter-mcp/Cargo.toml",
                "targets": [{ "kind": ["lib"] }],
                "dependencies": [
                    { "name": "rmcp", "kind": null, "rename": null },
                    { "name": "schemars", "kind": null, "rename": "sch" },
                    { "name": "serde", "kind": null },
                    { "name": "rmcp-macros", "kind": "build", "rename": "m" },
                    { "name": "wiremock", "kind": "dev", "rename": "wm" },
                    { "name": "lf-application", "kind": "dev" },
                    // Entrée sans nom : ignorée, comme pour `deps`.
                    { "kind": "dev" }
                ]
            }]
        });
        let packages = workspace_packages(&metadata).unwrap();
        assert_eq!(packages[0].deps, strings(&["rmcp", "schemars", "serde"]));
        assert_eq!(packages[0].dev_deps, strings(&["wiremock", "lf-application"]));
        assert_eq!(packages[0].renamed, strings(&["schemars"]));
    }

    #[test]
    fn reads_features_enabled_through_the_member_features_table() {
        let id = "path+file:///w/crates/lf-application#0.0.0";
        let metadata = json!({
            "workspace_members": [id],
            "packages": [{
                "id": id,
                "name": "lf-application",
                "version": "0.0.0",
                "edition": "2024",
                "manifest_path": "/w/crates/lf-application/Cargo.toml",
                "targets": [{ "kind": ["lib"] }],
                "dependencies": [
                    {
                        "name": "tokio", "kind": null,
                        "features": ["rt"], "uses_default_features": false
                    },
                    { "name": "chrono", "kind": null, "uses_default_features": false },
                    {
                        "name": "serde_json", "rename": "json", "kind": null,
                        "uses_default_features": false
                    },
                    { "name": "mockall", "kind": "dev", "uses_default_features": false }
                ],
                "features": {
                    "x": ["tokio/rt-multi-thread", "chrono?/clock", "dep:foo", "other"],
                    "y": ["json/preserve_order", "tokio/rt-multi-thread", "chrono/clock"],
                    "z": ["unknown/derive"]
                }
            }]
        });
        let packages = workspace_packages(&metadata).unwrap();
        assert_eq!(
            packages[0].features,
            vec![
                ("tokio".to_owned(), strings(&["rt", "rt-multi-thread"])),
                ("chrono".to_owned(), strings(&["clock"])),
                ("serde_json".to_owned(), strings(&["preserve_order"])),
                ("unknown".to_owned(), strings(&["derive"])),
            ]
        );
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
        // R2 (liste fermée) d'abord, puis R6 pour les crates aussi confinés.
        let confined = [
            ("rmcp", "lf-adapter-mcp"),
            ("reqwest", "lf-adapter-piste"),
            ("moka", "lf-adapter-cache"),
            ("governor", "lf-adapter-piste"),
            ("backon", "lf-adapter-piste"),
        ];
        let expected: Vec<String> = forbidden
            .iter()
            .map(|d| format!("lf-application must not depend on {d}"))
            .chain(confined.iter().map(|(d, door)| {
                format!("lf-application must not depend on {d} (confined to {door})")
            }))
            .collect();
        assert_eq!(violations(&graph), expected);
    }

    /// Crates confinés et leurs portes, recopiés à la main : les `const` ne sont
    /// pas mutées, cette copie protège le contenu de `CONFINED`.
    const DOORS: [(&str, &[&str]); 21] = [
        ("rmcp", &["lf-adapter-mcp"]),
        ("rmcp-macros", &["lf-adapter-mcp"]),
        ("schemars", &["lf-adapter-mcp"]),
        ("axum", &["lf-adapter-mcp"]),
        ("reqwest", &["lf-adapter-piste"]),
        ("rustls", &["lf-adapter-piste"]),
        ("backon", &["lf-adapter-piste"]),
        ("governor", &["lf-adapter-piste"]),
        ("moka", &["lf-adapter-cache"]),
        ("sha2", &["lf-adapter-audit", "xtask"]),
        ("fs4", &["lf-adapter-audit", "xtask"]),
        ("tantivy", &["lf-adapter-dila"]),
        ("quick-xml", &["lf-adapter-dila"]),
        ("tar", &["lf-adapter-dila"]),
        ("flate2", &["lf-adapter-dila"]),
        ("clap", &["mcp-legifrance"]),
        ("tracing-subscriber", &["mcp-legifrance"]),
        ("dirs", &["mcp-legifrance"]),
        ("anyhow", &["mcp-legifrance", "xtask"]),
        ("secrecy", &["lf-adapter-piste", "mcp-legifrance"]),
        ("chrono-tz", &["mcp-legifrance", "lf-adapter-piste"]),
    ];

    /// Membres connus de `RULES`.
    const MEMBERS: [&str; 9] = [
        "lf-domain",
        "lf-application",
        "lf-adapter-piste",
        "lf-adapter-cache",
        "lf-adapter-audit",
        "lf-adapter-mcp",
        "lf-adapter-dila",
        "mcp-legifrance",
        "xtask",
    ];

    /// Les seules violations de R6 d'un graphe.
    fn confinement_violations(graph: &[Package]) -> Vec<String> {
        violations(graph).into_iter().filter(|v| v.contains("(confined to ")).collect()
    }

    #[test]
    fn confined_crates_have_one_door() {
        for (dep, doors) in DOORS {
            for member in MEMBERS {
                let graph = [pkg("lf-domain", &[]), pkg(member, &[dep])];
                let expected: Vec<String> = if doors.contains(&member) {
                    Vec::new()
                } else {
                    vec![format!(
                        "{member} must not depend on {dep} (confined to {})",
                        doors.join(", ")
                    )]
                };
                assert_eq!(confinement_violations(&graph), expected, "{member} -> {dep}");
            }
        }
    }

    #[test]
    fn confinement_ignores_development_dependencies() {
        let graph = [
            pkg("lf-domain", &[]),
            developing(pkg("lf-application", &[]), &["reqwest", "rmcp", "moka"]),
            developing(pkg("lf-adapter-cache", &[]), &["sha2", "anyhow"]),
        ];
        assert_eq!(violations(&graph), Vec::<String>::new());
    }

    #[test]
    fn rmcp_must_not_be_renamed() {
        let mcp = || pkg("lf-adapter-mcp", &["lf-application", "rmcp", "schemars"]);
        let graph = |mcp| [pkg("lf-domain", &[]), mcp];
        assert_eq!(violations(&graph(mcp())), Vec::<String>::new());
        assert_eq!(violations(&graph(renaming(mcp(), &["schemars"]))), Vec::<String>::new());
        assert_eq!(
            violations(&graph(renaming(mcp(), &["rmcp", "schemars"]))),
            ["lf-adapter-mcp must not rename rmcp"]
        );
    }

    #[test]
    fn dev_dependencies_of_pure_crates_take_no_adapter() {
        let graph = [
            developing(
                pkg("lf-domain", &[]),
                &["proptest", "lf-adapter-piste", "mcp-legifrance", "lf-adapter-dila"],
            ),
            developing(
                pkg("lf-application", &["lf-domain"]),
                &["lf-domain", "tokio", "lf-adapter-mcp", "lf-adapter-cache"],
            ),
        ];
        assert_eq!(
            violations(&graph),
            [
                "lf-domain must not depend on lf-adapter-piste even in dev-dependencies",
                "lf-domain must not depend on mcp-legifrance even in dev-dependencies",
                "lf-domain must not depend on lf-adapter-dila even in dev-dependencies",
                "lf-application must not depend on lf-adapter-mcp even in dev-dependencies",
                "lf-application must not depend on lf-adapter-cache even in dev-dependencies",
            ]
        );
    }

    #[test]
    fn adapters_may_take_application_in_dev() {
        let graph = [
            pkg("lf-domain", &[]),
            developing(
                pkg("lf-adapter-piste", &[]),
                &["lf-application", "lf-domain", "tokio", "wiremock"],
            ),
            developing(pkg("lf-adapter-mcp", &[]), &["lf-application"]),
        ];
        assert_eq!(violations(&graph), Vec::<String>::new());
    }

    #[test]
    fn dev_dependencies_do_not_trigger_other_rules() {
        let dev = ["serde_json", "tokio", "reqwest", "async-trait", "lf-domain", "xtask"];
        let graph = [
            developing(pkg("lf-domain", &[]), &dev[..4]),
            developing(pkg("lf-application", &["lf-domain"]), &dev[..4]),
            developing(pkg("xtask", &[]), &dev),
            developing(app(&[]), &dev),
            developing(pkg("lf-adapter-cache", &[]), &["lf-adapter-piste"]),
        ];
        assert_eq!(violations(&graph), Vec::<String>::new());
    }

    #[test]
    fn application_features_are_bounded() {
        let bounded = enabling(
            pkg("lf-application", &["tokio", "chrono", "tracing"]),
            &[("tokio", &["time", "macros", "rt"]), ("chrono", &["std"]), ("tracing", &["std"])],
        );
        let unbounded =
            enabling(pkg("lf-adapter-piste", &["tokio"]), &[("tokio", &["full", "default"])]);
        assert_eq!(violations(&[pkg("lf-domain", &[]), bounded, unbounded]), Vec::<String>::new());
        let wide = enabling(
            pkg("lf-application", &["tokio", "chrono", "tracing", "thiserror"]),
            &[
                ("tokio", &["rt", "rt-multi-thread", "time"]),
                ("chrono", &["std", "clock", "default"]),
                ("tracing", &["std", "attributes"]),
                ("thiserror", &["default"]),
            ],
        );
        assert_eq!(
            violations(&[pkg("lf-domain", &[]), wide]),
            [
                "lf-application must not enable tokio feature rt-multi-thread",
                "lf-application must not enable chrono feature clock",
                "lf-application must not enable chrono feature default",
                "lf-application must not enable tracing feature attributes",
            ]
        );
    }

    #[test]
    fn application_features_enabled_through_its_own_table_are_bounded() {
        let metadata = json!({
            "workspace_members": ["a", "b"],
            "packages": [
                {
                    "id": "a", "name": "lf-domain", "version": "0.0.0", "edition": "2024",
                    "manifest_path": "/w/lf-domain/Cargo.toml", "targets": [],
                    "dependencies": []
                },
                {
                    "id": "b", "name": "lf-application", "version": "0.0.0", "edition": "2024",
                    "manifest_path": "/w/lf-application/Cargo.toml", "targets": [],
                    "dependencies": [
                        { "name": "tokio", "kind": null, "features": ["rt"],
                          "uses_default_features": false }
                    ],
                    "features": { "sneaky": ["tokio/rt-multi-thread"] }
                }
            ]
        });
        let graph: Vec<Package> = workspace_packages(&metadata)
            .unwrap()
            .into_iter()
            .map(|p| Package { lints_workspace: true, ..p })
            .collect();
        assert_eq!(
            violations(&graph),
            ["lf-application must not enable tokio feature rt-multi-thread"]
        );
    }

    /// Manifeste de fixture à deux membres : `lf-adapter-mcp` prend `lf-application`
    /// avec les `dependencies` données et les `features` de sa propre table.
    fn adapter_taking_application(dependencies: &Value, features: &Value) -> Vec<String> {
        let metadata = json!({
            "workspace_members": ["a", "b"],
            "packages": [
                {
                    "id": "a", "name": "lf-domain", "version": "0.0.0", "edition": "2024",
                    "manifest_path": "/w/lf-domain/Cargo.toml", "targets": [],
                    "dependencies": []
                },
                {
                    "id": "b", "name": "lf-adapter-mcp", "version": "0.0.0", "edition": "2024",
                    "manifest_path": "/w/lf-adapter-mcp/Cargo.toml", "targets": [],
                    "dependencies": dependencies,
                    "features": features
                }
            ]
        });
        let graph: Vec<Package> = workspace_packages(&metadata)
            .unwrap()
            .into_iter()
            .map(|p| Package { lints_workspace: true, ..p })
            .collect();
        violations(&graph)
    }

    #[test]
    fn test_support_is_only_for_dev_dependencies() {
        let forbidden = ["lf-adapter-mcp must not enable lf-application feature test-support \
              (dev-dependencies only)"];
        // Activée sur la déclaration normale.
        let normal = json!([
            { "name": "lf-application", "kind": null, "features": ["test-support"],
              "uses_default_features": false }
        ]);
        assert_eq!(adapter_taking_application(&normal, &json!({})), forbidden);
        // Activée par la table `[features]` du membre (`dep/feat`).
        let plain = json!([
            { "name": "lf-application", "kind": null, "uses_default_features": false }
        ]);
        let table = json!({ "sneaky": ["lf-application/test-support"] });
        assert_eq!(adapter_taking_application(&plain, &table), forbidden);
        // Permise en dépendance de développement, y compris à côté d'une normale sans feature.
        let both = json!([
            { "name": "lf-application", "kind": null, "uses_default_features": false },
            { "name": "lf-application", "kind": "dev", "features": ["test-support"],
              "uses_default_features": false }
        ]);
        assert_eq!(adapter_taking_application(&both, &json!({})), Vec::<String>::new());
    }

    #[test]
    fn no_other_feature_of_the_owner_may_list_test_support() {
        // `default = ["test-support"]` l'activerait chez tout membre qui prend `lf-application`
        // avec ses features par défaut (déclaration du workspace sans `default-features`), sans
        // que la déclaration du membre ne la nomme : R10 la refuse dans la table du propriétaire,
        // comme toute feature qui la liste (`full`), même en compagnie (F-6).
        let metadata = json!({
            "workspace_members": ["a", "b", "c"],
            "packages": [
                {
                    "id": "a", "name": "lf-domain", "version": "0.0.0", "edition": "2024",
                    "manifest_path": "/w/lf-domain/Cargo.toml", "targets": [],
                    "dependencies": []
                },
                {
                    "id": "b", "name": "lf-application", "version": "0.0.0", "edition": "2024",
                    "manifest_path": "/w/lf-application/Cargo.toml", "targets": [],
                    "dependencies": [],
                    "features": {
                        "test-support": [],
                        "default": ["test-support"],
                        "full": ["other", "test-support"],
                        "other": ["dep:foo"],
                        "near": ["test-supports", "lf-application/x"]
                    }
                },
                {
                    "id": "c", "name": "lf-adapter-piste", "version": "0.0.0", "edition": "2024",
                    "manifest_path": "/w/lf-adapter-piste/Cargo.toml", "targets": [],
                    "dependencies": [{ "name": "lf-application", "kind": null }],
                    "features": { "default": ["test-support"] }
                }
            ]
        });
        let graph: Vec<Package> = workspace_packages(&metadata)
            .unwrap()
            .into_iter()
            .map(|p| Package { lints_workspace: true, ..p })
            .collect();
        // La table `[features]` est lue telle quelle, dans l'ordre de `cargo metadata`.
        assert_eq!(
            graph[1].own_features,
            vec![
                ("default".to_owned(), strings(&["test-support"])),
                ("full".to_owned(), strings(&["other", "test-support"])),
                ("near".to_owned(), strings(&["test-supports", "lf-application/x"])),
                ("other".to_owned(), strings(&["dep:foo"])),
                ("test-support".to_owned(), Vec::new()),
            ]
        );
        // Seul le propriétaire est visé : une feature `test-support` d'un autre membre n'est
        // pas celle de `lf-application`.
        assert_eq!(
            violations(&graph),
            [
                "lf-application feature default must not enable test-support \
                 (dev-dependencies only)",
                "lf-application feature full must not enable test-support (dev-dependencies only)",
            ]
        );
        // Le workspace livré : `test-support = []` seule, aucune violation.
        let clean = declaring(pkg("lf-application", &[]), &[("test-support", &[])]);
        assert_eq!(violations(&[pkg("lf-domain", &[]), clean]), Vec::<String>::new());
    }

    #[test]
    fn rmcp_must_be_pinned_exactly() {
        let mcp = |requirements: &[(&str, &str)]| {
            let package =
                requiring(pkg("lf-adapter-mcp", &["lf-application", "rmcp"]), requirements);
            [pkg("lf-domain", &[]), package]
        };
        // Les deux épinglés `=3.5.1` : aucune violation (les autres exigences sont libres).
        let pinned = [("rmcp", "=3.5.1"), ("rmcp-macros", "=3.5.1"), ("schemars", "^1.2")];
        assert_eq!(violations(&mcp(&pinned)), Vec::<String>::new());
        // Exigence caret, plancher, autre version exacte ou intervalle : refusée, pour chacun.
        for (dep, found) in [
            ("rmcp", "^3.5.1"),
            ("rmcp", ">=3.5.1"),
            ("rmcp", "=3.5.2"),
            ("rmcp-macros", "^3.5"),
            ("rmcp-macros", "=3.5.1, <4"),
            ("rmcp", "*"),
        ] {
            assert_eq!(
                violations(&mcp(&[(dep, found)])),
                [format!("lf-adapter-mcp must pin {dep} to =3.5.1 (found {found})")],
                "{dep} {found}"
            );
        }
        // Les deux désassortis : deux violations, dans l'ordre des exigences.
        assert_eq!(
            violations(&mcp(&[("rmcp", "^3"), ("rmcp-macros", "^3")])),
            [
                "lf-adapter-mcp must pin rmcp to =3.5.1 (found ^3)",
                "lf-adapter-mcp must pin rmcp-macros to =3.5.1 (found ^3)",
            ]
        );
        // Lue dans `cargo metadata`, quel que soit le kind : un `rmcp` de développement non
        // épinglé (feature `client`) est refusé comme une dépendance normale.
        let metadata = json!({
            "workspace_members": ["a", "b"],
            "packages": [
                {
                    "id": "a", "name": "lf-domain", "version": "0.0.0", "edition": "2024",
                    "manifest_path": "/w/lf-domain/Cargo.toml", "targets": [],
                    "dependencies": []
                },
                {
                    "id": "b", "name": "lf-adapter-mcp", "version": "0.0.0", "edition": "2024",
                    "manifest_path": "/w/lf-adapter-mcp/Cargo.toml", "targets": [],
                    "dependencies": [
                        { "name": "rmcp", "kind": null, "req": "=3.5.1" },
                        { "name": "rmcp", "kind": "dev", "req": "^3.5", "features": ["client"] }
                    ]
                }
            ]
        });
        let graph: Vec<Package> = workspace_packages(&metadata)
            .unwrap()
            .into_iter()
            .map(|p| Package { lints_workspace: true, ..p })
            .collect();
        assert_eq!(violations(&graph), ["lf-adapter-mcp must pin rmcp to =3.5.1 (found ^3.5)"]);
    }

    #[test]
    fn dila_member_has_a_rule() {
        let dila = |deps: &[&str]| [pkg("lf-domain", &[]), pkg("lf-adapter-dila", deps)];
        // Ses portes (R6) et ses internes admis : `lf-application` et `lf-domain`.
        let own = ["lf-application", "lf-domain", "tantivy", "quick-xml", "tar", "flate2"];
        assert_eq!(violations(&dila(&own)), Vec::<String>::new());
        // R3 : ni autre adapter ni `mcp-legifrance` ; R6 : pas les crates des autres portes.
        assert_eq!(
            violations(&dila(&["lf-adapter-piste", "mcp-legifrance", "moka", "sha2"])),
            [
                "lf-adapter-dila must not depend on lf-adapter-piste",
                "lf-adapter-dila must not depend on mcp-legifrance",
                "lf-adapter-dila must not depend on moka (confined to lf-adapter-cache)",
                "lf-adapter-dila must not depend on sha2 (confined to lf-adapter-audit, xtask)",
            ]
        );
        // `mcp-legifrance` peut le prendre (feature `dila`) ; `lf-application` et un autre
        // adapter, non.
        let graph = [
            pkg("lf-domain", &[]),
            pkg("lf-application", &["lf-adapter-dila"]),
            pkg("lf-adapter-cache", &["lf-adapter-dila"]),
            app(&["lf-adapter-dila"]),
        ];
        assert_eq!(
            violations(&graph),
            [
                "lf-application must not depend on lf-adapter-dila",
                "lf-adapter-cache must not depend on lf-adapter-dila",
            ]
        );
    }

    #[test]
    fn domain_allows_m6_parsers() {
        let parsers = ["thiserror", "winnow", "aho-corasick", "regex", "unicode-normalization"];
        assert_eq!(violations(&[pkg("lf-domain", &parsers)]), Vec::<String>::new());
        // Seuls ces quatre crates sont admis en plus de `thiserror` : tout autre reste refusé.
        let graph = [pkg("lf-domain", &["winnow", "similar", "regex-lite", "serde", "tantivy"])];
        assert_eq!(
            violations(&graph),
            [
                "lf-domain must not depend on similar",
                "lf-domain must not depend on regex-lite",
                "lf-domain must not depend on serde",
                "lf-domain must not depend on tantivy",
                "lf-domain must not depend on tantivy (confined to lf-adapter-dila)",
            ]
        );
    }

    #[test]
    fn mcp_test_support_is_dev_only() {
        let forbidden = "mcp-legifrance must not enable lf-adapter-mcp feature test-support \
                         (dev-dependencies only)";
        let graph = |app: Package, mcp: Package| [pkg("lf-domain", &[]), mcp, app];
        let table = || declaring(pkg("lf-adapter-mcp", &[]), &[("test-support", &["rmcp/client"])]);
        // Le workspace livré : `test-support` seule chez le propriétaire, activée en dev chez
        // `mcp-legifrance` (que `cargo metadata` classe à part) : aucune violation.
        let dev = developing(app(&["lf-adapter-mcp"]), &["lf-adapter-mcp"]);
        assert_eq!(violations(&graph(dev, table())), Vec::<String>::new());
        // Activée sur la déclaration normale du binaire.
        let normal = enabling(app(&["lf-adapter-mcp"]), &[("lf-adapter-mcp", &["test-support"])]);
        assert_eq!(violations(&graph(normal, table())), [forbidden]);
        // Aucune autre feature du propriétaire ne la liste (`default` l'activerait partout).
        let listing = declaring(
            pkg("lf-adapter-mcp", &[]),
            &[("test-support", &["rmcp/client"]), ("default", &["test-support"])],
        );
        assert_eq!(
            violations(&graph(app(&["lf-adapter-mcp"]), listing)),
            ["lf-adapter-mcp feature default must not enable test-support (dev-dependencies only)"]
        );
        // `lf-application/test-support` reste couverte par la même règle, sans interférence.
        let both = enabling(
            app(&["lf-adapter-mcp", "lf-application"]),
            &[("lf-application", &["test-support"])],
        );
        assert_eq!(
            violations(&graph(both, table())),
            ["mcp-legifrance must not enable lf-application feature test-support \
              (dev-dependencies only)"]
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
