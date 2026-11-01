//! Réponse de `/search` (04 §8.3, recherche 03 §4, §9, §12.3).
//!
//! Un module par endpoint : ces structs ne sont partagées avec aucun autre endpoint, pas même
//! `/consult/juri`, dont la forme est voisine mais distincte. Elles ne servent qu'à lire du
//! JSON, sans règle métier : le texte est gardé tel quel (surlignages `<mark>`, retours à la
//! ligne), les dates restent brutes (`RawDate`).
//!
//! Chaque struct dérive `Default` et porte `#[serde(default)]` au niveau du conteneur : un champ
//! absent prend sa valeur par défaut (rien ou liste vide), sans répéter l'attribut sur chaque
//! champ. Elles ne dérivent `Debug` et `PartialEq` que pour les tests : elles portent des
//! extraits de décisions et de textes, et un `{:?}` accidentel ne doit pas compiler.
//!
//! Seuls les champs que le mapping lit sont déclarés (04 §8.1) : `facets`, qui peut peser jusqu'à
//! 88 % d'une réponse KALI, ne l'a jamais été ; depuis le plan 2g, `solution`, `nor`, `num`,
//! `themes`, `appellations` et `resumePrincipal` d'un résultat, `id` et `title` d'une section et
//! `searchFieldName` d'un extrait sont sautés par `serde_json` sans construire de valeur (mémoire
//! bornée par la taille du corps) et ne peuvent faire échouer aucun décodage. Aucune table indexée
//! par du texte du corps (`HashMap`) : une clé venue du corps pourrait entrer dans le chemin d'une
//! erreur.

use lf_application::ports::CorpusError;
use lf_domain::search::PageSize;
use serde::{Deserialize, Deserializer};

use super::de::{self, RawDate};

/// Nombre maximal de résultats d'une page : `PageSize::MAX` (04 §8.5, limite 1 (b)).
pub(crate) const MAX_RESULTS: usize = 100;

/// Lit `results` : au plus [`MAX_RESULTS`] éléments, le suivant donnant `too many elements`.
fn bounded_results<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<SearchResultDto>, D::Error> {
    de::capped_list(deserializer, MAX_RESULTS)
}

/// Réponse de `/search`.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct SearchResponseDto {
    /// Résultats de la page (`null` ou absent : aucun).
    #[serde(deserialize_with = "bounded_results")]
    pub results: Vec<SearchResultDto>,
    /// Nombre total de résultats, en nombre ou en chaîne de chiffres (sinon `None`).
    #[serde(deserialize_with = "de::opt_u64_lenient")]
    pub total_result_number: Option<u64>,
    /// Nombre total d'articles trouvés, lu comme le nombre de résultats.
    #[serde(deserialize_with = "de::opt_u64_lenient")]
    pub total_article_result_number: Option<u64>,
    /// Mode de pagination appliqué par PISTE : `DEFAUT` ou `ARTICLE` ; nombre lu comme chaîne ;
    /// tout autre type ignoré (R13).
    #[serde(deserialize_with = "de::opt_string_or_number")]
    pub type_pagination: Option<String>,
}

impl SearchResponseDto {
    /// Vérifie que la page ne porte pas plus de résultats que `page_size`.
    ///
    /// À appeler avant toute autre lecture de la réponse : PISTE peut renvoyer plus de résultats
    /// que demandé, et le domaine borne une page à `page_size` résultats.
    ///
    /// # Errors
    ///
    /// [`CorpusError::Decode`] (`<endpoint>: results: more results than page size`) si la page
    /// porte plus de `page_size` résultats.
    pub(crate) fn ensure_fits(
        &self,
        endpoint: &str,
        page_size: PageSize,
    ) -> Result<(), CorpusError> {
        if fits(self.results.len(), page_size.get()) {
            Ok(())
        } else {
            Err(CorpusError::Decode {
                detail: format!("{endpoint}: results: more results than page size"),
            })
        }
    }
}

impl SearchResponseDto {
    /// Ne garde que la fenêtre demandée d'une page reçue en taille `wire_size` (L3-05) : les
    /// `keep` résultats qui suivent les `skip` premiers.
    ///
    /// La page reçue est bornée à `wire_size`, la taille réellement envoyée à PISTE, avant toute
    /// coupe. Sans doublement de page (`skip` nul et `keep == wire_size`), la fenêtre garde tout.
    /// Une fenêtre au-delà des résultats reçus rend une page vide.
    ///
    /// # Errors
    ///
    /// [`CorpusError::Decode`] (`<endpoint>: results: more results than page size`) si la page
    /// porte plus de `wire_size` résultats.
    pub(crate) fn keep_window(
        &mut self,
        endpoint: &str,
        wire_size: u32,
        skip: usize,
        keep: u32,
    ) -> Result<(), CorpusError> {
        if !fits(self.results.len(), wire_size) {
            return Err(CorpusError::Decode {
                detail: format!("{endpoint}: results: more results than page size"),
            });
        }
        let kept = usize::try_from(keep).unwrap_or(usize::MAX);
        self.results =
            std::mem::take(&mut self.results).into_iter().skip(skip).take(kept).collect();
        Ok(())
    }
}

/// Vrai si `results` résultats tiennent dans une page de `page_size` (borne comprise).
fn fits(results: usize, page_size: u32) -> bool {
    u32::try_from(results).is_ok_and(|count| count <= page_size)
}

/// Un résultat de `/search` : texte, décision ou convention.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct SearchResultDto {
    /// Titres du résultat, un par version ou par texte.
    #[serde(deserialize_with = "de::bounded_list")]
    pub titles: Vec<SearchTitleDto>,
    /// Extrait de texte, avec ses surlignages `<mark>` et son retour à la ligne final.
    pub text: Option<String>,
    /// Nature, telle que PISTE l'écrit (`LOI`, `arret` en minuscules pour la jurisprudence).
    pub nature: Option<String>,
    /// Origine du résultat (`LEGI`, `JURI`, `KALI`…).
    pub origin: Option<String>,
    /// État juridique (`VIGUEUR`, `ABROGE`…).
    pub etat: Option<String>,
    /// Date principale, brute.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date: Option<RawDate>,
    /// Date de publication, brute.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_publication: Option<RawDate>,
    /// Date de signature, brute.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_signature: Option<RawDate>,
    /// Identifiant de convention collective, en chaîne ou en nombre, rendu en chaîne.
    #[serde(deserialize_with = "de::opt_string_or_number")]
    pub idcc: Option<String>,
    /// Raison sociale d'une partie à une convention.
    pub raison_sociale: Option<String>,
    /// Sections trouvées, avec leurs extraits.
    #[serde(deserialize_with = "de::bounded_list")]
    pub sections: Vec<SearchSectionDto>,
}

/// Titre d'un résultat.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct SearchTitleDto {
    /// Identifiant de la version ou du texte.
    pub id: Option<String>,
    /// Identifiant chronologique du texte.
    pub cid: Option<String>,
    /// Titre, avec ses surlignages `<mark>`.
    pub title: Option<String>,
}

/// Section d'un résultat.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct SearchSectionDto {
    /// Extraits trouvés dans la section.
    #[serde(deserialize_with = "de::bounded_list")]
    pub extracts: Vec<SearchExtractDto>,
}

/// Extrait d'une section.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct SearchExtractDto {
    /// Identifiant de l'article (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub id: Option<String>,
    /// Numéro de l'article, en chaîne ou en nombre, rendu en chaîne (`""` : alors `None`) : en KALI, du texte libre non fiable, non validé ici.
    #[serde(deserialize_with = "de::opt_string_or_number")]
    pub num: Option<String>,
    /// Passages trouvés, avec leurs surlignages.
    #[serde(deserialize_with = "de::bounded_list")]
    pub values: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::decode::decode;
    use lf_application::ports::CorpusError;
    use lf_domain::search::PageSize;

    const ENDPOINT: &str = "/search";

    /// `Some(valeur.to_owned())` : une macro et non une fonction, que `clippy::unnecessary_wraps`
    /// refuserait.
    macro_rules! text {
        ($value:expr) => {
            Some($value.to_owned())
        };
    }

    fn read(raw: &str) -> SearchResponseDto {
        decode(raw, ENDPOINT).unwrap()
    }

    fn first(raw: &str) -> SearchResultDto {
        read(raw).results.remove(0)
    }

    /// Premier résultat de `{"results":[{"<field>":<raw>}]}`.
    fn result_with(field: &str, raw: &str) -> SearchResultDto {
        first(&format!(r#"{{"results":[{{"{field}":{raw}}}]}}"#))
    }

    #[test]
    fn a_nominal_loda_response_is_read() {
        let raw = r#"{"executionTime":12,"totalResultNumber":1786,"totalArticleResultNumber":0,
            "typePagination":"DEFAUT","facets":[],
            "results":[{"titles":[{"id":"LEGITEXT000033205014_08-09-2023",
            "cid":"JORFTEXT000033202746","title":"LOI pour une <mark>République</mark> numérique"}],
            "type":"TEXTE","nature":"LOI","origin":"LEGI","etat":"VIGUEUR","legalStatus":"VIGUEUR",
            "descriptionFusionHtml":"<b>x</b>","date":"2016-10-09T00:00:00.000+0000",
            "datePublication":1475884800000,"dateSignature":null,"idcc":null,
            "nor":"ECFI1524250L","appellations":["loi Lemaire","loi numérique"],
            "sections":[],"additionalResult":{}}]}"#;
        assert_eq!(
            read(raw),
            SearchResponseDto {
                results: vec![SearchResultDto {
                    titles: vec![SearchTitleDto {
                        id: text!("LEGITEXT000033205014_08-09-2023"),
                        cid: text!("JORFTEXT000033202746"),
                        title: text!("LOI pour une <mark>République</mark> numérique"),
                    }],
                    nature: text!("LOI"),
                    origin: text!("LEGI"),
                    etat: text!("VIGUEUR"),
                    date: Some(RawDate::Text("2016-10-09T00:00:00.000+0000".to_owned())),
                    date_publication: Some(RawDate::Millis(1_475_884_800_000)),
                    ..Default::default()
                }],
                total_result_number: Some(1786),
                total_article_result_number: Some(0),
                type_pagination: text!("DEFAUT"),
            }
        );
    }

    #[test]
    fn a_juri_result_keeps_its_snippet_and_skips_what_the_mapping_never_reads() {
        let raw = r#"{"results":[{"titles":[{"id":"JURITEXT000001","cid":"JURITEXT000001",
            "title":"Cour de cassation, 1 janvier 2020, 23-12.345"}],
            "text":"une <mark>faute</mark> grave\n","nature":"arret","origin":"JURI",
            "solution":"Rejet","num":"23-12.345","themes":["Travail","Licenciement"],
            "raisonSociale":"ACME","resumePrincipal":["Un résumé.<br/>"],
            "dateSignature":"1475884800000",
            "sections":[{"id":null,"title":null,"extracts":[{"id":null,"num":null,
            "values":["a","b"],"searchFieldName":"Résumé principal"}]}]}]}"#;
        assert_eq!(
            first(raw),
            SearchResultDto {
                titles: vec![SearchTitleDto {
                    id: text!("JURITEXT000001"),
                    cid: text!("JURITEXT000001"),
                    title: text!("Cour de cassation, 1 janvier 2020, 23-12.345"),
                }],
                text: text!("une <mark>faute</mark> grave\n"),
                nature: text!("arret"),
                origin: text!("JURI"),
                raison_sociale: text!("ACME"),
                date_signature: Some(RawDate::Millis(1_475_884_800_000)),
                sections: vec![SearchSectionDto {
                    extracts: vec![SearchExtractDto {
                        id: None,
                        num: None,
                        values: vec!["a".to_owned(), "b".to_owned()],
                    }],
                }],
                ..Default::default()
            }
        );
    }

    #[test]
    fn results_null_or_absent_is_an_empty_list() {
        let counts =
            [r#"{"results":null}"#, "{}", r#"{"results":[]}"#].map(|raw| read(raw).results.len());
        assert_eq!(counts, [0, 0, 0]);
    }

    fn empty_results(count: usize) -> String {
        format!(r#"{{"results":[{}]}}"#, vec!["{}"; count].join(","))
    }

    fn expected_decode(detail: &str) -> CorpusError {
        CorpusError::Decode { detail: detail.to_owned() }
    }

    fn page_size(value: u32) -> PageSize {
        PageSize::new(value).unwrap()
    }

    #[test]
    fn max_results_equals_the_domain_page_size_maximum() {
        assert_eq!(u32::try_from(MAX_RESULTS), Ok(PageSize::MAX));
    }

    #[test]
    fn a_page_of_100_results_is_read() {
        assert_eq!(read(&empty_results(100)).results.len(), 100);
    }

    #[test]
    fn a_page_of_101_results_maps_to_decode_with_a_path() {
        let raw = empty_results(101);
        assert_eq!(raw.len(), 316);
        assert_eq!(
            decode::<SearchResponseDto>(&raw, ENDPOINT).unwrap_err(),
            expected_decode("/search: results[100]: too many elements")
        );
    }

    #[test]
    fn a_secret_in_a_refused_page_never_reaches_the_detail() {
        let raw =
            format!(r#"{{"results":[{}]}}"#, vec![r#"{"nature":"sekret-42-Ω"}"#; 101].join(","));
        let error = decode::<SearchResponseDto>(&raw, ENDPOINT).unwrap_err();
        assert!(!format!("{error:?}").contains("sekret"));
        assert!(!error.to_string().contains("sekret"));
    }

    #[test]
    fn the_search_lists_share_the_document_budget() {
        let document = |count: usize| {
            format!(
                r#"{{"results":[{{"sections":[{{"extracts":[{{"values":[{}]}}]}}]}}]}}"#,
                vec![r#""""#; count].join(",")
            )
        };
        let fitting = read(&document(199_997));
        assert_eq!(fitting.results[0].sections[0].extracts[0].values.len(), 199_997);
        assert_eq!(
            decode::<SearchResponseDto>(&document(199_998), ENDPOINT).unwrap_err(),
            expected_decode(
                "/search: results[0].sections[0].extracts[0].values[199997]: too many elements"
            )
        );
    }

    #[test]
    fn every_search_list_is_paid_on_the_budget() {
        fn not_armed<T: serde::de::DeserializeOwned>(raw: &str) -> bool {
            serde_json::from_str::<T>(raw)
                .err()
                .is_some_and(|error| error.to_string().starts_with("decoding budget not armed"))
        }
        let refused = [
            not_armed::<SearchResponseDto>(r#"{"results":[{}]}"#),
            not_armed::<SearchResultDto>(r#"{"titles":[{}]}"#),
            not_armed::<SearchResultDto>(r#"{"sections":[{}]}"#),
            not_armed::<SearchSectionDto>(r#"{"extracts":[{}]}"#),
            not_armed::<SearchExtractDto>(r#"{"values":[""]}"#),
        ];
        assert_eq!(refused, [true; 5]);
    }

    #[test]
    fn ensure_fits_table() {
        let accepted = [(0, 1), (1, 1), (10, 10), (100, 100)].map(|(count, size)| {
            read(&empty_results(count)).ensure_fits(ENDPOINT, page_size(size))
        });
        assert_eq!(accepted, [Ok(()), Ok(()), Ok(()), Ok(())]);
        let refused = [(2, 1), (11, 10)].map(|(count, size)| {
            read(&empty_results(count)).ensure_fits(ENDPOINT, page_size(size))
        });
        let expected = Err(expected_decode("/search: results: more results than page size"));
        assert_eq!(refused, [expected.clone(), expected]);
    }

    #[test]
    fn keep_window_cuts_the_wanted_slot_of_a_doubled_page() {
        let ids = |dto: &SearchResponseDto| -> Vec<Option<String>> {
            dto.results.iter().map(|r| r.titles.first().and_then(|t| t.id.clone())).collect()
        };
        let two = r#"{"results":[{"titles":[{"id":"A"}]},{"titles":[{"id":"B"}]}]}"#;
        for (skip, want) in [(0, "A"), (1, "B")] {
            let mut dto = read(two);
            dto.keep_window(ENDPOINT, 2, skip, 1).unwrap();
            assert_eq!(ids(&dto), [Some(want.to_owned())], "skip {skip}");
        }
        // Dernière page impaire : un seul résultat reçu, la seconde case est vide.
        let mut lone = read(r#"{"results":[{"titles":[{"id":"A"}]}]}"#);
        lone.keep_window(ENDPOINT, 2, 1, 1).unwrap();
        assert_eq!(lone.results.len(), 0);
        // Sans doublement la fenêtre garde tout.
        let mut whole = read(two);
        whole.keep_window(ENDPOINT, 2, 0, 2).unwrap();
        assert_eq!(ids(&whole).len(), 2);
    }

    #[test]
    fn keep_window_refuses_a_page_wider_than_the_wire_size() {
        let mut dto = read(&empty_results(3));
        assert_eq!(
            dto.keep_window(ENDPOINT, 2, 0, 1),
            Err(expected_decode("/search: results: more results than page size"))
        );
        assert_eq!(dto.results.len(), 3, "refusée avant toute coupe");
    }

    #[test]
    fn fits_is_inclusive_at_the_page_size() {
        assert!(fits(10, 10));
        assert!(!fits(11, 10));
        assert!(fits(0, 1));
        assert!(!fits(usize::MAX, u32::MAX));
    }

    #[test]
    fn num_accepts_int_and_string() {
        use crate::dto::tests::assert_number_reading;
        assert_number_reading::<SearchExtractDto>("num", ENDPOINT, |dto| &dto.num);
    }

    #[test]
    fn an_empty_object_is_all_defaults() {
        assert_eq!(read("{}"), SearchResponseDto::default());
        assert_eq!(first(r#"{"results":[{}]}"#), SearchResultDto::default());
    }

    #[test]
    fn total_result_number_accepts_number_string_and_absent() {
        let raws = ["122147", r#""122147""#, "null", "-1", r#""abc""#];
        let mut totals = raws
            .map(|raw| read(&format!(r#"{{"totalResultNumber":{raw}}}"#)).total_result_number)
            .to_vec();
        totals.push(read("{}").total_result_number);
        assert_eq!(totals, [Some(122_147), Some(122_147), None, None, None, None]);
        assert_eq!(
            read(r#"{"totalArticleResultNumber":"1"}"#).total_article_result_number,
            Some(1)
        );
    }

    #[test]
    fn type_pagination_accepts_a_string_a_number_and_ignores_the_rest() {
        let read_raw = |raw: &str| read(&format!(r#"{{"typePagination":{raw}}}"#)).type_pagination;
        let raws = [
            r#""ARTICLE""#,
            r#""DEFAUT""#,
            "3",
            "-1",
            "1.5",
            "true",
            r#"{"a":1}"#,
            "[1,2]",
            "null",
            r#""""#,
            r#""  ""#,
        ];
        assert_eq!(
            raws.map(read_raw),
            [
                text!("ARTICLE"),
                text!("DEFAUT"),
                text!("3"),
                text!("-1"),
                None,
                None,
                None,
                None,
                None,
                None,
                None
            ]
        );
        assert_eq!(read("{}").type_pagination, None);
    }

    #[test]
    fn type_pagination_skips_a_deep_value_without_building_it() {
        // 200 niveaux : un `serde_json::Value` échouerait dès 128.
        let deep = format!(r#"{{"typePagination":{}1{}}}"#, "[".repeat(200), "]".repeat(200));
        assert_eq!(read(&deep).type_pagination, None);
    }

    #[test]
    fn a_page_with_a_non_string_type_pagination_is_still_read() {
        let page = read(r#"{"typePagination":{"x":1},"totalResultNumber":5,"results":[{}]}"#);
        assert_eq!(page.total_result_number, Some(5));
        assert_eq!(page.results.len(), 1);
        assert_eq!(page.type_pagination, None);
    }

    #[test]
    fn dates_come_in_three_encodings_including_negative_milliseconds() {
        let encodings = SearchResultDto {
            date: result_with("date", "-5231606400000").date,
            date_publication: result_with("datePublication", r#""2999-01-01T00:00:00.000+0000""#)
                .date_publication,
            date_signature: result_with("dateSignature", r#""1475884800000""#).date_signature,
            ..Default::default()
        };
        assert_eq!(
            encodings,
            SearchResultDto {
                date: Some(RawDate::Millis(-5_231_606_400_000)),
                date_publication: Some(RawDate::Text("2999-01-01T00:00:00.000+0000".to_owned())),
                date_signature: Some(RawDate::Millis(1_475_884_800_000)),
                ..Default::default()
            }
        );
        let not_dates = [
            result_with("date", "true").date,
            result_with("datePublication", "{}").date_publication,
            result_with("dateSignature", "null").date_signature,
        ];
        assert_eq!(not_dates, [None, None, None]);
    }

    #[test]
    fn idcc_accepts_string_and_number() {
        let raws = [r#""275""#, "1486", "null", r#""""#, "1.5"];
        assert_eq!(
            raws.map(|raw| result_with("idcc", raw).idcc),
            [text!("275"), text!("1486"), None, None, None]
        );
    }

    #[test]
    fn kali_sections_with_empty_identifiers_become_none() {
        let raw = r#"{"results":[{"sections":[{"id":"","title":"  ","extracts":[
            {"id":"KALIARTI000001","num":"8.4 de la convention)","values":null},
            {"id":"","num":"","values":["x"]}]},
            {"id":"SEC-1","title":"Titre <mark>un</mark>","extracts":[]}]}]}"#;
        assert_eq!(
            first(raw).sections,
            vec![
                SearchSectionDto {
                    extracts: vec![
                        SearchExtractDto {
                            id: text!("KALIARTI000001"),
                            num: text!("8.4 de la convention)"),
                            values: Vec::new(),
                        },
                        SearchExtractDto { id: None, num: None, values: vec!["x".to_owned()] },
                    ],
                },
                SearchSectionDto { extracts: Vec::new() }
            ]
        );
    }

    #[test]
    fn null_lists_inside_a_result_are_empty() {
        let raw = r#"{"results":[{"titles":null,"sections":null}]}"#;
        assert_eq!(first(raw), SearchResultDto::default());
    }

    #[test]
    fn unknown_and_undeclared_fields_are_ignored() {
        let raw = r#"{"unknown":1,"totalResultNumber":3,"moreArticle":true,
            "moreArticlesCount":4,"results":[{"nature":"LOI","signataires":{"a":"b"},
            "additionalResult":{"x":[1]},"unknownField":"zzz"}]}"#;
        assert_eq!(
            read(raw),
            SearchResponseDto {
                results: vec![SearchResultDto { nature: text!("LOI"), ..Default::default() }],
                total_result_number: Some(3),
                ..Default::default()
            }
        );
    }

    #[test]
    fn facets_are_skipped_without_building_a_value() {
        // (a) 200 niveaux : un `serde_json::Value` échouerait dès 128.
        let deep = format!(
            r#"{{"facets":{}1{},"totalResultNumber":5}}"#,
            "[".repeat(200),
            "]".repeat(200)
        );
        assert_eq!(read(&deep).total_result_number, Some(5));
        // (b) environ 1 Mio de facettes.
        let elements = vec![r#"{"facetElem":"X","values":{"a":1}}"#; 30_000].join(",");
        let wide = format!(r#"{{"facets":[{elements}],"totalResultNumber":7}}"#);
        assert!(wide.len() > 1_000_000);
        assert_eq!(read(&wide).total_result_number, Some(7));
    }

    #[test]
    fn a_wrong_type_is_a_decode_error_with_a_json_path_and_no_value() {
        let error = decode::<SearchResponseDto>(
            r#"{"results":[{"titles":[{"id":"ok"}]},{"titles":"secret-text-42"}]}"#,
            ENDPOINT,
        )
        .unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            r#"Decode { detail: "/search: results[1].titles: invalid type: string, expected a sequence" }"#
        );
    }

    #[test]
    fn a_root_that_is_not_an_object_is_a_decode_error() {
        let details = ["null", r#""x""#, "<html>"].map(|raw| {
            let error = decode::<SearchResponseDto>(raw, ENDPOINT).unwrap_err();
            format!("{error:?}")
        });
        assert_eq!(
            details,
            [
                r#"Decode { detail: "/search: .: invalid type: null, expected struct SearchResponseDto" }"#,
                r#"Decode { detail: "/search: .: invalid type: string, expected struct SearchResponseDto" }"#,
                r#"Decode { detail: "/search: .: expected value at line 1 column 1" }"#,
            ]
        );
    }
}
