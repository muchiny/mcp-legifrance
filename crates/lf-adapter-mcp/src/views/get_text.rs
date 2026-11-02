//! Vue de sortie de `fr_get_text` (05 §5.3) : `ArticleView`.
//!
//! Construite depuis le [`GetTextOutput`] de l'application : les métadonnées de l'article, sa
//! citation et son URL, puis les champs de lecture communs ([`ReadingView`], aplatis). Les champs
//! facultatifs sont omis quand ils sont vides (05 §1.4 règle 1), les dates au-delà du seuil des
//! sentinelles aussi (règle 9). Les commentaires des champs sont des `//` : un doc comment
//! partirait dans l'`outputSchema`, dont la taille compte (05 §3.4).
//!
//! L'estimation du cas d'usage ignore les échappements JSON : la mesure exacte tranche. Si la vue
//! dépasse le plafond dur, la fenêtre (ou les passages) se raccourcit ([`ReadingView::degrade`]) et
//! `next_offset` reste exact (05 §6.4, étape 4).

use lf_application::output::get_text::GetTextOutput;
use lf_domain::{
    contract::{Citable, Eli},
    date::CivilDate,
    id::{ArticleId, DocumentId},
    status::LegalStatus,
};
use schemars::JsonSchema;
use serde::Serialize;

use super::{
    common::{date_unless_sentinel, etat_code},
    fit::fit_under_cap,
    reading::ReadingView,
};

/// Ce que `fr_get_text` rend : les métadonnées de l'article, sa citation et la lecture.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct ArticleView {
    article_id: String,
    // Numéro affiché.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    num: Option<String>,
    // Code d'API de l'état (VIGUEUR, ABROGE, VIGUEUR_DIFF...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String", description = crate::views::common::ETAT_GLOSSARY)]
    etat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    code_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String", description = "LEGITEXT or KALITEXT of the parent text.")]
    parent_id: Option<String>,
    // Titres hiérarchiques, du plus haut au plus bas.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    path: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        with = "String",
        description = "Omitted when the entry into force is not set yet (see etat)."
    )]
    date_debut: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        with = "String",
        description = "Omitted when the article is in force without an end date."
    )]
    date_fin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    nota: Option<String>,
    // ELI natif, jamais fabriqué.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    eli: Option<String>,
    citation: String,
    source_url: String,
    #[serde(flatten)]
    reading: ReadingView,
}

/// Identifiant du texte parent, seulement s'il est lisible par `fr_get_act` (`LEGITEXT`) ou
/// `fr_get_convention` (`KALITEXT`) : un `JORFTEXT` ou une autre famille est omis (05 §1.4,
/// règle 8).
fn readable_parent_id(id: &DocumentId) -> Option<String> {
    matches!(id, DocumentId::LegiText(_) | DocumentId::KaliText(_)).then(|| id.as_str().to_owned())
}

/// Fin de la note d'un article de code (LEGIARTI) terminé : `fr_resolve` avec une date et
/// `fr_diff_article` lisent les autres versions d'un article de code.
const LEGI_ENDED_ADVICE: &str =
    "Use fr_resolve with a date, or fr_diff_article, for the text in force.";
/// Fin de la note d'un article de code (LEGIARTI) dénoncé.
const LEGI_DENOUNCED_ADVICE: &str =
    "Use fr_resolve with a date, or fr_diff_article, to compare versions.";
/// Fin de la note d'un article de convention (KALIARTI) terminé : `fr_diff_article` refuse un
/// KALIARTI et `fr_resolve` ne retrouve pas « l'article 7 de la convention » par son numéro ; la
/// table des matières de la convention est le seul chemin. Aucune promesse de version courante :
/// sous un texte abrogé il n'y en a pas, et une entrée sans `etat` a celui de la convention.
const KALI_ENDED_ADVICE: &str = "fr_resolve and fr_diff_article cannot give another version of a convention article: call fr_get_convention with parent_id (if absent, fr_search on fond KALI) and look in its toc (toc_offset pages it) for the entry with the same num in the same section; an entry shown without etat has the etat of the convention itself, and there may be no current version.";
/// Fin de la note d'un article de convention (KALIARTI) dénoncé : pas de « même num » (le `toc`
/// d'une convention dénoncée ne contient que l'article lu et son voisin).
const KALI_DENOUNCED_ADVICE: &str = "fr_resolve and fr_diff_article cannot compare versions of a convention article: fr_get_convention with parent_id gives the etat of the convention itself.";

/// Note (champ `note` existant, aucun octet de schéma) quand l'article lu n'est plus la version en
/// vigueur : un texte exact d'une version terminée se lit sinon comme le droit actuel (L3-13).
///
/// Déclenchée par l'`etat` seul, sans horloge, selon [`LegalStatus::is_superseded`] (règle commune
/// avec `fr_resolve`) : `MODIFIE`, `MODIFIE_MORT_NE`, `ABROGE`, `PERIME`, `ANNULE`, `DISJOINT`,
/// `REMPLACE`, `SUBSTITUE`, `TRANSFERE`, `DENONCE` (dont le texte dit « Denounced… may still apply »). `VIGUEUR`, `VIGUEUR_ETEN`, `VIGUEUR_NON_ETEN`,
/// `ABROGE_DIFF` (encore en vigueur), `VIGUEUR_DIFF` (pas encore en vigueur : l'avertissement
/// parlerait d'une version terminée), `INITIALE`, `SANS_ETAT` et les codes inconnus n'en portent pas. La date de fin n'est citée que
/// si elle est réelle (hors zone sentinelle).
///
/// Le début de la note est le même pour les deux familles ; seule la fin dépend de l'identifiant :
/// le conseil des codes (LEGIARTI) renverrait un lecteur de convention (KALIARTI) vers deux outils
/// qui ne lisent pas une autre version de son article.
fn not_in_force_note(
    article_id: &ArticleId,
    etat: Option<&LegalStatus>,
    date_fin: Option<CivilDate>,
) -> Option<String> {
    let etat = etat?;
    if !etat.is_superseded() {
        return None;
    }
    let kali = matches!(article_id, ArticleId::Kali(_));
    let code = etat.as_api_code();
    let since =
        date_unless_sentinel(date_fin).map_or_else(String::new, |date| format!(", ended {date}"));
    // Une convention dénoncée peut encore s'appliquer pendant le préavis et la survie (glossaire
    // de `etat`) : l'avertissement le dit au lieu d'affirmer que ce n'est pas la version en vigueur.
    if matches!(etat, LegalStatus::Denonce) {
        let advice = if kali { KALI_DENOUNCED_ADVICE } else { LEGI_DENOUNCED_ADVICE };
        return Some(format!(
            "Denounced (etat={code}{since}); it may still apply during notice and survival. {advice}"
        ));
    }
    let advice = if kali { KALI_ENDED_ADVICE } else { LEGI_ENDED_ADVICE };
    Some(format!("Not the version in force (etat={code}{since}). {advice}"))
}

/// Joint la note de lecture et l'avertissement de version, séparés par une espace.
fn join_notes(reading: Option<String>, version: Option<String>) -> Option<String> {
    match (reading, version) {
        (Some(reading), Some(version)) => Some(format!("{reading} {version}")),
        (reading, version) => reading.or(version),
    }
}

impl From<GetTextOutput> for ArticleView {
    fn from(output: GetTextOutput) -> Self {
        let GetTextOutput { article, nota, reading } = output;
        let version_note =
            not_in_force_note(&article.article_id, article.etat.as_ref(), article.date_fin);
        let mut reading = ReadingView::from(reading);
        reading.note = join_notes(reading.note.take(), version_note);
        let contract = article.contract();
        let mut view = Self {
            article_id: article.article_id.as_str().to_owned(),
            num: article.num,
            etat: etat_code(article.etat.as_ref()),
            code_title: article.code_title,
            parent_id: article.parent.and_then(|parent| readable_parent_id(&parent.id)),
            path: article.path,
            date_debut: date_unless_sentinel(article.date_debut),
            date_fin: date_unless_sentinel(article.date_fin),
            nota,
            eli: contract.eli.as_ref().map(Eli::as_str).map(str::to_owned),
            citation: contract.citation.as_str().to_owned(),
            source_url: contract.source_url.to_string(),
            reading,
        };
        fit_under_cap(&mut view, |view| view.reading.degrade());
        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::output_schema;

    /// D-12 : la règle générale de `etat` est dans la description du champ (`VIGUEUR_ETEN`, `PERIME`,
    /// ANNULE... ne sont pas dans le glossaire des instructions).
    #[test]
    fn the_etat_field_carries_the_general_rule() {
        let schema = serde_json::Value::Object(output_schema::<ArticleView>().as_ref().clone());
        let text = schema.to_string();
        assert!(text.contains(crate::views::common::ETAT_GLOSSARY), "{text}");
    }
    use lf_application::{
        budget::{PassageOut, Reading, ResponseBudget},
        output::get_text::GetTextOutput,
    };
    use lf_domain::{
        date::CivilDate,
        entity::{ArticleText, ParentText},
        id::{ArticleId, DocumentId},
        status::LegalStatus,
        text::{LongText, Summary, SummarySource},
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    const ARTICLE: &str = "LEGIARTI000006419288";
    const TEXT: &str = "Chacun a droit au respect de sa vie privée. Les juges peuvent, sans préjudice de la réparation du dommage subi, prescrire toutes mesures, telles que séquestre, saisie et autres, propres à empêcher ou faire cesser une atteinte à l'intimité de la vie privée : ces mesures peuvent, s'il y a urgence, être ordonnées en référé.";

    fn day(year: u16, month: u8, day: u8) -> CivilDate {
        CivilDate::new(year, month, day).unwrap()
    }

    /// L'article 9 du Code civil de l'exemple de 05 §5.3, sans son texte (dans la lecture).
    fn article() -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(ARTICLE).unwrap(),
            num: Some("9".to_owned()),
            etat: LegalStatus::parse("VIGUEUR"),
            code_title: Some("Code civil".to_owned()),
            parent: Some(ParentText::new(
                DocumentId::parse("LEGITEXT000006070721").unwrap(),
                Some("CODE"),
            )),
            path: vec![
                "Livre Ier : Des personnes".to_owned(),
                "Titre Ier : Des droits civils".to_owned(),
            ],
            full_sections_titre: None,
            date_debut: Some(day(1970, 7, 19)),
            // Sentinelle de l'API : « en vigueur sans date de fin », donc absente de la vue.
            date_fin: Some(day(2999, 1, 1)),
            text: LongText::default(),
            nota: None,
        }
    }

    fn whole(text: &str) -> Reading {
        Reading {
            summary: None,
            text: Some(text.to_owned()),
            text_length: u32::try_from(text.chars().count()).unwrap(),
            next_offset: None,
            passages: None,
            note: None,
            summary_truncated: false,
        }
    }

    fn output(article: ArticleText, reading: Reading) -> GetTextOutput {
        GetTextOutput { article, nota: None, reading }
    }

    fn view_json(output: GetTextOutput) -> Value {
        serde_json::to_value(ArticleView::from(output)).unwrap()
    }

    #[test]
    fn article_view_matches_the_example_of_the_spec() {
        assert_eq!(
            view_json(output(article(), whole(TEXT))),
            json!({
                "article_id": ARTICLE,
                "num": "9",
                "etat": "VIGUEUR",
                "code_title": "Code civil",
                "parent_id": "LEGITEXT000006070721",
                "path": ["Livre Ier : Des personnes", "Titre Ier : Des droits civils"],
                "date_debut": "1970-07-19",
                "citation": "Code civil, art. 9",
                "source_url": "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288",
                "text": TEXT,
                "text_length": 322,
            })
        );
    }

    #[test]
    fn article_view_without_optional_data_keeps_only_the_contract_and_the_length() {
        let bare = ArticleText {
            num: None,
            etat: None,
            code_title: None,
            parent: None,
            path: Vec::new(),
            date_debut: None,
            date_fin: None,
            ..article()
        };

        assert_eq!(
            view_json(output(bare, whole(""))),
            json!({
                "article_id": ARTICLE,
                "citation": "Légifrance LEGIARTI000006419288",
                "source_url": "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288",
                "text": "",
                "text_length": 0,
            })
        );
    }

    #[test]
    fn a_kali_article_points_to_its_convention_and_the_conv_coll_page() {
        let kali = ArticleText {
            article_id: ArticleId::parse("KALIARTI000005805295").unwrap(),
            parent: Some(ParentText::new(DocumentId::parse("KALITEXT000005635534").unwrap(), None)),
            code_title: Some("Convention collective nationale de la métallurgie".to_owned()),
            ..article()
        };

        let view = view_json(output(kali, whole("Texte.")));

        assert_eq!(view["article_id"], "KALIARTI000005805295");
        assert_eq!(view["parent_id"], "KALITEXT000005635534");
        assert_eq!(
            view["source_url"],
            "https://www.legifrance.gouv.fr/conv_coll/id/KALIARTI000005805295"
        );
    }

    #[test]
    fn parent_id_is_only_rendered_for_a_text_fr_get_act_or_fr_get_convention_reads() {
        let parent = |id: &str, nature: &str| ArticleText {
            parent: Some(ParentText::new(DocumentId::parse(id).unwrap(), Some(nature))),
            ..article()
        };

        // Un texte de loi : l'identifiant consolidé est rendu.
        let law = view_json(output(parent("LEGITEXT000033205014", "LOI"), whole("Texte.")));
        assert_eq!(law["parent_id"], "LEGITEXT000033205014");

        // Un JORFTEXT n'est lu par aucun outil : la clé est omise.
        let jorf = view_json(output(parent("JORFTEXT000033202746", "LOI"), whole("Texte.")));
        assert!(jorf.get("parent_id").is_none(), "{jorf}");
        // Le reste de la vue est inchangé.
        assert_eq!(jorf["code_title"], "Code civil");
        assert_eq!(jorf["citation"], "Code civil, art. 9");
    }

    #[test]
    fn the_nota_is_rendered_after_the_dates_and_before_the_citation() {
        let mut with_nota = output(article(), whole("Texte."));
        with_nota.nota = Some("Version issue de la loi n° 2016-1321.".to_owned());

        let view = serde_json::to_string(&ArticleView::from(with_nota)).unwrap();

        let position = |key: &str| view.find(&format!("\"{key}\":")).expect(key);
        assert!(position("date_debut") < position("nota"));
        assert!(position("nota") < position("citation"));
        assert!(view.contains("\"nota\":\"Version issue de la loi n° 2016-1321.\""));
    }

    #[test]
    fn dates_in_the_sentinel_zone_are_omitted() {
        let mut dated = article();
        dated.date_debut = Some(day(2222, 2, 22));
        dated.date_fin = Some(day(2999, 1, 1));

        let view = view_json(output(dated, whole("Texte.")));
        assert!(view.get("date_debut").is_none() && view.get("date_fin").is_none());

        // Une vraie date de fin reste rendue : l'article est abrogé à cette date.
        let mut ended = article();
        ended.date_fin = Some(day(2023, 9, 7));
        assert_eq!(view_json(output(ended, whole("Texte.")))["date_fin"], "2023-09-07");
    }

    const NOTE_1382: &str = "Not the version in force (etat=MODIFIE, ended 2016-10-01). Use fr_resolve with a date, or fr_diff_article, for the text in force.";

    fn with_etat(code: &str, date_fin: Option<CivilDate>) -> ArticleText {
        ArticleText { etat: LegalStatus::parse(code), date_fin, ..article() }
    }

    /// L3-13 (get_text-11) : l'ancien article 1382 (`etat=MODIFIE`, fin 2016-10-01) porte l'alerte.
    #[test]
    fn an_ended_version_says_it_is_not_the_version_in_force() {
        let old = ArticleText {
            article_id: ArticleId::parse("LEGIARTI000006438819").unwrap(),
            ..with_etat("MODIFIE", Some(day(2016, 10, 1)))
        };

        let view = view_json(output(old, whole("Tout fait quelconque de l'homme...")));

        assert_eq!(view["note"], NOTE_1382);
        assert_eq!(view["etat"], "MODIFIE");
        assert_eq!(view["date_fin"], "2016-10-01");
        assert_eq!(view["text"], "Tout fait quelconque de l'homme...");
    }

    #[test]
    fn every_ended_etat_carries_the_warning_and_the_others_do_not() {
        let ended = [
            "MODIFIE",
            "MODIFIE_MORT_NE",
            "ABROGE",
            "PERIME",
            "ANNULE",
            "DISJOINT",
            "REMPLACE",
            "SUBSTITUE",
            "TRANSFERE",
        ];
        for code in ended {
            let view = view_json(output(with_etat(code, Some(day(2014, 3, 27))), whole("T.")));
            let expected = format!(
                "Not the version in force (etat={code}, ended 2014-03-27). Use fr_resolve with a date, or fr_diff_article, for the text in force."
            );
            assert_eq!(view["note"], expected, "{code}");
        }
        let current = [
            "VIGUEUR",
            "VIGUEUR_ETEN",
            "VIGUEUR_NON_ETEN",
            "VIGUEUR_DIFF",
            "ABROGE_DIFF",
            "INITIALE",
            "SANS_ETAT",
            "EN_COURS",
        ];
        for code in current {
            let view = view_json(output(with_etat(code, Some(day(2014, 3, 27))), whole("T.")));
            assert!(view.get("note").is_none(), "{code}: {view}");
        }
        // Sans `etat` : aucune note.
        let none = ArticleText { etat: None, ..article() };
        assert!(view_json(output(none, whole("T."))).get("note").is_none());
    }

    /// Second tour de la revue finale : une convention dénoncée peut encore s'appliquer pendant le
    /// préavis et la survie (glossaire de `etat`) ; l'avertissement ne dit donc pas « Not the version
    /// in force » pour `DENONCE`, mais l'état reste signalé, avec ou sans date de fin.
    #[test]
    fn a_denounced_text_is_flagged_without_saying_it_is_out_of_force() {
        let view = view_json(output(with_etat("DENONCE", Some(day(2014, 3, 27))), whole("T.")));
        assert_eq!(
            view["note"],
            "Denounced (etat=DENONCE, ended 2014-03-27); it may still apply during notice and \
             survival. Use fr_resolve with a date, or fr_diff_article, to compare versions."
        );
        let view = view_json(output(with_etat("DENONCE", None), whole("T.")));
        let note = view["note"].as_str().unwrap();
        assert!(note.starts_with("Denounced (etat=DENONCE); it may still apply"), "{note}");
        assert!(!note.contains("Not the version in force"), "{note}");
    }

    #[test]
    fn the_warning_omits_the_end_date_when_there_is_none_or_a_sentinel() {
        let expected = "Not the version in force (etat=ABROGE). Use fr_resolve with a date, or fr_diff_article, for the text in force.";
        for date_fin in [None, Some(day(2999, 1, 1))] {
            let view = view_json(output(with_etat("ABROGE", date_fin), whole("T.")));
            assert_eq!(view["note"], expected, "{date_fin:?}");
        }
    }

    #[test]
    fn the_warning_follows_the_existing_reading_note_after_a_space() {
        let reading = Reading { note: Some("Reading note.".to_owned()), ..whole("T.") };

        let ended =
            view_json(output(with_etat("MODIFIE", Some(day(2016, 10, 1))), reading.clone()));
        assert_eq!(ended["note"], format!("Reading note. {NOTE_1382}"));

        // Version en vigueur : la note de lecture reste seule, inchangée.
        let current = view_json(output(article(), reading));
        assert_eq!(current["note"], "Reading note.");
    }

    #[test]
    fn a_kali_article_that_is_ended_carries_the_warning() {
        let kali = ArticleText {
            article_id: ArticleId::parse("KALIARTI000005805295").unwrap(),
            ..with_etat("ABROGE", Some(day(2016, 8, 1)))
        };

        let view = view_json(output(kali, whole("Texte.")));

        assert_eq!(
            view["note"],
            format!(
                "Not the version in force (etat=ABROGE, ended 2016-08-01). {KALI_ENDED_ADVICE}"
            )
        );
    }

    // ---- U15 : la note d'un article de convention ne renvoie pas vers des outils qui ne le lisent pas.
    // Les textes attendus sont écrits en toutes lettres : le test fige les octets, pas une constante.

    const KALI_ENDED_ADVICE: &str = "fr_resolve and fr_diff_article cannot give another version of a convention article: call fr_get_convention with parent_id (if absent, fr_search on fond KALI) and look in its toc (toc_offset pages it) for the entry with the same num in the same section; an entry shown without etat has the etat of the convention itself, and there may be no current version.";
    const KALI_DENOUNCED_ADVICE: &str = "fr_resolve and fr_diff_article cannot compare versions of a convention article: fr_get_convention with parent_id gives the etat of the convention itself.";
    const ENDED_ETATS: [&str; 9] = [
        "MODIFIE",
        "MODIFIE_MORT_NE",
        "ABROGE",
        "PERIME",
        "ANNULE",
        "DISJOINT",
        "REMPLACE",
        "SUBSTITUE",
        "TRANSFERE",
    ];

    /// Syntec, art. 7 (`KALIARTI000044253031`), sous la convention `KALITEXT000044253019`.
    fn kali_with_etat(code: &str, date_fin: Option<CivilDate>) -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse("KALIARTI000044253031").unwrap(),
            num: Some("7".to_owned()),
            parent: Some(ParentText::new(DocumentId::parse("KALITEXT000044253019").unwrap(), None)),
            code_title: None,
            ..with_etat(code, date_fin)
        }
    }

    fn note_of(article: ArticleText) -> String {
        let view = view_json(output(article, whole("T.")));
        view["note"].as_str().expect("pas de note").to_owned()
    }

    /// Syntec art. 7 (c0798) : fin de phrase propre aux conventions, début inchangé.
    #[test]
    fn a_superseded_kali_article_points_to_its_convention_toc() {
        let note = note_of(kali_with_etat("REMPLACE", Some(day(2023, 5, 1))));

        assert_eq!(
            note,
            format!(
                "Not the version in force (etat=REMPLACE, ended 2023-05-01). {KALI_ENDED_ADVICE}"
            )
        );
        assert!(!note.contains("Use fr_resolve"), "{note}");
    }

    #[test]
    fn every_ended_etat_on_a_kali_article_gets_the_convention_advice() {
        for code in ENDED_ETATS {
            let note = note_of(kali_with_etat(code, Some(day(2014, 3, 27))));
            assert_eq!(
                note,
                format!(
                    "Not the version in force (etat={code}, ended 2014-03-27). {KALI_ENDED_ADVICE}"
                ),
                "{code}"
            );
            assert!(!note.contains("Use fr_resolve"), "{code}: {note}");
        }
        // Sans date de fin ou avec la sentinelle : la date n'est pas citée, le conseil est le même.
        for date_fin in [None, Some(day(2999, 1, 1))] {
            assert_eq!(
                note_of(kali_with_etat("ABROGE", date_fin)),
                format!("Not the version in force (etat=ABROGE). {KALI_ENDED_ADVICE}"),
                "{date_fin:?}"
            );
        }
    }

    #[test]
    fn a_denounced_kali_article_keeps_its_denounced_wording_with_the_convention_advice() {
        // KALIARTI000005803334 : réel en production (c1986), fin 2000-01-14.
        let denounced = |date_fin| ArticleText {
            article_id: ArticleId::parse("KALIARTI000005803334").unwrap(),
            parent: Some(ParentText::new(DocumentId::parse("KALITEXT000005660172").unwrap(), None)),
            ..kali_with_etat("DENONCE", date_fin)
        };

        assert_eq!(
            note_of(denounced(Some(day(2000, 1, 14)))),
            format!(
                "Denounced (etat=DENONCE, ended 2000-01-14); it may still apply during notice and \
                 survival. {KALI_DENOUNCED_ADVICE}"
            )
        );
        let without = note_of(denounced(None));
        assert!(without.starts_with("Denounced (etat=DENONCE); it may still apply"), "{without}");
        assert!(without.ends_with(KALI_DENOUNCED_ADVICE), "{without}");
        assert!(!without.contains("Not the version in force"), "{without}");
    }

    #[test]
    fn a_denounced_kali_article_does_not_send_the_reader_looking_for_another_version() {
        let note = note_of(kali_with_etat("DENONCE", Some(day(2000, 1, 14))));

        assert!(!note.contains("to compare versions"), "{note}");
        assert!(!note.contains("Use fr_resolve"), "{note}");
        assert!(!note.contains("for the text in force"), "{note}");
    }

    #[test]
    fn a_denounced_kali_article_points_to_the_convention_etat_not_to_a_same_num() {
        let note = note_of(kali_with_etat("DENONCE", Some(day(2000, 1, 14))));

        assert!(note.contains("fr_get_convention with parent_id gives the etat"), "{note}");
        assert!(!note.contains("same num"), "{note}");
        assert!(!note.contains("toc"), "{note}");
    }

    /// KALIARTI000033860231 : préambule sans `num`, sous un texte en vigueur (KALITEXT000033860222).
    #[test]
    fn an_ended_kali_article_without_num_under_a_living_text_says_there_may_be_no_current_version()
    {
        let preamble = ArticleText {
            article_id: ArticleId::parse("KALIARTI000033860231").unwrap(),
            num: None,
            parent: Some(ParentText::new(DocumentId::parse("KALITEXT000033860222").unwrap(), None)),
            ..kali_with_etat("ABROGE", Some(day(2019, 1, 1)))
        };

        let note = note_of(preamble);

        assert!(note.starts_with("Not the version in force (etat=ABROGE, ended 2019-01-01). "));
        assert!(note.ends_with("and there may be no current version."), "{note}");
    }

    #[test]
    fn the_kali_advice_follows_the_reading_note_after_a_space() {
        let reading = Reading { note: Some("Reading note.".to_owned()), ..whole("T.") };

        let ended =
            view_json(output(kali_with_etat("REMPLACE", Some(day(2023, 5, 1))), reading.clone()));
        assert_eq!(
            ended["note"],
            format!(
                "Reading note. Not the version in force (etat=REMPLACE, ended 2023-05-01). \
                 {KALI_ENDED_ADVICE}"
            )
        );
        // Version en vigueur : la note de lecture reste seule, inchangée.
        let current = view_json(output(kali_with_etat("VIGUEUR_ETEN", None), reading));
        assert_eq!(current["note"], "Reading note.");
    }

    /// `fr_resolve` lit un KALIARTI par identifiant (il rend l'état et la fin) : la note ne dit pas
    /// le contraire.
    #[test]
    fn the_kali_advice_does_not_claim_fr_resolve_cannot_read_a_kaliarti() {
        for code in ["REMPLACE", "ABROGE", "DENONCE"] {
            let note = note_of(kali_with_etat(code, Some(day(2023, 5, 1))));
            assert!(!note.contains("do not read"), "{code}: {note}");
            assert!(!note.contains("cannot read"), "{code}: {note}");
            assert!(!note.contains("does not read"), "{code}: {note}");
        }
    }

    #[test]
    fn a_kali_note_without_parent_id_names_the_search_fallback() {
        let orphan =
            ArticleText { parent: None, ..kali_with_etat("REMPLACE", Some(day(2023, 5, 1))) };

        let view = view_json(output(orphan, whole("T.")));

        assert!(view.get("parent_id").is_none(), "{view}");
        let note = view["note"].as_str().unwrap();
        assert!(note.contains("(if absent, fr_search on fond KALI)"), "{note}");
    }

    #[test]
    fn a_kali_article_with_an_unreadable_or_absent_parent_gets_the_same_note_and_no_parent_id() {
        let expected = format!(
            "Not the version in force (etat=REMPLACE, ended 2023-05-01). {KALI_ENDED_ADVICE}"
        );
        let jorf = Some(ParentText::new(DocumentId::parse("JORFTEXT000033202746").unwrap(), None));
        for parent in [None, jorf] {
            let article =
                ArticleText { parent, ..kali_with_etat("REMPLACE", Some(day(2023, 5, 1))) };
            let view = view_json(output(article, whole("T.")));
            assert!(view.get("parent_id").is_none(), "{view}");
            assert_eq!(view["note"], expected);
        }
    }

    #[test]
    fn the_kali_note_ignores_a_start_date_equal_to_the_end_date() {
        let same_day = ArticleText {
            date_debut: Some(day(2023, 5, 1)),
            ..kali_with_etat("REMPLACE", Some(day(2023, 5, 1)))
        };

        assert_eq!(
            note_of(same_day),
            format!(
                "Not the version in force (etat=REMPLACE, ended 2023-05-01). {KALI_ENDED_ADVICE}"
            )
        );
    }

    /// Un `match` exhaustif : un nouveau code de `LegalStatus` oblige à choisir ici.
    const fn status_index(status: &LegalStatus) -> usize {
        match status {
            LegalStatus::Vigueur => 0,
            LegalStatus::VigueurEtendue => 1,
            LegalStatus::VigueurNonEtendue => 2,
            LegalStatus::VigueurDifferee => 3,
            LegalStatus::Abroge => 4,
            LegalStatus::AbrogeDiff => 5,
            LegalStatus::Modifie => 6,
            LegalStatus::ModifieMortNe => 7,
            LegalStatus::Perime => 8,
            LegalStatus::Annule => 9,
            LegalStatus::Disjoint => 10,
            LegalStatus::Transfere => 11,
            LegalStatus::Remplace => 12,
            LegalStatus::Denonce => 13,
            LegalStatus::Initiale => 14,
            LegalStatus::SansEtat => 15,
            LegalStatus::Substitue => 16,
            LegalStatus::Other(_) => 17,
        }
    }

    #[test]
    fn superseded_status_x_family_note_shape() {
        let statuses = [
            LegalStatus::Vigueur,
            LegalStatus::VigueurEtendue,
            LegalStatus::VigueurNonEtendue,
            LegalStatus::VigueurDifferee,
            LegalStatus::Abroge,
            LegalStatus::AbrogeDiff,
            LegalStatus::Modifie,
            LegalStatus::ModifieMortNe,
            LegalStatus::Perime,
            LegalStatus::Annule,
            LegalStatus::Disjoint,
            LegalStatus::Transfere,
            LegalStatus::Remplace,
            LegalStatus::Denonce,
            LegalStatus::Initiale,
            LegalStatus::SansEtat,
            LegalStatus::Substitue,
            LegalStatus::Other("NOUVEAU_CODE".to_owned()),
        ];
        for (index, status) in statuses.iter().enumerate() {
            assert_eq!(status_index(status), index, "{status:?}");
            let code = status.as_api_code();
            for (family_id, is_kali) in
                [("LEGIARTI000006419288", false), ("KALIARTI000044253031", true)]
            {
                let article = ArticleText {
                    article_id: ArticleId::parse(family_id).unwrap(),
                    ..with_etat(code, Some(day(2014, 3, 27)))
                };
                let view = view_json(output(article, whole("T.")));
                let label = format!("{code} {family_id}");
                if !status.is_superseded() {
                    assert!(view.get("note").is_none(), "{label}: {view}");
                    continue;
                }
                let note = view["note"].as_str().unwrap();
                let start = if matches!(status, LegalStatus::Denonce) {
                    "Denounced (etat=DENONCE, ended 2014-03-27); it may still apply"
                } else {
                    "Not the version in force (etat="
                };
                assert!(note.starts_with(start), "{label}: {note}");
                // Un seul conseil, de la bonne famille.
                let legi_advice = note.contains("Use fr_resolve with a date");
                let kali_advice = note.contains("fr_get_convention");
                assert_eq!((legi_advice, kali_advice), (!is_kali, is_kali), "{label}: {note}");
            }
        }
    }

    // ---- Gardes : le texte des articles de code ne bouge pas (garantie 14 : octet pour octet).

    #[test]
    fn a_legi_article_keeps_the_resolve_and_diff_advice() {
        for code in ENDED_ETATS {
            assert_eq!(
                note_of(with_etat(code, Some(day(2014, 3, 27)))),
                format!(
                    "Not the version in force (etat={code}, ended 2014-03-27). Use fr_resolve with a date, or fr_diff_article, for the text in force."
                ),
                "{code}"
            );
        }
        assert_eq!(
            note_of(with_etat("DENONCE", Some(day(2014, 3, 27)))),
            "Denounced (etat=DENONCE, ended 2014-03-27); it may still apply during notice and survival. Use fr_resolve with a date, or fr_diff_article, to compare versions."
        );
    }

    /// Littéraux de la 1.1.0, 10 états x {date réelle, aucune, sentinelle}.
    #[test]
    fn every_ended_legi_note_is_byte_identical_to_1_1_0() {
        let ten = ENDED_ETATS.iter().copied().chain(["DENONCE"]);
        for code in ten {
            for (date_fin, since) in [
                (Some(day(2016, 10, 1)), ", ended 2016-10-01"),
                (None, ""),
                (Some(day(2999, 1, 1)), ""),
            ] {
                let expected = if code == "DENONCE" {
                    format!(
                        "Denounced (etat=DENONCE{since}); it may still apply during notice and survival. Use fr_resolve with a date, or fr_diff_article, to compare versions."
                    )
                } else {
                    format!(
                        "Not the version in force (etat={code}{since}). Use fr_resolve with a date, or fr_diff_article, for the text in force."
                    )
                };
                assert_eq!(note_of(with_etat(code, date_fin)), expected, "{code} {date_fin:?}");
            }
        }
    }

    #[test]
    fn a_current_kali_article_carries_no_version_note() {
        for code in [
            "VIGUEUR",
            "VIGUEUR_ETEN",
            "VIGUEUR_NON_ETEN",
            "VIGUEUR_DIFF",
            "ABROGE_DIFF",
            "INITIALE",
            "SANS_ETAT",
            "CODE_INCONNU",
        ] {
            let view = view_json(output(kali_with_etat(code, Some(day(2999, 1, 1))), whole("T.")));
            assert!(view.get("note").is_none(), "{code}: {view}");
        }
        let none = ArticleText { etat: None, ..kali_with_etat("VIGUEUR", None) };
        assert!(view_json(output(none, whole("T."))).get("note").is_none());
    }

    #[test]
    fn a_kali_article_without_parent_still_gets_the_warning() {
        let orphan =
            ArticleText { parent: None, ..kali_with_etat("REMPLACE", Some(day(2023, 5, 1))) };

        let view = view_json(output(orphan, whole("T.")));

        assert!(view.get("parent_id").is_none(), "{view}");
        let note = view["note"].as_str().unwrap();
        assert!(note.starts_with("Not the version in force (etat=REMPLACE, ended 2023-05-01)."));
    }

    #[test]
    fn a_long_kali_note_still_fits_under_the_response_cap() {
        let long = "Texte de l'article. ".repeat(2_000);
        let reading = Reading { text_length: 40_000, ..whole(&long) };

        let view = view_json(output(kali_with_etat("REMPLACE", Some(day(2023, 5, 1))), reading));

        let size = view.to_string().chars().count();
        assert!(size <= ResponseBudget::HARD_CAP_CHARS, "{size}");
        // La note n'est jamais amputée : la dégradation touche la lecture seulement.
        let note = view["note"].as_str().unwrap();
        assert!(note.starts_with("Not the version in force (etat=REMPLACE, ended 2023-05-01)."));
        assert!(note.ends_with('.'), "{note}");
    }

    /// Une fenêtre riche en échappements qui tient pour un article de code garde la même taille pour
    /// un article de convention : la note plus longue ne la raccourcit pas.
    #[test]
    fn the_longer_kali_note_does_not_shorten_an_escape_heavy_window_that_a_legi_article_keeps() {
        let heavy = "\"\\😀 ".repeat(5_000);
        let legi = view_json(output(with_etat("REMPLACE", Some(day(2023, 5, 1))), whole(&heavy)));
        let kali =
            view_json(output(kali_with_etat("REMPLACE", Some(day(2023, 5, 1))), whole(&heavy)));

        assert!(kali.to_string().chars().count() <= ResponseBudget::HARD_CAP_CHARS);
        assert_eq!(kali["text"], legi["text"]);
        assert_eq!(kali.get("next_offset"), legi.get("next_offset"));
    }

    /// Garde de dérive : le conseil ne nomme que des outils qui acceptent un KALIARTI pour ce qu'il
    /// promet. Si `fr_diff_article` acceptait un jour les KALIARTI, ce test force à relire le texte.
    #[test]
    fn the_kali_advice_names_only_tools_that_accept_a_kaliarti() {
        assert!(lf_domain::id::LegiArtiId::parse("KALIARTI000044253031").is_err());
        assert!(lf_domain::id::LegiArtiId::parse("LEGIARTI000006438819").is_ok());
        assert!(KALI_ENDED_ADVICE.contains("fr_get_convention"));
        assert!(KALI_DENOUNCED_ADVICE.contains("fr_get_convention"));
    }

    #[test]
    fn a_summary_comes_with_its_source_and_the_length_of_the_whole_text() {
        let reading = Reading {
            summary: Some(Summary {
                source: SummarySource::Lead,
                text: "Début.".to_owned(),
                truncated: true,
            }),
            text: None,
            text_length: 5_000,
            ..whole("")
        };

        let view = view_json(output(article(), reading));

        assert_eq!(view["summary"], "Début.");
        assert_eq!(view["summary_source"], "lead");
        assert_eq!(view["text_length"], 5_000);
        assert!(view.get("text").is_none());
    }

    #[test]
    fn serialization_writes_the_metadata_then_the_reading() {
        let reading = Reading {
            passages: Some(vec![PassageOut { offset: 0, text: "p".to_owned(), score: 1.0 }]),
            text: None,
            ..whole("")
        };
        let written =
            serde_json::to_string(&ArticleView::from(output(article(), reading))).unwrap();

        assert_eq!(
            written,
            concat!(
                r#"{"article_id":"LEGIARTI000006419288","num":"9","etat":"VIGUEUR","#,
                r#""code_title":"Code civil","parent_id":"LEGITEXT000006070721","#,
                r#""path":["Livre Ier : Des personnes","Titre Ier : Des droits civils"],"#,
                r#""date_debut":"1970-07-19","citation":"Code civil, art. 9","#,
                r#""source_url":"https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288","#,
                r#""text_length":0,"passages":[{"offset":0,"text":"p","score":1.0}]}"#
            )
        );
    }

    #[test]
    fn an_escape_rich_window_is_shortened_under_the_hard_cap_with_an_exact_next_offset() {
        // Guillemets, antislashs et emoji hors BMP : 6 caractères de JSON par groupe de 4.
        let heavy = "\"\\😀 ".repeat(10_000);
        let reading = whole(&heavy);

        let view = view_json(output(article(), reading));

        let size = view.to_string().chars().count();
        assert!(size <= ResponseBudget::HARD_CAP_CHARS, "{size}");
        let kept = view["text"].as_str().unwrap();
        assert!(heavy.starts_with(kept) && kept.chars().count() < heavy.chars().count());
        // La fenêtre raccourcie finit où la page suivante commence.
        assert_eq!(view["next_offset"], kept.chars().count());
        assert_eq!(view["text_length"], 40_000);
    }

    #[test]
    fn the_view_validates_against_its_output_schema() {
        let schema = Value::Object(output_schema::<ArticleView>().as_ref().clone());
        let validator = jsonschema::validator_for(&schema).unwrap();

        assert!(validator.is_valid(&view_json(output(article(), whole(TEXT)))));
        let summary = Reading {
            summary: Some(Summary {
                source: SummarySource::Lead,
                text: "Début.".to_owned(),
                truncated: true,
            }),
            text: None,
            ..whole("")
        };
        assert!(validator.is_valid(&view_json(output(article(), summary))));
        let mut broken = view_json(output(article(), whole(TEXT)));
        broken.as_object_mut().unwrap().remove("citation");
        assert!(!validator.is_valid(&broken));
    }

    #[test]
    fn the_output_schema_merges_the_reading_fields_and_has_no_defs_or_dollar_schema() {
        let schema = Value::Object(output_schema::<ArticleView>().as_ref().clone());

        assert!(schema.get("$schema").is_none());
        assert!(schema.get("$defs").is_none(), "{schema}");
        let properties = schema["properties"].as_object().unwrap();
        let mut names: Vec<&str> = properties.keys().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "article_id",
                "citation",
                "code_title",
                "date_debut",
                "date_fin",
                "eli",
                "etat",
                "next_offset",
                "nota",
                "note",
                "num",
                "parent_id",
                "passages",
                "path",
                "source_url",
                "summary",
                "summary_source",
                "summary_truncated",
                "text",
                "text_length",
            ]
        );
        let mut required: Vec<&str> =
            schema["required"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        required.sort_unstable();
        assert_eq!(required, ["article_id", "citation", "source_url", "text_length"]);
        // Les champs absents sont omis, jamais `null` : les types du schéma sont simples.
        assert!(!schema.to_string().contains("null"));
    }

    #[test]
    fn output_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        let schema = Value::Object(output_schema::<ArticleView>().as_ref().clone());
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("output_schema", schema);
        });
    }
}
