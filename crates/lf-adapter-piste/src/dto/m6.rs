//! DTO des réponses de M6 : `/list/code`, `kaliContIdcc` et `getJoWithNor` (aussi
//! `eliAndAliasRedirectionTexte`) (04 §7.1, §7.2, §8.3).
//!
//! `getArticleWithIdAndNum` et `getArticleWithIdEliOrAlias` répondent la même enveloppe que
//! `getArticle` (`{executionTime, dereferenced, article}`, relevé en sandbox le 2026-10-29) : ils
//! lisent [`GetArticleResponseDto`](super::article::GetArticleResponseDto), sans doublon. `/search`
//! lit [`SearchResponseDto`](super::search::SearchResponseDto).
//!
//! Mêmes conventions que les autres DTO : `Default` dérivé, `#[serde(default)]` au niveau du
//! conteneur, `Debug` et `PartialEq` sous `cfg(test)` seulement, et seuls les champs que le mapping
//! lit sont déclarés. Cela compte ici : `kaliContIdcc` renvoie la convention entière (193 Kio pour
//! l'IDCC 2121, `sections` et `articles` compris) et `/list/code` deux facettes de 108 titres
//! chacune, que `serde_json` saute sans construire de valeur. Les formes de `getJoWithNor` et des
//! endpoints ELI sont lues de façon tolérante : seul `getJoWithNor` a été enregistré, les endpoints
//! ELI restent **NON VÉRIFIÉS** [V-133 · eli-redirection-endpoints].

use serde::Deserialize;

use super::de;

/// Réponse de `/list/code`.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct CodeListResponseDto {
    /// Codes de la page (`null` ou absent : aucun).
    #[serde(deserialize_with = "de::bounded_list")]
    pub results: Vec<CodeEntryDto>,
    /// Nombre total de codes, en nombre ou en chaîne de chiffres (sinon `None`).
    #[serde(deserialize_with = "de::opt_u64_lenient")]
    pub total_result_number: Option<u64>,
}

/// Un code de `/list/code`, abrogé ou non.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct CodeEntryDto {
    /// Identifiant `LEGITEXT` du code.
    pub id: Option<String>,
    /// Identifiant chronologique, égal à `id` pour un code.
    pub cid: Option<String>,
    /// État juridique (`VIGUEUR`, `ABROGE`).
    pub etat: Option<String>,
    /// Titre du code, ex. `Code civil`.
    pub titre: Option<String>,
}

/// Réponse de `/consult/kaliContIdcc` : le conteneur d'une convention collective.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct KaliContDto {
    /// Identifiant du conteneur (`KALICONT…`).
    pub id: Option<String>,
    /// Titre de la convention.
    pub titre: Option<String>,
    /// IDCC, en chaîne ou en nombre, rendu en chaîne.
    #[serde(deserialize_with = "de::opt_string_or_number")]
    pub num: Option<String>,
    /// Identifiants `KALITEXT…` des textes de base du conteneur.
    #[serde(deserialize_with = "de::bounded_list")]
    pub texte_base_id: Vec<String>,
    /// Sections de premier niveau : une par texte de base (avec son `etat`), puis les textes
    /// attachés et salaires (sans `id`). Les sous-sections et les articles ne sont pas lus.
    #[serde(deserialize_with = "de::bounded_list")]
    pub sections: Vec<KaliSectionDto>,
}

/// Une section de premier niveau d'un conteneur KALI : le texte de base et son état.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct KaliSectionDto {
    /// Identifiant `KALITEXT…` du texte de base (absent des sections de textes attachés).
    pub id: Option<String>,
    /// État juridique du texte (`VIGUEUR_ETEN`, `ABROGE`…).
    pub etat: Option<String>,
    /// Intitulé, précédé de `Texte de base : ` pour un texte de base.
    pub title: Option<String>,
}

/// Réponse de `/consult/getJoWithNor` et de `/consult/eliAndAliasRedirectionTexte` : un texte du
/// Journal officiel.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ConsultJorfDto {
    /// Identifiant du texte, éventuellement suffixé d'une date de version (`JORFTEXT…_01-01-2999`).
    pub id: Option<String>,
    /// Identifiant chronologique du texte.
    pub cid: Option<String>,
    /// Titre du texte.
    pub title: Option<String>,
    /// Nature du texte (`LOI`, `DECRET`…).
    pub nature: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::decode::decode;
    use crate::dto::tests::{assert_number_reading, assert_too_many, repeated};

    /// `Some(valeur.to_owned())` : une macro et non une fonction, que `clippy::unnecessary_wraps`
    /// refuserait.
    macro_rules! text {
        ($value:expr) => {
            Some($value.to_owned())
        };
    }

    const PAGE_1: &str =
        include_str!("../../tests/fixtures/recorded/list/code/m6/list-code-page1.json");
    const PAGE_2: &str =
        include_str!("../../tests/fixtures/recorded/list/code/m6/list-code-page2.json");
    const IDCC_2121: &str = include_str!(
        "../../tests/fixtures/recorded/consult/kaliContIdcc/m6/kaliContIdcc-2121.json"
    );
    const IDCC_1486: &str =
        include_str!("../../tests/fixtures/derived/kaliContIdcc-1486-reduit.json");
    const NOR: &str = include_str!(
        "../../tests/fixtures/recorded/consult/getJoWithNor/m6/getJoWithNor-AGRX2502679L.json"
    );

    #[test]
    fn the_recorded_code_pages_hold_the_108_codes() {
        let first: CodeListResponseDto = decode(PAGE_1, "/list/code").unwrap();
        let second: CodeListResponseDto = decode(PAGE_2, "/list/code").unwrap();
        assert_eq!(
            (first.results.len(), second.results.len(), first.total_result_number),
            (100, 8, Some(108))
        );
        assert_eq!(second.total_result_number, Some(108));
        assert_eq!(
            first.results[0],
            CodeEntryDto {
                id: text!("LEGITEXT000006074232"),
                cid: text!("LEGITEXT000006074232"),
                etat: text!("VIGUEUR"),
                titre: text!("Code de déontologie des architectes"),
            }
        );
    }

    #[test]
    fn a_code_page_reads_loosely() {
        let page: CodeListResponseDto = decode(
            r#"{"totalResultNumber":"3","results":[{"id":"LEGITEXT000006070721"},{"titre":null}],
                "states":{"values":{"ABROGE":32}},"codeNames":{"values":{"Code civil":1}}}"#,
            "/list/code",
        )
        .unwrap();
        assert_eq!(page.total_result_number, Some(3));
        assert_eq!(
            page.results,
            [
                CodeEntryDto { id: text!("LEGITEXT000006070721"), ..CodeEntryDto::default() },
                CodeEntryDto::default()
            ]
        );
        let empty: CodeListResponseDto = decode("{}", "/list/code").unwrap();
        assert_eq!(empty, CodeListResponseDto::default());
        let null: CodeListResponseDto = decode(r#"{"results":null}"#, "/list/code").unwrap();
        assert_eq!(null, CodeListResponseDto::default());
    }

    #[test]
    fn the_code_list_is_bounded_by_the_document_budget() {
        let raw = format!(r#"{{"results":{}}}"#, repeated("{}", 200_001));
        assert_too_many::<CodeListResponseDto>(&raw, "/list/code", "results[200000]");
    }

    #[test]
    fn the_recorded_container_gives_its_base_text() {
        let cont: KaliContDto = decode(IDCC_2121, "/consult/kaliContIdcc").unwrap();
        assert_eq!(
            cont,
            KaliContDto {
                id: text!("KALICONT000005635096"),
                titre: text!("Convention collective nationale de l'édition du 14 janvier 2000\n"),
                num: text!("2121"),
                texte_base_id: vec!["KALITEXT000005654011".to_owned()],
                sections: vec![
                    KaliSectionDto {
                        id: text!("KALITEXT000005654011"),
                        etat: text!("VIGUEUR_ETEN"),
                        title: text!(
                            "Texte de base : Convention collective nationale de l'édition du 14 janvier 2000\n"
                        ),
                    },
                    KaliSectionDto {
                        id: None,
                        etat: text!("VIGUEUR"),
                        title: text!("Textes Attachés"),
                    },
                    KaliSectionDto {
                        id: None,
                        etat: text!("VIGUEUR"),
                        title: text!("Textes Salaires"),
                    },
                ],
            }
        );
    }

    #[test]
    fn the_reduced_container_of_the_idcc_1486_gives_the_state_of_each_base_text() {
        let cont: KaliContDto = decode(IDCC_1486, "/consult/kaliContIdcc").unwrap();
        assert_eq!(cont.texte_base_id, ["KALITEXT000005679895", "KALITEXT000044253019"]);
        let states: Vec<_> = cont
            .sections
            .iter()
            .map(|section| (section.id.as_deref(), section.etat.as_deref()))
            .collect();
        assert_eq!(
            states[..2],
            [
                (Some("KALITEXT000005679895"), Some("ABROGE")),
                (Some("KALITEXT000044253019"), Some("VIGUEUR_ETEN")),
            ]
        );
    }

    #[test]
    fn a_container_without_sections_reads_an_empty_list() {
        let cont: KaliContDto = decode(r#"{"sections":null}"#, "/consult/kaliContIdcc").unwrap();
        assert_eq!(cont, KaliContDto::default());
    }

    #[test]
    fn the_section_list_is_bounded_by_the_document_budget() {
        let raw = format!(r#"{{"sections":{}}}"#, repeated("{}", 200_001));
        assert_too_many::<KaliContDto>(&raw, "/consult/kaliContIdcc", "sections[200000]");
    }

    #[test]
    fn the_idcc_is_read_as_a_string_or_a_number() {
        assert_number_reading::<KaliContDto>("num", "/consult/kaliContIdcc", |dto| &dto.num);
    }

    #[test]
    fn the_base_text_list_is_bounded_by_the_document_budget() {
        let raw = format!(r#"{{"texteBaseId":{}}}"#, repeated(r#""x""#, 200_001));
        assert_too_many::<KaliContDto>(&raw, "/consult/kaliContIdcc", "texteBaseId[200000]");
    }

    #[test]
    fn the_recorded_journal_text_gives_its_identity() {
        let jorf: ConsultJorfDto = decode(NOR, "/consult/getJoWithNor").unwrap();
        assert_eq!(
            jorf,
            ConsultJorfDto {
                id: text!("JORFTEXT000052075814_01-01-2999"),
                cid: text!("JORFTEXT000052075814"),
                title: text!(
                    "LOI n° 2025-794 du 11 août 2025 visant à lever les contraintes à l'exercice du métier d'agriculteur (1)"
                ),
                nature: text!("LOI"),
            }
        );
    }

    #[test]
    fn an_empty_envelope_is_the_default() {
        let jorf: ConsultJorfDto = decode("{}", "/consult/getJoWithNor").unwrap();
        assert_eq!(jorf, ConsultJorfDto::default());
    }
}
