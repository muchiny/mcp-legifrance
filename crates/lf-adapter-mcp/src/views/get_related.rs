//! Vue de sortie de `fr_get_related` (05 §5.11) : `RelatedView`.
//!
//! Construite depuis le [`GetRelatedOutput`] de l'application. `links` est toujours présent, même
//! vide ; `truncated` seulement quand il est vrai ; `note` seulement quand l'API ne donne aucune
//! référence pour la décision (une liste vide ne dit alors pas qu'elle ne cite rien) ; `id` et
//! `source_url` d'un lien seulement quand la cible est lisible. L'application a déjà borné la liste (cent liens, estimation sous la
//! cible) ; la mesure exacte du JSON compact suit la construction (`views::fit`), car les
//! échappements JSON peuvent encore faire dépasser le plafond dur : la vue retire alors des liens
//! par la fin et `truncated` le dit. `total` garde le compte d'avant toute troncature. Les
//! commentaires des champs sont des `//` : un doc comment partirait dans l'`outputSchema`, dont la
//! taille compte (05 §3.4).

use lf_application::{
    citation::Note,
    output::get_related::{GetRelatedOutput, LinkOut},
};
use schemars::JsonSchema;
use serde::Serialize;

use super::fit::fit_under_cap;

/// Ce que `fr_get_related` rend : la page demandée et ses liens.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct RelatedView {
    // Identifiant demandé.
    id: String,
    // Citation de l'article ou de la décision demandé.
    citation: String,
    // Page Légifrance de l'identifiant demandé.
    source_url: String,
    // Nombre de liens avant troncature.
    total: u32,
    // Les liens, cent au plus ; toujours présent.
    links: Vec<LinkView>,
    // Présent (et vrai) quand des liens ont été retirés : au-delà de cent, ou pour tenir sous le
    // plafond de réponse. Il n'y a pas de page suivante.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    truncated: Option<bool>,
    // Présente quand l'API ne donne aucune référence pour cette décision : la liste vide ne prouve
    // pas qu'elle ne cite rien.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    note: Option<String>,
}

// Un lien vers ou depuis la page demandée.
#[derive(Debug, Serialize, JsonSchema)]
struct LinkView {
    // `cites` et `cited_by` pour un article ; `cites_case_law` et `applies_text` pour une décision.
    relation: &'static str,
    // Famille de la cible, dans le vocabulaire de `fr_verify_citations` (`code_article`, `text`,
    // `decision`, `unsupported`...).
    kind: &'static str,
    // Forme canonique, ou titre de l'API nettoyé.
    citation: String,
    // Identifiant de la cible, quand un outil sait la lire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    id: Option<String>,
    // Page Légifrance de la cible, avec `id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    source_url: Option<String>,
}

impl From<GetRelatedOutput> for RelatedView {
    /// Vue de la réponse, ramenée sous le plafond dur de 45 000 caractères.
    fn from(output: GetRelatedOutput) -> Self {
        let mut view = Self {
            id: output.id,
            citation: output.citation,
            source_url: output.source_url,
            total: output.total,
            links: output.links.into_iter().map(LinkView::from).collect(),
            truncated: output.truncated.then_some(true),
            note: output.note.as_ref().map(Note::text),
        };
        view.fit();
        view
    }
}

impl RelatedView {
    /// Mesure la vue et retire des liens, par la fin, tant qu'elle dépasse le plafond dur
    /// (05 §6.4) ; `truncated` le dit dès le premier retrait.
    fn fit(&mut self) {
        fit_under_cap(self, |view| {
            let removed = view.links.pop().is_some();
            view.truncated = view.truncated.or(removed.then_some(true));
            removed
        });
    }
}

impl From<LinkOut> for LinkView {
    fn from(link: LinkOut) -> Self {
        Self {
            relation: link.relation.as_str(),
            kind: link.kind.snake_case(),
            citation: link.citation,
            id: link.id,
            source_url: link.source_url,
        }
    }
}

#[cfg(test)]
mod tests {
    use lf_application::{budget::ResponseBudget, output::get_related::Relation};
    use lf_domain::citation::CitationKind;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::schema::output_schema;

    const CAP: usize = ResponseBudget::HARD_CAP_CHARS;

    fn link(relation: Relation, kind: CitationKind, citation: &str) -> LinkOut {
        LinkOut { relation, kind, citation: citation.to_owned(), id: None, source_url: None }
    }

    fn output(links: Vec<LinkOut>) -> GetRelatedOutput {
        GetRelatedOutput {
            id: "LEGIARTI000006419288".to_owned(),
            citation: "Code civil, art. 9".to_owned(),
            source_url: "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288"
                .to_owned(),
            total: u32::try_from(links.len()).unwrap(),
            links,
            truncated: false,
            note: None,
        }
    }

    fn json_of(output: GetRelatedOutput) -> Value {
        serde_json::to_value(RelatedView::from(output)).unwrap()
    }

    #[test]
    fn the_view_renders_the_documented_shape() {
        let mut identified =
            link(Relation::Cites, CitationKind::CodeArticle, "Code civil, art. 1240");
        identified.id = Some("LEGIARTI000032041571".to_owned());
        identified.source_url =
            Some("https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000032041571".to_owned());
        let view = json_of(output(vec![
            identified,
            link(Relation::CitedBy, CitationKind::Unsupported, "Circulaire du 14 septembre 2011"),
        ]));
        assert_eq!(
            view,
            json!({
                "id": "LEGIARTI000006419288",
                "citation": "Code civil, art. 9",
                "source_url": "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288",
                "total": 2,
                "links": [
                    {
                        "relation": "cites",
                        "kind": "code_article",
                        "citation": "Code civil, art. 1240",
                        "id": "LEGIARTI000032041571",
                        "source_url": "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000032041571"
                    },
                    {
                        "relation": "cited_by",
                        "kind": "unsupported",
                        "citation": "Circulaire du 14 septembre 2011"
                    }
                ]
            })
        );
    }

    #[test]
    fn a_decision_link_has_no_id_and_the_four_relations_are_spelled_out() {
        let view = json_of(output(vec![
            link(
                Relation::CitesCaseLaw,
                CitationKind::Decision,
                "Cass. 1re civ., 25 mai 2023, n° 21-23.174",
            ),
            link(Relation::AppliesText, CitationKind::Ecli, "ECLI:FR:CCASS:2025:C100389"),
        ]));
        assert_eq!(view["links"][0]["relation"], "cites_case_law");
        assert_eq!(view["links"][0]["kind"], "decision");
        assert_eq!(view["links"][1]["relation"], "applies_text");
        assert_eq!(view["links"][1]["kind"], "ecli");
        assert!(
            view["links"][0].get("id").is_none() && view["links"][0].get("source_url").is_none()
        );
    }

    #[test]
    fn links_is_present_even_when_empty_and_truncated_only_when_true() {
        let view = json_of(output(vec![]));
        assert_eq!(view["links"], json!([]));
        assert_eq!(view["total"], 0);
        assert!(view.get("truncated").is_none());
        let mut out = output(vec![link(Relation::Cites, CitationKind::Text, "Loi n° 1")]);
        out.truncated = true;
        out.total = 250;
        let view = json_of(out);
        assert_eq!(view["truncated"], true);
        assert_eq!(view["total"], 250, "le total d'avant la troncature");
    }

    #[test]
    fn the_no_references_note_is_rendered_only_when_the_application_sets_it() {
        let mut out = output(vec![]);
        out.note = Some(Note::NoDecisionReferences);
        let view = json_of(out);
        assert_eq!(view["links"], json!([]));
        assert_eq!(
            view["note"],
            "The API gives no references for this decision; its text may still cite some: read it \
             with fr_get_decision and check it with fr_verify_citations."
        );
        assert!(json_of(output(vec![])).get("note").is_none());
    }

    #[test]
    fn a_response_over_the_hard_cap_loses_its_last_links_and_says_so() {
        // Trois liens de 20 000 caractères : 60 000 caractères de JSON, au-delà du plafond.
        let big = |n: u32| {
            link(Relation::Cites, CitationKind::Text, &format!("{n}{}", "x".repeat(20_000)))
        };
        let mut out = output(vec![big(1), big(2), big(3)]);
        out.total = 3;
        let view = RelatedView::from(out);
        let compact = serde_json::to_string(&view).unwrap();
        assert!(compact.chars().count() <= CAP);
        assert_eq!(view.links.len(), 2, "le dernier lien est parti");
        assert!(view.links[1].citation.starts_with('2'));
        assert_eq!(view.truncated, Some(true));
        assert_eq!(view.total, 3, "le total ne change pas");
    }

    #[test]
    fn json_escapes_can_push_a_response_over_the_cap_and_the_exact_measure_catches_it() {
        // 100 liens de 300 guillemets : 300 caractères estimés, 600 une fois échappés ; l'application
        // les aurait gardés (estimation), la mesure exacte en retire.
        let quoted =
            |n: u32| link(Relation::Cites, CitationKind::Text, &format!("{n}{}", "\"".repeat(300)));
        let out = output((1..=100).map(quoted).collect());
        let view = RelatedView::from(out);
        let compact = serde_json::to_string(&view).unwrap();
        let size = compact.chars().count();
        assert!(size <= CAP, "{size}");
        assert!(view.links.len() < 100);
        assert_eq!(view.truncated, Some(true));
    }

    #[test]
    fn a_single_link_that_cannot_fit_is_dropped_rather_than_sent_over_the_cap() {
        let huge = link(Relation::Cites, CitationKind::Text, &"x".repeat(50_000));
        let view = RelatedView::from(output(vec![huge]));
        assert!(view.links.is_empty());
        assert_eq!(view.truncated, Some(true));
    }

    #[test]
    fn the_flag_set_by_the_application_survives_the_fit() {
        let mut out = output(vec![link(Relation::Cites, CitationKind::Text, "Loi n° 1")]);
        out.truncated = true;
        let view = RelatedView::from(out);
        assert_eq!(view.truncated, Some(true));
        assert_eq!(view.links.len(), 1);
    }

    #[test]
    fn output_schema_snapshot() {
        let schema = Value::Object(output_schema::<RelatedView>().as_ref().clone());
        assert_eq!(schema["type"], "object");
        assert!(schema.get("$schema").is_none());
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("output_schema", schema);
        });
    }
}
