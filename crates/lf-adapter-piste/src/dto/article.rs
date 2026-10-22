//! Réponse de `/consult/getArticle` (04 §8.3, recherche 03 §7, §12.5).
//!
//! La réponse est enveloppée : `{executionTime, dereferenced, article}`. Seule la clé `article`
//! est déclarée ; `executionTime` et `dereferenced` sont sautés. Les structs suivent la
//! convention de `consult_text` : `Default` dérivé, `#[serde(default)]` au niveau du conteneur,
//! `Debug` et `PartialEq` sous `cfg(test)` seulement (elles portent du texte juridique), dates
//! brutes (`RawDate`) et texte gardé tel quel. Seuls `nota`, `notaHtml`, `versionPrecedente` et
//! les identifiants de contexte vides deviennent `None` ; `texte` et `texteHtml` vides restent
//! `Some("")` : seul le mapping juge leur vide.
//!
//! Deux pièges de nommage. `context.titresTM` porte deux majuscules : `rename_all = "camelCase"`
//! produirait `titresTm` et le champ resterait vide en silence, d'où un `rename` explicite.
//! L'indicateur de versions multiples s'écrit `isMultipleVersions` dans la réponse observée et
//! `multipleVersions` dans le Swagger et en KALI : un `alias` sur un seul champ lit les deux
//! graphies ; un corps qui porterait les deux donnerait `duplicate field`, donc `Decode`.
//!
//! Les champs que le mapping n'emploie pas ne sont pas déclarés (`articleVersions`, les listes de
//! liens réservées à M7, `computedNums`, `activitePro`…) : `serde_json` les saute sans rien
//! allouer et ils ne peuvent faire échouer aucun décodage.

use serde::Deserialize;

use super::de::{self, RawDate};

/// Enveloppe de la réponse de `/consult/getArticle`.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct GetArticleResponseDto {
    /// Article demandé ; `None` si la clé est absente ou `null`.
    pub article: Option<ArticleDto>,
}

/// Article d'un texte consolidé.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ArticleDto {
    /// Identifiant de la version de l'article.
    pub id: Option<String>,
    /// Identifiant chronologique de l'article.
    pub cid: Option<String>,
    /// Numéro de l'article.
    pub num: Option<String>,
    /// État juridique (`VIGUEUR`, `ABROGE`…).
    pub etat: Option<String>,
    /// Texte brut de l'article, gardé tel quel (`""` reste `Some("")`).
    pub texte: Option<String>,
    /// Texte de l'article en HTML, gardé tel quel (`""` reste `Some("")`).
    pub texte_html: Option<String>,
    /// Nota de l'article (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub nota: Option<String>,
    /// Nota de l'article en HTML (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub nota_html: Option<String>,
    /// Identifiant de la version précédente (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub version_precedente: Option<String>,
    /// Début de vigueur, brut.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_debut: Option<RawDate>,
    /// Fin de vigueur, brute (`2999-01-01` : sans fin).
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_fin: Option<RawDate>,
    /// Titres des sections traversées, tels que PISTE les écrit (incomplets).
    pub full_sections_titre: Option<String>,
    /// Titre de la section parente.
    pub section_parent_titre: Option<String>,
    /// Chemin de l'article dans le texte (`null` : contexte vide).
    #[serde(deserialize_with = "de::null_default")]
    pub context: ArticleContextDto,
    /// Titres du texte parent.
    #[serde(deserialize_with = "de::null_default")]
    pub text_titles: Vec<TextTitleDto>,
    /// Vrai si l'article a plusieurs versions (`isMultipleVersions` ou `multipleVersions`).
    #[serde(alias = "multipleVersions")]
    pub is_multiple_versions: Option<bool>,
}

/// Chemin d'un article dans son texte.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ArticleContextDto {
    /// Sections traversées, de la plus haute à la plus basse (clé `titresTM`).
    #[serde(rename = "titresTM", deserialize_with = "de::null_default")]
    pub titres_tm: Vec<ContextEntryDto>,
    /// Texte parent.
    #[serde(deserialize_with = "de::null_default")]
    pub titre_txt: Vec<ContextEntryDto>,
}

/// Section ou texte du contexte d'un article.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ContextEntryDto {
    /// Identifiant de la section ou du texte.
    pub id: Option<String>,
    /// Titre de la section ou du texte.
    pub titre: Option<String>,
    /// Début de vigueur, brut.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub debut: Option<RawDate>,
    /// Fin de vigueur, brute.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub fin: Option<RawDate>,
}

/// Titre du texte parent d'un article.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct TextTitleDto {
    /// Identifiant de la version du texte.
    pub id: Option<String>,
    /// Identifiant chronologique du texte.
    pub cid: Option<String>,
    /// Titre du texte.
    pub titre: Option<String>,
    /// Nature du texte (`CODE`…), casse conservée.
    pub nature: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::decode::decode;

    const ENDPOINT: &str = "/consult/getArticle";

    /// `Some(valeur.to_owned())` : une macro et non une fonction, que `clippy::unnecessary_wraps`
    /// refuserait.
    macro_rules! text {
        ($value:expr) => {
            Some($value.to_owned())
        };
    }

    fn read(raw: &str) -> GetArticleResponseDto {
        decode(raw, ENDPOINT).unwrap()
    }

    fn article(raw: &str) -> ArticleDto {
        read(raw).article.unwrap()
    }

    /// Message de l'erreur `Decode` rendue pour `raw`.
    fn error_of(raw: &str) -> String {
        format!("{:?}", decode::<GetArticleResponseDto>(raw, ENDPOINT).unwrap_err())
    }

    #[test]
    fn a_nominal_article_is_read() {
        let raw = r#"{"executionTime":12,"dereferenced":false,"article":{
            "id":"LEGIARTI000006419288","cid":"LEGIARTI000006419288","idTexte":null,
            "type":"AUTONOME","texte":"Chacun a droit au respect de sa vie privée.","num":"9",
            "origine":"LEGI","nature":"Article","versionArticle":"3.0","etat":"VIGUEUR",
            "dateDebut":17193600000,"dateFin":32472144000000,"ordre":128847,
            "context":{"titresTM":[{"debut":"1804-03-21","fin":"2999-01-01",
            "titre":"Livre Ier : Des personnes","xPath":"/x","cid":"LEGISCTA1","id":"LEGISCTA1",
            "etat":"VIGUEUR"}],
            "titreTxt":[{"debut":"1804-03-21","fin":"2999-01-01","titre":"Code civil",
            "cid":"LEGITEXT000006070721","id":"LEGITEXT000006070721","etat":"VIGUEUR"}],
            "nombreVersionParent":1},
            "sectionParentTitre":"Titre Ier : Des droits civils",
            "fullSectionsTitre":"Livre Ier : Des personnes",
            "textTitles":[{"id":"LEGITEXT000006070721","cid":"LEGITEXT000006070721",
            "titre":"Code civil","titreLong":"Code civil","nature":"CODE",
            "dateDebut":-5231606400000,"nor":""}],
            "nota":"","versionPrecedente":"LEGIARTI000050396821","isMultipleVersions":true,
            "comporteLiensSP":false}}"#;
        assert_eq!(
            article(raw),
            ArticleDto {
                id: text!("LEGIARTI000006419288"),
                cid: text!("LEGIARTI000006419288"),
                num: text!("9"),
                etat: text!("VIGUEUR"),
                texte: text!("Chacun a droit au respect de sa vie privée."),
                nota: None,
                version_precedente: text!("LEGIARTI000050396821"),
                date_debut: Some(RawDate::Millis(17_193_600_000)),
                date_fin: Some(RawDate::Millis(32_472_144_000_000)),
                full_sections_titre: text!("Livre Ier : Des personnes"),
                section_parent_titre: text!("Titre Ier : Des droits civils"),
                context: ArticleContextDto {
                    titres_tm: vec![ContextEntryDto {
                        id: text!("LEGISCTA1"),
                        titre: text!("Livre Ier : Des personnes"),
                        debut: Some(RawDate::Text("1804-03-21".to_owned())),
                        fin: Some(RawDate::Text("2999-01-01".to_owned())),
                    }],
                    titre_txt: vec![ContextEntryDto {
                        id: text!("LEGITEXT000006070721"),
                        titre: text!("Code civil"),
                        debut: Some(RawDate::Text("1804-03-21".to_owned())),
                        fin: Some(RawDate::Text("2999-01-01".to_owned())),
                    }],
                },
                text_titles: vec![TextTitleDto {
                    id: text!("LEGITEXT000006070721"),
                    cid: text!("LEGITEXT000006070721"),
                    titre: text!("Code civil"),
                    nature: text!("CODE"),
                }],
                is_multiple_versions: Some(true),
                ..Default::default()
            }
        );
    }

    #[test]
    fn titres_tm_is_renamed() {
        let raw = r#"{"article":{"context":{"titresTM":[{"id":"LEGISCTA1","titre":"Livre Ier"}],
            "titresTm":[{"id":"mauvaise clé"}],"titreTxt":[{"id":"LEGITEXT1"}]}}}"#;
        let context = article(raw).context;
        assert_eq!(
            context.titres_tm,
            vec![ContextEntryDto {
                id: text!("LEGISCTA1"),
                titre: text!("Livre Ier"),
                ..Default::default()
            }]
        );
        assert_eq!(
            context.titre_txt,
            vec![ContextEntryDto { id: text!("LEGITEXT1"), ..Default::default() }]
        );
    }

    #[test]
    fn multiple_versions_alias() {
        let raws = [
            r#"{"article":{"isMultipleVersions":true}}"#,
            r#"{"article":{"multipleVersions":false}}"#,
            r#"{"article":{"multipleVersions":null}}"#,
            r#"{"article":{}}"#,
        ];
        assert_eq!(
            raws.map(|raw| article(raw).is_multiple_versions),
            [Some(true), Some(false), None, None]
        );
    }

    #[test]
    fn both_spellings_of_the_multiple_versions_flag_are_a_decode_error() {
        assert_eq!(
            error_of(r#"{"article":{"isMultipleVersions":true,"multipleVersions":true}}"#),
            "Decode { detail: \"/consult/getArticle: article: duplicate field \
             `isMultipleVersions`\" }"
        );
    }

    #[test]
    fn empty_string_is_none() {
        let raw = r#"{"article":{"nota":"","notaHtml":"   ","versionPrecedente":"",
            "texte":"","texteHtml":""}}"#;
        assert_eq!(
            article(raw),
            ArticleDto { texte: text!(""), texte_html: text!(""), ..Default::default() }
        );
    }

    #[test]
    fn null_envelope_or_null_lists_give_empty_dtos() {
        let raws = [r#"{"article":null}"#, "{}", r#"{"executionTime":1,"dereferenced":false}"#];
        assert_eq!(raws.map(|raw| read(raw).article), [None, None, None]);
        let raws = [
            r#"{"article":{"context":null,"textTitles":null}}"#,
            r#"{"article":{"context":{"titresTM":null,"titreTxt":null}}}"#,
        ];
        assert_eq!(raws.map(article), [ArticleDto::default(), ArticleDto::default()]);
    }

    #[test]
    fn dates_in_every_encoding_are_kept_raw() {
        let raw = r#"{"article":{"dateDebut":"1475884800000","dateFin":"2999-01-01",
            "context":{"titresTM":[{"debut":-5231606400000,"fin":true}]}}}"#;
        assert_eq!(
            article(raw),
            ArticleDto {
                date_debut: Some(RawDate::Millis(1_475_884_800_000)),
                date_fin: Some(RawDate::Text("2999-01-01".to_owned())),
                context: ArticleContextDto {
                    titres_tm: vec![ContextEntryDto {
                        debut: Some(RawDate::Millis(-5_231_606_400_000)),
                        fin: None,
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                ..Default::default()
            }
        );
    }

    #[test]
    fn undeclared_fields_are_ignored() {
        let raw = r#"{"article":{"id":"LEGIARTI1","articleVersions":[{"id":1}],
            "lienCitations":"x","lienModifications":{"a":[1]},"computedNums":["9"],
            "activitePro":[],"inap":null}}"#;
        assert_eq!(article(raw), ArticleDto { id: text!("LEGIARTI1"), ..Default::default() });
    }

    #[test]
    fn a_wrong_type_is_a_decode_error_with_a_json_path_and_no_value() {
        assert_eq!(
            error_of(r#"{"article":{"textTitles":[{"id":"ok"},{"titre":["secret-text-42"]}]}}"#),
            "Decode { detail: \"/consult/getArticle: article.textTitles[1].titre: invalid type: \
             sequence, expected a string\" }"
        );
    }

    #[test]
    fn a_root_that_is_not_an_object_is_a_decode_error() {
        assert_eq!(
            error_of("null"),
            "Decode { detail: \"/consult/getArticle: .: invalid type: null, expected struct \
             GetArticleResponseDto\" }"
        );
    }
}
