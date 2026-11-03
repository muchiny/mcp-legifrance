//! Version d'un article (`canonicalArticleVersion`, M7, 03 §4.11).

use crate::{
    date::{CivilDate, VersionDate, open_ended},
    id::LegiArtiId,
    status::LegalStatus,
};

/// Une version d'un article, pour `article_versions` et `fr_diff_article`.
///
/// Les dates suivent le sens de 03 §6.2 : `date_fin` à `None` veut dire « en vigueur, sans date
/// de fin » (sentinelle de l'API déjà écartée par le mapper), `date_debut` à `None` « date
/// d'entrée en vigueur non déterminée ».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArticleVersion {
    /// Identifiant de cette version de l'article.
    pub id: LegiArtiId,
    /// Numéro affiché, ex. `L. 1234-5`.
    pub num: Option<String>,
    /// État juridique ; `None` : absent ou non reconnu.
    pub etat: Option<LegalStatus>,
    /// Date d'entrée en vigueur.
    pub date_debut: Option<CivilDate>,
    /// Date de fin de vigueur, absente pour la version en vigueur.
    pub date_fin: Option<CivilDate>,
}

/// La version en vigueur à `date` : celle dont `date_debut..date_fin` la contient (début compris,
/// fin exclue), la plus récente si plusieurs la contiennent. Une version sans `date_debut` ne
/// contient aucune date (U01).
///
/// La `date_fin` réelle d'une version est le premier jour hors vigueur : le jour où la suivante
/// prend effet, ou le jour de l'abrogation. Au jour de bascule, seule la suivante contient la date ;
/// sans suivante (dernière version abrogée, trou entre deux versions), aucune version n'est en
/// vigueur. Une version de durée nulle ou « mort-née » (fin avant début) ne contient jamais rien.
/// Une `date_fin` dans la zone sentinelle ne limite pas ([`open_ended`]).
///
/// Règle unique de `fr_resolve` (repli daté) et de `fr_diff_article` ; la fenêtre du DILA
/// (`debut <= at < fin`) est la même, sauf pour le début absent, qu'elle laisse ouvert.
#[must_use]
pub fn in_force_on(versions: &[ArticleVersion], date: VersionDate) -> Option<&ArticleVersion> {
    let day = date.as_civil();
    versions
        .iter()
        .filter(|version| {
            open_ended(version.date_debut).is_some_and(|start| start <= day)
                && open_ended(version.date_fin).is_none_or(|end| day < end)
        })
        .max_by_key(|version| version.date_debut)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn version_compares_by_value() {
        let version = ArticleVersion {
            id: LegiArtiId::parse("LEGIARTI000006419288").unwrap(),
            num: Some("9".into()),
            etat: Some(LegalStatus::Vigueur),
            date_debut: CivilDate::new(2016, 10, 1),
            date_fin: None,
        };
        assert_eq!(version.clone(), version);
        assert_ne!(
            ArticleVersion { date_fin: CivilDate::new(2020, 1, 1), ..version.clone() },
            version
        );
        assert_eq!(version.id.as_str(), "LEGIARTI000006419288");
    }

    fn version(n: u32, start: Option<(u16, u8, u8)>, end: Option<(u16, u8, u8)>) -> ArticleVersion {
        let civil = |(y, m, d): (u16, u8, u8)| CivilDate::new(y, m, d).unwrap();
        ArticleVersion {
            id: LegiArtiId::parse(&format!("LEGIARTI{n:012}")).unwrap(),
            num: None,
            etat: None,
            date_debut: start.map(civil),
            date_fin: end.map(civil),
        }
    }

    fn id_on(versions: &[ArticleVersion], raw: &str) -> Option<u32> {
        let found = in_force_on(versions, VersionDate::parse(raw).unwrap())?;
        found.id.as_str().strip_prefix("LEGIARTI").and_then(|n| n.parse().ok())
    }

    #[test]
    fn in_force_on_picks_the_newest_version_on_the_switch_day() {
        // `date_fin` de A et `date_debut` de B sont le même jour : B l'emporte ce jour-là.
        let versions = vec![
            version(1, Some((2016, 10, 9)), Some((2017, 9, 24))),
            version(2, Some((2017, 9, 24)), None),
        ];
        assert_eq!(id_on(&versions, "2016-10-08"), None, "la veille du début");
        assert_eq!(id_on(&versions, "2017-09-23"), Some(1), "la veille");
        assert_eq!(id_on(&versions, "2017-09-24"), Some(2), "le jour de bascule");
        assert_eq!(id_on(&versions, "2221-12-31"), Some(2), "sans date de fin");
    }

    #[test]
    fn one_version_per_day() {
        // Versions contiguës, sans trou : chaque jour couvert est dans exactement une version,
        // sauf le jour de bascule où la plus récente seule le contient.
        let versions = vec![
            version(1, Some((2000, 1, 1)), Some((2005, 3, 1))),
            version(2, Some((2005, 3, 1)), Some((2010, 7, 14))),
            version(3, Some((2010, 7, 14)), None),
        ];
        let cases = [
            ("1999-12-31", None),
            ("2000-01-01", Some(1)),
            ("2005-02-28", Some(1)),
            ("2005-03-01", Some(2)),
            ("2010-07-13", Some(2)),
            ("2010-07-14", Some(3)),
            ("2100-01-01", Some(3)),
        ];
        for (day, wanted) in cases {
            assert_eq!(id_on(&versions, day), wanted, "{day}");
        }
    }

    #[test]
    fn the_end_day_of_the_last_version_is_out_of_force() {
        // `date_fin` est le premier jour hors vigueur : sans suivante, aucune version ce jour-là.
        let versions = vec![version(1, Some((2000, 1, 1)), Some((2010, 1, 1)))];
        assert_eq!(id_on(&versions, "2009-12-31"), Some(1), "la veille de la fin");
        assert_eq!(id_on(&versions, "2010-01-01"), None, "le jour de la fin");
        assert_eq!(id_on(&versions, "2010-01-02"), None, "le lendemain");
    }

    /// Garde : vert sur la souche (qui ne sert jamais rien) et après.
    #[test]
    fn a_version_without_start_is_never_in_force() {
        let versions =
            vec![version(1, None, None), version(2, Some((2000, 1, 1)), Some((2010, 1, 1)))];
        assert_eq!(id_on(&versions, "2020-01-01"), None);
        assert_eq!(id_on(&[version(1, None, None)], "2005-01-01"), None);
    }
}
