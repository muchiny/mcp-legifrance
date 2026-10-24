//! Réponse de `/consult/juri` et de `/consult/cnil` (04 §8.3, recherche 03 §8, §12.6).
//!
//! Les deux endpoints renvoient la même forme, enveloppée : `{executionTime, dereferenced,
//! text}`. Seule la clé `text` est déclarée. Les structs suivent la convention de
//! `consult_text` : `Default` dérivé, `#[serde(default)]` au niveau du conteneur, `Debug` et
//! `PartialEq` sous `cfg(test)` seulement (elles portent du texte de décision), dates brutes
//! (`RawDate`), texte et casse gardés tels quels (`ARRET`, `QPC`, `Texte`, `Délibération`).
//! `decisionAttaquee`, souvent `null`, est lue comme une struct vide : « absent » et « vide » se
//! confondent. Le mapping ne la lit pas (D30, renvoyée à M7) ; un test vérifie seulement qu'elle ne
//! change pas la décision (`attacked_decision_does_not_change_the_decision`, `map/decision.rs`).
//!
//! Ce module ne partage aucun type avec `consult_text` : le même nom de champ y a des types
//! différents selon l'endpoint (les signataires sont un objet en KALI et une chaîne en JURI).
//! Ils ne sont déclarés ni ici ni là, et une garde de test vérifie que les deux fichiers restent
//! indépendants.
//!
//! Les personnes (président, avocats, rapporteur, commissaire…) ne sont jamais déclarées : la
//! loi interdit d'évaluer les pratiques des magistrats à partir de leur identité (recherche 05
//! §5.1), et ne pas les lire garantit qu'elles ne laissent aucune trace, même de type imprévu.
//! Les autres champs que le mapping n'emploie pas sont sautés par `serde_json` sans construire
//! de valeur (mémoire bornée par la taille du corps).

use serde::Deserialize;

use super::de::{self, RawDate};

/// Enveloppe de la réponse de `/consult/juri` et de `/consult/cnil`.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ConsultJuriResponseDto {
    /// Décision demandée ; `None` si la clé est absente ou `null`.
    pub text: Option<JuriTextDto>,
}

/// Décision de justice (JURI, CONSTIT, CETAT) ou délibération de la CNIL.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct JuriTextDto {
    /// Identifiant de la décision.
    pub id: Option<String>,
    /// Fonds d'origine (`JURI`, `CONSTIT`, `CETAT`, `CNIL`).
    pub origine: Option<String>,
    /// Nature de la décision, casse conservée.
    pub nature: Option<String>,
    /// Titre de la décision.
    pub titre: Option<String>,
    /// Titre long de la décision.
    pub titre_long: Option<String>,
    /// Juridiction.
    pub juridiction: Option<String>,
    /// Formation (code ou texte libre selon le fonds).
    pub formation: Option<String>,
    /// Solution (`Cassation`, `Conformité`…).
    pub solution: Option<String>,
    /// Texte brut, gardé tel quel.
    pub texte: Option<String>,
    /// Texte en HTML, gardé tel quel.
    pub texte_html: Option<String>,
    /// Nature qualifiée d'une décision du Conseil constitutionnel.
    pub nature_qualifiee: Option<String>,
    /// Nature d'une délibération de la CNIL (`Sanction`…).
    pub nature_delib: Option<String>,
    /// État juridique (délibérations de la CNIL).
    pub etat: Option<String>,
    /// Numéro de la décision, en chaîne ou en nombre, rendu en chaîne (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_string_or_number")]
    pub num: Option<String>,
    /// Identifiant européen de jurisprudence (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub ecli: Option<String>,
    /// Jurisprudence citée, en prose (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub citation_jp: Option<String>,
    /// Niveau de publication au recueil (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub publication_recueil: Option<String>,
    /// Niveau de publication au bulletin (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub type_publication_bulletin: Option<String>,
    /// Titre de la loi déférée (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub titre_loi_def: Option<String>,
    /// Identifiant du texte au Journal officiel (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub id_texte_jo: Option<String>,
    /// Référence au Journal officiel (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub titre_jo: Option<String>,
    /// Ancien identifiant de la décision (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub ancien_id: Option<String>,
    /// Numéros de pourvoi ou d'affaire.
    #[serde(deserialize_with = "de::bounded_list")]
    pub numero_affaire: Vec<String>,
    /// Plan de classement de la décision.
    #[serde(deserialize_with = "de::bounded_list")]
    pub titrages: Vec<String>,
    /// Résumés officiels.
    #[serde(deserialize_with = "de::bounded_list")]
    pub sommaire: Vec<SommaireDto>,
    /// Textes appliqués ou cités.
    #[serde(deserialize_with = "de::bounded_list")]
    pub liens: Vec<TexteLienDto>,
    /// Date du texte, brute.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_texte: Option<RawDate>,
    /// Date du texte si elle est valide, brute (à lire en priorité).
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_texte_computed: Option<RawDate>,
    /// Date de publication, brute.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_publi: Option<RawDate>,
    /// Date de publication si elle est valide, brute.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_publi_computed: Option<RawDate>,
    /// Décision attaquée (`null` : valeur vide).
    #[serde(deserialize_with = "de::null_default")]
    pub decision_attaquee: DecisionAttaqueeDto,
}

/// Résumé officiel d'une décision.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct SommaireDto {
    /// Identifiant du résumé.
    pub id: Option<String>,
    /// Résumé principal (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub resume_principal: Option<String>,
    /// Mots-clés de classement (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub abstrats: Option<String>,
    /// Autre résumé (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub autre_resume: Option<String>,
}

/// Lien d'une décision vers un texte appliqué.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct TexteLienDto {
    /// Type du lien (`CITATION`…).
    pub type_lien: Option<String>,
    /// Sens du lien (`source`, `cible`).
    pub sens: Option<String>,
    /// Titre du texte lié.
    pub title: Option<String>,
    /// Identifiant chronologique du texte lié (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub cid_texte: Option<String>,
    /// Identifiant du lien (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub id: Option<String>,
    /// Numéro du texte lié, en chaîne ou en nombre, rendu en chaîne (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_string_or_number")]
    pub num: Option<String>,
    /// Nature du texte lié (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub nature_texte: Option<String>,
}

/// Décision attaquée par une décision de justice.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct DecisionAttaqueeDto {
    /// Formation ou juridiction de la décision attaquée.
    pub formation: Option<String>,
    /// Date de la décision attaquée, brute.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date: Option<RawDate>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::consult_text::ConsultTextDto;
    use crate::dto::decode::decode;
    use crate::dto::tests::{assert_too_many, repeated};

    const JURI: &str = "/consult/juri";
    const CNIL: &str = "/consult/cnil";

    /// `Some(valeur.to_owned())` : une macro et non une fonction, que `clippy::unnecessary_wraps`
    /// refuserait.
    macro_rules! text {
        ($value:expr) => {
            Some($value.to_owned())
        };
    }

    /// Décision de la Cour de cassation (recherche 03 §8.1), avec les champs jamais déclarés.
    const JURI_BODY: &str = r#"{"executionTime":9,"dereferenced":false,"text":{
        "id":"JURITEXT000051743650","cid":null,"origine":"JURI","nature":"ARRET","num":"12500399",
        "numeroAffaire":["24-13.470"],"ecli":"ECLI:FR:CCASS:2025:C100399",
        "juridiction":"Cour de cassation","formation":"CHAMBRE_CIVILE_1","solution":"Cassation",
        "titre":"Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470",
        "titreLong":"Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470, Publié",
        "texte":"Sur le moyen unique","dateTexte":1748995200000,"dateTexteComputed":1748995200000,
        "datePubli":null,"publicationRecueil":"C","typePublicationBulletin":"T",
        "sommaire":[{"id":"1","abstrats":"Responsabilité","resumePrincipal":"Il résulte de",
        "autreResume":"Autre résumé"}],
        "citationJp":"Sur l'interprétation du droit interne",
        "liens":[{"cidTexte":"","datePubliTexte":"","dateSignaTexte":"","id":"","natureTexte":"",
        "norTexte":"","num":"","numTexte":"","sens":"source","typeLien":"CITATION",
        "title":"article"},
        {"cidTexte":"LEGITEXT000006070721","id":"LIEN-2","natureTexte":"CODE","num":"1240",
        "sens":"cible","typeLien":"APPLICATION","title":"Code civil"}],
        "titrages":["JURINOME1-JURINOME2"],
        "decisionAttaquee":{"formation":"Cour d'appel de Lyon","date":1704844800000},
        "president":"Mme Champalaune","avocats":"SARL Le Prado","signataires":"texte"}}"#;

    fn read_as(raw: &str, endpoint: &str) -> ConsultJuriResponseDto {
        decode(raw, endpoint).unwrap()
    }

    fn read(raw: &str) -> ConsultJuriResponseDto {
        read_as(raw, JURI)
    }

    fn juri(raw: &str) -> JuriTextDto {
        read(raw).text.unwrap()
    }

    /// Message de l'erreur `Decode` rendue pour `raw`.
    fn error_of(raw: &str) -> String {
        format!("{:?}", decode::<ConsultJuriResponseDto>(raw, JURI).unwrap_err())
    }

    /// Enveloppe `text` autour de `fields`, qui sont des champs JSON.
    fn wrapped(fields: &str) -> String {
        format!(r#"{{"text":{{{fields}}}}}"#)
    }

    #[test]
    fn every_juri_list_is_budgeted() {
        let strings = repeated(r#""x""#, 200_001);
        let objects = repeated("{}", 200_001);
        for (field, elements) in [
            ("numeroAffaire", &strings),
            ("titrages", &strings),
            ("sommaire", &objects),
            ("liens", &objects),
        ] {
            assert_too_many::<ConsultJuriResponseDto>(
                &wrapped(&format!(r#""{field}":{elements}"#)),
                JURI,
                &format!("text.{field}[200000]"),
            );
        }
    }

    #[test]
    fn a_nominal_juri_decision_is_read() {
        assert_eq!(
            juri(JURI_BODY),
            JuriTextDto {
                id: text!("JURITEXT000051743650"),
                origine: text!("JURI"),
                nature: text!("ARRET"),
                num: text!("12500399"),
                numero_affaire: vec!["24-13.470".to_owned()],
                ecli: text!("ECLI:FR:CCASS:2025:C100399"),
                juridiction: text!("Cour de cassation"),
                formation: text!("CHAMBRE_CIVILE_1"),
                solution: text!("Cassation"),
                titre: text!("Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470"),
                titre_long: text!(
                    "Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470, Publié"
                ),
                texte: text!("Sur le moyen unique"),
                date_texte: Some(RawDate::Millis(1_748_995_200_000)),
                date_texte_computed: Some(RawDate::Millis(1_748_995_200_000)),
                date_publi: None,
                publication_recueil: text!("C"),
                type_publication_bulletin: text!("T"),
                sommaire: vec![SommaireDto {
                    id: text!("1"),
                    abstrats: text!("Responsabilité"),
                    resume_principal: text!("Il résulte de"),
                    autre_resume: text!("Autre résumé"),
                }],
                citation_jp: text!("Sur l'interprétation du droit interne"),
                liens: vec![
                    TexteLienDto {
                        type_lien: text!("CITATION"),
                        sens: text!("source"),
                        title: text!("article"),
                        ..Default::default()
                    },
                    TexteLienDto {
                        type_lien: text!("APPLICATION"),
                        sens: text!("cible"),
                        title: text!("Code civil"),
                        cid_texte: text!("LEGITEXT000006070721"),
                        id: text!("LIEN-2"),
                        num: text!("1240"),
                        nature_texte: text!("CODE"),
                    },
                ],
                titrages: vec!["JURINOME1-JURINOME2".to_owned()],
                decision_attaquee: DecisionAttaqueeDto {
                    formation: text!("Cour d'appel de Lyon"),
                    date: Some(RawDate::Millis(1_704_844_800_000)),
                },
                ..Default::default()
            }
        );
    }

    #[test]
    fn a_constit_decision_is_read_with_a_null_appealed_decision() {
        let raw = wrapped(
            r#""id":"CONSTEXT000052555275","origine":"CONSTIT","nature":"QPC","num":"2025-1173",
            "ecli":"ECLI:FR:CC:2025:2025.1173.QPC",
            "natureQualifiee":"Dispositions promulguées (QPC)",
            "titreLoiDef":"Loi de finances pour 2025","numLoiDef":"","ancienId":"CONSTEXT0001",
            "idTexteJo":"JORFTEXT000052541898",
            "titreJo":"JORF n°0263 du 8 novembre 2025, texte n° 152",
            "texteHtml":"<p>Le Conseil</p>","dateTexteComputed":1762473600000,"nor":"CSCX2531180S",
            "citationJp":null,"decisionAttaquee":null,"formation":null"#,
        );
        assert_eq!(
            juri(&raw),
            JuriTextDto {
                id: text!("CONSTEXT000052555275"),
                origine: text!("CONSTIT"),
                nature: text!("QPC"),
                num: text!("2025-1173"),
                ecli: text!("ECLI:FR:CC:2025:2025.1173.QPC"),
                nature_qualifiee: text!("Dispositions promulguées (QPC)"),
                titre_loi_def: text!("Loi de finances pour 2025"),
                ancien_id: text!("CONSTEXT0001"),
                id_texte_jo: text!("JORFTEXT000052541898"),
                titre_jo: text!("JORF n°0263 du 8 novembre 2025, texte n° 152"),
                texte_html: text!("<p>Le Conseil</p>"),
                date_texte_computed: Some(RawDate::Millis(1_762_473_600_000)),
                decision_attaquee: DecisionAttaqueeDto::default(),
                ..Default::default()
            }
        );
    }

    #[test]
    fn a_cnil_deliberation_is_read() {
        let raw = wrapped(
            r#""id":"CNILTEXT000053352643","origine":"CNIL","nature":"Délibération",
            "num":"SAN-2026-002","natureDelib":"Sanction","etat":"VIGUEUR","ecli":null,
            "dateTexte":1767830400000,"datePubli":1768348800000,"datePubliComputed":1768348800000"#,
        );
        assert_eq!(
            read_as(&raw, CNIL).text.unwrap(),
            JuriTextDto {
                id: text!("CNILTEXT000053352643"),
                origine: text!("CNIL"),
                nature: text!("Délibération"),
                num: text!("SAN-2026-002"),
                nature_delib: text!("Sanction"),
                etat: text!("VIGUEUR"),
                date_texte: Some(RawDate::Millis(1_767_830_400_000)),
                date_publi: Some(RawDate::Millis(1_768_348_800_000)),
                date_publi_computed: Some(RawDate::Millis(1_768_348_800_000)),
                ..Default::default()
            }
        );
    }

    #[test]
    fn nature_case_is_preserved() {
        let natures = ["ARRET", "QPC", "Texte", "Délibération", "arret"];
        let read_nature = |nature: &str| juri(&wrapped(&format!(r#""nature":"{nature}""#))).nature;
        assert_eq!(natures.map(read_nature), natures.map(|nature| text!(nature)));
    }

    #[test]
    fn num_accepts_int_and_string() {
        use crate::dto::tests::assert_number_reading;
        assert_number_reading::<JuriTextDto>("num", JURI, |dto| &dto.num);
        assert_number_reading::<TexteLienDto>("num", JURI, |dto| &dto.num);
    }

    #[test]
    fn numero_affaire_scalar_is_a_decode_error() {
        let error = decode::<JuriTextDto>(r#"{"numeroAffaire":12}"#, "/consult/juri").unwrap_err();
        let debug = format!("{error:?}");
        assert!(
            debug.starts_with(r#"Decode { detail: "/consult/juri: numeroAffaire: "#),
            "{debug}"
        );
    }

    #[test]
    fn empty_string_is_none() {
        let raw = wrapped(
            r#""num":"","ecli":" ","citationJp":"","publicationRecueil":"",
            "typePublicationBulletin":"  ","titreLoiDef":"","idTexteJo":"","titreJo":"",
            "ancienId":"""#,
        );
        assert_eq!(juri(&raw), JuriTextDto::default());
        let raw = wrapped(r#""sommaire":[{"resumePrincipal":"","abstrats":" ","autreResume":""}]"#);
        assert_eq!(juri(&raw).sommaire, vec![SommaireDto::default()]);
        // Le premier lien de `JURI_BODY` porte des identifiants vides.
        let liens = juri(JURI_BODY).liens;
        let first = &liens[0];
        assert_eq!(
            (&first.cid_texte, &first.id, &first.num, &first.nature_texte),
            (&None, &None, &None, &None)
        );
    }

    #[test]
    fn null_envelope_or_null_lists_give_empty_dtos() {
        let raws = [r#"{"text":null}"#, "{}", r#"{"dereferenced":false}"#];
        assert_eq!(raws.map(|raw| read(raw).text), [None, None, None]);
        let raw = wrapped(
            r#""numeroAffaire":null,"sommaire":null,"liens":null,"titrages":null,
            "decisionAttaquee":null"#,
        );
        assert_eq!(juri(&raw), JuriTextDto::default());
    }

    #[test]
    fn dates_in_every_encoding_are_kept_raw() {
        let raw = wrapped(
            r#""dateTexte":"1748995200000","dateTexteComputed":"2025-06-04","datePubli":true,
            "datePubliComputed":-5231606400000,
            "decisionAttaquee":{"date":"2024-01-10T00:00:00.000+0000"}"#,
        );
        assert_eq!(
            juri(&raw),
            JuriTextDto {
                date_texte: Some(RawDate::Millis(1_748_995_200_000)),
                date_texte_computed: Some(RawDate::Text("2025-06-04".to_owned())),
                date_publi: None,
                date_publi_computed: Some(RawDate::Millis(-5_231_606_400_000)),
                decision_attaquee: DecisionAttaqueeDto {
                    formation: None,
                    date: Some(RawDate::Text("2024-01-10T00:00:00.000+0000".to_owned())),
                },
                ..Default::default()
            }
        );
    }

    #[test]
    fn persons_are_never_declared() {
        let raw = wrapped(
            r#""id":"JURITEXT1","president":{"nom":["x"]},"avocats":[1,2],"rapporteur":42,
            "commissaire":true,"avocatGl":"SCP X""#,
        );
        assert_eq!(juri(&raw), JuriTextDto { id: text!("JURITEXT1"), ..Default::default() });
    }

    #[test]
    fn a_wrong_summary_type_is_a_decode_error_with_a_json_path_and_no_value() {
        assert_eq!(
            error_of(
                r#"{"text":{"sommaire":[{"id":"1","abstrats":"ok"},{"abstrats":123456789}]}}"#
            ),
            "Decode { detail: \"/consult/juri: text.sommaire[1].abstrats: invalid type: integer, \
             expected a string\" }"
        );
        assert_eq!(
            error_of(r#"{"text":{"sommaire":[{"abstrats":["secret-text-42"]}]}}"#),
            "Decode { detail: \"/consult/juri: text.sommaire[0].abstrats: invalid type: \
             sequence, expected a string\" }"
        );
    }

    #[test]
    fn a_root_that_is_not_an_object_is_a_decode_error() {
        assert_eq!(
            error_of("null"),
            "Decode { detail: \"/consult/juri: .: invalid type: null, expected struct \
             ConsultJuriResponseDto\" }"
        );
        for raw in [r#""x""#, "<html>", "7"] {
            assert!(decode::<ConsultJuriResponseDto>(raw, JURI).is_err(), "{raw}");
        }
        // Un tableau se lit comme une struct (champs positionnels) : consigné, non refusé.
        assert_eq!(read("[]"), ConsultJuriResponseDto::default());
    }

    #[test]
    fn kali_and_juri_dto_are_distinct() {
        let kali = r#"{"text":{"id":"KALITEXT1","signataires":{"faitA":"Paris"}}}"#;
        assert_eq!(juri(kali).id, text!("KALITEXT1"));
        let juri_body = r#"{"id":"JURITEXT1","signataires":"texte"}"#;
        assert_eq!(decode::<ConsultTextDto>(juri_body, JURI).unwrap().id, text!("JURITEXT1"));

        let production = |source: &'static str| source.split("#[cfg(test)]").next().unwrap();
        let juri_source = production(include_str!("juri.rs"));
        let text_source = production(include_str!("consult_text.rs"));
        assert!(!juri_source.contains("consult_text::"));
        assert!(!text_source.contains("juri::"));
        assert!(!juri_source.contains("signataires:"));
        assert!(!text_source.contains("signataires:"));
    }
}
