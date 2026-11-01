//! Vue de la table des matières : entrées, `toc_total`, `next_toc_offset` (05 §5.0.4).
//!
//! Partagée par `fr_get_act` et `fr_get_convention`, qui l'aplatissent (`#[serde(flatten)]`) dans
//! leur vue. Une section rend `{depth, title}` ; un article `{depth, article_id, num?, etat?}`,
//! l'`etat` étant omis quand il égale celui du texte. La liste est toujours présente (« vide » se
//! distingue d'« absent »). Les commentaires des champs sont des `//` : un doc comment partirait
//! dans le schéma de sortie.

use lf_application::output::get_act::TocLine;
use lf_domain::status::LegalStatus;
use schemars::JsonSchema;
use serde::Serialize;

use super::common::etat_code;

// Une entrée de la table des matières rendue. Commentaire `//` : un doc comment partirait, en
// français, dans la description de l'élément de `toc` de l'`outputSchema`.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(inline)]
pub(crate) struct TocEntryView {
    // Profondeur, 1 au premier niveau.
    depth: u16,
    // Titre d'une section.
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    // Identifiant d'un article, à lire avec `fr_get_text`.
    #[serde(skip_serializing_if = "Option::is_none")]
    article_id: Option<String>,
    // Numéro d'un article.
    #[serde(skip_serializing_if = "Option::is_none")]
    num: Option<String>,
    // État d'un article, omis quand il égale celui du texte.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Omitted when equal to the etat of the text.")]
    etat: Option<String>,
}

/// La table des matières d'une réponse : la page, le total et la suite.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct TocView {
    // La page, en préordre ; toujours présente.
    toc: Vec<TocEntryView>,
    // Entrées de la table entière.
    toc_total: u32,
    // Début de la page suivante, absent en fin de table.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Pass as toc_offset to read the next page; absent on the last page.")]
    next_toc_offset: Option<u32>,
}

impl TocView {
    /// Vue de la page `lines` d'une table de `total` entrées, dont la suite commence à `next`.
    /// `text_etat` est l'état du texte, que les articles de même état n'ont pas à répéter.
    pub(crate) fn new(
        lines: &[TocLine],
        total: u32,
        next: Option<u32>,
        text_etat: Option<&LegalStatus>,
    ) -> Self {
        let toc = lines
            .iter()
            .map(|line| match line {
                TocLine::Section { depth, title } => TocEntryView {
                    depth: *depth,
                    title: Some(title.clone()),
                    article_id: None,
                    num: None,
                    etat: None,
                },
                TocLine::Article { depth, article_id, num, etat } => TocEntryView {
                    depth: *depth,
                    title: None,
                    article_id: Some(article_id.as_str().to_owned()),
                    num: num.clone(),
                    etat: etat_code(etat.as_ref().filter(|etat| Some(*etat) != text_etat)),
                },
            })
            .collect();
        Self { toc, toc_total: total, next_toc_offset: next }
    }

    /// Retire la dernière entrée et recule `next_toc_offset` d'autant : la page est plus courte,
    /// la suite commence là où elle s'arrête. Rend `false` quand il ne reste qu'une entrée (la
    /// lecture doit avancer) ou aucune.
    pub(crate) fn drop_last(&mut self) -> bool {
        if self.toc.len() <= 1 {
            return false;
        }
        // La page se termine à `next_toc_offset`, ou à la fin de la table quand il est absent.
        let end = self.next_toc_offset.unwrap_or(self.toc_total);
        self.toc.pop();
        self.next_toc_offset = Some(end.saturating_sub(1));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lf_domain::id::ArticleId;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn section(depth: u16, title: &str) -> TocLine {
        TocLine::Section { depth, title: title.to_owned() }
    }

    fn article(depth: u16, id: &str, num: Option<&str>, etat: Option<&str>) -> TocLine {
        TocLine::Article {
            depth,
            article_id: ArticleId::parse(id).unwrap(),
            num: num.map(str::to_owned),
            etat: etat.and_then(LegalStatus::parse),
        }
    }

    fn rendered(view: &TocView) -> Value {
        serde_json::to_value(view).unwrap()
    }

    #[test]
    fn a_section_renders_depth_and_title_and_an_article_depth_id_and_num() {
        let lines = [
            section(1, "Chapitre 1er : Recours au CDD d'usage"),
            article(2, "KALIARTI000049886059", Some("19"), None),
        ];

        let view = TocView::new(&lines, 2, None, None);

        assert_eq!(
            rendered(&view),
            json!({
                "toc": [
                    {"depth": 1, "title": "Chapitre 1er : Recours au CDD d'usage"},
                    {"depth": 2, "article_id": "KALIARTI000049886059", "num": "19"},
                ],
                "toc_total": 2,
            })
        );
    }

    #[test]
    fn toc_etat_omitted_when_equal_to_text() {
        let lines = [
            article(1, "LEGIARTI000033205126", Some("1"), Some("VIGUEUR")),
            article(1, "LEGIARTI000033205128", Some("2"), Some("ABROGE")),
            article(1, "LEGIARTI000033205130", Some("3"), None),
        ];
        let text_etat = LegalStatus::parse("VIGUEUR");

        let view = TocView::new(&lines, 3, None, text_etat.as_ref());

        // Égal à celui du texte : omis ; différent : rendu en code d'API ; absent : absent.
        assert_eq!(
            rendered(&view)["toc"],
            json!([
                {"depth": 1, "article_id": "LEGIARTI000033205126", "num": "1"},
                {"depth": 1, "article_id": "LEGIARTI000033205128", "num": "2", "etat": "ABROGE"},
                {"depth": 1, "article_id": "LEGIARTI000033205130", "num": "3"},
            ])
        );
    }

    #[test]
    fn toc_etat_is_omitted_under_a_repealed_text_given_as_an_accented_label() {
        // get_act-4 : le texte porte `jurisState` « Abrogé », les articles le code ABROGE.
        let lines = [
            article(1, "LEGIARTI000033205126", Some("1"), Some("ABROGE")),
            article(1, "LEGIARTI000033205128", Some("2"), Some("VIGUEUR")),
        ];
        let text_etat = lf_domain::status::status_of(None, Some("Abrogé"));

        let view = TocView::new(&lines, 2, None, text_etat.as_ref());

        // Jumeau : l'article de même état est omis, celui d'un autre état reste rendu.
        assert_eq!(
            rendered(&view)["toc"],
            json!([
                {"depth": 1, "article_id": "LEGIARTI000033205126", "num": "1"},
                {"depth": 1, "article_id": "LEGIARTI000033205128", "num": "2", "etat": "VIGUEUR"},
            ])
        );
    }

    #[test]
    fn toc_etat_is_kept_when_the_text_has_none() {
        let lines = [article(1, "LEGIARTI000033205126", None, Some("VIGUEUR"))];

        let view = TocView::new(&lines, 1, None, None);

        assert_eq!(
            rendered(&view)["toc"],
            json!([{"depth": 1, "article_id": "LEGIARTI000033205126", "etat": "VIGUEUR"}])
        );
    }

    #[test]
    fn the_list_is_always_present_even_when_empty_and_next_offset_only_when_set() {
        let empty = TocView::new(&[], 0, None, None);
        assert_eq!(rendered(&empty), json!({"toc": [], "toc_total": 0}));

        let paged = TocView::new(&[section(1, "Titre")], 88, Some(8), None);
        assert_eq!(rendered(&paged)["next_toc_offset"], json!(8));
    }

    #[test]
    fn dropping_the_last_entry_moves_next_toc_offset_back() {
        // Page [10, 13) d'une table de 20 entrées : la suite est à 13.
        let lines = [section(1, "A"), section(1, "B"), section(1, "C")];
        let mut view = TocView::new(&lines, 20, Some(13), None);

        assert!(view.drop_last());

        assert_eq!(rendered(&view)["toc"].as_array().unwrap().len(), 2);
        assert_eq!(rendered(&view)["next_toc_offset"], json!(12));
    }

    #[test]
    fn dropping_from_the_last_page_creates_a_next_offset() {
        // Dernière page [18, 20) : sans suite. Retirer une entrée en crée une, à 19.
        let lines = [section(1, "A"), section(1, "B")];
        let mut view = TocView::new(&lines, 20, None, None);

        assert!(view.drop_last());

        assert_eq!(rendered(&view)["next_toc_offset"], json!(19));
    }

    #[test]
    fn the_last_remaining_entry_is_never_dropped() {
        // Sans cela `next_toc_offset` ne ferait plus avancer la lecture.
        let mut view = TocView::new(&[section(1, "A")], 20, Some(1), None);
        assert!(!view.drop_last());
        assert_eq!(rendered(&view)["toc"].as_array().unwrap().len(), 1);
        assert_eq!(rendered(&view)["next_toc_offset"], json!(1));

        let mut empty = TocView::new(&[], 0, None, None);
        assert!(!empty.drop_last());
    }
}
