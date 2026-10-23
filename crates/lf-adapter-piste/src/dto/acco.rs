//! Réponse de `/consult/acco` (04 §8.3, recherche 03 §10, §12.7).
//!
//! La réponse est enveloppée : `{executionTime, dereferenced, acco}`. Seule la clé `acco` est
//! déclarée. Les structs suivent la convention de `consult_text` : `Default` dérivé,
//! `#[serde(default)]` au niveau du conteneur, `Debug` et `PartialEq` sous `cfg(test)`
//! seulement, dates brutes (`RawDate`). Le code IDCC est lu en chaîne ou en nombre
//! (`opt_string_or_number`), comme `fileSize` et `numero`, déclarés mais jamais lus par le
//! mapping : un type inattendu sur l'un d'eux ne doit pas faire perdre l'accord (D-24). Le retrait
//! des champs non lus (ceux-là, `dateDiffusion`, `dateFin`, `conformeVersionIntegrale`) est
//! renvoyé au plan 2g.
//!
//! `data` (le fichier joint en base64, jusqu'à 1 Mio) et `attachment` ne sont jamais déclarés :
//! `serde_json` saute une valeur non déclarée sans construire de valeur, à toute profondeur
//! (mémoire bornée par la taille du corps). Il en va de même de `signataires` et de
//! `adressesPostales`.

use serde::Deserialize;

use super::de::{self, RawDate};

/// Enveloppe de la réponse de `/consult/acco`.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ConsultAccoResponseDto {
    /// Accord demandé ; `None` si la clé est absente ou `null`.
    pub acco: Option<AccordDto>,
}

/// Accord d'entreprise.
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct AccordDto {
    /// Identifiant de l'accord.
    pub id: Option<String>,
    /// Nature du texte (`ACCORD`…).
    pub nature: Option<String>,
    /// Titre de l'accord.
    pub titre_texte: Option<String>,
    /// Raison sociale de l'entreprise.
    pub raison_sociale: Option<String>,
    /// Secteur d'activité (`""` reste `Some("")`).
    pub secteur: Option<String>,
    /// Taille du fichier joint, avec son unité (`624,3 Ko`), en chaîne ou en nombre, rendue en
    /// chaîne (`""` : alors `None`) ; jamais lue par le mapping, tolérante pour ne pas faire perdre
    /// l'accord (D-24).
    #[serde(deserialize_with = "de::opt_string_or_number")]
    pub file_size: Option<String>,
    /// Numéro SIRET (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub siret: Option<String>,
    /// Code APE (`""` : alors `None`).
    #[serde(deserialize_with = "de::opt_nonempty")]
    pub code_ape: Option<String>,
    /// Numéro de dépôt, en chaîne ou en nombre, rendu en chaîne (`""` : alors `None`) ; jamais lu
    /// par le mapping, tolérant pour ne pas faire perdre l'accord (D-24).
    #[serde(deserialize_with = "de::opt_string_or_number")]
    pub numero: Option<String>,
    /// Code IDCC, lu en chaîne ou en nombre et rendu en chaîne.
    #[serde(deserialize_with = "de::opt_string_or_number")]
    pub code_idcc: Option<String>,
    /// Thèmes de l'accord.
    #[serde(deserialize_with = "de::bounded_list")]
    pub themes: Vec<LibelleDto>,
    /// Syndicats signataires.
    #[serde(deserialize_with = "de::bounded_list")]
    pub syndicats: Vec<LibelleDto>,
    /// Date du texte, brute.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_texte: Option<RawDate>,
    /// Date d'effet, brute.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_effet: Option<RawDate>,
    /// Date de dépôt, brute.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_depot: Option<RawDate>,
    /// Date de diffusion, brute.
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_diffusion: Option<RawDate>,
    /// Date de fin, brute (`2999-01-01` : sans fin).
    #[serde(deserialize_with = "de::opt_raw_date")]
    pub date_fin: Option<RawDate>,
    /// Vrai si le fichier joint est conforme à la version intégrale.
    pub conforme_version_integrale: Option<bool>,
}

/// Code et libellé (thème ou syndicat).
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct LibelleDto {
    /// Code.
    pub code: Option<String>,
    /// Libellé.
    pub libelle: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::decode::decode;
    use crate::dto::tests::{assert_too_many, repeated};

    const ENDPOINT: &str = "/consult/acco";

    /// `Some(valeur.to_owned())` : une macro et non une fonction, que `clippy::unnecessary_wraps`
    /// refuserait.
    macro_rules! text {
        ($value:expr) => {
            Some($value.to_owned())
        };
    }

    fn read(raw: &str) -> ConsultAccoResponseDto {
        decode(raw, ENDPOINT).unwrap()
    }

    fn accord(raw: &str) -> AccordDto {
        read(raw).acco.unwrap()
    }

    fn label(code: &str, libelle: &str) -> LibelleDto {
        LibelleDto { code: text!(code), libelle: text!(libelle) }
    }

    /// Message de l'erreur `Decode` rendue pour `raw`.
    fn error_of(raw: &str) -> String {
        format!("{:?}", decode::<ConsultAccoResponseDto>(raw, ENDPOINT).unwrap_err())
    }

    /// Enveloppe `acco` autour de `fields`, qui sont des champs JSON.
    fn wrapped(fields: &str) -> String {
        format!(r#"{{"acco":{{{fields}}}}}"#)
    }

    #[test]
    fn every_acco_list_is_budgeted() {
        let objects = repeated("{}", 200_001);
        for field in ["themes", "syndicats"] {
            let raw = format!(r#"{{"acco":{{"{field}":{objects}}}}}"#);
            assert_too_many::<ConsultAccoResponseDto>(
                &raw,
                ENDPOINT,
                &format!("acco.{field}[200000]"),
            );
        }
    }

    #[test]
    fn a_nominal_agreement_is_read() {
        let raw = r#"{"executionTime":3,"dereferenced":false,"acco":{
            "id":"ACCOTEXT000053643828","origine":"ACCO","nature":"ACCORD",
            "url":"texte/acco/ACCO/TEXT/00/00/53/64/38/ACCOTEXT000053643828.xml",
            "titreTexte":"Accord relatif au télétravail","raisonSociale":"CORSAIR (Télétravail)",
            "siret":"32862158600143","codeIdcc":"275","codeApe":"5110Z",
            "secteur":"Transport aérien",
            "themes":[{"code":"141","libelle":"Télétravail","groupe":"12"}],
            "syndicats":[{"code":"CFTC","libelle":"CFTC"},{"code":"AUT","libelle":"Autre"}],
            "numero":"T09426064936","fileSize":"624,3 Ko","conformeVersionIntegrale":true,
            "dateTexte":1766102400000,"dateEffet":1766188800000,"dateDepot":1772150400000,
            "dateDiffusion":1773100800000,"dateFin":32472144000000,
            "attachementUrl":"/2025/12/19/T09426064936-32862158600143.docx",
            "codeUniteSignataire":"05","dateMaj":1772409600000,"signataires":["91"],
            "adressesPostales":[]}}"#;
        assert_eq!(
            accord(raw),
            AccordDto {
                id: text!("ACCOTEXT000053643828"),
                nature: text!("ACCORD"),
                titre_texte: text!("Accord relatif au télétravail"),
                raison_sociale: text!("CORSAIR (Télétravail)"),
                secteur: text!("Transport aérien"),
                file_size: text!("624,3 Ko"),
                siret: text!("32862158600143"),
                code_ape: text!("5110Z"),
                numero: text!("T09426064936"),
                code_idcc: text!("275"),
                themes: vec![label("141", "Télétravail")],
                syndicats: vec![label("CFTC", "CFTC"), label("AUT", "Autre")],
                date_texte: Some(RawDate::Millis(1_766_102_400_000)),
                date_effet: Some(RawDate::Millis(1_766_188_800_000)),
                date_depot: Some(RawDate::Millis(1_772_150_400_000)),
                date_diffusion: Some(RawDate::Millis(1_773_100_800_000)),
                date_fin: Some(RawDate::Millis(32_472_144_000_000)),
                conforme_version_integrale: Some(true),
            }
        );
    }

    #[test]
    fn idcc_accepts_string_and_number() {
        let raws = ["\"275\"", "275", "\"0275\"", "\"\"", "null", "2.5", "true"];
        let read_idcc = |value: &str| accord(&wrapped(&format!(r#""codeIdcc":{value}"#))).code_idcc;
        assert_eq!(
            raws.map(read_idcc),
            [text!("275"), text!("275"), text!("0275"), None, None, None, None]
        );
    }

    #[test]
    fn unread_file_size_and_numero_accept_any_json_value() {
        use crate::dto::tests::assert_number_reading;
        assert_number_reading::<AccordDto>("fileSize", ENDPOINT, |dto| &dto.file_size);
        assert_number_reading::<AccordDto>("numero", ENDPOINT, |dto| &dto.numero);
        let raw = wrapped(r#""fileSize":624,"numero":[1],"titreTexte":"Accord""#);
        assert_eq!(
            accord(&raw),
            AccordDto {
                titre_texte: text!("Accord"),
                file_size: text!("624"),
                ..Default::default()
            }
        );
    }

    #[test]
    fn data_and_attachment_are_never_declared() {
        let raw = format!(
            r#"{{"acco":{{"id":"ACCOTEXT1","data":"{}","attachment":{{"content":["x"]}},
            "signataires":["91"]}}}}"#,
            "A".repeat(1 << 20)
        );
        assert_eq!(accord(&raw), AccordDto { id: text!("ACCOTEXT1"), ..Default::default() });
        let nested = format!(
            r#"{{"acco":{{"id":"ACCOTEXT1","data":{}0{}}}}}"#,
            r#"{"a":["#.repeat(100),
            "]}".repeat(100)
        );
        assert_eq!(accord(&nested), AccordDto { id: text!("ACCOTEXT1"), ..Default::default() });
    }

    #[test]
    fn empty_string_is_none() {
        let raw = wrapped(r#""siret":"","codeApe":" ","numero":"","secteur":"""#);
        assert_eq!(accord(&raw), AccordDto { secteur: text!(""), ..Default::default() });
    }

    #[test]
    fn null_envelope_or_null_lists_give_empty_dtos() {
        let raws = [r#"{"acco":null}"#, "{}", r#"{"executionTime":1,"dereferenced":false}"#];
        assert_eq!(raws.map(|raw| read(raw).acco), [None, None, None]);
        let raw = wrapped(r#""themes":null,"syndicats":null"#);
        assert_eq!(accord(&raw), AccordDto::default());
    }

    #[test]
    fn dates_in_every_encoding_are_kept_raw() {
        let raw =
            wrapped(r#""dateDepot":-5231606400000,"dateDiffusion":true,"dateEffet":"2025-12-20""#);
        assert_eq!(
            accord(&raw),
            AccordDto {
                date_depot: Some(RawDate::Millis(-5_231_606_400_000)),
                date_diffusion: None,
                date_effet: Some(RawDate::Text("2025-12-20".to_owned())),
                ..Default::default()
            }
        );
    }

    #[test]
    fn a_wrong_type_is_a_decode_error_with_a_json_path_and_no_value() {
        assert_eq!(
            error_of(r#"{"acco":{"themes":[{"code":"141"},{"libelle":["secret-text-42"]}]}}"#),
            "Decode { detail: \"/consult/acco: acco.themes[1].libelle: invalid type: sequence, \
             expected a string\" }"
        );
    }

    #[test]
    fn a_root_that_is_not_an_object_is_a_decode_error() {
        assert_eq!(
            error_of("null"),
            "Decode { detail: \"/consult/acco: .: invalid type: null, expected struct \
             ConsultAccoResponseDto\" }"
        );
    }
}
