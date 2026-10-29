//! Vue de sortie de `fr_coverage` (05 §5.8) : ce que le serveur couvre, son environnement et l'état
//! de sa configuration.
//!
//! Le domaine calcule le statut, les familles (une par fond servi) et les lacunes (catalogue
//! FR-001 à FR-009, puis FR-101 à FR-105) ; la vue y ajoute ce qui est de présentation : le libellé
//! et les outils de chaque famille (le domaine ne nomme aucun outil), les outils transverses selon
//! la surface annoncée, la source, l'attribution des données et la date de consultation (Q15).
//! Pas de commentaire de doc sur les champs : le schéma de sortie n'en porte aucun.

use lf_application::{
    audited::AuditMode, output::coverage::CoverageOutput, ports::PisteEnvironment, tools::ToolName,
};
use lf_domain::{
    contract::ELI_NOTE,
    coverage::{CoverageFamily, CoverageGap, CoverageStatus},
    fond::Fond,
};
use schemars::JsonSchema;
use serde::Serialize;

use crate::text::{SURFACE, Surface};

/// `source` : d'où viennent les données (05 §5.8).
pub(crate) const SOURCE: &str = "Légifrance (DILA), through the official PISTE API.";

/// `source` quand les textes viennent de l'index local DILA : un instantané des dumps LEGI, non
/// l'API PISTE interrogée en direct (V4-02).
pub(crate) const SNAPSHOT_SOURCE: &str =
    "Légifrance (DILA), from a local snapshot of the LEGI dumps; not queried live.";

/// `attribution` : mention exacte de la Licence Ouverte (00 §12, Q15).
pub(crate) const ATTRIBUTION: &str = "Source : Légifrance / DILA — Licence Ouverte 2.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum StatusView {
    Ok,
    Degraded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum EnvironmentView {
    Sandbox,
    Production,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(inline)]
pub(crate) enum AuditModeView {
    Strict,
    BestEffort,
}

#[derive(Debug, Serialize, JsonSchema)]
#[schemars(inline)]
pub(crate) struct ConfigurationView {
    pub(crate) credentials_configured: bool,
    pub(crate) audit_mode: AuditModeView,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) problems: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[schemars(inline)]
pub(crate) struct FamilyView {
    pub(crate) fond: String,
    pub(crate) label: String,
    pub(crate) tools: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[schemars(inline)]
pub(crate) struct GapView {
    pub(crate) id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fond: Option<String>,
    pub(crate) missing: String,
    pub(crate) fallback: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct CoverageView {
    pub(crate) status: StatusView,
    pub(crate) environment: EnvironmentView,
    pub(crate) configuration: ConfigurationView,
    pub(crate) source: String,
    pub(crate) attribution: String,
    #[schemars(extend("format" = "date"))]
    pub(crate) consulted_on: String,
    pub(crate) as_of_note: String,
    pub(crate) eli_note: String,
    pub(crate) general_tools: Vec<String>,
    pub(crate) families: Vec<FamilyView>,
    pub(crate) gaps: Vec<GapView>,
}

/// Libellé d'une famille (le fond, côté lecteur).
const fn label(fond: Fond) -> &'static str {
    match fond {
        Fond::Loda => "Laws, ordinances and decrees (consolidated)",
        Fond::Code => "Legal codes (consolidated)",
        Fond::Juri => "Judicial case law (Cour de cassation, courts of appeal; selection)",
        Fond::Constit => "Conseil constitutionnel decisions",
        Fond::Cetat => "Conseil d'État, administrative courts of appeal and tribunals (selection)",
        Fond::Cnil => "CNIL deliberations",
        Fond::Kali => "Collective agreements (conventions collectives)",
        Fond::Acco => "Company agreements (metadata only)",
    }
}

/// Outils de lecture d'un fond selon la surface annoncée : `fr_get_related` s'ajoute aux fonds
/// LODA, CODE, JURI, CONSTIT et CETAT, et `fr_diff_article` à LODA et CODE, en v0.3 (05 §5.8).
fn tools_of(fond: Fond, surface: Surface) -> Vec<&'static str> {
    let mut tools = match fond {
        Fond::Loda | Fond::Code => vec![ToolName::FR_GET_ACT, ToolName::FR_GET_TEXT],
        Fond::Juri | Fond::Constit | Fond::Cetat => vec![ToolName::FR_GET_DECISION],
        Fond::Cnil => vec![ToolName::FR_GET_DELIBERATION],
        Fond::Kali => vec![ToolName::FR_GET_CONVENTION, ToolName::FR_GET_TEXT],
        Fond::Acco => vec![ToolName::FR_GET_COMPANY_AGREEMENT],
    };
    if surface == Surface::V03 {
        if matches!(fond, Fond::Loda | Fond::Code | Fond::Juri | Fond::Constit | Fond::Cetat) {
            tools.push(ToolName::FR_GET_RELATED);
        }
        if matches!(fond, Fond::Loda | Fond::Code) {
            tools.push(ToolName::FR_DIFF_ARTICLE);
        }
    }
    tools.into_iter().map(ToolName::as_str).collect()
}

/// Outils transverses selon la surface : `fr_search` et `fr_coverage`, plus la vérification et la
/// résolution de citations en v0.2, plus la recherche multi-fonds en v0.3.
fn general_tools(surface: Surface) -> Vec<&'static str> {
    let mut tools = vec![ToolName::FR_SEARCH, ToolName::FR_COVERAGE];
    if surface != Surface::V01 {
        tools.extend([ToolName::FR_RESOLVE, ToolName::FR_VERIFY_CITATIONS]);
    }
    if surface == Surface::V03 {
        tools.push(ToolName::FR_SEARCH_ALL);
    }
    tools.into_iter().map(ToolName::as_str).collect()
}

const fn status_view(status: CoverageStatus) -> StatusView {
    match status {
        CoverageStatus::Ok => StatusView::Ok,
        CoverageStatus::Degraded => StatusView::Degraded,
        CoverageStatus::Failed => StatusView::Failed,
    }
}

const fn environment_view(environment: PisteEnvironment) -> EnvironmentView {
    match environment {
        PisteEnvironment::Sandbox => EnvironmentView::Sandbox,
        PisteEnvironment::Production => EnvironmentView::Production,
    }
}

const fn audit_mode_view(mode: AuditMode) -> AuditModeView {
    match mode {
        AuditMode::Strict => AuditModeView::Strict,
        AuditMode::BestEffort => AuditModeView::BestEffort,
    }
}

/// Famille rendue : le domaine nomme le fond (`CoverageFamily::name`), la vue y rattache le
/// libellé et les outils. Un nom qui ne désigne aucun fond (impossible depuis `coverage()`) est
/// écarté.
fn family_view(family: &CoverageFamily, surface: Surface) -> Option<FamilyView> {
    let fond = Fond::parse(&family.name).ok()?;
    Some(FamilyView {
        fond: fond.name().to_owned(),
        label: label(fond).to_owned(),
        tools: tools_of(fond, surface).into_iter().map(str::to_owned).collect(),
    })
}

/// Lacune rendue : `fond` n'est présent que si la lacune en désigne un.
fn gap_view(gap: &CoverageGap) -> GapView {
    GapView {
        id: gap.id.clone(),
        fond: Fond::parse(&gap.family).ok().map(|fond| fond.name().to_owned()),
        missing: gap.missing.clone(),
        fallback: gap.fallback.clone(),
    }
}

impl CoverageView {
    /// La vue de `output` pour la surface `surface` (v0.1, v0.2 ou v0.3).
    pub(crate) fn new(output: CoverageOutput, surface: Surface) -> Self {
        let CoverageOutput { coverage, environment, configuration, consulted_on } = output;
        let source = if configuration.local_snapshot { SNAPSHOT_SOURCE } else { SOURCE };
        Self {
            status: status_view(coverage.status()),
            environment: environment_view(environment),
            configuration: ConfigurationView {
                credentials_configured: configuration.credentials_configured,
                audit_mode: audit_mode_view(configuration.audit_mode),
                problems: configuration.problems,
            },
            source: source.to_owned(),
            attribution: ATTRIBUTION.to_owned(),
            consulted_on: consulted_on.to_string(),
            as_of_note: coverage.as_of_note().to_owned(),
            eli_note: ELI_NOTE.to_owned(),
            general_tools: general_tools(surface).into_iter().map(str::to_owned).collect(),
            families: coverage
                .families()
                .iter()
                .filter_map(|family| family_view(family, surface))
                .collect(),
            gaps: coverage.known_gaps().iter().map(gap_view).collect(),
        }
    }
}

impl From<CoverageOutput> for CoverageView {
    fn from(output: CoverageOutput) -> Self {
        Self::new(output, SURFACE)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use lf_application::output::coverage::CoverageConfiguration;
    use lf_domain::{
        coverage::{ConfigFacts, coverage},
        date::CivilDate,
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::schema::output_schema;

    /// Sortie de l'application pour un environnement et une configuration.
    fn output(
        environment: PisteEnvironment,
        credentials_configured: bool,
        custom_host: bool,
        problems: &[&str],
    ) -> CoverageOutput {
        let facts = ConfigFacts {
            credentials_configured,
            custom_host,
            has_problems: !problems.is_empty(),
            local_snapshot: false,
        };
        CoverageOutput {
            coverage: coverage(environment, &facts, &Fond::ALL),
            environment,
            configuration: CoverageConfiguration {
                credentials_configured,
                audit_mode: AuditMode::Strict,
                problems: problems.iter().map(|problem| (*problem).to_owned()).collect(),
                local_snapshot: false,
            },
            consulted_on: CivilDate::new(2026, 10, 15).unwrap(),
        }
    }

    fn sandbox_view(surface: Surface) -> Value {
        serde_json::to_value(CoverageView::new(
            output(PisteEnvironment::Sandbox, true, false, &[]),
            surface,
        ))
        .unwrap()
    }

    fn strings(value: &Value) -> Vec<&str> {
        value.as_array().unwrap().iter().map(|item| item.as_str().unwrap()).collect()
    }

    /// Tous les outils nommés par la vue : transverses et par famille.
    fn named_tools(view: &Value) -> BTreeSet<&str> {
        let mut names: BTreeSet<&str> = strings(&view["general_tools"]).into_iter().collect();
        for family in view["families"].as_array().unwrap() {
            names.extend(strings(&family["tools"]));
        }
        names
    }

    #[test]
    fn coverage_v01_sandbox() {
        // Règle S : les snapshots vivent dans `src/snapshots/`, pas dans `src/views/snapshots/`.
        insta::with_settings!({snapshot_path => "../snapshots"}, {
            insta::assert_json_snapshot!(sandbox_view(Surface::V01));
        });
    }

    #[test]
    fn the_v01_sandbox_response_stays_near_its_target() {
        // Cible de 05 §6.5 : 3 389 caractères de JSON compact, plus 10 % de marge.
        let size = serde_json::to_string(&sandbox_view(Surface::V01)).unwrap().chars().count();
        assert!(size <= 3_389 * 11 / 10, "{size} caractères");
    }

    #[test]
    fn coverage_attribution_and_consultation_date() {
        let view = sandbox_view(Surface::V01);
        assert_eq!(view["attribution"], "Source : Légifrance / DILA — Licence Ouverte 2.0");
        assert_eq!(view["source"], "Légifrance (DILA), through the official PISTE API.");
        // La date est celle de la sortie, au format `AAAA-MM-JJ`.
        assert_eq!(view["consulted_on"], "2026-10-15");
        let schema = Value::Object(output_schema::<CoverageView>().as_ref().clone());
        assert_eq!(schema["properties"]["consulted_on"]["format"], "date");
    }

    /// V4-02 : depuis l'index local DILA, la vue ne dit plus « through the official PISTE API » ni
    /// « queried live » ; la lacune FR-105 est rendue et le statut est dégradé.
    #[test]
    fn a_local_snapshot_is_not_presented_as_a_live_piste_source() {
        let mut out = output(PisteEnvironment::Production, true, false, &[]);
        let facts = ConfigFacts {
            credentials_configured: true,
            custom_host: false,
            has_problems: false,
            local_snapshot: true,
        };
        out.coverage = coverage(PisteEnvironment::Production, &facts, &[Fond::Loda, Fond::Code]);
        out.configuration.local_snapshot = true;
        let view = serde_json::to_value(CoverageView::from(out)).unwrap();
        assert_eq!(
            view["source"],
            "Légifrance (DILA), from a local snapshot of the LEGI dumps; not queried live."
        );
        assert_eq!(view["status"], "degraded");
        assert!(
            view["as_of_note"].as_str().unwrap().starts_with("Data comes from a local snapshot")
        );
        let ids: Vec<&str> = view["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|gap| gap["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids.last(), Some(&"FR-105"));
        let fonds: Vec<&str> = view["families"]
            .as_array()
            .unwrap()
            .iter()
            .map(|family| family["fond"].as_str().unwrap())
            .collect();
        assert_eq!(fonds, ["LODA", "CODE"]);
        // Le schéma de sortie n'a pas changé : le drapeau n'est pas un champ de la vue.
        assert!(view["configuration"].get("local_snapshot").is_none());
    }

    #[test]
    fn the_notes_are_the_documented_texts() {
        let view = sandbox_view(Surface::V01);
        assert_eq!(view["eli_note"], ELI_NOTE);
        assert!(view["eli_note"].as_str().unwrap().contains("never build an /eli/ path"));
        assert!(view["as_of_note"].as_str().unwrap().starts_with("Data is queried live"));
    }

    #[test]
    fn the_status_environment_and_configuration_are_rendered() {
        let view = sandbox_view(Surface::V01);
        assert_eq!(view["status"], "ok");
        assert_eq!(view["environment"], "sandbox");
        assert_eq!(
            view["configuration"],
            json!({"credentials_configured": true, "audit_mode": "strict"})
        );
        let degraded = serde_json::to_value(CoverageView::from(output(
            PisteEnvironment::Production,
            false,
            false,
            &["LF_PISTE_ENV is invalid"],
        )))
        .unwrap();
        assert_eq!(degraded["status"], "degraded");
        assert_eq!(degraded["environment"], "production");
        assert_eq!(
            degraded["configuration"],
            json!({
                "credentials_configured": false,
                "audit_mode": "strict",
                "problems": ["LF_PISTE_ENV is invalid"]
            })
        );
    }

    #[test]
    fn the_best_effort_mode_is_written_with_a_dash() {
        let mut best_effort = output(PisteEnvironment::Production, true, false, &[]);
        best_effort.configuration.audit_mode = AuditMode::BestEffort;
        let view = serde_json::to_value(CoverageView::from(best_effort)).unwrap();
        assert_eq!(view["configuration"]["audit_mode"], "best-effort");
    }

    #[test]
    fn coverage_degraded_without_credentials() {
        let view = serde_json::to_value(CoverageView::from(output(
            PisteEnvironment::Production,
            false,
            false,
            &[],
        )))
        .unwrap();
        assert_eq!(view["status"], "degraded");
        let ids: Vec<&str> = view["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|gap| gap["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"FR-102"), "{ids:?}");
    }

    #[test]
    fn coverage_custom_host_is_a_gap_not_a_problem() {
        let view = serde_json::to_value(CoverageView::from(output(
            PisteEnvironment::Production,
            true,
            true,
            &[],
        )))
        .unwrap();
        assert_eq!(view["status"], "ok");
        assert!(view["configuration"].get("problems").is_none());
        let last = view["gaps"].as_array().unwrap().last().unwrap().clone();
        assert_eq!(last["id"], "FR-103");
    }

    #[test]
    fn coverage_lists_sandbox_gap() {
        let view = sandbox_view(Surface::V01);
        let gaps = view["gaps"].as_array().unwrap();
        assert_eq!(gaps.len(), 10);
        assert_eq!(
            gaps.last().unwrap(),
            &json!({
                "id": "FR-101",
                "missing": "Running on the PISTE sandbox: data may differ from production and availability is not guaranteed.",
                "fallback": "Set LF_PISTE_ENV=production with production credentials."
            })
        );
    }

    #[test]
    fn coverage_lists_pseudonymisation_gap() {
        let view = sandbox_view(Surface::V01);
        let gap =
            view["gaps"].as_array().unwrap().iter().find(|gap| gap["id"] == "FR-006").unwrap();
        assert_eq!(gap["fond"], "JURI");
        assert!(gap["missing"].as_str().unwrap().contains("pseudonymised"));
    }

    #[test]
    fn gaps_never_empty() {
        for environment in [PisteEnvironment::Sandbox, PisteEnvironment::Production] {
            let view =
                serde_json::to_value(CoverageView::from(output(environment, true, false, &[])))
                    .unwrap();
            assert_ne!(view["gaps"].as_array().unwrap().len(), 0);
        }
    }

    #[test]
    fn a_gap_names_its_fond_only_when_it_has_one() {
        let view = sandbox_view(Surface::V01);
        let fonds: Vec<(&str, Option<&str>)> = view["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|gap| (gap["id"].as_str().unwrap(), gap.get("fond").map(|f| f.as_str().unwrap())))
            .collect();
        assert_eq!(
            fonds,
            [
                ("FR-001", None),
                ("FR-002", None),
                ("FR-003", Some("ACCO")),
                ("FR-004", Some("CETAT")),
                ("FR-005", Some("JURI")),
                ("FR-006", Some("JURI")),
                ("FR-007", None),
                ("FR-008", None),
                ("FR-009", None),
                ("FR-101", None),
            ]
        );
    }

    #[test]
    fn every_served_fond_has_a_family() {
        let view = sandbox_view(Surface::V01);
        let families = view["families"].as_array().unwrap();
        let fonds: Vec<&str> = families.iter().map(|f| f["fond"].as_str().unwrap()).collect();
        let wanted: Vec<&str> = Fond::ALL.iter().map(|fond| fond.name()).collect();
        assert_eq!(fonds, wanted);
        for family in families {
            assert_ne!(family["label"].as_str().unwrap(), "");
            assert!(!family["tools"].as_array().unwrap().is_empty(), "{family}");
        }
        assert_eq!(
            families[0],
            json!({
                "fond": "LODA",
                "label": "Laws, ordinances and decrees (consolidated)",
                "tools": ["fr_get_act", "fr_get_text"]
            })
        );
    }

    /// Les outils que chaque surface annonce (05 §3).
    fn registered(surface: Surface) -> BTreeSet<&'static str> {
        let v01 = ToolName::ALL[..8].iter().map(|tool| tool.as_str());
        let v02 = [ToolName::FR_VERIFY_CITATIONS, ToolName::FR_RESOLVE].map(ToolName::as_str);
        let v03 = [ToolName::FR_GET_RELATED, ToolName::FR_DIFF_ARTICLE, ToolName::FR_SEARCH_ALL]
            .map(ToolName::as_str);
        let mut tools: BTreeSet<&str> = v01.into_iter().collect();
        if surface != Surface::V01 {
            tools.extend(v02);
        }
        if surface == Surface::V03 {
            tools.extend(v03);
        }
        tools
    }

    #[test]
    fn every_registered_tool_is_in_a_family_or_general() {
        // Tout outil enregistré figure dans `general_tools` ou dans une famille (D-34), et la
        // vue ne nomme aucun outil qui ne le soit pas, à chacune des trois surfaces.
        for surface in [Surface::V01, Surface::V02, Surface::V03] {
            let view = sandbox_view(surface);
            assert_eq!(named_tools(&view), registered(surface), "{surface:?}");
        }
        // Le binaire annonce `SURFACE`, la surface complète : les treize outils de `ToolName::ALL`.
        let announced = named_tools(
            &serde_json::to_value(CoverageView::from(output(
                PisteEnvironment::Sandbox,
                true,
                false,
                &[],
            )))
            .unwrap(),
        )
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
        for tool in ToolName::ALL {
            assert!(announced.contains(tool.as_str()), "{}", tool.as_str());
        }
    }

    #[test]
    fn the_general_tools_grow_with_the_surface() {
        assert_eq!(
            strings(&sandbox_view(Surface::V01)["general_tools"]),
            ["fr_search", "fr_coverage"]
        );
        assert_eq!(
            strings(&sandbox_view(Surface::V02)["general_tools"]),
            ["fr_search", "fr_coverage", "fr_resolve", "fr_verify_citations"]
        );
        assert_eq!(
            strings(&sandbox_view(Surface::V03)["general_tools"]),
            ["fr_search", "fr_coverage", "fr_resolve", "fr_verify_citations", "fr_search_all"]
        );
    }

    #[test]
    fn related_and_diff_join_their_families_in_v03() {
        let tools_of_family = |view: &Value, fond: &str| -> Vec<String> {
            let family =
                view["families"].as_array().unwrap().iter().find(|f| f["fond"] == fond).unwrap();
            strings(&family["tools"]).into_iter().map(str::to_owned).collect()
        };
        let v02 = sandbox_view(Surface::V02);
        let v03 = sandbox_view(Surface::V03);
        assert_eq!(tools_of_family(&v02, "LODA"), ["fr_get_act", "fr_get_text"]);
        assert_eq!(
            tools_of_family(&v03, "LODA"),
            ["fr_get_act", "fr_get_text", "fr_get_related", "fr_diff_article"]
        );
        assert_eq!(
            tools_of_family(&v03, "CODE"),
            ["fr_get_act", "fr_get_text", "fr_get_related", "fr_diff_article"]
        );
        for fond in ["JURI", "CONSTIT", "CETAT"] {
            assert_eq!(
                tools_of_family(&v03, fond),
                ["fr_get_decision", "fr_get_related"],
                "{fond}"
            );
        }
        for fond in ["CNIL", "KALI", "ACCO"] {
            assert_eq!(tools_of_family(&v03, fond), tools_of_family(&v02, fond), "{fond}");
        }
    }

    #[test]
    fn the_output_schema_inlines_its_enums_and_lists_the_required_fields() {
        let schema = Value::Object(output_schema::<CoverageView>().as_ref().clone());
        assert_eq!(schema["type"], "object");
        assert_eq!(
            schema["required"],
            json!([
                "status",
                "environment",
                "configuration",
                "source",
                "attribution",
                "consulted_on",
                "as_of_note",
                "eli_note",
                "general_tools",
                "families",
                "gaps"
            ])
        );
        assert!(schema.get("$defs").is_none(), "{schema}");
        // `problems` est omis quand il est vide : il n'est pas requis ; `fond` d'une lacune non plus.
        assert_eq!(
            schema["properties"]["configuration"]["required"],
            json!(["credentials_configured", "audit_mode"])
        );
        assert_eq!(
            schema["properties"]["gaps"]["items"]["required"],
            json!(["id", "missing", "fallback"])
        );
        assert_eq!(schema["properties"]["status"]["enum"], json!(["ok", "degraded", "failed"]));
        assert_eq!(schema["properties"]["environment"]["enum"], json!(["sandbox", "production"]));
        assert_eq!(
            schema["properties"]["configuration"]["properties"]["audit_mode"]["enum"],
            json!(["strict", "best-effort"])
        );
        assert!(schema.get("$schema").is_none());
    }
}
