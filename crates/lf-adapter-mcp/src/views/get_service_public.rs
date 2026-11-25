//! Vue de sortie de `fr_get_service_public` (05 §5.17) : `GuidanceView`.
//!
//! Construite depuis le [`GetServicePublicOutput`] de l'application :
//!
//! - **une fiche** : ses métadonnées, sa citation, son adresse, les textes de Légifrance qu'elle
//!   cite (`legal_references`), puis les champs de lecture communs ([`ReadingView`], aplatis) ;
//! - **un dossier ou un thème** : ses métadonnées et ses entrées (`entries`), sans texte.
//!
//! Chaque réponse porte les mentions de la DILA (ADR-0033, décision 9) : `attribution` du public
//! lu, `source_file` (l'archive et l'entrée lue) et [`READ_GUIDANCE_NOTE`]. Les champs absents sont
//! omis (jamais `null`). L'application a déjà tenu les métadonnées sous 20 000 caractères ; la vue
//! raccourcit la fenêtre de texte si le JSON exact dépasse le plafond dur. Les commentaires des
//! champs sont des `//` : un commentaire de documentation partirait dans l'`outputSchema`, dont la
//! taille compte (05 §3.4).

use lf_application::output::get_service_public::GetServicePublicOutput;
use lf_domain::guidance::{
    ARCHIVE_NAME, GuidanceContent, LegalReference, NavigationEntry, archive_url,
};
use schemars::JsonSchema;
use serde::Serialize;

use super::{
    fit::fit_under_cap,
    reading::ReadingView,
    search_service_public::{
        AudienceView, GuidanceKindView, archive_date, attribution, guidance_note,
    },
};

/// `guidance_note` de la lecture : celle de la recherche, sans la phrase sur ce que la recherche
/// lit, suivie d'une phrase fixe sur les textes cités (ADR-0033, décision 9).
pub(crate) const READ_GUIDANCE_NOTE: &str = concat!(
    guidance_note!(),
    " Before relying on a precise point (an amount, a deadline, a condition), read the Legifrance \
     texts listed in legal_references: the sheet may lag behind a change of the law."
);

// Les noms de champs et leur ordre sont ceux de 05 §5.17.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct GuidanceView {
    // `F…` ou `N…`, tel que validé.
    id: String,
    audience: AudienceView,
    kind: GuidanceKindView,
    // Type de fiche du flux, par exemple « Fiche d'information conditionnée ».
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    sheet_type: Option<String>,
    title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    theme: Option<String>,
    // Chemin dans le site (Q41), omis hors du menu.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(description = "Place in the site, from the broadest heading to the narrowest.")]
    path: Vec<String>,
    // Dernière modification importante, `AAAA-MM-JJ`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    updated: Option<String>,
    citation: String,
    source_url: String,
    // Fiche : textes de Légifrance cités, omis s'il n'y en a aucun.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(
        description = "Legifrance texts the sheet relies on, at most 50; id is given when a tool of this server reads it."
    )]
    legal_references: Vec<LegalReferenceView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(description = "true when legal_references is incomplete.")]
    legal_references_truncated: Option<bool>,
    // Dossier ou thème : ses fiches et sous-dossiers, sans texte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<EntryView>")]
    entries: Option<Vec<EntryView>>,
    // Fiche : lecture de son texte.
    #[serde(flatten)]
    reading: Option<ReadingView>,
    attribution: String,
    #[schemars(description = "The archive and the entry read.")]
    source_file: SourceEntryView,
    #[schemars(description = "What Service-Public sheets are, and the limit of this source.")]
    guidance_note: String,
}

// Un texte de Légifrance cité par la fiche.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(inline)]
struct LegalReferenceView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    title: Option<String>,
    source_url: String,
}

impl From<&LegalReference> for LegalReferenceView {
    fn from(reference: &LegalReference) -> Self {
        Self {
            id: reference.id().map(str::to_owned),
            title: reference.title().map(str::to_owned),
            source_url: reference.source_url().to_owned(),
        }
    }
}

// Une entrée d'un dossier ou d'un thème.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(inline)]
struct EntryView {
    id: String,
    kind: GuidanceKindView,
    title: String,
    // Intitulé du sous-dossier ou du sous-thème qui porte l'entrée dans la page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    group: Option<String>,
}

impl From<NavigationEntry> for EntryView {
    fn from(entry: NavigationEntry) -> Self {
        Self {
            id: entry.id.as_str().to_owned(),
            kind: entry.kind.into(),
            title: entry.title,
            group: entry.group,
        }
    }
}

// L'archive lue et l'entrée, dans les termes de la DILA.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(inline)]
struct SourceEntryView {
    url: String,
    name: String,
    // En-tête `Last-Modified` de l'archive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    date: Option<String>,
    // Nom de l'entrée lue, `{id}.xml`.
    entry: String,
}

impl From<GetServicePublicOutput> for GuidanceView {
    fn from(output: GetServicePublicOutput) -> Self {
        let source_file = SourceEntryView {
            url: archive_url(output.file.audience).to_owned(),
            name: ARCHIVE_NAME.to_owned(),
            date: archive_date(&output.file),
            entry: output.entry(),
        };
        let GetServicePublicOutput { doc, reading, .. } = output;
        let citation = doc.citation();
        let kind = doc.kind().into();
        let (legal_references, legal_references_truncated, entries) = match doc.content {
            GuidanceContent::Sheet { references, .. } => (
                references.references().iter().map(LegalReferenceView::from).collect(),
                references.truncated().then_some(true),
                None,
            ),
            GuidanceContent::Folder(entries) | GuidanceContent::Theme(entries) => {
                (Vec::new(), None, Some(entries.into_iter().map(EntryView::from).collect()))
            }
        };
        let mut view = Self {
            id: doc.id.as_str().to_owned(),
            audience: doc.audience.into(),
            kind,
            sheet_type: doc.sheet_type,
            title: doc.title,
            description: doc.description,
            theme: doc.theme,
            path: doc.path,
            updated: doc.updated.map(|date| date.to_string()),
            citation,
            source_url: doc.source_url.as_str().to_owned(),
            legal_references,
            legal_references_truncated,
            entries,
            reading: reading.map(ReadingView::from),
            attribution: attribution(doc.audience),
            source_file,
            guidance_note: READ_GUIDANCE_NOTE.to_owned(),
        };
        // L'application ne voit pas les échappements JSON : la mesure exacte tranche, et la
        // fenêtre raccourcie garde un `next_offset` exact. Un dossier n'a rien à retirer.
        fit_under_cap(&mut view, Self::degrade);
        view
    }
}

impl GuidanceView {
    /// Retire un cran de texte à la fiche lue ; `false` s'il n'y a plus rien à retirer, ou pour
    /// un dossier.
    fn degrade(&mut self) -> bool {
        self.reading.as_mut().is_some_and(ReadingView::degrade)
    }
}

#[cfg(test)]
mod tests {
    use lf_application::{
        budget::{Reading, ResponseBudget},
        output::search_service_public::GUIDANCE_CONSTANTS_RESERVE_CHARS,
        ports::ArchiveFile,
    };
    use lf_domain::{
        date::CivilDate,
        guidance::{
            Audience, Block, GuidanceDoc, GuidanceKind, Inline, LegalReferences, ServicePublicUrl,
            SheetId, legal_references, render,
        },
        text::{Summary, SummarySource},
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::{schema::output_schema, views::search_service_public::SEARCH_GUIDANCE_NOTE};

    const ARTICLE: &str = "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000032041571";
    const SECTION: &str =
        "https://www.legifrance.gouv.fr/codes/section_lc/LEGITEXT000006070721/LEGISCTA000006118032";

    fn id(raw: &str) -> SheetId {
        SheetId::parse(raw).unwrap()
    }

    fn file(audience: Audience) -> ArchiveFile {
        ArchiveFile { audience, last_modified: Some("2026-11-24T14:26:09Z".parse().unwrap()) }
    }

    /// La fiche F1437 de Particuliers, un paragraphe, deux références (une lue par un outil, une
    /// section sans numéro).
    fn sheet() -> GuidanceDoc {
        sheet_citing(legal_references([
            (Some("Code des procédures civiles d'exécution : article R162-2"), ARTICLE),
            (None, SECTION),
        ]))
    }

    /// La fiche F1437 de Particuliers, un paragraphe, qui cite `references`.
    fn sheet_citing(references: LegalReferences) -> GuidanceDoc {
        let id = id("F1437");
        let blocks = [Block::Paragraph(vec![Inline::Text("Un texte court.".to_owned())])];
        GuidanceDoc {
            source_url: ServicePublicUrl::read(None, Audience::Particuliers, &id),
            id,
            audience: Audience::Particuliers,
            sheet_type: Some("Fiche d'information".to_owned()),
            title: "Solde bancaire insaisissable (SBI)".to_owned(),
            description: Some("Somme laissée sur le compte.".to_owned()),
            theme: Some("Argent".to_owned()),
            path: vec!["Justice".to_owned(), "Saisies et recouvrements".to_owned()],
            updated: CivilDate::new(2026, 4, 1),
            content: GuidanceContent::Sheet { text: render(&blocks).unwrap(), references },
        }
    }

    /// Le dossier N31901 d'Entreprendre, deux entrées dont une groupée ; hors du menu, sans date.
    fn folder() -> GuidanceDoc {
        let id = id("N31901");
        let entries = vec![
            NavigationEntry {
                id: SheetId::parse("F2408").unwrap(),
                kind: GuidanceKind::Sheet,
                title: "Frais professionnels".to_owned(),
                group: Some("Salariés".to_owned()),
            },
            NavigationEntry {
                id: SheetId::parse("N24267").unwrap(),
                kind: GuidanceKind::Folder,
                title: "Paie".to_owned(),
                group: None,
            },
        ];
        GuidanceDoc {
            source_url: ServicePublicUrl::read(None, Audience::Entreprendre, &id),
            id,
            audience: Audience::Entreprendre,
            sheet_type: None,
            title: "Rémunération".to_owned(),
            description: None,
            theme: None,
            path: Vec::new(),
            updated: None,
            content: GuidanceContent::Folder(entries),
        }
    }

    fn reading(text: &str, text_length: u32) -> Reading {
        Reading {
            summary: None,
            text: Some(text.to_owned()),
            text_length,
            next_offset: None,
            passages: None,
            note: None,
            summary_truncated: false,
        }
    }

    fn json_of(output: GetServicePublicOutput) -> Value {
        serde_json::to_value(GuidanceView::from(output)).unwrap()
    }

    /// Un cran de dégradation raccourcit la lecture d'une fiche ; un dossier n'a rien à retirer.
    #[test]
    fn degrading_cuts_a_sheet_reading_and_leaves_a_folder() {
        let mut sheet = GuidanceView::from(GetServicePublicOutput {
            doc: sheet(),
            reading: Some(reading("Un texte court.", 15)),
            file: file(Audience::Particuliers),
        });
        assert!(sheet.degrade());
        let mut folder = GuidanceView::from(GetServicePublicOutput {
            doc: folder(),
            reading: None,
            file: file(Audience::Entreprendre),
        });
        assert!(!folder.degrade());
    }

    #[test]
    fn a_sheet_carries_its_metadata_references_reading_and_mentions() {
        let output = GetServicePublicOutput {
            doc: sheet(),
            reading: Some(reading("Un texte court.", 15)),
            file: file(Audience::Particuliers),
        };
        assert_eq!(
            json_of(output),
            json!({
                "id": "F1437",
                "audience": "particuliers",
                "kind": "sheet",
                "sheet_type": "Fiche d'information",
                "title": "Solde bancaire insaisissable (SBI)",
                "description": "Somme laissée sur le compte.",
                "theme": "Argent",
                "path": ["Justice", "Saisies et recouvrements"],
                "updated": "2026-04-01",
                "citation": "Service-Public.gouv.fr, Solde bancaire insaisissable (SBI) (F1437), mise à jour du 1er avril 2026",
                "source_url": "https://www.service-public.gouv.fr/particuliers/vosdroits/F1437",
                "legal_references": [
                    {
                        "id": "LEGIARTI000032041571",
                        "title": "Code des procédures civiles d'exécution : article R162-2",
                        "source_url": ARTICLE
                    },
                    {"source_url": SECTION}
                ],
                "text": "Un texte court.",
                "text_length": 15,
                "attribution": "Source : Service-Public.gouv.fr / DILA — Licence Ouverte 2.0",
                "source_file": {
                    "url": "https://lecomarquage.service-public.gouv.fr/vdd/3.5/part/zip/vosdroits-latest.zip",
                    "name": "vosdroits-latest.zip",
                    "date": "2026-11-24T14:26:09Z",
                    "entry": "F1437.xml"
                },
                "guidance_note": READ_GUIDANCE_NOTE,
            })
        );
    }

    #[test]
    fn a_folder_lists_its_entries_without_text() {
        let output = GetServicePublicOutput {
            doc: folder(),
            reading: None,
            file: ArchiveFile { audience: Audience::Entreprendre, last_modified: None },
        };
        assert_eq!(
            json_of(output),
            json!({
                "id": "N31901",
                "audience": "entreprendre",
                "kind": "folder",
                "title": "Rémunération",
                "citation": "Entreprendre.Service-Public.gouv.fr, Rémunération (N31901)",
                "source_url": "https://entreprendre.service-public.gouv.fr/vosdroits/N31901",
                "entries": [
                    {"id": "F2408", "kind": "sheet", "title": "Frais professionnels", "group": "Salariés"},
                    {"id": "N24267", "kind": "folder", "title": "Paie"}
                ],
                "attribution": "Source : Entreprendre.Service-Public.gouv.fr / DILA — Licence Ouverte 2.0",
                "source_file": {
                    "url": "https://lecomarquage.service-public.gouv.fr/vdd/3.5/pro/zip/vosdroits-latest.zip",
                    "name": "vosdroits-latest.zip",
                    "entry": "N31901.xml"
                },
                "guidance_note": READ_GUIDANCE_NOTE,
            })
        );
    }

    #[test]
    fn a_theme_lists_its_entries_and_a_truncated_list_is_flagged() {
        let theme = GuidanceDoc { content: GuidanceContent::Theme(Vec::new()), ..folder() };
        let output = GetServicePublicOutput {
            doc: theme,
            reading: None,
            file: file(Audience::Entreprendre),
        };
        let value = json_of(output);
        assert_eq!((&value["kind"], &value["entries"]), (&json!("theme"), &json!([])));

        let many: Vec<(Option<&str>, String)> = (1..=51)
            .map(|n| (None, format!("https://www.legifrance.gouv.fr/jorf/id/JORFTEXT{n:012}")))
            .collect();
        let references = legal_references(many.iter().map(|(title, url)| (*title, url.as_str())));
        let output = GetServicePublicOutput {
            doc: sheet_citing(references),
            reading: Some(reading("Un texte court.", 15)),
            file: file(Audience::Particuliers),
        };
        let value = json_of(output);
        assert_eq!(value["legal_references"].as_array().unwrap().len(), 50);
        assert_eq!(value["legal_references_truncated"], true);
        assert!(value["legal_references"][0].get("id").is_none());
    }

    #[test]
    fn a_sheet_without_reference_omits_the_list() {
        let output = GetServicePublicOutput {
            doc: sheet_citing(legal_references([])),
            reading: Some(Reading {
                summary: Some(Summary {
                    source: SummarySource::Official,
                    text: "Somme laissée sur le compte.".to_owned(),
                    truncated: false,
                }),
                ..reading("Un texte court.", 15)
            }),
            file: file(Audience::Particuliers),
        };
        let value = json_of(output);
        assert!(value.get("legal_references").is_none());
        assert!(value.get("legal_references_truncated").is_none());
        assert!(value.get("entries").is_none());
        assert_eq!(value["summary"], "Somme laissée sur le compte.");
        assert_eq!(value["summary_source"], "official");
    }

    /// Les mêmes littéraux que les tests de taille de l'application (`reference_cost`,
    /// `entry_cost` de `usecase/get_service_public.rs`) : un champ renommé, déplacé ou ajouté dans
    /// la vue casse ce test, et dit de reprendre le calcul.
    #[test]
    fn the_compact_json_of_a_reference_and_an_entry_is_the_one_the_application_counts() {
        let references = legal_references([(Some("Code \"civil\""), ARTICLE)]);
        let view = LegalReferenceView::from(&references.references()[0]);
        assert_eq!(
            serde_json::to_string(&view).unwrap(),
            format!(
                r#"{{"id":"LEGIARTI000032041571","title":"Code \"civil\"","source_url":"{ARTICLE}"}}"#
            )
        );
        let entry = EntryView::from(NavigationEntry {
            id: SheetId::parse("F1437").unwrap(),
            kind: GuidanceKind::Sheet,
            title: "Compte".to_owned(),
            group: Some("Banque".to_owned()),
        });
        assert_eq!(
            serde_json::to_string(&entry).unwrap(),
            r#"{"id":"F1437","kind":"sheet","title":"Compte","group":"Banque"}"#
        );
    }

    /// Les mentions de la lecture tiennent dans la réserve que l'application leur garde.
    #[test]
    fn the_mentions_fit_in_the_reserve_of_the_application() {
        let output = GetServicePublicOutput {
            doc: folder(),
            reading: None,
            file: file(Audience::Entreprendre),
        };
        let value = json_of(output);
        let mentions = format!(
            r#","attribution":{},"source_file":{},"guidance_note":{}"#,
            value["attribution"], value["source_file"], value["guidance_note"]
        );
        assert_eq!(mentions.chars().count(), 647);
        assert!(mentions.chars().count() <= GUIDANCE_CONSTANTS_RESERVE_CHARS);
        assert_eq!(
            serde_json::to_string(READ_GUIDANCE_NOTE).unwrap(),
            format!("\"{READ_GUIDANCE_NOTE}\"")
        );
    }

    #[test]
    fn the_note_of_the_reading_sends_to_the_legal_references() {
        let common = SEARCH_GUIDANCE_NOTE.split(" The search").next().unwrap();
        assert!(READ_GUIDANCE_NOTE.starts_with(common));
        assert!(READ_GUIDANCE_NOTE.contains("legal_references"));
        assert!(!READ_GUIDANCE_NOTE.contains("titles and summaries"));
        assert!(!READ_GUIDANCE_NOTE.contains("http"));
    }

    /// Une fenêtre plus longue que le plafond dur est raccourcie, la réponse reste un objet
    /// complet et `next_offset` désigne la suite.
    #[test]
    fn a_window_over_the_hard_cap_is_shortened() {
        let text = "Phrase assez longue pour la mesure. ".repeat(1_400);
        let length = u32::try_from(text.chars().count()).unwrap();
        let output = GetServicePublicOutput {
            doc: sheet(),
            reading: Some(Reading { summary_truncated: false, ..reading(&text, length) }),
            file: file(Audience::Particuliers),
        };
        let view = GuidanceView::from(output);
        let size = serde_json::to_string(&view).unwrap().chars().count();
        assert!(size <= ResponseBudget::HARD_CAP_CHARS, "{size}");
        let reading = view.reading.unwrap();
        assert!(reading.text.unwrap().chars().count() < text.chars().count());
        assert!(reading.next_offset.is_some());
        // Un dossier n'a rien à retirer.
        let mut folder = GuidanceView::from(GetServicePublicOutput {
            doc: folder(),
            reading: None,
            file: file(Audience::Entreprendre),
        });
        assert!(!folder.degrade());
    }

    #[test]
    fn the_output_schema_names_the_fields_in_order() {
        let schema = Value::Object(output_schema::<GuidanceView>().as_ref().clone());
        assert_eq!(schema["type"], "object");
        assert_eq!(
            schema["required"],
            json!([
                "id",
                "audience",
                "kind",
                "title",
                "citation",
                "source_url",
                "attribution",
                "source_file",
                "guidance_note"
            ])
        );
        let reference = &schema["properties"]["legal_references"]["items"];
        assert_eq!(reference["required"], json!(["source_url"]));
        let entry = &schema["properties"]["entries"]["items"];
        assert_eq!(entry["required"], json!(["id", "kind", "title"]));
        let file = &schema["properties"]["source_file"];
        assert_eq!(file["required"], json!(["url", "name", "entry"]));
        assert!(schema["properties"].get("text_length").is_some());
        assert!(schema.get("$schema").is_none());
    }

    #[test]
    fn output_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        let schema = Value::Object(output_schema::<GuidanceView>().as_ref().clone());
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("output_schema", schema);
        });
    }
}
