//! Vue de sortie de `fr_search_service_public` (05 §5.16) : `GuidanceSearchView`.
//!
//! Construite depuis le [`SearchServicePublicOutput`] de l'application : une page de fiches et de
//! dossiers de Service-Public, jamais leur texte. La vue y ajoute les mentions de chaque réponse
//! (ADR-0033, décision 9) : `attribution` selon les publics cherchés, `source_files` (l'archive de
//! chaque public, dans les termes de la DILA) et [`SEARCH_GUIDANCE_NOTE`]. L'application leur
//! réserve
//! [`GUIDANCE_CONSTANTS_RESERVE_CHARS`](lf_application::output::search_service_public::GUIDANCE_CONSTANTS_RESERVE_CHARS)
//! caractères dans son calcul de taille : un test vérifie qu'elles y tiennent.
//!
//! L'application compte exactement le JSON de chaque résultat (échappements compris) et raccourcit
//! titres puis chemins avant de construire la vue : la mesure de `views::fit` n'est ici qu'un
//! filet, qui ne retire rien (jamais un résultat, 05 §6.4). Les commentaires des champs sont des
//! `//` : un doc comment partirait dans l'`outputSchema`, dont la taille compte (05 §3.4).

use lf_application::{
    output::search_service_public::SearchServicePublicOutput, ports::ArchiveFile,
};
use lf_domain::guidance::{
    ARCHIVE_NAME, Audience, AudienceFilter, GuidanceHit, GuidanceKind, archive_url,
};
use schemars::JsonSchema;
use serde::Serialize;

use super::fit::fit_under_cap;

/// Texte commun aux deux notes de Service-Public : ce qu'est une fiche, et sa limite. En macro,
/// pour que les deux notes le reprennent mot pour mot (`concat!` n'accepte que des littéraux).
macro_rules! guidance_note {
    () => {
        "Service-Public sheets are administrative guidance written for the public by the DILA: they explain rules in plain language, they are not the law. Only today's version is available."
    };
}
pub(crate) use guidance_note;

/// `guidance_note` de la recherche : nature des fiches, version du jour, et ce que la recherche
/// lit (titres et résumés seulement).
pub(crate) const SEARCH_GUIDANCE_NOTE: &str = concat!(
    guidance_note!(),
    " The search reads titles and summaries only, never the full text: an empty result does not \
     mean nothing is published on the subject."
);

/// « Source : {site} / DILA — Licence Ouverte 2.0 » : paternité exigée par la DILA, propre à
/// chaque public (ADR-0033, décision 9).
pub(crate) fn attribution(audience: Audience) -> String {
    format!("Source : {} / DILA — Licence Ouverte 2.0", audience.site())
}

/// Mentions des publics cherchés, séparées par « ; » pour `all`.
fn attributions(audience: AudienceFilter) -> String {
    let all: Vec<String> = audience.audiences().iter().map(|one| attribution(*one)).collect();
    all.join(" ; ")
}

/// Date d'une archive, `AAAA-MM-JJTHH:MM:SSZ`, lue dans l'en-tête `Last-Modified`.
pub(crate) fn archive_date(file: &ArchiveFile) -> Option<String> {
    file.last_modified.map(|date| date.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

// Public d'une archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum AudienceView {
    Particuliers,
    Entreprendre,
}

impl From<Audience> for AudienceView {
    fn from(audience: Audience) -> Self {
        match audience {
            Audience::Particuliers => Self::Particuliers,
            Audience::Entreprendre => Self::Entreprendre,
        }
    }
}

// Nature d'une page : fiche, dossier ou thème.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum GuidanceKindView {
    Sheet,
    Folder,
    Theme,
}

impl From<GuidanceKind> for GuidanceKindView {
    fn from(kind: GuidanceKind) -> Self {
        match kind {
            GuidanceKind::Sheet => Self::Sheet,
            GuidanceKind::Folder => Self::Folder,
            GuidanceKind::Theme => Self::Theme,
        }
    }
}

// L'archive cherchée d'un public, dans les termes de la DILA.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(inline)]
struct SourceFileView {
    url: String,
    name: String,
    // En-tête `Last-Modified` de l'archive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    date: Option<String>,
}

impl From<&ArchiveFile> for SourceFileView {
    fn from(file: &ArchiveFile) -> Self {
        Self {
            url: archive_url(file.audience).to_owned(),
            name: ARCHIVE_NAME.to_owned(),
            date: archive_date(file),
        }
    }
}

// Un résultat : de quoi choisir la page et la lire avec `fr_get_service_public`.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(inline)]
struct GuidanceHitView {
    // `F…` (fiche) ou `N…` (dossier, thème).
    id: String,
    audience: AudienceView,
    kind: GuidanceKindView,
    title: String,
    // Chemin dans le site (Q41), omis hors du menu.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(description = "Place in the site, from the broadest heading to the narrowest.")]
    path: Vec<String>,
    // Dernière modification importante, `AAAA-MM-JJ` (fiches).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    updated: Option<String>,
    source_url: String,
}

impl From<GuidanceHit> for GuidanceHitView {
    fn from(hit: GuidanceHit) -> Self {
        Self {
            id: hit.id.as_str().to_owned(),
            audience: hit.audience.into(),
            kind: hit.kind.into(),
            title: hit.title,
            path: hit.path,
            updated: hit.updated.map(|date| date.to_string()),
            source_url: hit.source_url.as_str().to_owned(),
        }
    }
}

/// Ce que `fr_search_service_public` rend : une page de résultats et les mentions de la DILA.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct GuidanceSearchView {
    total: u64,
    page: u32,
    #[schemars(description = "True if the next page exists.")]
    has_more: bool,
    hits: Vec<GuidanceHitView>,
    attribution: String,
    #[schemars(description = "The archive searched for each audience.")]
    source_files: Vec<SourceFileView>,
    #[schemars(description = "What Service-Public sheets are, and the limit of this source.")]
    guidance_note: String,
    // Indice facultatif : aucun résultat, titres raccourcis, chemins retirés.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    note: Option<String>,
}

impl From<SearchServicePublicOutput> for GuidanceSearchView {
    fn from(output: SearchServicePublicOutput) -> Self {
        let mut view = Self {
            total: output.total,
            page: output.page,
            has_more: output.has_more,
            hits: output.hits.into_iter().map(GuidanceHitView::from).collect(),
            attribution: attributions(output.audience),
            source_files: output.files.iter().map(SourceFileView::from).collect(),
            guidance_note: SEARCH_GUIDANCE_NOTE.to_owned(),
            note: output.note,
        };
        // Filet : l'application a déjà ramené la page sous le plafond. Rien n'est retiré ici, un
        // dépassement serait seulement signalé au journal.
        fit_under_cap(&mut view, |_| false);
        view
    }
}

#[cfg(test)]
mod tests {
    use lf_application::{
        budget::ResponseBudget, output::search_service_public::GUIDANCE_CONSTANTS_RESERVE_CHARS,
    };
    use lf_domain::{
        date::CivilDate,
        guidance::{ServicePublicUrl, SheetId},
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::schema::output_schema;

    /// Archive de `audience`, datée du 2026-11-24 à 14 h 26 min 9 s UTC (en-tête relevé ce jour).
    fn file(audience: Audience) -> ArchiveFile {
        ArchiveFile { audience, last_modified: Some("2026-11-24T14:26:09Z".parse().unwrap()) }
    }

    /// Résultat complet : la fiche `raw` de `audience`, de titre `title`.
    fn hit(audience: Audience, raw: &str, title: &str) -> GuidanceHit {
        let id = SheetId::parse(raw).unwrap();
        GuidanceHit {
            source_url: ServicePublicUrl::read(None, audience, &id),
            audience,
            id,
            kind: GuidanceKind::Sheet,
            title: title.to_owned(),
            path: vec!["Argent".to_owned(), "Banque".to_owned(), "Compte bancaire".to_owned()],
            updated: CivilDate::new(2026, 4, 1),
        }
    }

    /// Résultat réduit à ses champs obligatoires : un dossier hors du menu, sans date.
    fn bare(audience: Audience, raw: &str) -> GuidanceHit {
        GuidanceHit {
            kind: GuidanceKind::Folder,
            path: Vec::new(),
            updated: None,
            ..hit(audience, raw, "a")
        }
    }

    fn output(audience: AudienceFilter, hits: Vec<GuidanceHit>) -> SearchServicePublicOutput {
        SearchServicePublicOutput {
            audience,
            total: 357,
            page: 2,
            has_more: true,
            hits,
            files: audience.audiences().iter().map(|one| file(*one)).collect(),
            note: None,
        }
    }

    fn json_of(output: SearchServicePublicOutput) -> Value {
        serde_json::to_value(GuidanceSearchView::from(output)).unwrap()
    }

    #[test]
    fn the_view_carries_the_page_its_hits_and_the_mentions_of_the_dila() {
        let value = json_of(output(
            AudienceFilter::Particuliers,
            vec![hit(Audience::Particuliers, "F1437", "Solde bancaire insaisissable")],
        ));
        assert_eq!(
            value,
            json!({
                "total": 357,
                "page": 2,
                "has_more": true,
                "hits": [{
                    "id": "F1437",
                    "audience": "particuliers",
                    "kind": "sheet",
                    "title": "Solde bancaire insaisissable",
                    "path": ["Argent", "Banque", "Compte bancaire"],
                    "updated": "2026-04-01",
                    "source_url": "https://www.service-public.gouv.fr/particuliers/vosdroits/F1437"
                }],
                "attribution": "Source : Service-Public.gouv.fr / DILA — Licence Ouverte 2.0",
                "source_files": [{
                    "url": "https://lecomarquage.service-public.gouv.fr/vdd/3.5/part/zip/vosdroits-latest.zip",
                    "name": "vosdroits-latest.zip",
                    "date": "2026-11-24T14:26:09Z"
                }],
                "guidance_note": SEARCH_GUIDANCE_NOTE,
            })
        );
    }

    #[test]
    fn a_search_on_both_audiences_names_both_sites_and_both_archives() {
        let value =
            json_of(output(AudienceFilter::All, vec![bare(Audience::Entreprendre, "N24267")]));
        assert_eq!(
            value["attribution"],
            "Source : Service-Public.gouv.fr / DILA — Licence Ouverte 2.0 ; Source : \
             Entreprendre.Service-Public.gouv.fr / DILA — Licence Ouverte 2.0"
        );
        let urls: Vec<&str> = value["source_files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| file["url"].as_str().unwrap())
            .collect();
        assert_eq!(
            urls,
            [
                "https://lecomarquage.service-public.gouv.fr/vdd/3.5/part/zip/vosdroits-latest.zip",
                "https://lecomarquage.service-public.gouv.fr/vdd/3.5/pro/zip/vosdroits-latest.zip",
            ]
        );
        let entreprendre = json_of(output(AudienceFilter::Entreprendre, Vec::new()));
        assert_eq!(
            entreprendre["attribution"],
            "Source : Entreprendre.Service-Public.gouv.fr / DILA — Licence Ouverte 2.0"
        );
    }

    #[test]
    fn absent_fields_are_omitted_and_the_note_is_passed_on() {
        let mut empty =
            output(AudienceFilter::Entreprendre, vec![bare(Audience::Entreprendre, "N24267")]);
        empty.has_more = false;
        empty.note = Some("No sheet or folder has these words.".to_owned());
        empty.files = vec![ArchiveFile { audience: Audience::Entreprendre, last_modified: None }];
        let value = json_of(empty);
        assert_eq!(
            value["hits"],
            json!([{
                "id": "N24267",
                "audience": "entreprendre",
                "kind": "folder",
                "title": "a",
                "source_url": "https://entreprendre.service-public.gouv.fr/vosdroits/N24267"
            }])
        );
        assert_eq!(
            value["source_files"],
            json!([{
                "url": "https://lecomarquage.service-public.gouv.fr/vdd/3.5/pro/zip/vosdroits-latest.zip",
                "name": "vosdroits-latest.zip"
            }])
        );
        assert_eq!(value["has_more"], false);
        assert_eq!(value["note"], "No sheet or folder has these words.");
        // Sans remarque, la clé `note` n'existe pas.
        let quiet = json_of(output(AudienceFilter::Particuliers, Vec::new()));
        assert!(quiet.get("note").is_none());
        assert_eq!(quiet["hits"], json!([]));
    }

    #[test]
    fn every_kind_and_audience_has_its_name() {
        let kinds = [GuidanceKind::Sheet, GuidanceKind::Folder, GuidanceKind::Theme]
            .map(|kind| serde_json::to_value(GuidanceKindView::from(kind)).unwrap());
        assert_eq!(kinds, [json!("sheet"), json!("folder"), json!("theme")]);
        for kind in [GuidanceKind::Sheet, GuidanceKind::Folder, GuidanceKind::Theme] {
            assert_eq!(serde_json::to_value(GuidanceKindView::from(kind)).unwrap(), kind.as_str());
        }
        for audience in Audience::ALL {
            assert_eq!(
                serde_json::to_value(AudienceView::from(audience)).unwrap(),
                audience.name()
            );
        }
    }

    /// Parité avec le calcul de taille de l'application (`hit_cost`, `envelope_cost` de
    /// `usecase/search_service_public.rs`) : le même littéral JSON que son test, à l'octet. Un
    /// champ renommé, déplacé ou ajouté dans la vue casse ce test, et dit de reprendre le calcul.
    #[test]
    fn the_compact_json_is_the_one_the_application_counts() {
        let full = GuidanceHitView::from(hit(Audience::Entreprendre, "F31901", "Compte \"pro\""));
        assert_eq!(
            serde_json::to_string(&full).unwrap(),
            r#"{"id":"F31901","audience":"entreprendre","kind":"sheet","title":"Compte \"pro\"","path":["Argent","Banque","Compte bancaire"],"updated":"2026-04-01","source_url":"https://entreprendre.service-public.gouv.fr/vosdroits/F31901"}"#
        );
        let reduced = GuidanceHitView::from(bare(Audience::Particuliers, "N5"));
        assert_eq!(
            serde_json::to_string(&reduced).unwrap(),
            r#"{"id":"N5","audience":"particuliers","kind":"folder","title":"a","source_url":"https://www.service-public.gouv.fr/particuliers/vosdroits/N5"}"#
        );

        // L'enveloppe : celle que l'application compte, plus les champs constants de la vue.
        let mut noted = output(AudienceFilter::Particuliers, Vec::new());
        noted.has_more = false;
        noted.note = Some("titres \"raccourcis\"".to_owned());
        let constants = constants(AudienceFilter::Particuliers);
        assert_eq!(
            serde_json::to_string(&GuidanceSearchView::from(noted)).unwrap(),
            format!(
                r#"{{"total":357,"page":2,"has_more":false,"hits":[]{constants},"note":"titres \"raccourcis\""}}"#
            )
        );
    }

    /// Les champs constants que la vue ajoute à l'enveloppe, clés, guillemets et virgules compris,
    /// dans l'ordre de la vue.
    fn constants(audience: AudienceFilter) -> String {
        let view = GuidanceSearchView::from(output(audience, Vec::new()));
        format!(
            r#","attribution":{},"source_files":{},"guidance_note":{}"#,
            serde_json::to_string(&view.attribution).unwrap(),
            serde_json::to_string(&view.source_files).unwrap(),
            serde_json::to_string(&view.guidance_note).unwrap()
        )
    }

    /// Les champs constants tiennent dans la réserve que l'application leur garde, même pour les
    /// deux publics ; aucune constante n'a besoin d'échappement JSON.
    #[test]
    fn the_constants_fit_in_the_reserve_of_the_application() {
        let widest = constants(AudienceFilter::All).chars().count();
        assert_eq!(widest, 804);
        assert!(widest <= GUIDANCE_CONSTANTS_RESERVE_CHARS);
        assert_eq!(GUIDANCE_CONSTANTS_RESERVE_CHARS, 1_500);
        for audience in Audience::ALL {
            let mention = attribution(audience);
            assert_eq!(serde_json::to_string(&mention).unwrap(), format!("\"{mention}\""));
        }
        assert_eq!(
            serde_json::to_string(SEARCH_GUIDANCE_NOTE).unwrap(),
            format!("\"{SEARCH_GUIDANCE_NOTE}\"")
        );
    }

    #[test]
    fn the_note_says_what_a_sheet_is_and_what_the_search_reads() {
        assert!(
            SEARCH_GUIDANCE_NOTE.starts_with("Service-Public sheets are administrative guidance")
        );
        assert!(SEARCH_GUIDANCE_NOTE.contains("they are not the law"));
        assert!(SEARCH_GUIDANCE_NOTE.contains("Only today's version is available."));
        assert!(SEARCH_GUIDANCE_NOTE.contains("titles and summaries only"));
        // Aucune adresse : seuls les liens vers Légifrance sont admis dans les textes fixes.
        assert!(!SEARCH_GUIDANCE_NOTE.contains("http"));
    }

    /// Une page pleine de titres et de chemins de taille réelle passe sous le plafond dur, et la
    /// vue ne retire jamais un résultat.
    #[test]
    fn a_full_page_stays_whole_under_the_hard_cap() {
        let title = "é".repeat(120);
        let hits: Vec<GuidanceHit> = (1..=50)
            .map(|number| hit(Audience::Particuliers, &format!("F{number}"), &title))
            .collect();
        let view = GuidanceSearchView::from(output(AudienceFilter::All, hits));
        assert_eq!(view.hits.len(), 50);
        let size = serde_json::to_string(&view).unwrap().chars().count();
        assert!(size <= ResponseBudget::HARD_CAP_CHARS);
    }

    #[test]
    fn the_output_schema_names_the_fields_in_order() {
        let schema = Value::Object(output_schema::<GuidanceSearchView>().as_ref().clone());
        assert_eq!(schema["type"], "object");
        assert_eq!(
            schema["required"],
            json!([
                "total",
                "page",
                "has_more",
                "hits",
                "attribution",
                "source_files",
                "guidance_note"
            ])
        );
        let hit = &schema["properties"]["hits"]["items"];
        assert_eq!(hit["required"], json!(["id", "audience", "kind", "title", "source_url"]));
        assert_eq!(hit["properties"]["kind"]["enum"], json!(["sheet", "folder", "theme"]));
        assert_eq!(hit["properties"]["audience"]["enum"], json!(["particuliers", "entreprendre"]));
        let file = &schema["properties"]["source_files"]["items"];
        assert_eq!(file["required"], json!(["url", "name"]));
        assert!(schema.get("$schema").is_none());
    }

    #[test]
    fn output_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        let schema = Value::Object(output_schema::<GuidanceSearchView>().as_ref().clone());
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("output_schema", schema);
        });
    }
}
