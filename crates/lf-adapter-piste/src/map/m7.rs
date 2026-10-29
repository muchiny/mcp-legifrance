//! Mapping des réponses de M7 vers le domaine : `ArticleVersion`, `RelatedLinks` (04 §9).
//!
//! [`article_versions`] et [`listed_article_versions`] rendent les mêmes `ArticleVersion` depuis
//! `canonicalArticleVersion` (métadonnées) et `getArticleByCid` (repli, versions complètes), triées
//! de la plus ancienne à la plus récente : par début de vigueur, puis par `ordre` (04 §9.2). Une
//! version sans début de vigueur passe avant les autres (l'ordre de `Option`), à `ordre` égal ou
//! absent l'ordre reçu est gardé (tri stable). [`related_links`] garde tous les liens, dans l'ordre
//! reçu : la borne de 100 liens relève de l'application (05 §5.11).
//!
//! Aucune valeur du corps ne sort d'ici : une erreur `Decode` ne nomme que l'endpoint et le chemin
//! du champ (`articleVersions[].id`, `liensCite[].dateVigeur`).

use lf_application::ports::CorpusError;
use lf_domain::entity::{ArticleVersion, RelatedLink, RelatedLinks, RelatedNature};
use lf_domain::id::{DocumentId, LegiArtiId};
use lf_domain::status::LegalStatus;
use lf_domain::text::clean_field;

use super::body::non_blank;
use super::dates::civil;
use super::rules::parse_counted;
use super::{Diagnostics, Field, not_found};
use crate::dto::de::RawDate;
use crate::dto::m7::{
    ArticleVersionDto, CanonicalVersionsDto, GetListArticleResponseDto, LienRelatifDto,
    ListedArticleDto, RelatedLinksDto,
};

/// Gabarit de `/search/canonicalArticleVersion` : versions d'un article, métadonnées seules.
pub(crate) const CANONICAL_ARTICLE_VERSION: &str = "/search/canonicalArticleVersion";
/// Gabarit de `/consult/getArticleByCid` : toutes les versions d'un article, texte compris.
pub(crate) const GET_ARTICLE_BY_CID: &str = "/consult/getArticleByCid";
/// Gabarit de `/consult/relatedLinksArticle` : liens d'un article.
pub(crate) const RELATED_LINKS_ARTICLE: &str = "/consult/relatedLinksArticle";

/// Chemins des champs lus d'une liste de versions, pour les erreurs `Decode`.
struct VersionFields {
    /// Identifiant de la version.
    id: Field,
    /// Début de vigueur.
    date_debut: Field,
    /// Fin de vigueur.
    date_fin: Field,
}

/// Champs de `canonicalArticleVersion`.
const CANONICAL_FIELDS: VersionFields = VersionFields {
    id: Field { endpoint: CANONICAL_ARTICLE_VERSION, path: "articleVersions[].id" },
    date_debut: Field { endpoint: CANONICAL_ARTICLE_VERSION, path: "articleVersions[].dateDebut" },
    date_fin: Field { endpoint: CANONICAL_ARTICLE_VERSION, path: "articleVersions[].dateFin" },
};

/// Champs de `getArticleByCid`.
const LISTED_FIELDS: VersionFields = VersionFields {
    id: Field { endpoint: GET_ARTICLE_BY_CID, path: "listArticle[].id" },
    date_debut: Field { endpoint: GET_ARTICLE_BY_CID, path: "listArticle[].dateDebut" },
    date_fin: Field { endpoint: GET_ARTICLE_BY_CID, path: "listArticle[].dateFin" },
};

/// Champ `dateVigeur` d'un lien de `liensCite`.
const CITES_DATE: Field = Field { endpoint: RELATED_LINKS_ARTICLE, path: "liensCite[].dateVigeur" };
/// Champ `dateVigeur` d'un lien de `liensCitePar`.
const CITED_BY_DATE: Field =
    Field { endpoint: RELATED_LINKS_ARTICLE, path: "liensCitePar[].dateVigeur" };

/// Champs lus d'une version, quelle que soit la réponse qui la porte.
struct Row<'a> {
    /// Identifiant de la version.
    id: Option<&'a str>,
    /// Numéro de l'article.
    num: Option<&'a str>,
    /// État juridique brut.
    etat: Option<&'a str>,
    /// Début de vigueur brut.
    date_debut: Option<&'a RawDate>,
    /// Fin de vigueur brute.
    date_fin: Option<&'a RawDate>,
    /// Numéro d'ordre brut.
    ordre: Option<&'a str>,
}

impl<'a> From<&'a ArticleVersionDto> for Row<'a> {
    fn from(dto: &'a ArticleVersionDto) -> Self {
        Self {
            id: dto.id.as_deref(),
            num: dto.numero.as_deref(),
            etat: dto.etat.as_deref(),
            date_debut: dto.date_debut.as_ref(),
            date_fin: dto.date_fin.as_ref(),
            ordre: dto.ordre.as_deref(),
        }
    }
}

impl<'a> From<&'a ListedArticleDto> for Row<'a> {
    fn from(dto: &'a ListedArticleDto) -> Self {
        Self {
            id: dto.id.as_deref(),
            num: dto.num.as_deref(),
            etat: dto.etat.as_deref(),
            date_debut: dto.date_debut.as_ref(),
            date_fin: dto.date_fin.as_ref(),
            ordre: dto.ordre.as_deref(),
        }
    }
}

/// Une version et sa clé de tri secondaire (`ordre`, `None` s'il est absent ou illisible).
type Ranked = (Option<i64>, ArticleVersion);

/// La version de `row`, avec son `ordre`.
///
/// # Errors
///
/// [`CorpusError::Decode`] de `fields.id` (`missing identifier` s'il est absent ou blanc,
/// `invalid identifier` s'il n'est pas un `LEGIARTI`) ou de `fields.date_debut` ou
/// `fields.date_fin` (`unreadable date`).
fn version(
    row: &Row<'_>,
    fields: &VersionFields,
    diags: &mut Diagnostics,
) -> Result<Ranked, CorpusError> {
    let raw = non_blank(row.id).ok_or_else(|| fields.id.decode("missing identifier"))?;
    let id = parse_counted::<LegiArtiId>(raw, diags).map_err(|error| fields.id.domain(&error))?;
    let date_debut = civil(row.date_debut, fields.date_debut, diags)?;
    let date_fin = civil(row.date_fin, fields.date_fin, diags)?;
    let ordre = row.ordre.and_then(|ordre| ordre.trim().parse::<i64>().ok());
    Ok((
        ordre,
        ArticleVersion {
            id,
            num: clean_field(row.num),
            etat: row.etat.and_then(LegalStatus::parse),
            date_debut,
            date_fin,
        },
    ))
}

/// Les versions de `items`, de la plus ancienne à la plus récente (début de vigueur, puis
/// `ordre`) ; l'ordre reçu départage le reste.
///
/// # Errors
///
/// [`CorpusError::Decode`] de la première version refusée : voir `version`.
fn version_list<'a, T>(
    items: &'a [T],
    fields: &VersionFields,
    diags: &mut Diagnostics,
) -> Result<Vec<ArticleVersion>, CorpusError>
where
    Row<'a>: From<&'a T>,
{
    let mut ranked = items
        .iter()
        .map(|item| version(&Row::from(item), fields, diags))
        .collect::<Result<Vec<Ranked>, _>>()?;
    ranked.sort_by_key(|(ordre, version)| (version.date_debut, *ordre));
    Ok(ranked.into_iter().map(|(_, version)| version).collect())
}

/// Versions de la réponse `dto` de `/search/canonicalArticleVersion`, de la plus ancienne à la plus
/// récente ; leurs anomalies sont comptées dans `diags`.
///
/// Une liste vide est un résultat vide, non une erreur : c'est l'appelant qui bascule alors sur
/// `getArticleByCid` (04 §7.3).
///
/// # Errors
///
/// [`CorpusError::Decode`] de la première version dont l'identifiant ou une date est refusé :
/// voir `version`.
pub(crate) fn article_versions(
    dto: CanonicalVersionsDto,
    diags: &mut Diagnostics,
) -> Result<Vec<ArticleVersion>, CorpusError> {
    let CanonicalVersionsDto { article_versions } = dto;
    version_list(&article_versions, &CANONICAL_FIELDS, diags)
}

/// Versions de la réponse `dto` de `/consult/getArticleByCid`, de la plus ancienne à la plus
/// récente ; leurs anomalies sont comptées dans `diags`.
///
/// # Errors
///
/// [`CorpusError::NotFound`] (`/consult/getArticleByCid`) si la liste est absente, `null` ou
/// vide : c'est le repli, et un article sans aucune version n'existe pas ; `diags` n'est alors pas
/// touché. [`CorpusError::Decode`] de la première version dont l'identifiant ou une date est
/// refusé : voir `version`.
pub(crate) fn listed_article_versions(
    dto: GetListArticleResponseDto,
    diags: &mut Diagnostics,
) -> Result<Vec<ArticleVersion>, CorpusError> {
    let GetListArticleResponseDto { list_article } = dto;
    if list_article.is_empty() {
        return Err(not_found(GET_ARTICLE_BY_CID));
    }
    version_list(&list_article, &LISTED_FIELDS, diags)
}

/// Nature d'un lien : les cinq valeurs de l'API, casse et blancs ignorés ; toute autre valeur est
/// gardée nettoyée dans [`RelatedNature::Other`] (vide si la nature est absente ou blanche).
fn nature_of(raw: Option<&str>) -> RelatedNature {
    match raw.map_or_else(String::new, |nature| nature.trim().to_ascii_uppercase()).as_str() {
        "LODA" => RelatedNature::Loda,
        "JORF" => RelatedNature::Jorf,
        "CODE" => RelatedNature::Code,
        "KALI" => RelatedNature::Kali,
        "JORFCONT" => RelatedNature::JorfCont,
        _ => RelatedNature::Other(clean_field(raw).unwrap_or_default()),
    }
}

/// Cible d'un lien : son `id`, ou `cidText` quand `id` est absent ou blanc (04 §7.3). Un
/// identifiant qui n'est pas d'un type consultable (`JORFARTI`, `JORFCONT`) ou n'est pas un
/// identifiant donne `None` : le lien reste, sans cible, et `cidText` n'est alors pas utilisé (il
/// désigne le texte parent, pas l'article lié).
fn target_of(link: &LienRelatifDto, diags: &mut Diagnostics) -> Option<DocumentId> {
    let raw = non_blank(link.id.as_deref()).or_else(|| non_blank(link.cid_text.as_deref()))?;
    parse_counted::<DocumentId>(raw, diags).ok()
}

/// Le lien `link`, dont la date est lue au champ `date_field`.
fn related_link(
    link: &LienRelatifDto,
    date_field: Field,
    diags: &mut Diagnostics,
) -> Result<RelatedLink, CorpusError> {
    Ok(RelatedLink {
        target: target_of(link, diags),
        nature: nature_of(link.nature.as_deref()),
        title: clean_field(link.name.as_deref()),
        date: civil(link.date_vigeur.as_ref(), date_field, diags)?,
    })
}

/// Liens de `links`, dans l'ordre reçu.
fn related_list(
    links: &[LienRelatifDto],
    date_field: Field,
    diags: &mut Diagnostics,
) -> Result<Vec<RelatedLink>, CorpusError> {
    links.iter().map(|link| related_link(link, date_field, diags)).collect()
}

/// Liens de la réponse `dto` de `/consult/relatedLinksArticle` : `liensCite` donne `cites` et
/// `liensCitePar` donne `cited_by` ; leurs anomalies sont comptées dans `diags`.
///
/// # Errors
///
/// [`CorpusError::Decode`] (`liensCite[].dateVigeur` ou `liensCitePar[].dateVigeur`,
/// `unreadable date`) du premier lien dont la date est illisible.
pub(crate) fn related_links(
    dto: RelatedLinksDto,
    diags: &mut Diagnostics,
) -> Result<RelatedLinks, CorpusError> {
    let RelatedLinksDto { liens_cite, liens_cite_par } = dto;
    Ok(RelatedLinks {
        cites: related_list(&liens_cite, CITES_DATE, diags)?,
        cited_by: related_list(&liens_cite_par, CITED_BY_DATE, diags)?,
    })
}

#[cfg(test)]
mod tests {
    use lf_application::ports::CorpusError;
    use lf_domain::entity::{ArticleVersion, RelatedLink, RelatedLinks, RelatedNature};
    use lf_domain::id::{DocumentId, LegiArtiId};
    use lf_domain::status::LegalStatus;

    use super::*;
    use crate::map::Diagnostics;
    use crate::map::fixtures::{d, decode_error, read, text};

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
    /// Article sans aucun lien : `liensCite` et `liensCitePar` vides.
    const NO_LINKS_FIXTURE: &str = include_str!(
        "../../tests/fixtures/recorded/consult/relatedLinksArticle/m7/relatedLinksArticle-LEGIARTI000033205058.json"
    );

    fn legi(raw: &str) -> LegiArtiId {
        LegiArtiId::parse(raw).unwrap()
    }

    fn document(raw: &str) -> DocumentId {
        DocumentId::parse(raw).unwrap()
    }

    /// Versions de la réponse `canonicalArticleVersion` `json`.
    fn canonical(json: &str, diags: &mut Diagnostics) -> Result<Vec<ArticleVersion>, CorpusError> {
        article_versions(read(json, CANONICAL_ARTICLE_VERSION), diags)
    }

    /// Versions de la réponse `getArticleByCid` `json`.
    fn listed(json: &str, diags: &mut Diagnostics) -> Result<Vec<ArticleVersion>, CorpusError> {
        listed_article_versions(read(json, GET_ARTICLE_BY_CID), diags)
    }

    /// Liens de la réponse `relatedLinksArticle` `json`.
    fn links(json: &str, diags: &mut Diagnostics) -> Result<RelatedLinks, CorpusError> {
        related_links(read(json, RELATED_LINKS_ARTICLE), diags)
    }

    /// Les trois versions de l'article 9 du Code civil, de la plus ancienne à la plus récente.
    fn civil_code_article_9(num: Option<&str>) -> Vec<ArticleVersion> {
        let num = num.map(str::to_owned);
        vec![
            ArticleVersion {
                id: legi("LEGIARTI000050396803"),
                num: num.clone(),
                etat: Some(LegalStatus::Modifie),
                date_debut: Some(d(1889, 6, 28)),
                date_fin: Some(d(1927, 8, 14)),
            },
            ArticleVersion {
                id: legi("LEGIARTI000050396821"),
                num: num.clone(),
                etat: Some(LegalStatus::Abroge),
                date_debut: Some(d(1927, 8, 14)),
                date_fin: Some(d(1970, 7, 19)),
            },
            ArticleVersion {
                id: legi("LEGIARTI000006419288"),
                num,
                etat: Some(LegalStatus::Vigueur),
                date_debut: Some(d(1970, 7, 19)),
                date_fin: None,
            },
        ]
    }

    #[test]
    fn the_recorded_canonical_response_maps_oldest_first() {
        let mut diags = Diagnostics::default();
        assert_eq!(canonical(CANONICAL_FIXTURE, &mut diags), Ok(civil_code_article_9(None)));
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn the_recorded_by_cid_response_maps_to_the_same_versions() {
        let mut diags = Diagnostics::default();
        assert_eq!(listed(BY_CID_FIXTURE, &mut diags), Ok(civil_code_article_9(Some("9"))));
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn versions_sort_by_start_date_then_ordre() {
        let json = r#"{"articleVersions":[
            {"id":"LEGIARTI000000000003","dateDebut":86400000,"ordre":3},
            {"id":"LEGIARTI000000000009","dateDebut":172800000,"ordre":1},
            {"id":"LEGIARTI000000000001","dateDebut":86400000,"ordre":"1"},
            {"id":"LEGIARTI000000000002","dateDebut":86400000,"ordre":2},
            {"id":"LEGIARTI000000000008","ordre":5},
            {"id":"LEGIARTI000000000007","dateDebut":86400000}]}"#;
        let ids: Vec<String> = canonical(json, &mut Diagnostics::default())
            .unwrap()
            .iter()
            .map(|version| version.id.as_str().to_owned())
            .collect();
        // Sans date en tête, puis à date égale par `ordre` (absent d'abord), puis la date suivante.
        assert_eq!(ids, [8, 7, 1, 2, 3, 9].map(|n| format!("LEGIARTI{n:012}")),);
    }

    #[test]
    fn version_fields_are_cleaned_and_read_leniently() {
        let json = r#"{"articleVersions":[{"id":" legiarti000006419288 ","numero":" L.  1234-5 ",
            "etat":"vigueur_eten","dateDebut":"2024-03-15","dateFin":"2999-01-01"}]}"#;
        let mut diags = Diagnostics::default();
        assert_eq!(
            canonical(json, &mut diags),
            Ok(vec![ArticleVersion {
                id: legi("LEGIARTI000006419288"),
                num: text!("L. 1234-5"),
                etat: Some(LegalStatus::VigueurEtendue),
                date_debut: Some(d(2024, 3, 15)),
                date_fin: None,
            }])
        );
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn a_suffix_on_a_version_id_is_stripped_and_counted() {
        let json = r#"{"articleVersions":[{"id":"LEGIARTI000006419288_01-01-2020"}]}"#;
        let mut diags = Diagnostics::default();
        let versions = canonical(json, &mut diags).unwrap();
        assert_eq!(versions[0].id, legi("LEGIARTI000006419288"));
        assert_eq!(diags.upstream_suffix_stripped, 1);
    }

    #[test]
    fn a_version_date_off_midnight_is_kept_and_counted() {
        let json = r#"{"articleVersions":[{"id":"LEGIARTI000006419288","dateDebut":17193600001}]}"#;
        let mut diags = Diagnostics::default();
        let versions = canonical(json, &mut diags).unwrap();
        assert_eq!(versions[0].date_debut, Some(d(1970, 7, 19)));
        assert_eq!(diags.non_midnight_timestamps, 1);
    }

    #[test]
    fn a_version_without_a_readable_identifier_is_a_decode_error() {
        for (id, category) in [
            (r#""id":"JORFTEXT000000000001""#, "invalid identifier"),
            (r#""id":"LEGIARTI12""#, "invalid identifier"),
            (r#""id":"  ""#, "missing identifier"),
            (r#""num":"9""#, "missing identifier"),
        ] {
            let json = format!(r#"{{"articleVersions":[{{{id}}}]}}"#);
            assert_eq!(
                canonical(&json, &mut Diagnostics::default()),
                Err(decode_error(
                    CANONICAL_ARTICLE_VERSION,
                    &format!("articleVersions[].id: {category}")
                )),
                "{id}"
            );
            let json = json.replace("articleVersions", "listArticle");
            assert_eq!(
                listed(&json, &mut Diagnostics::default()),
                Err(decode_error(GET_ARTICLE_BY_CID, &format!("listArticle[].id: {category}"))),
                "{id}"
            );
        }
    }

    #[test]
    fn an_unreadable_version_date_is_a_decode_error() {
        for field in ["dateDebut", "dateFin"] {
            let json = format!(
                r#"{{"articleVersions":[{{"id":"LEGIARTI000006419288","{field}":"pas une date"}}]}}"#
            );
            assert_eq!(
                canonical(&json, &mut Diagnostics::default()),
                Err(decode_error(
                    CANONICAL_ARTICLE_VERSION,
                    &format!("articleVersions[].{field}: unreadable date")
                ))
            );
        }
    }

    #[test]
    fn an_empty_canonical_list_is_an_empty_result_not_an_error() {
        // C'est l'appelant qui bascule alors sur `getArticleByCid` (04 §7.3).
        for json in ["{}", r#"{"articleVersions":[]}"#] {
            assert_eq!(canonical(json, &mut Diagnostics::default()), Ok(Vec::new()), "{json}");
        }
    }

    #[test]
    fn an_empty_by_cid_list_is_not_found() {
        for json in ["{}", r#"{"listArticle":[]}"#, r#"{"listArticle":null}"#] {
            assert_eq!(
                listed(json, &mut Diagnostics::default()),
                Err(CorpusError::NotFound { what: GET_ARTICLE_BY_CID.to_owned() }),
                "{json}"
            );
        }
    }

    // ---- relatedLinksArticle --------------------------------------------------------------

    /// Un lien `cites` d'une réponse à un seul lien, de la nature `nature`.
    fn nature_of(nature: &str) -> RelatedNature {
        let json = format!(
            r#"{{"liensCite":[{{"id":"LEGITEXT000006070721","nature":{nature},"name":"Code civil"}}]}}"#
        );
        links(&json, &mut Diagnostics::default()).unwrap().cites.remove(0).nature
    }

    #[test]
    fn related_links_maps_natures() {
        assert_eq!(nature_of(r#""LODA""#), RelatedNature::Loda);
        assert_eq!(nature_of(r#""JORF""#), RelatedNature::Jorf);
        assert_eq!(nature_of(r#""CODE""#), RelatedNature::Code);
        assert_eq!(nature_of(r#""KALI""#), RelatedNature::Kali);
        assert_eq!(nature_of(r#""JORFCONT""#), RelatedNature::JorfCont);
        // Casse et blancs sont ignorés ; une nature inconnue est gardée nettoyée.
        assert_eq!(nature_of(r#"" loda ""#), RelatedNature::Loda);
        assert_eq!(nature_of(r#""Code""#), RelatedNature::Code);
        assert_eq!(
            nature_of(r#"" Accord   collectif ""#),
            RelatedNature::Other("Accord collectif".into())
        );
        // Absente ou blanche : `Other` vide, le lien est gardé.
        assert_eq!(nature_of("null"), RelatedNature::Other(String::new()));
        assert_eq!(nature_of(r#""  ""#), RelatedNature::Other(String::new()));
    }

    #[test]
    fn related_links_keep_the_direction_of_each_list() {
        let json = r#"{"liensCite":[{"id":"LEGITEXT000006070721","nature":"CODE","name":"cité"}],
            "liensCitePar":[{"id":"KALITEXT000049878950","nature":"KALI","name":"citant"}]}"#;
        assert_eq!(
            links(json, &mut Diagnostics::default()),
            Ok(RelatedLinks {
                cites: vec![RelatedLink {
                    target: Some(document("LEGITEXT000006070721")),
                    nature: RelatedNature::Code,
                    title: text!("cité"),
                    date: None,
                }],
                cited_by: vec![RelatedLink {
                    target: Some(document("KALITEXT000049878950")),
                    nature: RelatedNature::Kali,
                    title: text!("citant"),
                    date: None,
                }],
            })
        );
    }

    #[test]
    fn related_link_without_consultable_target_keeps_title() {
        // Deux liens enregistrés : un `JORFARTI` (dont le texte `JORFTEXT` est dans `cidText`, mais
        // n'est pas la cible) et un `JORFCONT`, plus un identifiant qui n'en est pas un.
        let json = r#"{"liensCitePar":[
            {"name":"Circulaire  du 14 septembre 2011 - art., v. init.","id":"JORFARTI000024559972",
             "cidParent":null,"cidText":"JORFTEXT000024559971","dateVigeur":32472144000000,
             "nature":"LODA"},
            {"name":"Sommaire du JO","id":"JORFCONT000000000001","cidText":"JORFTEXT000000000002",
             "dateVigeur":1214956800000,"nature":"JORFCONT"},
            {"name":"Texte sans identifiant","id":"n'importe quoi","nature":"JORF"}]}"#;
        let mut diags = Diagnostics::default();
        assert_eq!(
            links(json, &mut diags).unwrap().cited_by,
            vec![
                RelatedLink {
                    target: None,
                    nature: RelatedNature::Loda,
                    title: text!("Circulaire du 14 septembre 2011 - art., v. init."),
                    date: None,
                },
                RelatedLink {
                    target: None,
                    nature: RelatedNature::JorfCont,
                    title: text!("Sommaire du JO"),
                    date: Some(d(2008, 7, 2)),
                },
                RelatedLink {
                    target: None,
                    nature: RelatedNature::Jorf,
                    title: text!("Texte sans identifiant"),
                    date: None,
                },
            ]
        );
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn the_target_comes_from_cid_text_only_when_id_is_absent() {
        let json = r#"{"liensCite":[
            {"cidText":" jorftext000024096255 ","nature":"LODA"},
            {"id":" ","cidText":"LEGITEXT000006070721","nature":"CODE"},
            {"nature":"LODA","name":"Ni l'un ni l'autre"}]}"#;
        let cites = links(json, &mut Diagnostics::default()).unwrap().cites;
        assert_eq!(
            cites.iter().map(|link| link.target.clone()).collect::<Vec<_>>(),
            vec![
                Some(document("JORFTEXT000024096255")),
                Some(document("LEGITEXT000006070721")),
                None
            ]
        );
    }

    #[test]
    fn a_link_date_is_a_civil_day_without_the_open_ended_sentinel() {
        let json = r#"{"liensCite":[
            {"id":"LEGITEXT000006070721","nature":"CODE","dateVigeur":1214956800000},
            {"id":"LEGITEXT000006070721","nature":"CODE","dateVigeur":32472144000000},
            {"id":"LEGITEXT000006070721","nature":"CODE","dateVigeur":"2011-05-31"},
            {"id":"LEGITEXT000006070721","nature":"CODE"}]}"#;
        let dates: Vec<_> = links(json, &mut Diagnostics::default())
            .unwrap()
            .cites
            .into_iter()
            .map(|link| link.date)
            .collect();
        assert_eq!(dates, vec![Some(d(2008, 7, 2)), None, Some(d(2011, 5, 31)), None]);
    }

    #[test]
    fn an_unreadable_link_date_is_a_decode_error_naming_the_list() {
        for (list, path) in [("liensCite", "liensCite[]"), ("liensCitePar", "liensCitePar[]")] {
            let json =
                format!(r#"{{"{list}":[{{"id":"LEGITEXT000006070721","dateVigeur":"hier"}}]}}"#);
            assert_eq!(
                links(&json, &mut Diagnostics::default()),
                Err(decode_error(
                    RELATED_LINKS_ARTICLE,
                    &format!("{path}.dateVigeur: unreadable date")
                ))
            );
        }
    }

    #[test]
    fn a_suffix_on_a_link_target_is_stripped_and_counted() {
        let json = r#"{"liensCite":[{"id":"LEGIARTI000006492398_01-01-2020","nature":"LODA"}]}"#;
        let mut diags = Diagnostics::default();
        let cites = links(json, &mut diags).unwrap().cites;
        assert_eq!(cites[0].target, Some(document("LEGIARTI000006492398")));
        assert_eq!(diags.upstream_suffix_stripped, 1);
    }

    #[test]
    fn the_recorded_related_links_response_maps_every_link() {
        let mut diags = Diagnostics::default();
        let RelatedLinks { cites, cited_by } = links(RELATED_FIXTURE, &mut diags).unwrap();
        assert_eq!(cites, Vec::new());
        assert_eq!(cited_by.len(), 14);
        assert_eq!(
            cited_by[0],
            RelatedLink {
                target: Some(document("JORFTEXT000019109412")),
                nature: RelatedNature::Loda,
                title: text!("Délibération n° 2008-161 du 3 juin 2008, v. init."),
                date: Some(d(2008, 7, 2)),
            }
        );
        assert_eq!(
            cited_by[6],
            RelatedLink {
                target: Some(document("KALIARTI000036512428")),
                nature: RelatedNature::Kali,
                title: text!("Accompagnement du développement numérique - art. 3.1 (VE)"),
                date: Some(d(2017, 9, 22)),
            }
        );
        // Les `JORFARTI` n'ont pas de cible ; les `LEGIARTI`, `KALIARTI` et `JORFTEXT` en ont une.
        let without_target = cited_by.iter().filter(|link| link.target.is_none()).count();
        assert_eq!(without_target, 8);
        assert!(cited_by.iter().all(|link| link.title.is_some()));
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn an_article_without_links_maps_to_no_links() {
        assert_eq!(
            links(NO_LINKS_FIXTURE, &mut Diagnostics::default()),
            Ok(RelatedLinks::default())
        );
        assert_eq!(links("{}", &mut Diagnostics::default()), Ok(RelatedLinks::default()));
    }
}
