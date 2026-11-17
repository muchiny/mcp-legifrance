//! Vue de sortie de `fr_get_bofip` (05 §5.15) : `BofipView`.
//!
//! Construite depuis le [`GetBofipOutput`] de l'application, qui a deux formes :
//!
//! - **une publication lue** : ses métadonnées (numéro, titre, nature, série, division, début de
//!   validité), sa citation et son adresse, puis les champs de lecture communs
//!   ([`ReadingView`], aplatis) ;
//! - **une liste** : les publications qui portent le même identifiant, sans aucun texte, quand
//!   l'appel ne dit pas laquelle lire.
//!
//! Une publication lue porte aussi les articles de code que cite son texte entier
//! (`cited_articles`, ADR-0032).
//!
//! Les deux formes partagent un seul schéma de sortie : `id` et les deux constantes du BOFiP
//! ([`BOFIP_ATTRIBUTION`], [`READ_DOCTRINE_NOTE`]) sont toujours là, tout le reste est facultatif. Les
//! champs absents sont omis (jamais `null`). Les commentaires des champs sont des `//` : un
//! commentaire de documentation partirait dans l'`outputSchema`, dont la taille compte (05 §3.4).

use lf_application::output::get_bofip::{
    BofipChoice, BofipPublication, GetBofipOutput, PublicationRef,
};
use schemars::JsonSchema;
use serde::Serialize;

use super::{
    fit::fit_under_cap,
    reading::ReadingView,
    search_bofip::{BOFIP_ATTRIBUTION, BofipKindView, doctrine_note},
};

/// `doctrine_note` de la lecture : celle de la recherche ([`DOCTRINE_NOTE`](super::search_bofip::DOCTRINE_NOTE)),
/// suivie d'une phrase fixe sur les articles cités (ADR-0032, décision 5). La lecture ne juge pas
/// si une publication est périmée : la sonde du 2026-11-16 a trouvé sept changements sans effet
/// sur dix ; elle dit comment le vérifier.
pub(crate) const READ_DOCTRINE_NOTE: &str = concat!(
    doctrine_note!(),
    " A publication may be older than the law it explains: before relying on it, check the code \
     articles it cites (cited_articles, the same on every page) once with fr_verify_citations, \
     which gives the start date of each version in force; for an article whose version started \
     after valid_from, fr_diff_article from valid_from shows what changed."
);

// Les noms de champs et leur ordre sont ceux de 05 §5.15.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct BofipView {
    // `BOI-…` ou `ACTU-…`, tel que validé.
    id: String,
    // Forme « publication lue ».
    #[serde(flatten)]
    publication: Option<PublicationView>,
    // Forme « liste » : l'identifiant désigne plusieurs publications.
    #[serde(flatten)]
    choice: Option<ChoiceView>,
    attribution: String,
    #[schemars(description = "What BOFiP is, and the limit of this source.")]
    doctrine_note: String,
}

// Une publication et la lecture de son texte.
#[derive(Debug, Serialize, JsonSchema)]
struct PublicationView {
    #[schemars(description = "Number of the publication on the BOFiP site.")]
    publication_id: u32,
    title: String,
    kind: BofipKindView,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    serie: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    division: Option<String>,
    // Début de validité, `AAAA-MM-JJ`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    valid_from: Option<String>,
    citation: String,
    // Omis quand le permalien du jeu ne suit pas la grammaire stricte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    source_url: Option<String>,
    #[serde(flatten)]
    reading: ReadingView,
    // Articles de code cités par le texte entier, quelle que soit la fenêtre (ADR-0032).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(
        description = "Code articles cited by the whole text, at most 50: check them with fr_verify_citations."
    )]
    cited_articles: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(description = "true when cited_articles is incomplete.")]
    cited_articles_truncated: Option<bool>,
}

// Les publications d'un même identifiant, et comment en lire une.
#[derive(Debug, Serialize, JsonSchema)]
struct ChoiceView {
    #[schemars(
        description = "Present when the id designates several publications: call again with one publication_id."
    )]
    publications: Vec<PublicationRefView>,
    note: String,
}

// Une publication de la liste : de quoi la choisir, jamais son texte.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(inline)]
struct PublicationRefView {
    publication_id: u32,
    title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    valid_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    source_url: Option<String>,
}

impl From<BofipPublication> for PublicationView {
    fn from(publication: BofipPublication) -> Self {
        let BofipPublication { doc, reading, cited } = publication;
        Self {
            citation: doc.citation(),
            publication_id: doc.publication_id.get(),
            title: doc.title,
            kind: doc.kind.into(),
            serie: doc.serie.map(|serie| serie.as_str().to_owned()),
            division: doc.division,
            valid_from: doc.valid_from.map(|date| date.to_string()),
            source_url: doc.source_url.map(|url| url.as_str().to_owned()),
            reading: reading.into(),
            cited_articles: cited.articles,
            cited_articles_truncated: cited.truncated.then_some(true),
        }
    }
}

impl From<PublicationRef> for PublicationRefView {
    fn from(publication: PublicationRef) -> Self {
        Self {
            publication_id: publication.publication_id.get(),
            title: publication.title,
            valid_from: publication.valid_from.map(|date| date.to_string()),
            source_url: publication.source_url.map(|url| url.as_str().to_owned()),
        }
    }
}

impl From<BofipChoice> for ChoiceView {
    fn from(choice: BofipChoice) -> Self {
        Self {
            publications: choice.publications.into_iter().map(PublicationRefView::from).collect(),
            note: choice.note,
        }
    }
}

impl From<GetBofipOutput> for BofipView {
    fn from(output: GetBofipOutput) -> Self {
        let (id, publication, choice) = match output {
            GetBofipOutput::Publication(publication) => {
                (publication.doc.id.as_str().to_owned(), Some((*publication).into()), None)
            }
            GetBofipOutput::Choice(choice) => {
                (choice.id.as_str().to_owned(), None, Some(choice.into()))
            }
        };
        let mut view = Self {
            id,
            publication,
            choice,
            attribution: BOFIP_ATTRIBUTION.to_owned(),
            doctrine_note: READ_DOCTRINE_NOTE.to_owned(),
        };
        // L'application ne voit pas les échappements JSON : la mesure exacte tranche, et la
        // fenêtre raccourcie garde un `next_offset` exact. Une liste n'a rien à retirer.
        fit_under_cap(&mut view, Self::degrade);
        view
    }
}

impl BofipView {
    /// Retire un cran de texte à la publication lue ; `false` s'il n'y a plus rien à retirer, ou
    /// pour une liste.
    fn degrade(&mut self) -> bool {
        self.publication.as_mut().is_some_and(|publication| publication.reading.degrade())
    }
}

#[cfg(test)]
mod tests {
    use lf_application::budget::{PassageOut, Reading, ResponseBudget};
    use lf_domain::{
        date::CivilDate,
        doctrine::{
            BoiId, CITED_ARTICLES_MAX, CitedArticles, DoctrineDoc, DoctrineKind, Permalink, Serie,
        },
        text::{LongText, Summary, SummarySource},
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::{schema::output_schema, views::search_bofip::DOCTRINE_NOTE};

    const ID: &str = "BOI-ENR-DMTG-10-50-20";
    const URL: &str = "https://bofip.impots.gouv.fr/bofip/3369-PGP.html/identifiant=BOI-ENR-DMTG-10-50-20-20230524";

    /// La publication de l'exemple de 05 §5.15, texte vidé (la lecture est à part).
    fn doc() -> DoctrineDoc {
        let link = Permalink::parse(URL).unwrap();
        DoctrineDoc {
            id: BoiId::parse(ID).unwrap(),
            publication_id: link.publication_id,
            title: "ENR - Mutations à titre gratuit - Donations - Réductions de droits".to_owned(),
            kind: DoctrineKind::Doctrine,
            serie: Some(Serie::parse("ENR").unwrap()),
            division: Some("DMTG".to_owned()),
            valid_from: CivilDate::new(2023, 5, 24),
            source_url: link.url,
            text: LongText::default(),
        }
    }

    fn reading(text_length: u32) -> Reading {
        Reading {
            summary: None,
            text: None,
            text_length,
            next_offset: None,
            passages: None,
            note: None,
            summary_truncated: false,
        }
    }

    fn published(doc: DoctrineDoc, reading: Reading) -> GetBofipOutput {
        GetBofipOutput::Publication(Box::new(BofipPublication {
            doc,
            reading,
            cited: CitedArticles::default(),
        }))
    }

    fn json_of(output: GetBofipOutput) -> Value {
        serde_json::to_value(BofipView::from(output)).unwrap()
    }

    fn reference(number: u32, dated: bool) -> PublicationRef {
        let raw = format!("https://bofip.impots.gouv.fr/bofip/{number}-PGP.html");
        let link = Permalink::parse(&raw).unwrap();
        PublicationRef {
            publication_id: link.publication_id,
            title: format!("TH - Dégrèvements ({number})"),
            valid_from: dated.then(|| CivilDate::new(2020, 12, 22)).flatten(),
            source_url: dated.then_some(link.url).flatten(),
        }
    }

    fn choice() -> GetBofipOutput {
        GetBofipOutput::Choice(BofipChoice {
            id: BoiId::parse("BOI-IF-TH-10-50-40").unwrap(),
            publications: vec![reference(648, true), reference(11040, false)],
            note: "2 publications share this id; pass publication_id to read one.".to_owned(),
        })
    }

    #[test]
    fn a_summary_carries_the_metadata_the_citation_and_the_two_constants() {
        let summary = Summary {
            source: SummarySource::Lead,
            text: "Les réductions de droits prévues à l'article 790 du CGI".to_owned(),
            truncated: true,
        };
        let read = Reading { summary: Some(summary), summary_truncated: true, ..reading(28_323) };
        assert_eq!(
            json_of(published(doc(), read)),
            json!({
                "id": ID,
                "publication_id": 3369,
                "title": "ENR - Mutations à titre gratuit - Donations - Réductions de droits",
                "kind": "doctrine",
                "serie": "ENR",
                "division": "DMTG",
                "valid_from": "2023-05-24",
                "citation": "BOFiP, BOI-ENR-DMTG-10-50-20, 24 mai 2023",
                "source_url": URL,
                "summary": "Les réductions de droits prévues à l'article 790 du CGI",
                "summary_source": "lead",
                "summary_truncated": true,
                "text_length": 28_323,
                "attribution": "Source : BOFiP-Impôts / DGFiP — Licence Ouverte 2.0",
                "doctrine_note": READ_DOCTRINE_NOTE,
            })
        );
    }

    #[test]
    fn a_window_and_passages_are_rendered_like_the_other_long_readings() {
        let window = Reading {
            text: Some("Premier paragraphe.".to_owned()),
            next_offset: Some(19),
            ..reading(28_323)
        };
        let value = json_of(published(doc(), window));
        assert_eq!(value["text"], "Premier paragraphe.");
        assert_eq!(value["next_offset"], 19);
        assert!(value.get("summary").is_none() && value.get("passages").is_none());

        let found = Reading {
            passages: Some(vec![PassageOut {
                offset: 120,
                text: "réduction de 50 %".to_owned(),
                score: 1.5,
            }]),
            ..reading(28_323)
        };
        assert_eq!(
            json_of(published(doc(), found))["passages"],
            json!([{"offset": 120, "text": "réduction de 50 %", "score": 1.5}])
        );
    }

    /// Une publication sans série, division, date ni adresse sûre : les quatre champs sont omis,
    /// la citation se passe de date. Sans texte : `text_length` à 0 et la remarque de l'application.
    #[test]
    fn absent_metadata_is_omitted_and_a_publication_without_text_keeps_its_note() {
        let bare = DoctrineDoc {
            kind: DoctrineKind::News,
            serie: None,
            division: None,
            valid_from: None,
            source_url: None,
            ..doc()
        };
        let read = Reading { note: Some("This publication has no text.".to_owned()), ..reading(0) };
        assert_eq!(
            json_of(published(bare, read)),
            json!({
                "id": ID,
                "publication_id": 3369,
                "title": "ENR - Mutations à titre gratuit - Donations - Réductions de droits",
                "kind": "news",
                "citation": "BOFiP, BOI-ENR-DMTG-10-50-20",
                "text_length": 0,
                "note": "This publication has no text.",
                "attribution": BOFIP_ATTRIBUTION,
                "doctrine_note": READ_DOCTRINE_NOTE,
            })
        );
    }

    #[test]
    fn a_choice_lists_the_publications_without_any_text() {
        assert_eq!(
            json_of(choice()),
            json!({
                "id": "BOI-IF-TH-10-50-40",
                "publications": [
                    {
                        "publication_id": 648,
                        "title": "TH - Dégrèvements (648)",
                        "valid_from": "2020-12-22",
                        "source_url": "https://bofip.impots.gouv.fr/bofip/648-PGP.html"
                    },
                    {"publication_id": 11040, "title": "TH - Dégrèvements (11040)"}
                ],
                "note": "2 publications share this id; pass publication_id to read one.",
                "attribution": BOFIP_ATTRIBUTION,
                "doctrine_note": READ_DOCTRINE_NOTE,
            })
        );
    }

    fn cited(articles: &[&str], truncated: bool) -> CitedArticles {
        CitedArticles { articles: articles.iter().map(|a| (*a).to_owned()).collect(), truncated }
    }

    fn with_cited(reading: Reading, cited: CitedArticles) -> GetBofipOutput {
        GetBofipOutput::Publication(Box::new(BofipPublication { doc: doc(), reading, cited }))
    }

    /// Les articles cités suivent la lecture ; le drapeau n'est rendu que s'il vaut `true`.
    #[test]
    fn the_cited_articles_follow_the_reading_and_the_flag_only_when_true() {
        let articles = ["Code général des impôts, art. 790", "Code civil, art. 9"];
        let value = json_of(with_cited(reading(28_323), cited(&articles, false)));
        assert_eq!(value["cited_articles"], json!(articles));
        assert!(value.get("cited_articles_truncated").is_none());
        // L'ordre des champs est celui de la sortie sérialisée (une `Value` les trierait).
        let view = BofipView::from(with_cited(reading(28_323), cited(&articles, false)));
        let raw = serde_json::to_string(&view).unwrap();
        let at = |name: &str| raw.find(&format!("\"{name}\":")).unwrap();
        assert!(
            at("text_length") < at("cited_articles") && at("cited_articles") < at("attribution")
        );

        let value = json_of(with_cited(reading(28_323), cited(&articles, true)));
        assert_eq!(value["cited_articles_truncated"], true);

        let value = json_of(with_cited(reading(28_323), CitedArticles::default()));
        assert!(
            value.get("cited_articles").is_none()
                && value.get("cited_articles_truncated").is_none()
        );
    }

    /// La note de la lecture reprend celle de la recherche mot pour mot, puis dit comment
    /// vérifier les articles cités, sans juger la publication.
    #[test]
    fn the_reading_note_extends_the_search_note_with_how_to_check_the_articles() {
        let added = READ_DOCTRINE_NOTE.strip_prefix(DOCTRINE_NOTE).unwrap();
        assert_eq!(
            added,
            " A publication may be older than the law it explains: before relying on it, check \
             the code articles it cites (cited_articles, the same on every page) once with \
             fr_verify_citations, which gives the start date of each version in force; for an \
             article whose version started after valid_from, fr_diff_article from valid_from \
             shows what changed."
        );
        assert_eq!(json_of(choice())["doctrine_note"], READ_DOCTRINE_NOTE);
    }

    /// Cinquante entrées de la pire longueur (le plus long titre du dictionnaire, le plus long
    /// numéro lisible), avec une fenêtre qui tiendrait seule sous le plafond dur : la liste la
    /// fait dépasser, la fenêtre raccourcit et la liste reste entière.
    #[test]
    fn fifty_long_citations_keep_the_response_under_the_hard_cap() {
        let longest = lf_domain::citation::CODES
            .iter()
            .map(|entry| entry.title)
            .max_by_key(|title| title.chars().count())
            .unwrap();
        // Le numéro le plus long que la fenêtre du parseur puisse lire (ancre + 432 caractères) :
        // environ 500 caractères par entrée. Dans le jeu, la plus longue en compte 96.
        let articles: Vec<String> = (0..CITED_ARTICLES_MAX)
            .map(|i| format!("{longest}, art. {i}{}", "-1".repeat(216)))
            .collect();
        assert!(articles.iter().all(|a| a.chars().count() > 490));
        let size = |view: &BofipView| serde_json::to_string(view).unwrap().chars().count();
        let kept = |view: &BofipView| {
            view.publication.as_ref().unwrap().reading.text.as_ref().unwrap().chars().count()
        };
        let with_window = |text: &str, cited: CitedArticles| {
            let length = u32::try_from(text.chars().count()).unwrap();
            BofipView::from(with_cited(
                Reading { text: Some(text.to_owned()), ..reading(length) },
                cited,
            ))
        };
        // Une fenêtre qui laisse 500 caractères sous le plafond quand elle est seule.
        let room = ResponseBudget::HARD_CAP_CHARS.saturating_sub(500);
        let text =
            "a".repeat(room.saturating_sub(size(&with_window("", CitedArticles::default()))));
        let alone = with_window(&text, CitedArticles::default());
        assert_eq!(kept(&alone), text.chars().count(), "seule, la fenêtre tient entière");

        let list = CitedArticles { articles: articles.clone(), truncated: true };
        let view = with_window(&text, list);
        let total = size(&view);
        assert!(total <= ResponseBudget::HARD_CAP_CHARS, "{total}");
        assert_eq!(view.publication.as_ref().unwrap().cited_articles, articles);
        let (shortened, whole) = (kept(&view), kept(&alone));
        assert!(shortened < whole, "{shortened} {whole}");
    }

    /// Une fenêtre riche en échappements JSON dépasse le plafond dur : la vue la raccourcit et
    /// garde un `next_offset` exact.
    #[test]
    fn a_window_over_the_hard_cap_is_shortened_with_an_exact_next_offset() {
        let text = "\"mot\" ".repeat(6_600);
        let length = u32::try_from(text.chars().count()).unwrap();
        let window = Reading { text: Some(text), ..reading(length) };
        let view = BofipView::from(published(doc(), window));
        let size = serde_json::to_string(&view).unwrap().chars().count();
        assert!(size <= ResponseBudget::HARD_CAP_CHARS);
        let read = &view.publication.as_ref().unwrap().reading;
        let kept = u32::try_from(read.text.as_ref().unwrap().chars().count()).unwrap();
        assert!(kept < length);
        assert_eq!(read.next_offset, Some(kept));
    }

    /// Une liste n'a rien à retirer, un résumé non plus : le cran rend `false` et ne touche à
    /// rien. Test à part du précédent : un cran qui rendrait toujours `true` fait boucler
    /// `fit_under_cap` sans fin, et seul un test qui échoue vite le fait tuer au lieu d'expirer.
    #[test]
    fn a_choice_and_a_summary_have_nothing_to_degrade() {
        let mut list = BofipView::from(choice());
        assert!(!list.degrade());
        assert_eq!(list.choice.as_ref().unwrap().publications.len(), 2);

        let summary = Summary {
            source: SummarySource::Lead,
            text: "Les réductions de droits".to_owned(),
            truncated: true,
        };
        let read = Reading { summary: Some(summary), summary_truncated: true, ..reading(28_323) };
        let mut short = BofipView::from(published(doc(), read));
        assert!(!short.degrade());
        let kept = &short.publication.as_ref().unwrap().reading;
        assert_eq!(kept.summary.as_deref(), Some("Les réductions de droits"));
    }

    /// Un seul schéma pour les deux formes : seuls `id` et les deux constantes sont requis, donc
    /// une publication lue comme une liste le respectent.
    #[test]
    fn the_output_schema_requires_only_what_both_shapes_carry() {
        let schema = Value::Object(output_schema::<BofipView>().as_ref().clone());
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"], json!(["id", "attribution", "doctrine_note"]));
        assert!(schema.get("$schema").is_none());
        let mut names: Vec<_> = schema["properties"].as_object().unwrap().keys().cloned().collect();
        names.sort();
        assert_eq!(
            names,
            [
                "attribution",
                "citation",
                "cited_articles",
                "cited_articles_truncated",
                "division",
                "doctrine_note",
                "id",
                "kind",
                "next_offset",
                "note",
                "passages",
                "publication_id",
                "publications",
                "serie",
                "source_url",
                "summary",
                "summary_source",
                "summary_truncated",
                "text",
                "text_length",
                "title",
                "valid_from",
            ]
        );
        assert_eq!(schema["properties"]["kind"]["enum"], json!(["doctrine", "news"]));
        assert_eq!(
            schema["properties"]["publications"]["items"]["required"],
            json!(["publication_id", "title"])
        );
    }

    #[test]
    fn output_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        let schema = Value::Object(output_schema::<BofipView>().as_ref().clone());
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("output_schema", schema);
        });
    }
}
