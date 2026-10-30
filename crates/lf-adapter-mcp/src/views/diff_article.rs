//! Vue de sortie de `fr_diff_article` (05 §5.12) : `DiffView`.
//!
//! Construite depuis le [`DiffArticleOutput`] de l'application. Les dates des versions suivent le
//! §1.4 règle 9 (la zone sentinelle vaut « absente ») et l'`etat` est rendu en code d'API.
//! `hunks` est toujours présent, même vide ; `truncated` seulement quand il est vrai. L'application
//! a déjà borné le texte des hunks (8 000 caractères) ; la mesure exacte du JSON compact suit la
//! construction (`views::fit`), car les échappements JSON peuvent encore faire dépasser le plafond
//! dur : la vue retire alors des hunks par la fin. Les commentaires des champs sont des `//` : un
//! doc comment partirait dans l'`outputSchema`, dont la taille compte (05 §3.4).

use lf_application::output::diff_article::{DiffArticleOutput, HunkOut, VersionOut};
use schemars::JsonSchema;
use serde::Serialize;

use super::{
    common::{date_unless_sentinel, etat_code},
    fit::fit_under_cap,
};

/// Ce que `fr_diff_article` rend : les deux versions et ce qui a changé de l'une à l'autre.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct DiffView {
    // Identifiant demandé.
    article_id: String,
    // Citation de l'article, selon la version B.
    citation: String,
    // Version en vigueur à `date_a`.
    version_a: VersionView,
    // Version en vigueur à `date_b`.
    version_b: VersionView,
    // Vrai quand les deux dates tombent dans la même version.
    identical: bool,
    // Changements, mot à mot, dans l'ordre du texte de la version A ; toujours présent.
    hunks: Vec<HunkView>,
    // Présent (et vrai) quand des changements ont été coupés ou retirés pour tenir sous les bornes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    truncated: Option<bool>,
}

// Une version de l'article.
#[derive(Debug, Serialize, JsonSchema)]
struct VersionView {
    article_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    date_debut: Option<String>,
    // Absente : la version n'a pas de date de fin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    date_fin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    etat: Option<String>,
}

// Un changement de la version A vers la version B.
#[derive(Debug, Serialize, JsonSchema)]
struct HunkView {
    // Position du changement dans le texte de la version A, en caractères.
    at: u32,
    // Les 60 caractères au plus qui précèdent le changement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    context: Option<String>,
    // Passage retiré ; absent pour une insertion pure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    removed: Option<String>,
    // Passage ajouté ; absent pour une suppression pure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    added: Option<String>,
}

impl From<DiffArticleOutput> for DiffView {
    /// Vue de la réponse, ramenée sous le plafond dur de 45 000 caractères.
    fn from(output: DiffArticleOutput) -> Self {
        let mut view = Self {
            article_id: output.article_id.as_str().to_owned(),
            citation: output.citation,
            version_a: VersionView::from(output.version_a),
            version_b: VersionView::from(output.version_b),
            identical: output.identical,
            hunks: output.hunks.into_iter().map(HunkView::from).collect(),
            truncated: output.truncated.then_some(true),
        };
        view.fit();
        view
    }
}

impl DiffView {
    /// Mesure la vue et retire des hunks, par la fin, tant qu'elle dépasse le plafond dur
    /// (05 §6.4) ; `truncated` le dit dès le premier retrait.
    fn fit(&mut self) {
        fit_under_cap(self, |view| {
            let removed = view.hunks.pop().is_some();
            view.truncated = view.truncated.or(removed.then_some(true));
            removed
        });
    }
}

impl From<VersionOut> for VersionView {
    fn from(version: VersionOut) -> Self {
        Self {
            article_id: version.article_id.as_str().to_owned(),
            date_debut: date_unless_sentinel(version.date_debut),
            date_fin: date_unless_sentinel(version.date_fin),
            etat: etat_code(version.etat.as_ref()),
        }
    }
}

impl From<HunkOut> for HunkView {
    fn from(hunk: HunkOut) -> Self {
        Self { at: hunk.at, context: hunk.context, removed: hunk.removed, added: hunk.added }
    }
}

#[cfg(test)]
mod tests {
    use lf_application::budget::ResponseBudget;
    use lf_domain::{date::CivilDate, id::LegiArtiId, status::LegalStatus};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::schema::output_schema;

    const CAP: usize = ResponseBudget::HARD_CAP_CHARS;

    fn arti(n: u32) -> LegiArtiId {
        LegiArtiId::parse(&format!("LEGIARTI{n:012}")).unwrap()
    }

    fn version(n: u32, from: Option<CivilDate>, to: Option<CivilDate>) -> VersionOut {
        VersionOut {
            article_id: arti(n),
            date_debut: from,
            date_fin: to,
            etat: Some(LegalStatus::Vigueur),
        }
    }

    fn hunk(at: u32, removed: &str, added: &str) -> HunkOut {
        HunkOut {
            at,
            context: Some("… est puni de".to_owned()),
            removed: Some(removed.to_owned()),
            added: Some(added.to_owned()),
        }
    }

    fn output(hunks: Vec<HunkOut>) -> DiffArticleOutput {
        DiffArticleOutput {
            article_id: arti(2),
            citation: "Code pénal, art. 222-1".to_owned(),
            version_a: version(1, CivilDate::new(2016, 10, 9), CivilDate::new(2023, 9, 7)),
            version_b: version(2, CivilDate::new(2023, 9, 8), None),
            identical: false,
            hunks,
            truncated: false,
        }
    }

    fn json_of(output: DiffArticleOutput) -> Value {
        serde_json::to_value(DiffView::from(output)).unwrap()
    }

    #[test]
    fn the_view_renders_versions_and_hunks_in_the_documented_shape() {
        let view = json_of(output(vec![hunk(120, "un an", "deux ans")]));
        assert_eq!(
            view,
            json!({
                "article_id": "LEGIARTI000000000002",
                "citation": "Code pénal, art. 222-1",
                "version_a": {
                    "article_id": "LEGIARTI000000000001",
                    "date_debut": "2016-10-09",
                    "date_fin": "2023-09-07",
                    "etat": "VIGUEUR"
                },
                "version_b": {
                    "article_id": "LEGIARTI000000000002",
                    "date_debut": "2023-09-08",
                    "etat": "VIGUEUR"
                },
                "identical": false,
                "hunks": [
                    {"at": 120, "context": "… est puni de", "removed": "un an", "added": "deux ans"}
                ]
            })
        );
    }

    #[test]
    fn sentinel_dates_and_missing_fields_are_omitted() {
        let mut out = output(vec![HunkOut {
            at: 0,
            context: None,
            removed: None,
            added: Some("nouveau".to_owned()),
        }]);
        // 2222-02-22 : « en vigueur sans date de fin » (règle 9).
        out.version_b = VersionOut {
            article_id: arti(2),
            date_debut: None,
            date_fin: CivilDate::new(2222, 2, 22),
            etat: None,
        };
        let view = json_of(out);
        assert_eq!(view["version_b"], json!({"article_id": "LEGIARTI000000000002"}));
        assert_eq!(view["hunks"], json!([{"at": 0, "added": "nouveau"}]));
    }

    #[test]
    fn identical_versions_have_an_empty_but_present_hunk_list_and_no_truncated_flag() {
        let mut out = output(vec![]);
        out.identical = true;
        let view = json_of(out);
        assert_eq!(view["identical"], true);
        assert_eq!(view["hunks"], json!([]));
        assert!(view.get("truncated").is_none());
    }

    #[test]
    fn truncated_is_present_only_when_true() {
        let mut out = output(vec![hunk(0, "a", "b")]);
        out.truncated = true;
        assert_eq!(json_of(out)["truncated"], true);
    }

    #[test]
    fn a_response_over_the_hard_cap_loses_its_last_hunks_and_says_so() {
        // Trois hunks de 20 000 caractères : 60 000 caractères de JSON, au-delà du plafond.
        let big = |at: u32| hunk(at, &"r".repeat(10_000), &"a".repeat(10_000));
        let view = DiffView::from(output(vec![big(0), big(1), big(2)]));
        let compact = serde_json::to_string(&view).unwrap();
        assert!(compact.chars().count() <= CAP);
        assert_eq!(view.hunks.len(), 2, "le dernier hunk est parti");
        assert_eq!(view.hunks[1].at, 1);
        assert_eq!(view.truncated, Some(true));
    }

    #[test]
    fn a_single_hunk_that_cannot_fit_is_dropped_rather_than_sent_over_the_cap() {
        let huge = hunk(0, &"r".repeat(30_000), &"a".repeat(30_000));
        let view = DiffView::from(output(vec![huge]));
        assert!(view.hunks.is_empty());
        assert_eq!(view.truncated, Some(true));
    }

    #[test]
    fn the_flag_set_by_the_application_survives_the_fit() {
        let mut out = output(vec![hunk(0, "a", "b")]);
        out.truncated = true;
        let view = DiffView::from(out);
        assert_eq!(view.truncated, Some(true));
        assert_eq!(view.hunks.len(), 1);
    }

    #[test]
    fn output_schema_snapshot() {
        let schema = Value::Object(output_schema::<DiffView>().as_ref().clone());
        assert_eq!(schema["type"], "object");
        assert!(schema.get("$schema").is_none());
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("output_schema", schema);
        });
    }
}
