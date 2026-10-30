//! Texte consulté par `/consult/lawDecree` et `/consult/kaliText` (04 §8.3, recherche 03 §6, §9).
//!
//! Une seule famille de structs pour les deux endpoints : la racine de la réponse est le texte,
//! sans enveloppe. Elle n'est partagée avec aucun autre endpoint, pas même `/consult/juri`.
//! Ces structs ne servent qu'à lire du JSON, sans règle métier : le texte est gardé tel quel et
//! les dates restent brutes (`RawDate`).
//!
//! Chaque struct dérive `Default` et porte `#[serde(default)]` au niveau du conteneur : un champ
//! absent prend sa valeur par défaut (rien ou liste vide), sans répéter l'attribut sur chaque
//! champ. Elles ne dérivent `Debug` et `PartialEq` que pour les tests : elles portent du texte
//! juridique, et un `{:?}` accidentel ne doit pas compiler.
//!
//! Seuls les champs que le mapping lit sont déclarés (04 §8.1) : `signataires` (objet en KALI,
//! chaîne en JURI), `eli`, `visas`, les `liens` du texte… ne l'ont jamais été ; depuis le plan 2g,
//! `textAbroge` et `typeTexte` du texte, `dateDebut` et `dateFin` des sections et des articles et
//! les listes `lstLienCitation` et `lstLienModification` des articles sont sautés par `serde_json`
//! sans construire de valeur (mémoire bornée par la taille du corps) et ne peuvent faire échouer
//! aucun décodage.
//!
//! La table des matières est lue récursivement. `serde_json` part d'une profondeur restante de
//! 128 et échoue quand elle atteint 0 : 127 tableaux et objets imbriqués passent, le 128e donne
//! `CorpusError::Decode`. L'objet racine et chaque niveau de section (un objet et un tableau)
//! les consomment, et les articles de la section la plus profonde en consomment encore : le
//! plafond effectif est de 62 ou 63 niveaux de sections selon le contenu de la plus profonde (63
//! pour des sections feuilles vides, 62 avec des articles). Les champs non déclarés, dont les
//! `lstLien*` des articles depuis le plan 2g, et les lecteurs tolérants (ignorés par
//! `IgnoredAny`, sans limite de profondeur) y échappent.

use serde::Deserialize;

use super::de::{self, RawDate};

/// Texte de loi, de décret ou de convention collective : la racine de la réponse.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ConsultTextDto {
    /// Identifiant de la version du texte.
    pub id: Option<String>,
    /// Identifiant chronologique du texte.
    pub cid: Option<String>,
    /// Titre du texte.
    pub title: Option<String>,
    /// Numéro d'identification national (NOR).
    pub nor: Option<String>,
    /// Nature du texte (`LOI`, `DECRET`…).
    pub nature: Option<String>,
    /// État juridique, quand PISTE l'écrit à cet endroit.
    pub etat: Option<String>,
    /// État juridique de la version (`Vigueur`, `Abrogé`…).
    pub juris_state: Option<String>,
    /// Date de parution, brute.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_parution: Option<RawDate>,
    /// Date de début de la version, brute.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_debut_version: Option<RawDate>,
    /// Date de fin de la version, brute (`2999-01-01` : sans fin).
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_fin_version: Option<RawDate>,
    /// Numéro de parution au Journal officiel, en chaîne ou en nombre, rendu en chaîne (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_string_or_number")]
    pub num_parution: Option<String>,
    /// Référence au Journal officiel, ex. `JORF n°0235 du 8 octobre 2016` : un libellé, pas un
    /// identifiant (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub jorf_text: Option<String>,
    /// Numéro du bulletin officiel d'un texte KALI (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub numero_bo: Option<String>,
    /// Origine de la publication d'un texte KALI (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub origine_publi: Option<String>,
    /// Noms d'usage du texte.
    #[serde(deserialize_with = "de::bounded_list")]
    pub appellations: Vec<String>,
    /// Sections de premier niveau de la table des matières.
    #[serde(deserialize_with = "de::bounded_list")]
    pub sections: Vec<TocSectionDto>,
    /// Articles rattachés directement à la racine.
    #[serde(deserialize_with = "de::bounded_list")]
    pub articles: Vec<TocArticleDto>,
    /// Conteneurs (conventions collectives) du texte KALI.
    #[serde(deserialize_with = "de::bounded_list")]
    pub conteneurs: Vec<ConteneurDto>,
}

/// Section de la table des matières, récursive.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct TocSectionDto {
    /// Identifiant de la section.
    pub id: Option<String>,
    /// Titre de la section.
    pub title: Option<String>,
    /// État juridique de la section.
    pub etat: Option<String>,
    /// Rang de la section dans son parent.
    pub int_ordre: Option<i64>,
    /// Sous-sections.
    #[serde(deserialize_with = "de::bounded_list")]
    pub sections: Vec<TocSectionDto>,
    /// Articles de la section.
    #[serde(deserialize_with = "de::bounded_list")]
    pub articles: Vec<TocArticleDto>,
}

/// Article de la table des matières.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct TocArticleDto {
    /// Identifiant de l'article.
    pub id: Option<String>,
    /// Numéro de l'article, en chaîne ou en nombre, rendu en chaîne (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_string_or_number")]
    pub num: Option<String>,
    /// État juridique de l'article.
    pub etat: Option<String>,
    /// Contenu de l'article, en HTML, quand PISTE l'inclut.
    pub content: Option<String>,
    /// Rang de l'article dans son parent.
    pub int_ordre: Option<i64>,
}

/// Conteneur d'un texte KALI : la convention collective qui le porte.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ConteneurDto {
    /// Identifiant chronologique du conteneur.
    pub cid: Option<String>,
    /// Numéro du conteneur, en chaîne ou en nombre, rendu en chaîne (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_string_or_number")]
    pub num: Option<String>,
    /// Titre du conteneur, avec son retour à la ligne final.
    pub titre: Option<String>,
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "tests : valeurs bornées par les données du test"
)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::budget::{self, MAX_ELEMENTS, remaining};
    use crate::dto::decode::decode;
    use crate::dto::tests::{assert_too_many, repeated};

    const LODA: &str = "/consult/lawDecree";
    const KALI: &str = "/consult/kaliText";

    /// `Some(valeur.to_owned())` : une macro et non une fonction, que `clippy::unnecessary_wraps`
    /// refuserait.
    macro_rules! text {
        ($value:expr) => {
            Some($value.to_owned())
        };
    }

    fn read(raw: &str) -> ConsultTextDto {
        decode(raw, LODA).unwrap()
    }

    /// `levels` objets de section imbriqués, le dernier contenant `{}`.
    fn nested_sections(levels: usize) -> String {
        format!("{}{{}}{}", r#"{"sections":["#.repeat(levels), "]}".repeat(levels))
    }

    /// Profondeur de la première branche de sections.
    fn depth(sections: &[TocSectionDto]) -> usize {
        sections.first().map_or(0, |section| 1 + depth(&section.sections))
    }

    #[test]
    fn a_nominal_law_decree_is_read() {
        let raw = r#"{"id":"LEGITEXT000033205014_08-09-2023","cid":"JORFTEXT000033202746",
            "title":"Loi n° 2016-1321 pour une République numérique","nor":"ECFI1524250L",
            "nature":"LOI","jurisState":"Vigueur","etat":"VIGUEUR","dateDebutVersion":"2023-09-08",
            "dateFinVersion":"2999-01-01","dateParution":1475884800000,"numParution":"0235",
            "jorfText":"JORFTEXT000033202746","textAbroge":false,"appellations":["loi Lemaire"],
            "eli":null,"motsCles":[],
            "articles":[{"id":"LEGIARTI0","num":"0","etat":"ABROGE"}],
            "sections":[{"id":"LEGISCTA1","title":"Titre Ier","etat":"MODIFIE","intOrdre":1,
            "articles":[{"id":"LEGIARTI1","num":"1","etat":"VIGUEUR"}]}]}"#;
        assert_eq!(
            read(raw),
            ConsultTextDto {
                id: text!("LEGITEXT000033205014_08-09-2023"),
                cid: text!("JORFTEXT000033202746"),
                title: text!("Loi n° 2016-1321 pour une République numérique"),
                nor: text!("ECFI1524250L"),
                nature: text!("LOI"),
                etat: text!("VIGUEUR"),
                juris_state: text!("Vigueur"),
                date_parution: Some(RawDate::Millis(1_475_884_800_000)),
                date_debut_version: Some(RawDate::Text("2023-09-08".to_owned())),
                date_fin_version: Some(RawDate::Text("2999-01-01".to_owned())),
                num_parution: text!("0235"),
                jorf_text: text!("JORFTEXT000033202746"),
                appellations: vec!["loi Lemaire".to_owned()],
                articles: vec![TocArticleDto {
                    id: text!("LEGIARTI0"),
                    num: text!("0"),
                    etat: text!("ABROGE"),
                    ..Default::default()
                }],
                sections: vec![TocSectionDto {
                    id: text!("LEGISCTA1"),
                    title: text!("Titre Ier"),
                    etat: text!("MODIFIE"),
                    int_ordre: Some(1),
                    articles: vec![TocArticleDto {
                        id: text!("LEGIARTI1"),
                        num: text!("1"),
                        etat: text!("VIGUEUR"),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }
        );
    }

    #[test]
    fn a_nominal_kali_text_is_read_with_mixed_date_encodings() {
        let raw = r#"{"id":"KALITEXT000005635096","dateParution":32472144000000,
            "numeroBo":"2024-20","originePubli":"BO n°2024-20","typeTexte":"TEXTE_ATTACHE",
            "conteneurs":[{"cid":"KALICONT000005635096","num":"2121","numero":"0000000000002121",
            "titre":"Convention collective\n"}],
            "sections":[{"id":"KALISCTA1","title":"Titre I","intOrdre":1074003967,
            "dateDebut":"2008-06-30","dateFin":"2999-01-01",
            "articles":[{"id":"KALIARTI1","num":"1","etat":"VIGUEUR","content":"<p>Le texte</p>",
            "intOrdre":1073741823,"dateDebut":1214784000000,"dateFin":32472144000000,
            "lstLienModification":[{"linkType":"EXTENSION","linkOrientation":"cible",
            "textTitle":"Arrêté du 1er janvier","natureText":"ARRETE","textCid":"JORFTEXT1",
            "articleId":"JORFARTI1","articleNum":"1","dateDebutCible":"2010-01-01"}],
            "lstLienCitation":[{"linkType":"CITATION","linkOrientation":"source",
            "textTitle":"Code du travail","textCid":"LEGITEXT1","articleId":"LEGIARTI2",
            "articleNum":"L1242-2","parentCid":null}]}]}]}"#;
        assert_eq!(
            decode::<ConsultTextDto>(raw, KALI).unwrap(),
            ConsultTextDto {
                id: text!("KALITEXT000005635096"),
                date_parution: Some(RawDate::Millis(32_472_144_000_000)),
                numero_bo: text!("2024-20"),
                origine_publi: text!("BO n°2024-20"),
                conteneurs: vec![ConteneurDto {
                    cid: text!("KALICONT000005635096"),
                    num: text!("2121"),
                    titre: text!("Convention collective\n"),
                }],
                sections: vec![TocSectionDto {
                    id: text!("KALISCTA1"),
                    title: text!("Titre I"),
                    int_ordre: Some(1_074_003_967),
                    articles: vec![TocArticleDto {
                        id: text!("KALIARTI1"),
                        num: text!("1"),
                        etat: text!("VIGUEUR"),
                        content: text!("<p>Le texte</p>"),
                        int_ordre: Some(1_073_741_823),
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }
        );
    }

    #[test]
    fn kali_signataires_is_an_object_and_is_not_declared() {
        assert_eq!(
            read(r#"{"signataires":{"faitA":"Paris"},"id":"KALITEXT1"}"#),
            ConsultTextDto { id: text!("KALITEXT1"), ..Default::default() }
        );
        assert_eq!(read(r#"{"signataires":"texte"}"#), ConsultTextDto::default());
    }

    #[test]
    fn empty_strings_and_null_lists_become_none_and_empty() {
        let raw = r#"{"numParution":"","jorfText":"  ","numeroBo":"","originePubli":" ",
            "typeTexte":"","appellations":null,"sections":null,"articles":null,
            "conteneurs":null}"#;
        assert_eq!(read(raw), ConsultTextDto::default());
        assert_eq!(decode::<ConteneurDto>(r#"{"num":""}"#, LODA).unwrap(), ConteneurDto::default());
    }

    #[test]
    fn num_accepts_int_and_string() {
        use crate::dto::tests::assert_number_reading;
        assert_number_reading::<ConsultTextDto>("numParution", KALI, |dto| &dto.num_parution);
        assert_number_reading::<TocArticleDto>("num", LODA, |dto| &dto.num);
        assert_number_reading::<ConteneurDto>("num", LODA, |dto| &dto.num);
        let conteneur = decode::<ConteneurDto>(r#"{"num":2121}"#, KALI).unwrap();
        assert_eq!(conteneur.num, text!("2121"));
    }

    #[test]
    fn an_empty_object_is_all_defaults_and_null_is_a_decode_error() {
        assert_eq!(read("{}"), ConsultTextDto::default());
        let error = decode::<ConsultTextDto>("null", LODA).unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            r#"Decode { detail: "/consult/lawDecree: .: invalid type: null, expected struct ConsultTextDto" }"#
        );
    }

    #[test]
    fn dates_in_every_encoding_are_kept_raw() {
        let raw = r#"{"dateParution":-5231606400000,"dateDebutVersion":"1475884800000",
            "dateFinVersion":true}"#;
        assert_eq!(
            read(raw),
            ConsultTextDto {
                date_parution: Some(RawDate::Millis(-5_231_606_400_000)),
                date_debut_version: Some(RawDate::Millis(1_475_884_800_000)),
                date_fin_version: None,
                ..Default::default()
            }
        );
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let raw = r#"{"visas":[{"a":1}],"liens":[],"signers":{"x":"y"},"title":"Un titre"}"#;
        assert_eq!(read(raw), ConsultTextDto { title: text!("Un titre"), ..Default::default() });
    }

    #[test]
    fn a_wrong_type_is_a_decode_error_with_a_json_path_and_no_value() {
        let error = decode::<ConsultTextDto>(
            r#"{"sections":[{"articles":[{"intOrdre":"secret-text-42"}]}]}"#,
            LODA,
        )
        .unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            r#"Decode { detail: "/consult/lawDecree: sections[0].articles[0].intOrdre: invalid type: string, expected i64" }"#
        );
    }

    /// Décode `raw` en `ConsultTextDto` : l'erreur attendue est celle d'un budget épuisé à `path`.
    fn assert_budgeted(raw: &str, path: &str) {
        assert_too_many::<ConsultTextDto>(raw, LODA, path);
    }

    #[test]
    fn every_consult_text_list_is_budgeted() {
        let strings = repeated(r#""x""#, 200_001);
        let objects = repeated("{}", 200_001);
        assert_budgeted(&format!(r#"{{"appellations":{strings}}}"#), "appellations[200000]");
        for list in ["conteneurs", "sections", "articles"] {
            assert_budgeted(&format!(r#"{{"{list}":{objects}}}"#), &format!("{list}[200000]"));
        }
        assert_budgeted(
            &format!(r#"{{"sections":[{{"sections":{objects}}}]}}"#),
            "sections[0].sections[199999]",
        );
        assert_budgeted(
            &format!(r#"{{"sections":[{{"articles":{objects}}}]}}"#),
            "sections[0].articles[199999]",
        );
    }

    /// Lit `raw` par `serde_json` sous un budget armé à la main : le texte et les éléments payés.
    fn read_spending(raw: &str) -> (ConsultTextDto, u32) {
        let guard = budget::arm();
        let text = serde_json::from_str::<ConsultTextDto>(raw).unwrap();
        let spent = MAX_ELEMENTS - remaining().unwrap();
        drop(guard);
        (text, spent)
    }

    #[test]
    fn exactly_200_000_elements_are_decoded() {
        let raw = format!(r#"{{"articles":{}}}"#, repeated("{}", 200_000));
        assert_eq!(raw.len(), 600_014);
        assert_eq!(read(&raw).articles.len(), 200_000);
    }

    #[test]
    fn one_element_more_maps_to_decode_with_a_path() {
        let raw = format!(r#"{{"articles":{}}}"#, repeated("{}", 200_001));
        assert_eq!(raw.len(), 600_017);
        // `map(drop)` : un succès de 200 001 articles ne serait pas affiché par `unwrap_err`.
        let error = decode::<ConsultTextDto>(&raw, LODA).map(drop).unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            r#"Decode { detail: "/consult/lawDecree: articles[200000]: too many elements" }"#
        );
    }

    #[test]
    fn the_budget_spans_the_root_lists() {
        let raw = format!(
            r#"{{"articles":{},"sections":{}}}"#,
            repeated("{}", 100_000),
            repeated("{}", 100_001)
        );
        assert_budgeted(&raw, "sections[100000]");
    }

    /// Une TOC de la taille d'un code (04 §8.5, limite 1) : 2 000 sections de 9 articles, soit
    /// 20 000 nœuds = 20 000 éléments, 10 % du budget ; les 3 liens de chaque article, non
    /// déclarés depuis le plan 2g, ne coûtent rien.
    #[test]
    fn a_code_sized_toc_stays_far_under_the_budget() {
        let article = r#"{"id":"LEGIARTI000000000001","num":"1","etat":"VIGUEUR","lstLienCitation":[{},{}],"lstLienModification":[{}]}"#;
        let section = format!(
            r#"{{"id":"LEGISCTA000000000001","title":"T","sections":[],"articles":{}}}"#,
            repeated(article, 9)
        );
        let body = format!(r#"{{"sections":{}}}"#, repeated(&section, 2_000));
        assert_eq!(body.len(), 2_118_014);
        let (text, spent) = read_spending(&body);
        assert_eq!(spent, 20_000);
        assert_eq!(text.sections.len(), 2_000);
        assert_eq!(text.sections[0].articles.len(), 9);
        assert_eq!(text.sections[1_999].articles[8].num, text!("1"));
        assert_eq!(
            decode::<ConsultTextDto>(&body, LODA).map(|text| text.sections.len()),
            Ok(2_000)
        );
    }

    #[test]
    fn the_links_of_an_article_are_skipped_without_cost() {
        let article = format!(r#"{{"num":"1","lstLienCitation":{}}}"#, repeated("{}", 9));
        let raw = format!(r#"{{"articles":{}}}"#, repeated(&article, 1_000));
        let (text, spent) = read_spending(&raw);
        assert_eq!(spent, 1_000);
        assert_eq!(text.articles.len(), 1_000);
        assert_eq!(text.articles[999].num, text!("1"));
    }

    #[test]
    fn a_toc_of_63_levels_is_decoded() {
        assert_eq!(depth(&read(&nested_sections(63)).sections), 63);
    }

    #[test]
    fn a_toc_of_64_levels_or_more_maps_to_decode() {
        for levels in [64, 200] {
            let error = decode::<ConsultTextDto>(&nested_sections(levels), LODA).unwrap_err();
            let message = format!("{error:?}");
            assert!(
                message.starts_with(r#"Decode { detail: "/consult/lawDecree: sections[0]"#),
                "{levels} niveaux : {message}"
            );
            assert!(
                message.contains("recursion limit exceeded at line 1 column"),
                "{levels} niveaux : {message}"
            );
        }
    }
}
