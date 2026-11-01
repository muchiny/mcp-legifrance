//! Réponse de `/consult/getArticle` (04 §8.3, recherche 03 §7, §12.5).
//!
//! La réponse est enveloppée : `{executionTime, dereferenced, article}`. Seule la clé `article`
//! est déclarée ; `executionTime` et `dereferenced` sont sautés. Les structs suivent la
//! convention de `consult_text` : `Default` dérivé, `#[serde(default)]` au niveau du conteneur,
//! `Debug` et `PartialEq` sous `cfg(test)` seulement (elles portent du texte juridique), dates
//! brutes (`RawDate`) et texte gardé tel quel. Seuls `nota` et `notaHtml` vides deviennent
//! `None` ; `texte` et `texteHtml` vides restent `Some("")` : seul le mapping juge leur vide.
//!
//! Piège de nommage : `context.titresTM` porte deux majuscules ; `rename_all = "camelCase"`
//! produirait `titresTm` et le champ resterait vide en silence, d'où un `rename` explicite.
//!
//! Seuls les champs que le mapping lit sont déclarés (04 §8.1) : `articleVersions`, les listes de
//! liens réservées à M7, `computedNums`, `activitePro`… ne l'ont jamais été ; depuis le plan 2g,
//! `versionPrecedente`, `sectionParentTitre`, `isMultipleVersions` (et sa graphie
//! `multipleVersions`, du Swagger et de KALI), `context.titreTxt` et l'`id` d'une entrée de
//! `titresTM` sont sautés par `serde_json` sans construire de valeur (mémoire bornée par la
//! taille du corps) et ne peuvent faire échouer aucun décodage : les deux graphies ensemble ne
//! donnent plus `duplicate field`. Depuis L3-02, le `debut`, la `fin`, le `xPath` et l'`etat`
//! d'une entrée de `titresTM` sont lus, tolérants : un type inattendu donne `None`, jamais une
//! erreur (le mapping choisit les sections en vigueur d'après eux).

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
    /// Numéro de l'article, en chaîne ou en nombre, rendu en chaîne (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_string_or_number")]
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
    /// Début de vigueur, brut.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_debut: Option<RawDate>,
    /// Fin de vigueur, brute (`2999-01-01` : sans fin).
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_fin: Option<RawDate>,
    /// Titres des sections traversées, tels que PISTE les écrit (incomplets).
    pub full_sections_titre: Option<String>,
    /// Chemin de l'article dans le texte (`null` : contexte vide).
    #[serde(deserialize_with = "de::null_default")]
    pub context: ArticleContextDto,
    /// Titres du texte parent.
    #[serde(deserialize_with = "de::bounded_list")]
    pub text_titles: Vec<TextTitleDto>,
}

/// Chemin d'un article dans son texte.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ArticleContextDto {
    /// Sections traversées, de la plus haute à la plus basse (clé `titresTM`).
    #[serde(rename = "titresTM", deserialize_with = "de::bounded_list")]
    pub titres_tm: Vec<ContextEntryDto>,
}

/// Section traversée par un article (entrée de `titresTM`).
///
/// PISTE rend une entrée par version d'une section : l'ancienne et la nouvelle structure d'un
/// code remanié figurent ensemble, chacune avec sa période (`debut`, `fin`), son niveau (`xPath`,
/// un segment `TM` par niveau) et son état. Le mapping choisit celles qui étaient en vigueur.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ContextEntryDto {
    /// Titre de la section.
    pub titre: Option<String>,
    /// Début de vigueur de cette version de la section, brut (illisible : `None`).
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub debut: Option<RawDate>,
    /// Fin de vigueur de cette version de la section, brute (`2999-01-01` : sans fin).
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub fin: Option<RawDate>,
    /// Chemin XML de la section (`/ARTICLE/CONTEXTE/TEXTE/TM/TM/TITRE_TM` : niveau 2).
    #[serde(deserialize_with = "de::opt_string_or_number")]
    pub x_path: Option<String>,
    /// État juridique de cette version de la section (`VIGUEUR`, `MODIFIE`…).
    #[serde(deserialize_with = "de::opt_string_or_number")]
    pub etat: Option<String>,
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
    /// Titre du texte (court : « Arrêté du 20 mai 2026 »).
    pub titre: Option<String>,
    /// Titre officiel complet (clé `titreLong`), avec l'objet du texte et un éventuel appel de
    /// note final ; égal à `titre` pour un code.
    pub titre_long: Option<String>,
    /// Nature du texte (`CODE`…), casse conservée.
    pub nature: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::decode::decode;
    use crate::dto::tests::{assert_too_many, repeated};

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
    fn num_accepts_int_and_string() {
        crate::dto::tests::assert_number_reading::<ArticleDto>("num", ENDPOINT, |dto| &dto.num);
    }

    #[test]
    fn every_article_list_is_budgeted() {
        let objects = repeated("{}", 200_001);
        for (fields, path) in [
            (format!(r#""textTitles":{objects}"#), "article.textTitles[200000]"),
            (format!(r#""context":{{"titresTM":{objects}}}"#), "article.context.titresTM[200000]"),
        ] {
            let raw = format!(r#"{{"article":{{{fields}}}}}"#);
            assert_too_many::<GetArticleResponseDto>(&raw, ENDPOINT, path);
        }
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
            "texteHtml":"<p>Chacun a droit.</p>","nota":"Nota de l'article",
            "notaHtml":"<p>Nota de l'article</p>",
            "versionPrecedente":"LEGIARTI000050396821","isMultipleVersions":true,
            "comporteLiensSP":false}}"#;
        assert_eq!(
            article(raw),
            ArticleDto {
                id: text!("LEGIARTI000006419288"),
                cid: text!("LEGIARTI000006419288"),
                num: text!("9"),
                etat: text!("VIGUEUR"),
                texte: text!("Chacun a droit au respect de sa vie privée."),
                texte_html: text!("<p>Chacun a droit.</p>"),
                nota: text!("Nota de l'article"),
                nota_html: text!("<p>Nota de l'article</p>"),
                date_debut: Some(RawDate::Millis(17_193_600_000)),
                date_fin: Some(RawDate::Millis(32_472_144_000_000)),
                full_sections_titre: text!("Livre Ier : Des personnes"),
                context: ArticleContextDto {
                    titres_tm: vec![ContextEntryDto {
                        titre: text!("Livre Ier : Des personnes"),
                        debut: Some(RawDate::Text("1804-03-21".to_owned())),
                        fin: Some(RawDate::Text("2999-01-01".to_owned())),
                        x_path: text!("/x"),
                        etat: text!("VIGUEUR"),
                    }],
                },
                text_titles: vec![TextTitleDto {
                    id: text!("LEGITEXT000006070721"),
                    cid: text!("LEGITEXT000006070721"),
                    titre: text!("Code civil"),
                    titre_long: text!("Code civil"),
                    nature: text!("CODE"),
                }],
            }
        );
    }

    #[test]
    fn titres_tm_is_renamed() {
        let raw = r#"{"article":{"context":{"titresTM":[{"id":"LEGISCTA1","titre":"Livre Ier"}],
            "titresTm":[{"id":"mauvaise clé"}],"titreTxt":[{"id":"LEGITEXT1"}]}}}"#;
        assert_eq!(
            article(raw).context.titres_tm,
            vec![ContextEntryDto { titre: text!("Livre Ier"), ..Default::default() }]
        );
    }

    #[test]
    fn fields_the_mapping_never_reads_are_not_declared() {
        // Les deux graphies du drapeau de versions ensemble ne font plus `duplicate field`, et
        // aucun type inattendu sur un champ non lu ne fait perdre l'article (plan 2g).
        let raw = r#"{"article":{"id":"LEGIARTI1","isMultipleVersions":true,
            "multipleVersions":"oui","versionPrecedente":[1],"sectionParentTitre":{"a":1},
            "context":{"titresTM":[{"id":7,"debut":true,"fin":[],"titre":"Livre Ier"}],
            "titreTxt":"x"}}}"#;
        assert_eq!(
            article(raw),
            ArticleDto {
                id: text!("LEGIARTI1"),
                context: ArticleContextDto {
                    titres_tm: vec![ContextEntryDto {
                        titre: text!("Livre Ier"),
                        ..Default::default()
                    }],
                },
                ..Default::default()
            }
        );
    }

    #[test]
    fn section_entry_period_level_and_state_are_read_leniently() {
        let raw = r#"{"article":{"context":{"titresTM":[
            {"titre":"A","debut":"1804-03-21","fin":32472144000000,
             "xPath":"/ARTICLE/CONTEXTE/TEXTE/TM/TM/TITRE_TM","etat":"MODIFIE"},
            {"titre":"B","debut":true,"fin":[],"xPath":7,"etat":{"a":1}},
            {"titre":"C","debut":"  ","fin":null,"xPath":"","etat":null}]}}}"#;
        assert_eq!(
            article(raw).context.titres_tm,
            vec![
                ContextEntryDto {
                    titre: text!("A"),
                    debut: Some(RawDate::Text("1804-03-21".to_owned())),
                    fin: Some(RawDate::Millis(32_472_144_000_000)),
                    x_path: text!("/ARTICLE/CONTEXTE/TEXTE/TM/TM/TITRE_TM"),
                    etat: text!("MODIFIE"),
                },
                ContextEntryDto { titre: text!("B"), x_path: text!("7"), ..Default::default() },
                ContextEntryDto { titre: text!("C"), ..Default::default() },
            ]
        );
    }

    #[test]
    fn empty_string_is_none() {
        let raw = r#"{"article":{"nota":"","notaHtml":"   ","texte":"","texteHtml":""}}"#;
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
            r#"{"article":{"context":{"titresTM":null}}}"#,
        ];
        assert_eq!(raws.map(article), [ArticleDto::default(), ArticleDto::default()]);
    }

    #[test]
    fn dates_in_every_encoding_are_kept_raw() {
        let raw = r#"{"article":{"dateDebut":"1475884800000","dateFin":"2999-01-01"}}"#;
        assert_eq!(
            article(raw),
            ArticleDto {
                date_debut: Some(RawDate::Millis(1_475_884_800_000)),
                date_fin: Some(RawDate::Text("2999-01-01".to_owned())),
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
