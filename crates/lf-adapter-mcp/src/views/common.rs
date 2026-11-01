//! Règles de rendu communes aux vues (05 §5.0.4, §1.4 règle 9).

use lf_domain::{
    date::{CivilDate, open_ended},
    status::LegalStatus,
    text::flatten_whitespace,
};

/// `title` sauf s'il est déjà dans `citation` (égalité ou inclusion). Les deux textes sont déjà
/// nettoyés par le mapping (`clean`), qui garde les sauts de ligne de la source alors que la
/// citation est repliée sur une ligne : la comparaison replie donc le blanc des deux côtés
/// (`CNILTEXT000044033275`). Le titre rendu garde ses blancs d'origine, bords retirés. Un titre
/// absent ou vide donne `None`.
pub(crate) fn title_unless_in_citation(title: Option<&str>, citation: &str) -> Option<String> {
    let title = title?.trim();
    // Un titre vide est contenu dans toute citation : il tombe dans le même cas.
    (!flatten_whitespace(citation).contains(&flatten_whitespace(title))).then(|| title.to_owned())
}

/// Code d'API de l'`etat` (`VIGUEUR`, `VIGUEUR_ETEN`, `ABROGE`…), via
/// [`LegalStatus::as_api_code`] ; un code inconnu garde son texte nettoyé.
pub(crate) fn etat_code(status: Option<&LegalStatus>) -> Option<String> {
    status.map(|status| status.as_api_code().to_owned())
}

/// Date ISO `AAAA-MM-JJ`, sauf dans la zone sentinelle (2222-01-01 et après), qui vaut « absente ».
pub(crate) fn date_unless_sentinel(date: Option<CivilDate>) -> Option<String> {
    open_ended(date).map(|date| date.to_string())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

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
