//! Règles de rendu communes aux vues (05 §5.0.4, §1.4 règle 9).

use lf_application::citation::Validity;
use lf_domain::{
    date::{CivilDate, open_ended},
    status::LegalStatus,
    text::title_unless_in_citation as domain_rule,
};

/// `title` sauf s'il est déjà dans `citation` (égalité ou inclusion), blanc replié des deux côtés
/// (`CNILTEXT000044033275`) : la règle est celle du domaine, que l'estimation de taille de
/// l'application applique aussi. Le titre rendu garde ses blancs d'origine, bords retirés. Un
/// titre absent ou vide donne `None`.
pub(crate) fn title_unless_in_citation(title: Option<&str>, citation: &str) -> Option<String> {
    domain_rule(title, citation).map(str::to_owned)
}

/// Glossaire de `etat`, repris dans la description du champ de chaque vue qui le rend (D-12) :
/// chacun des 17 codes de [`LegalStatus`] y figure une fois, sans joker. `VIGUEUR_DIFF` n'est pas
/// en vigueur (entrée différée) ; `ABROGE_DIFF` l'est encore ; `DENONCE` peut s'appliquer pendant
/// le préavis et la survie (code du travail, L2261-10) ; `INITIALE` et `SANS_ETAT` ne disent rien.
/// Un code inconnu n'est classé nulle part.
pub(crate) const ETAT_GLOSSARY: &str = "In force: VIGUEUR, VIGUEUR_ETEN, VIGUEUR_NON_ETEN, \
ABROGE_DIFF. Not yet in force: VIGUEUR_DIFF (deferred entry, date_debut may be absent). No longer \
in force: MODIFIE, ABROGE, PERIME, ANNULE, REMPLACE, TRANSFERE, SUBSTITUE, DISJOINT, \
MODIFIE_MORT_NE (never in force: modified before taking effect). DENONCE: denounced, may still apply during notice and survival (Labour Code, \
article L2261-10). INITIALE, SANS_ETAT: no signal.";

/// Version courte du glossaire, pour les champs `etat` des autres vues (`fr_search`, `fr_resolve`,
/// `fr_verify_citations`, `fr_get_act`) : ce que le glossaire complet range en vigueur, pas encore
/// en vigueur et dénoncé, sans classer le reste ; le classement complet est celui de `fr_get_text`.
/// Elle tient la taille de `tools/list` sous le plafond (AM-73).
pub(crate) const ETAT_GLOSSARY_SHORT: &str = "In force: VIGUEUR, VIGUEUR_ETEN, VIGUEUR_NON_ETEN, \
ABROGE_DIFF. Not yet: VIGUEUR_DIFF. DENONCE may still apply. Every code is classed in the etat of \
fr_get_text.";

/// Code d'API de l'`etat` (`VIGUEUR`, `VIGUEUR_ETEN`, `ABROGE`…), via
/// [`LegalStatus::as_api_code`] ; un code inconnu garde son texte nettoyé.
pub(crate) fn etat_code(status: Option<&LegalStatus>) -> Option<String> {
    status.map(|status| status.as_api_code().to_owned())
}

/// Date ISO `AAAA-MM-JJ`, sauf dans la zone sentinelle (2222-01-01 et après), qui vaut « absente ».
pub(crate) fn date_unless_sentinel(date: Option<CivilDate>) -> Option<String> {
    open_ended(date).map(|date| date.to_string())
}

/// Période de validité d'un article résolu, rendue champ par champ (D-03) : l'`etat` de l'API et
/// les deux dates ISO, la fin sentinelle (2999) et toute valeur absente étant omises.
#[derive(Debug, Default)]
pub(crate) struct ValidityFields {
    pub(crate) etat: Option<String>,
    pub(crate) date_debut: Option<String>,
    pub(crate) date_fin: Option<String>,
}

impl From<Option<Validity>> for ValidityFields {
    fn from(validity: Option<Validity>) -> Self {
        validity.map_or_else(Self::default, |validity| Self {
            etat: etat_code(validity.etat.as_ref()),
            date_debut: date_unless_sentinel(validity.date_debut),
            date_fin: date_unless_sentinel(validity.date_fin),
        })
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    /// Les phrases du glossaire de `etat`, dans l'ordre : en vigueur, pas encore, plus en vigueur,
    /// dénoncé, sans signal.
    fn glossary_sentences() -> Vec<&'static str> {
        ETAT_GLOSSARY.split(". ").collect()
    }

    /// Codes cités par une phrase du glossaire.
    fn codes_of(sentence: &str) -> Vec<&str> {
        sentence
            .split(|c: char| !(c.is_ascii_uppercase() || c == '_'))
            .filter(|word| word.len() > 4 && word.contains(|c: char| c.is_ascii_uppercase()))
            .filter(|word| {
                LegalStatus::parse(word).is_some_and(|s| !matches!(s, LegalStatus::Other(_)))
            })
            .collect()
    }

    /// La version courte ne dit que des choses vraies du glossaire complet, sans joker.
    #[test]
    fn the_short_etat_glossary_agrees_with_the_full_one() {
        let short = ETAT_GLOSSARY_SHORT;
        assert!(!short.contains('*') && !short.contains("..."), "pas de joker");
        assert!(!short.contains("any other") && !short.contains("other codes"), "pas de joker");
        let full = ETAT_GLOSSARY;
        let in_force = "In force: VIGUEUR, VIGUEUR_ETEN, VIGUEUR_NON_ETEN, ABROGE_DIFF.";
        assert!(full.starts_with(in_force) && ETAT_GLOSSARY_SHORT.starts_with(in_force));
        assert!(full.contains("Not yet in force: VIGUEUR_DIFF"));
        assert!(ETAT_GLOSSARY_SHORT.contains("Not yet: VIGUEUR_DIFF."));
        assert!(full.contains("DENONCE: denounced, may still apply"));
        assert!(ETAT_GLOSSARY_SHORT.contains("DENONCE may still apply."));
        assert!(ETAT_GLOSSARY_SHORT.ends_with("fr_get_text."));
        assert!(ETAT_GLOSSARY_SHORT.len() < ETAT_GLOSSARY.len() / 2);
    }

    /// Chaque code de `LegalStatus` figure une fois dans le glossaire, sans joker, et sa classe
    /// suit `is_superseded` : « plus en vigueur » et `DENONCE` sont ceux que la règle commune
    /// signale ; les autres classes ne le sont pas.
    #[test]
    fn the_etat_glossary_names_each_status_once_and_follows_is_superseded() {
        assert!(!ETAT_GLOSSARY.contains('*'), "pas de joker");
        assert!(!ETAT_GLOSSARY.contains("..."), "pas de joker");
        assert!(!ETAT_GLOSSARY.contains("any other"), "pas de joker");
        let sentences = glossary_sentences();
        assert_eq!(sentences.len(), 5, "{sentences:?}");
        let classes: Vec<Vec<&str>> = sentences.iter().map(|s| codes_of(s)).collect();
        let all: Vec<&str> = classes.iter().flatten().copied().collect();
        assert_eq!(all.len(), 17, "{all:?}");
        let mut sorted = all.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 17, "un code deux fois : {all:?}");
        let expected_superseded = [false, false, true, true, false];
        for (codes, superseded) in classes.iter().zip(expected_superseded) {
            for code in codes {
                let status = LegalStatus::parse(code).unwrap();
                assert_eq!(status.is_superseded(), superseded, "{code}");
            }
        }
        // Les dix codes de la règle commune sont exactement ceux des deux dernières classes de
        // « plus en vigueur » et de `DENONCE`.
        assert_eq!(classes[2].len() + classes[3].len(), 10);
        assert_eq!(classes[3], ["DENONCE"]);
        assert_eq!(classes[1], ["VIGUEUR_DIFF"]);
        assert_eq!(classes[4], ["INITIALE", "SANS_ETAT"]);
    }

    #[test]
    fn title_is_omitted_when_equal_to_the_citation() {
        assert_eq!(title_unless_in_citation(Some("Loi n° 2016-1321"), "Loi n° 2016-1321"), None);
    }

    #[test]
    fn title_is_omitted_when_the_citation_contains_it() {
        // Titre ACCO contenu dans « {titre} ({raison sociale}) » (05 §5.0.4).
        assert_eq!(
            title_unless_in_citation(
                Some("Accord sur le temps de travail"),
                "Accord sur le temps de travail (ACME SAS)"
            ),
            None
        );
    }

    #[test]
    fn title_is_kept_when_the_citation_does_not_contain_it() {
        // Décision du Conseil constitutionnel : le titre porte l'objet, pas la citation.
        assert_eq!(
            title_unless_in_citation(
                Some("Décision 2025-1173 QPC - Objet [Conformité]"),
                "Cons. const., décision n° 2025-1173 QPC"
            ),
            Some("Décision 2025-1173 QPC - Objet [Conformité]".to_owned())
        );
    }

    #[test]
    fn title_is_trimmed_before_the_comparison() {
        assert_eq!(title_unless_in_citation(Some("  Loi X  "), "Loi X, art. 1"), None);
        assert_eq!(title_unless_in_citation(Some("  Loi Y  "), "Loi X"), Some("Loi Y".to_owned()));
    }

    /// L3-07 (search-15) : une citation repliée sur une ligne contient le titre multi-lignes
    /// de la même délibération ; le blanc se compare replié des deux côtés.
    #[test]
    fn title_is_compared_with_its_whitespace_folded() {
        assert_eq!(title_unless_in_citation(Some("a\nb"), "a b"), None);
        assert_eq!(title_unless_in_citation(Some("a b"), "a\n b, art. 1"), None);
        let citation = "Délibération n° 2021-087 du 22 juillet 2021 (rectificatif) (demande d’approbation n° 20005213)";
        let title = "Délibération n° 2021-087 du 22 juillet 2021 (rectificatif)\n(demande d’approbation n° 20005213)\n";
        assert_eq!(title_unless_in_citation(Some(title), citation), None);
    }

    /// Jumeau : un titre qui diffère vraiment de la citation reste, même replié.
    #[test]
    fn a_different_multi_line_title_is_kept() {
        assert_eq!(
            title_unless_in_citation(Some("Objet\nde la décision"), "Cons. const., décision n° 1"),
            Some("Objet\nde la décision".to_owned())
        );
        // Les mots collés par un repli naïf (sans espace) ne sont pas confondus.
        assert_eq!(title_unless_in_citation(Some("a\nb"), "ab"), Some("a\nb".to_owned()));
    }

    #[test]
    fn an_absent_or_blank_title_gives_none() {
        assert_eq!(title_unless_in_citation(None, "Loi X"), None);
        assert_eq!(title_unless_in_citation(Some(""), "Loi X"), None);
        assert_eq!(title_unless_in_citation(Some("  \n "), "Loi X"), None);
    }

    #[test]
    fn the_inclusion_is_case_sensitive() {
        assert_eq!(title_unless_in_citation(Some("loi x"), "Loi X"), Some("loi x".to_owned()));
    }

    #[test]
    fn etat_is_the_api_code() {
        assert_eq!(etat_code(Some(&LegalStatus::Vigueur)), Some("VIGUEUR".to_owned()));
        assert_eq!(etat_code(Some(&LegalStatus::VigueurEtendue)), Some("VIGUEUR_ETEN".to_owned()));
        assert_eq!(etat_code(Some(&LegalStatus::Abroge)), Some("ABROGE".to_owned()));
    }

    #[test]
    fn an_unknown_etat_keeps_its_cleaned_text() {
        let status = LegalStatus::parse("  NOUVEAU_CODE ").unwrap();
        assert_eq!(etat_code(Some(&status)), Some("NOUVEAU_CODE".to_owned()));
    }

    #[test]
    fn an_absent_etat_gives_none() {
        assert_eq!(etat_code(None), None);
    }

    #[test]
    fn a_date_before_the_sentinel_zone_is_rendered_in_iso() {
        let date = CivilDate::new(2016, 10, 8).unwrap();
        assert_eq!(date_unless_sentinel(Some(date)), Some("2016-10-08".to_owned()));
    }

    #[test]
    fn the_last_day_before_the_threshold_is_kept() {
        let date = CivilDate::new(2221, 12, 31).unwrap();
        assert_eq!(date_unless_sentinel(Some(date)), Some("2221-12-31".to_owned()));
    }

    #[test]
    fn dates_from_2222_01_01_are_omitted() {
        for (year, month, day) in [(2222, 1, 1), (2222, 2, 22), (2999, 1, 1)] {
            let date = CivilDate::new(year, month, day).unwrap();
            assert_eq!(date_unless_sentinel(Some(date)), None, "{date}");
        }
    }

    #[test]
    fn an_absent_date_gives_none() {
        assert_eq!(date_unless_sentinel(None), None);
    }
}
