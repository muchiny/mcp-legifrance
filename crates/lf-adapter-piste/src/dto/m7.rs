//! DTO des réponses de M7 : `canonicalArticleVersion`, `getArticleByCid` et `relatedLinksArticle`
//! (04 §7.3, §8.3).
//!
//! Seuls les champs que le mapping lit sont déclarés (04 §8.1) ; les structs suivent la
//! convention de `article` : `Default` dérivé, `#[serde(default)]` au niveau du conteneur, `Debug`
//! et `PartialEq` sous `cfg(test)` seulement, dates brutes (`RawDate`), listes payées sur le
//! budget du document (`bounded_list`).
//!
//! `getArticleByCid` rend toutes les versions d'un article **avec leur texte**, ce qui pèse vite :
//! `ListedArticleDto` ne déclare donc ni `texte`, ni `texteHtml`, ni `nota`, ni `context`, que
//! serde saute sans construire de valeur, et non l'`ArticleDto` de `/consult/getArticle` que la
//! table de 04 §8.3 réutilisait. Le repli ne lit que ce qu'il faut à une `ArticleVersion`.
//!
//! `ordre` (numéro d'ordre de l'article, `null` dans les fixtures de `canonicalArticleVersion`)
//! est lu en chaîne ou en nombre par `opt_string_or_number` puis converti par le mapping : une
//! forme inattendue ne fait pas perdre le document, elle laisse seulement l'ordre indéterminé.
//! `cidParent` de `relatedLinksArticle` n'est pas lu et n'est pas déclaré.

use serde::Deserialize;

use super::de::{self, RawDate};

/// Réponse de `/search/canonicalArticleVersion` : `{articleVersions: [...]}`.
///
/// Un identifiant inconnu donne un 200 sans aucun octet (fixture `inconnu`) : `classify` le range
/// en « introuvable » avant le décodage. Une enveloppe sans la clé, `null` ou vide est une liste
/// vide.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct CanonicalVersionsDto {
    /// Versions de l'article, métadonnées seules.
    #[serde(deserialize_with = "de::bounded_list")]
    pub article_versions: Vec<ArticleVersionDto>,
}

/// Une version d'article de `canonicalArticleVersion`.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ArticleVersionDto {
    /// Identifiant de la version.
    pub id: Option<String>,
    /// Numéro de l'article (`null` dans les fixtures), en chaîne ou en nombre.
    #[serde(deserialize_with = "de::opt_string_or_number")]
    pub numero: Option<String>,
    /// État juridique (`VIGUEUR`, `ABROGE`, `MODIFIE`…).
    pub etat: Option<String>,
    /// Début de vigueur, brut.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_debut: Option<RawDate>,
    /// Fin de vigueur, brute (`2999-01-01` : sans fin).
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_fin: Option<RawDate>,
    /// Numéro d'ordre, en chaîne ou en nombre.
    #[serde(deserialize_with = "de::opt_string_or_number")]
    pub ordre: Option<String>,
}

/// Réponse de `/consult/getArticleByCid` : `{listArticle: [...]}`.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct GetListArticleResponseDto {
    /// Versions de l'article, du moins ce qu'en lit le mapping.
    #[serde(deserialize_with = "de::bounded_list")]
    pub list_article: Vec<ListedArticleDto>,
}

/// Une version d'article de `getArticleByCid`, sans son texte.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ListedArticleDto {
    /// Identifiant de la version.
    pub id: Option<String>,
    /// Numéro de l'article, en chaîne ou en nombre.
    #[serde(deserialize_with = "de::opt_string_or_number")]
    pub num: Option<String>,
    /// État juridique.
    pub etat: Option<String>,
    /// Début de vigueur, brut.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_debut: Option<RawDate>,
    /// Fin de vigueur, brute (`2999-01-01` : sans fin).
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_fin: Option<RawDate>,
    /// Numéro d'ordre, en chaîne ou en nombre.
    #[serde(deserialize_with = "de::opt_string_or_number")]
    pub ordre: Option<String>,
}

/// Réponse de `/consult/relatedLinksArticle` : les deux sens des liens d'un article.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct RelatedLinksDto {
    /// Liens que l'article cite.
    #[serde(deserialize_with = "de::bounded_list")]
    pub liens_cite: Vec<LienRelatifDto>,
    /// Liens qui citent l'article.
    #[serde(deserialize_with = "de::bounded_list")]
    pub liens_cite_par: Vec<LienRelatifDto>,
}

/// Un lien (`LiensRelatifsDTO`).
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct LienRelatifDto {
    /// Identifiant de la cible (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub id: Option<String>,
    /// Identifiant du texte de la cible, lu quand `id` est absent (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub cid_text: Option<String>,
    /// Nature du lien (`LODA`, `JORF`, `CODE`, `KALI`, `JORFCONT`), casse conservée.
    pub nature: Option<String>,
    /// Titre du lien.
    pub name: Option<String>,
    /// Date de vigueur, brute (`2999-01-01` : sans fin).
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_vigeur: Option<RawDate>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::de::RawDate;
    use crate::dto::decode::decode;
    use crate::dto::tests::{assert_number_reading, assert_too_many, repeated};

    const CANONICAL: &str = "/search/canonicalArticleVersion";
    const BY_CID: &str = "/consult/getArticleByCid";
    const RELATED: &str = "/consult/relatedLinksArticle";

    /// Réponse enregistrée de `canonicalArticleVersion` pour le Code civil, article 9.
    const CANONICAL_FIXTURE: &str = include_str!(
        "../../tests/fixtures/recorded/search/canonicalArticleVersion/m7/canonicalArticleVersion-LEGIARTI000006419288.json"
    );
    /// Réponse enregistrée de `getArticleByCid`, texte de chaque version compris.
    const BY_CID_FIXTURE: &str = include_str!(
        "../../tests/fixtures/recorded/consult/getArticleByCid/m7/getArticleByCid-LEGIARTI000006419288.json"
    );
    /// Réponse enregistrée de `relatedLinksArticle` : quatorze liens « cité par ».
    const RELATED_FIXTURE: &str = include_str!(
        "../../tests/fixtures/recorded/consult/relatedLinksArticle/m7/relatedLinksArticle-LEGIARTI000006419288.json"
    );

    macro_rules! text {
        ($value:expr) => {
            Some($value.to_owned())
        };
    }

    fn canonical(raw: &str) -> CanonicalVersionsDto {
        decode(raw, CANONICAL).unwrap()
    }

    fn by_cid(raw: &str) -> GetListArticleResponseDto {
        decode(raw, BY_CID).unwrap()
    }

    fn related(raw: &str) -> RelatedLinksDto {
        decode(raw, RELATED).unwrap()
    }

    #[test]
    fn the_recorded_canonical_response_is_read() {
        let dto = canonical(CANONICAL_FIXTURE);
        assert_eq!(dto.article_versions.len(), 3);
        assert_eq!(
            dto.article_versions[2],
            ArticleVersionDto {
                id: text!("LEGIARTI000006419288"),
                numero: None,
                etat: text!("VIGUEUR"),
                date_debut: Some(RawDate::Millis(17_193_600_000)),
                date_fin: Some(RawDate::Millis(32_472_144_000_000)),
                ordre: None,
            }
        );
    }

    #[test]
    fn a_canonical_response_without_versions_is_an_empty_list() {
        for raw in [
            "{}",
            r#"{"articleVersions":null}"#,
            r#"{"articleVersions":[]}"#,
            r#"{"executionTime":0}"#,
        ] {
            assert_eq!(canonical(raw), CanonicalVersionsDto::default(), "{raw}");
        }
    }

    #[test]
    fn numero_and_ordre_accept_int_and_string() {
        assert_number_reading::<ArticleVersionDto>("numero", CANONICAL, |dto| &dto.numero);
        assert_number_reading::<ArticleVersionDto>("ordre", CANONICAL, |dto| &dto.ordre);
        assert_number_reading::<ListedArticleDto>("num", BY_CID, |dto| &dto.num);
        assert_number_reading::<ListedArticleDto>("ordre", BY_CID, |dto| &dto.ordre);
    }

    #[test]
    fn the_recorded_by_cid_response_keeps_no_text() {
        let dto = by_cid(BY_CID_FIXTURE);
        assert_eq!(dto.list_article.len(), 3);
        assert_eq!(
            dto.list_article[1],
            ListedArticleDto {
                id: text!("LEGIARTI000050396821"),
                num: text!("9"),
                etat: text!("ABROGE"),
                date_debut: Some(RawDate::Millis(-1_337_558_400_000)),
                date_fin: Some(RawDate::Millis(17_193_600_000)),
                ordre: text!("128847"),
            }
        );
    }

    #[test]
    fn a_by_cid_response_without_articles_is_an_empty_list() {
        for raw in ["{}", r#"{"listArticle":null}"#, r#"{"listArticle":[]}"#] {
            assert_eq!(by_cid(raw), GetListArticleResponseDto::default(), "{raw}");
        }
    }

    #[test]
    fn the_recorded_related_links_response_is_read() {
        let dto = related(RELATED_FIXTURE);
        assert_eq!(dto.liens_cite, Vec::new());
        assert_eq!(dto.liens_cite_par.len(), 14);
        assert_eq!(
            dto.liens_cite_par[1],
            LienRelatifDto {
                id: text!("JORFARTI000024559972"),
                cid_text: text!("JORFTEXT000024559971"),
                nature: text!("LODA"),
                name: text!("Circulaire  du 14 septembre 2011 - art., v. init."),
                date_vigeur: Some(RawDate::Millis(32_472_144_000_000)),
            }
        );
    }

    #[test]
    fn a_link_date_in_digits_is_read_in_milliseconds() {
        let raw = r#"{"liensCite":[{"id":"LEGITEXT000006070721","dateVigeur":"1470787200000"}]}"#;
        assert_eq!(
            related(raw).liens_cite[0].date_vigeur,
            Some(RawDate::Millis(1_470_787_200_000))
        );
    }

    #[test]
    fn blank_link_fields_are_absent_and_undeclared_ones_are_skipped() {
        // `cidParent` n'est pas lu par le mapping : il est sauté, quelle que soit sa forme.
        let raw = r#"{"liensCite":[{"id":"  ","cidText":"","cidParent":{"a":[1]},"name":"x"}],
            "liensCitePar":null}"#;
        let dto = related(raw);
        assert_eq!(
            dto.liens_cite,
            vec![LienRelatifDto { name: text!("x"), ..LienRelatifDto::default() }]
        );
        assert_eq!(dto.liens_cite_par, Vec::new());
    }

    #[test]
    fn every_m7_list_is_budgeted() {
        let objects = repeated("{}", 200_001);
        assert_too_many::<CanonicalVersionsDto>(
            &format!(r#"{{"articleVersions":{objects}}}"#),
            CANONICAL,
            "articleVersions[200000]",
        );
        assert_too_many::<GetListArticleResponseDto>(
            &format!(r#"{{"listArticle":{objects}}}"#),
            BY_CID,
            "listArticle[200000]",
        );
        for key in ["liensCite", "liensCitePar"] {
            assert_too_many::<RelatedLinksDto>(
                &format!(r#"{{"{key}":{objects}}}"#),
                RELATED,
                &format!("{key}[200000]"),
            );
        }
    }
}
