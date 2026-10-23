//! Décision de justice de `/consult/juri` vers `Decision` (03 §4.4 ; 04 §9.1, §9.2).
//!
//! Ce fichier porte pour l'instant les trois règles propres aux décisions : l'ECLI natif
//! ([`native_ecli`]), le numéro d'affaire ([`numero_affaire`], D30) et la répartition du
//! sommaire officiel par fond ([`summary_of`], D29). Aucune valeur du corps ne sort d'ici : un
//! ECLI natif refusé est compté (`invalid_native_ecli`) puis omis, sans erreur.

use lf_domain::{ecli::Ecli, entity::OfficialSummary, fond::Fond, text::clean_field};

use super::{Diagnostics, body::non_blank};
use crate::dto::juri::SommaireDto;

/// ECLI natif de `raw`, validé par `Ecli::parse` et jamais fabriqué.
///
/// Absent ou blanc : `None`, sans diagnostic. Présent mais refusé : `None`, et l'anomalie est
/// comptée (`invalid_native_ecli`).
fn native_ecli(raw: Option<&str>, diags: &mut Diagnostics) -> Option<Ecli> {
    let raw = non_blank(raw)?;
    let ecli = Ecli::parse(raw).ok();
    if ecli.is_none() {
        diags.note_invalid_ecli();
    }
    ecli
}

/// Numéros d'affaire de la décision (D30).
///
/// Les entrées de `listed` passent par `clean_field`, dans l'ordre reçu, celles qui sont vides
/// écartées, sans dédoublonnage. Si aucune n'est gardée et que le fond est `Cetat`, le numéro
/// `num` du texte, nettoyé, la remplace quand il n'est pas vide ; les autres fonds n'ont pas de
/// repli.
fn numero_affaire(fond: Fond, listed: &[String], num: Option<&str>) -> Vec<String> {
    let cleaned: Vec<String> = listed.iter().filter_map(|raw| clean_field(Some(raw))).collect();
    if cleaned.is_empty() && fond == Fond::Cetat {
        return clean_field(num).into_iter().collect();
    }
    cleaned
}

/// Sommaire officiel de la décision, réparti selon le fond (D29).
///
/// - `Fond::Juri` : `abstrats` donne les analyses ; `resumePrincipal` puis `autreResume`
///   donnent les titrages.
/// - `Fond::Cetat` : `resumePrincipal` donne les analyses ; `abstrats` puis `autreResume`
///   donnent les titrages (`autreResume` n'est pas attesté pour ce fond).
/// - Tout autre fond (`CONSTIT`, `CNIL`…) : sommaire vide.
///
/// L'ordre des entrées est gardé ; le nettoyage, le retrait du code de classement et le
/// dédoublonnage sont ceux d'[`OfficialSummary::new`]. Les `titrages` de premier niveau de la
/// réponse (identifiants de nomenclature) ne sont jamais lus.
fn summary_of(fond: Fond, sommaire: &[SommaireDto]) -> OfficialSummary {
    let (analyses, titrages) = match fond {
        Fond::Juri => (
            texts(sommaire, |entry| [entry.abstrats.as_deref(), None]),
            texts(sommaire, |entry| {
                [entry.resume_principal.as_deref(), entry.autre_resume.as_deref()]
            }),
        ),
        Fond::Cetat => (
            texts(sommaire, |entry| [entry.resume_principal.as_deref(), None]),
            texts(sommaire, |entry| [entry.abstrats.as_deref(), entry.autre_resume.as_deref()]),
        ),
        _ => return OfficialSummary::default(),
    };
    OfficialSummary::new(analyses, titrages)
}

/// Textes présents que `pick` tire de chaque entrée, entrée après entrée.
fn texts<'a>(
    sommaire: &'a [SommaireDto],
    pick: impl Fn(&'a SommaireDto) -> [Option<&'a str>; 2],
) -> Vec<&'a str> {
    sommaire.iter().flat_map(pick).flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Entrée de sommaire de `abstrats`, `resumePrincipal` et `autreResume`.
    fn entry(abstrats: Option<&str>, principal: Option<&str>, autre: Option<&str>) -> SommaireDto {
        SommaireDto {
            id: None,
            abstrats: abstrats.map(str::to_owned),
            resume_principal: principal.map(str::to_owned),
            autre_resume: autre.map(str::to_owned),
        }
    }

    /// `values` en `Vec<String>`.
    fn owned(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    /// Sommaire attendu de `analyses` et `titrages`.
    fn summary(analyses: &[&str], titrages: &[&str]) -> OfficialSummary {
        OfficialSummary { analyses: owned(analyses), titrages: owned(titrages) }
    }

    #[test]
    fn native_ecli_keeps_a_valid_value() {
        for raw in [
            "ECLI:FR:CCASS:2025:C100399",
            "ECLI:FR:CC:2025:2025.1173.QPC",
            "ECLI:FR:CECHR:2026:506507.20260529",
        ] {
            let mut diags = Diagnostics::default();
            let ecli = native_ecli(Some(raw), &mut diags);
            assert_eq!(ecli.as_ref().map(Ecli::as_str), Some(raw));
            assert_eq!(diags, Diagnostics::default(), "{raw}");
        }
    }

    #[test]
    fn native_ecli_invalid_is_none_and_counted() {
        for raw in [
            "ECLI:FR:CC:2025",
            "ecli:fr:cc:2025:2025.1173.qpc",
            "ECLI:EN:CC:2025:1",
            "ECLI:FR:CC:2025:2025.1173.QPC:extra",
        ] {
            let mut diags = Diagnostics::default();
            assert_eq!(native_ecli(Some(raw), &mut diags), None, "{raw}");
            assert_eq!(diags.invalid_native_ecli, 1, "{raw}");

            let mut diags = Diagnostics { invalid_native_ecli: 2, ..Diagnostics::default() };
            assert_eq!(native_ecli(Some(raw), &mut diags), None, "{raw}");
            assert_eq!(diags.invalid_native_ecli, 3, "{raw}");
        }
    }

    #[test]
    fn native_ecli_absent_or_blank_is_none_and_uncounted() {
        for raw in [None, Some(""), Some("  \n")] {
            let mut diags = Diagnostics { invalid_native_ecli: 2, ..Diagnostics::default() };
            assert_eq!(native_ecli(raw, &mut diags), None, "{raw:?}");
            assert_eq!(diags, Diagnostics { invalid_native_ecli: 2, ..Diagnostics::default() });
        }
    }

    #[test]
    fn numero_affaire_keeps_the_listed_values() {
        assert_eq!(numero_affaire(Fond::Juri, &owned(&["24-13.470"]), None), owned(&["24-13.470"]));
        assert_eq!(
            numero_affaire(Fond::Juri, &owned(&["", " 24-13.470 ", "  ", "25-00.001"]), None),
            owned(&["24-13.470", "25-00.001"])
        );
        assert_eq!(
            numero_affaire(Fond::Juri, &owned(&["24-13.470", "24-13.470"]), Some("ignoré")),
            owned(&["24-13.470", "24-13.470"])
        );
    }

    #[test]
    fn numero_affaire_fallback_is_for_cetat_only() {
        for num in [Some("506507"), Some(" 506507 ")] {
            assert_eq!(numero_affaire(Fond::Cetat, &[], num), owned(&["506507"]), "{num:?}");
        }
        assert_eq!(
            numero_affaire(Fond::Cetat, &owned(&["", " "]), Some("506507")),
            owned(&["506507"])
        );
        assert_eq!(
            numero_affaire(Fond::Cetat, &owned(&["22NC02259"]), Some("506507")),
            owned(&["22NC02259"])
        );
        for num in [None, Some("  ")] {
            assert_eq!(numero_affaire(Fond::Cetat, &[], num), Vec::<String>::new(), "{num:?}");
        }
        for fond in [Fond::Juri, Fond::Constit] {
            assert_eq!(numero_affaire(fond, &[], Some("506507")), Vec::<String>::new(), "{fond:?}");
        }
    }

    #[test]
    fn summary_of_juri_splits_abstrats_from_titrages() {
        let one = [entry(Some("Analyse A"), Some("Résumé principal"), Some("Autre résumé"))];
        assert_eq!(
            summary_of(Fond::Juri, &one),
            summary(&["Analyse A"], &["Résumé principal", "Autre résumé"])
        );
        let two = [entry(Some("A1"), Some("R1"), Some("X1")), entry(Some("A2"), Some("R2"), None)];
        assert_eq!(summary_of(Fond::Juri, &two), summary(&["A1", "A2"], &["R1", "X1", "R2"]));
    }

    #[test]
    fn summary_of_cetat_splits_resume_from_abstrats() {
        let entries = [
            entry(None, Some("01-03-02-02 Analyse un."), None),
            entry(None, Some("44 Analyse un."), None),
            entry(Some("CETAT01-03-02-02 TITRAGE UN."), None, None),
            entry(Some("CETAT44 TITRAGE DEUX."), None, None),
        ];
        assert_eq!(
            summary_of(Fond::Cetat, &entries),
            summary(&["Analyse un."], &["TITRAGE UN.", "TITRAGE DEUX."])
        );
    }

    #[test]
    fn summary_of_cetat_keeps_autre_resume_as_titrage() {
        let entries = [entry(Some("T1"), Some("A1"), Some("T2"))];
        assert_eq!(summary_of(Fond::Cetat, &entries), summary(&["A1"], &["T1", "T2"]));
    }

    #[test]
    fn summary_of_other_fonds_is_empty() {
        let full = [entry(Some("A"), Some("R"), Some("X"))];
        for fond in [Fond::Constit, Fond::Cnil] {
            assert_eq!(summary_of(fond, &full), OfficialSummary::default(), "{fond:?}");
        }
        for fond in [Fond::Juri, Fond::Cetat] {
            assert_eq!(summary_of(fond, &[]), OfficialSummary::default(), "{fond:?}");
            assert_eq!(
                summary_of(fond, &[SommaireDto::default()]),
                OfficialSummary::default(),
                "{fond:?}"
            );
        }
    }

    #[test]
    fn summary_ignores_the_nomenclature_ids() {
        use crate::dto::{decode::decode, juri::JuriTextDto};

        let json = r#"{"titrages": ["JURINOME000007644451-JURINOME000007645358"], "sommaire": []}"#;
        let dto: JuriTextDto = decode(json, "/consult/juri").unwrap();
        assert_eq!(dto.titrages, owned(&["JURINOME000007644451-JURINOME000007645358"]));
        for fond in [Fond::Juri, Fond::Cetat] {
            assert_eq!(summary_of(fond, &dto.sommaire), OfficialSummary::default(), "{fond:?}");
        }
    }
}
