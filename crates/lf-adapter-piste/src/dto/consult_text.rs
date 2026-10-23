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
//! Les champs que le mapping n'emploie pas (`signataires`, `eli`, `visas`, `liens` du texte…)
//! ne sont pas déclarés : `serde_json` les saute sans construire de valeur (mémoire bornée par la
//! taille du corps), et `signataires`, objet en KALI et chaîne en JURI, ne peut faire échouer
//! aucun décodage.
//!
//! La table des matières est lue récursivement. `serde_json` part d'une profondeur restante de
//! 128 et échoue quand elle atteint 0 : 127 tableaux et objets imbriqués passent, le 128e donne
//! `CorpusError::Decode`. L'objet racine et chaque niveau de section (un objet et un tableau)
//! les consomment, et les articles de la section la plus profonde, puis leurs listes de liens,
//! en consomment encore : le plafond effectif est de 61 à 63 niveaux de sections selon le contenu
//! de la plus profonde (63 pour des sections feuilles vides, 62 avec des articles, 61 si ces
//! articles portent des `lstLien*`). Les champs non déclarés et les lecteurs tolérants (ignorés
//! par `IgnoredAny`, sans limite de profondeur) y échappent.

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
    /// Vrai si le texte est abrogé.
    pub text_abroge: Option<bool>,
    /// Date de parution, brute.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_parution: Option<RawDate>,
    /// Date de début de la version, brute.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_debut_version: Option<RawDate>,
    /// Date de fin de la version, brute (`2999-01-01` : sans fin).
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_fin_version: Option<RawDate>,
    /// Numéro de parution au Journal officiel (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub num_parution: Option<String>,
    /// Identifiant du texte au Journal officiel (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub jorf_text: Option<String>,
    /// Numéro du bulletin officiel d'un texte KALI (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub numero_bo: Option<String>,
    /// Origine de la publication d'un texte KALI (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub origine_publi: Option<String>,
    /// Type d'un texte KALI (`TEXTE_ATTACHE`…, `""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub type_texte: Option<String>,
    /// Noms d'usage du texte.
    #[serde(deserialize_with = "de::null_default")]
    pub appellations: Vec<String>,
    /// Sections de premier niveau de la table des matières.
    #[serde(deserialize_with = "de::null_default")]
    pub sections: Vec<TocSectionDto>,
    /// Articles rattachés directement à la racine.
    #[serde(deserialize_with = "de::null_default")]
    pub articles: Vec<TocArticleDto>,
    /// Conteneurs (conventions collectives) du texte KALI.
    #[serde(deserialize_with = "de::null_default")]
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
    /// Date de début de la section, brute.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_debut: Option<RawDate>,
    /// Date de fin de la section, brute.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_fin: Option<RawDate>,
    /// Sous-sections.
    #[serde(deserialize_with = "de::null_default")]
    pub sections: Vec<TocSectionDto>,
    /// Articles de la section.
    #[serde(deserialize_with = "de::null_default")]
    pub articles: Vec<TocArticleDto>,
}

/// Article de la table des matières.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct TocArticleDto {
    /// Identifiant de l'article.
    pub id: Option<String>,
    /// Numéro de l'article.
    pub num: Option<String>,
    /// État juridique de l'article.
    pub etat: Option<String>,
    /// Contenu de l'article, en HTML, quand PISTE l'inclut.
    pub content: Option<String>,
    /// Rang de l'article dans son parent.
    pub int_ordre: Option<i64>,
    /// Date de début de l'article, brute.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_debut: Option<RawDate>,
    /// Date de fin de l'article, brute.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_fin: Option<RawDate>,
    /// Liens vers les articles qui citent celui-ci ou qu'il cite.
    #[serde(deserialize_with = "de::null_default")]
    pub lst_lien_citation: Vec<LienArticleDto>,
    /// Liens vers les articles qui modifient celui-ci ou qu'il modifie.
    #[serde(deserialize_with = "de::null_default")]
    pub lst_lien_modification: Vec<LienArticleDto>,
}

/// Lien d'un article vers un autre article.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct LienArticleDto {
    /// Type du lien (`CITATION`, `EXTENSION`…).
    pub link_type: Option<String>,
    /// Orientation du lien (`cible`, `source`).
    pub link_orientation: Option<String>,
    /// Titre du texte lié.
    pub text_title: Option<String>,
    /// Nature du texte lié.
    pub nature_text: Option<String>,
    /// Identifiant chronologique du texte lié (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub text_cid: Option<String>,
    /// Identifiant de l'article lié (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub article_id: Option<String>,
    /// Numéro de l'article lié (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub article_num: Option<String>,
}

/// Conteneur d'un texte KALI : la convention collective qui le porte.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ConteneurDto {
    /// Identifiant chronologique du conteneur.
    pub cid: Option<String>,
    /// Numéro du conteneur (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub num: Option<String>,
    /// Titre du conteneur, avec son retour à la ligne final.
    pub titre: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::decode::decode;

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
                text_abroge: Some(false),
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
                type_texte: text!("TEXTE_ATTACHE"),
                conteneurs: vec![ConteneurDto {
                    cid: text!("KALICONT000005635096"),
                    num: text!("2121"),
                    titre: text!("Convention collective\n"),
                }],
                sections: vec![TocSectionDto {
                    id: text!("KALISCTA1"),
                    title: text!("Titre I"),
                    int_ordre: Some(1_074_003_967),
                    date_debut: Some(RawDate::Text("2008-06-30".to_owned())),
                    date_fin: Some(RawDate::Text("2999-01-01".to_owned())),
                    articles: vec![TocArticleDto {
                        id: text!("KALIARTI1"),
                        num: text!("1"),
                        etat: text!("VIGUEUR"),
                        content: text!("<p>Le texte</p>"),
                        int_ordre: Some(1_073_741_823),
                        date_debut: Some(RawDate::Millis(1_214_784_000_000)),
                        date_fin: Some(RawDate::Millis(32_472_144_000_000)),
                        lst_lien_citation: vec![LienArticleDto {
                            link_type: text!("CITATION"),
                            link_orientation: text!("source"),
                            text_title: text!("Code du travail"),
                            nature_text: None,
                            text_cid: text!("LEGITEXT1"),
                            article_id: text!("LEGIARTI2"),
                            article_num: text!("L1242-2"),
                        }],
                        lst_lien_modification: vec![LienArticleDto {
                            link_type: text!("EXTENSION"),
                            link_orientation: text!("cible"),
                            text_title: text!("Arrêté du 1er janvier"),
                            nature_text: text!("ARRETE"),
                            text_cid: text!("JORFTEXT1"),
                            article_id: text!("JORFARTI1"),
                            article_num: text!("1"),
                        }],
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
        let article = r#"{"lstLienCitation":null,"lstLienModification":null}"#;
        assert_eq!(decode::<TocArticleDto>(article, LODA).unwrap(), TocArticleDto::default());
        let link = r#"{"textCid":"","articleId":" ","articleNum":""}"#;
        assert_eq!(decode::<LienArticleDto>(link, LODA).unwrap(), LienArticleDto::default());
        assert_eq!(decode::<ConteneurDto>(r#"{"num":""}"#, LODA).unwrap(), ConteneurDto::default());
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
