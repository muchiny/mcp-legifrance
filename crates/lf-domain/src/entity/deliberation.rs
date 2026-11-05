//! Délibération CNIL (`fr_get_deliberation`, `citations.py:363-395`).

use crate::{
    contract::{Citable, CitationContract, CnilWord, cnil_consult_citation, is_cnil_closure_title},
    date::CivilDate,
    id::CnilTextId,
    status::LegalStatus,
    text::{LongText, fold},
    url::{ResourceKind, legifrance_url},
};

/// Délibération de la CNIL ; ECLI et ELI toujours nuls côté API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deliberation {
    /// Identifiant.
    pub deliberation_id: CnilTextId,
    /// Numéro, ex. `SAN-2026-002`.
    pub num: Option<String>,
    /// Nature.
    pub nature: Option<String>,
    /// Nature de délibération.
    pub nature_delib: Option<String>,
    /// État juridique ; `None` : absent ou non reconnu.
    pub etat: Option<LegalStatus>,
    /// Date de la délibération ; `None` : inconnue.
    pub date_texte: Option<CivilDate>,
    /// Date de publication ; `None` : inconnue.
    pub date_publication: Option<CivilDate>,
    /// Titre court.
    pub title: Option<String>,
    /// Titre long.
    pub title_long: Option<String>,
    /// Texte intégral, nettoyé (vide : l'API n'en fournit pas).
    pub text: LongText,
}

/// Premier mot de `text`, replié (casse et accents), sans ponctuation.
fn first_word(text: &str) -> String {
    let folded = fold(text);
    folded
        .split(|c: char| !c.is_alphabetic())
        .find(|word| !word.is_empty())
        .unwrap_or("")
        .to_owned()
}

impl Deliberation {
    /// Titre long non blanc.
    fn long_title(&self) -> Option<&str> {
        self.title_long.as_deref().map(str::trim).filter(|title| !title.is_empty())
    }

    /// Clôture de mise en demeure : le titre long commence par « Clôture » ou « Courrier de
    /// clôture ». Sans titre long, `nature_delib` « Cloture » le dit. Un titre long qui ne
    /// commence pas ainsi l'emporte sur `nature_delib` (CNILTEXT000023602913).
    fn is_closure(&self) -> bool {
        match self.long_title() {
            Some(long) => is_cnil_closure_title(long),
            None => self.nature_delib.as_deref().is_some_and(|kind| fold(kind).trim() == "cloture"),
        }
    }

    /// Mot de la citation : le premier mot du titre long (« décision » ou « délibération »), la
    /// nature « Décision » seulement quand il n'y a pas de titre long ; sinon « délibération ».
    /// `nature` n'est pas fiable à elle seule (MEDP-2024-001 : nature Décision, titre « Délibération
    /// du bureau »), `nature_delib` n'est jamais un indice.
    fn word(&self) -> CnilWord {
        let decision = match self.long_title() {
            Some(long) => first_word(long) == "decision",
            None => self.nature.as_deref().is_some_and(|nature| fold(nature).trim() == "decision"),
        };
        if decision { CnilWord::Decision } else { CnilWord::Deliberation }
    }

    /// Citation selon l'usage (AM-80). Une clôture est citée par son titre long, sans date ; sans
    /// titre long, par sa forme neutre. Sinon « CNIL, {mot} n° {num} du {date} », ou sans la date
    /// quand l'enregistrement n'en a pas de fiable. Sans numéro : le titre court si la date est
    /// établie (comportement de la 1.1.0), sinon le titre long (le titre court est fabriqué avec
    /// la date de repli).
    fn citation(&self) -> Option<String> {
        let num = self.num.as_deref().filter(|num| !num.is_empty());
        if self.is_closure() {
            return self
                .long_title()
                .map(str::to_owned)
                .or_else(|| num.map(|num| format!("CNIL, clôture de la décision n° {num}")));
        }
        cnil_consult_citation(self.word(), num, self.date_texte).or_else(|| {
            if self.date_texte.is_some() {
                self.title.clone()
            } else {
                self.long_title().map(str::to_owned)
            }
        })
    }
}

impl Citable for Deliberation {
    fn contract(&self) -> CitationContract {
        let id = self.deliberation_id.as_str();
        let citation = self.citation();
        CitationContract::from_url(legifrance_url(ResourceKind::Cnil, id), citation.as_deref(), id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn deliberation(num: Option<&str>) -> Deliberation {
        Deliberation {
            deliberation_id: CnilTextId::parse("CNILTEXT000053352643").unwrap(),
            num: num.map(Into::into),
            nature: None,
            nature_delib: None,
            etat: None,
            date_texte: CivilDate::new(2026, 1, 8),
            date_publication: None,
            title: Some("Délibération SAN-2026-002 du 8 janvier 2026".into()),
            title_long: None,
            text: LongText::default(),
        }
    }

    #[test]
    fn cites_with_cnil_convention() {
        let c = deliberation(Some("SAN-2026-002")).contract();
        assert_eq!(
            (c.citation.as_str(), c.source_url.as_str()),
            (
                "CNIL, délibération n° SAN-2026-002 du 8 janvier 2026",
                "https://www.legifrance.gouv.fr/cnil/id/CNILTEXT000053352643"
            )
        );
    }

    #[test]
    fn falls_back_to_title() {
        let c = deliberation(None).contract();
        assert_eq!(c.citation.as_str(), "Délibération SAN-2026-002 du 8 janvier 2026");
    }

    #[test]
    fn deliberation_status_and_text_types() {
        let mut d = deliberation(Some("SAN-2026-002"));
        d.etat = LegalStatus::parse("VIGUEUR");
        d.text =
            LongText::from_html("<p>La formation restreinte</p><p>Après en avoir délibéré</p>")
                .unwrap();
        assert_eq!(
            (
                d.etat.as_ref().map(LegalStatus::as_api_code),
                d.text.as_str(),
                d.text.paragraphs().count(),
            ),
            (Some("VIGUEUR"), "La formation restreinte\n\nAprès en avoir délibéré", 2)
        );
        // Valeurs par défaut : état inconnu et texte vide, la citation ne change pas.
        let empty = deliberation(Some("SAN-2026-002"));
        assert_eq!((empty.etat, empty.text.is_empty()), (None, true));
        assert_eq!(
            d.contract().citation.as_str(),
            "CNIL, délibération n° SAN-2026-002 du 8 janvier 2026"
        );
    }

    /// Délibération synthétique d'identifiant `CNILTEXT000053352643` : numéro, date, nature,
    /// nature de délibération, titre court et titre long à la demande.
    fn record(
        num: Option<&str>,
        date: Option<CivilDate>,
        nature: Option<&str>,
        nature_delib: Option<&str>,
        title: Option<&str>,
        title_long: Option<&str>,
    ) -> Deliberation {
        Deliberation {
            num: num.map(Into::into),
            date_texte: date,
            nature: nature.map(Into::into),
            nature_delib: nature_delib.map(Into::into),
            title: title.map(Into::into),
            title_long: title_long.map(Into::into),
            ..deliberation(None)
        }
    }

    fn citation_of(d: &Deliberation) -> String {
        d.contract().citation.as_str().to_owned()
    }

    #[test]
    fn decision_nature_cites_decision() {
        let cases = [
            // MED-2024-150 : nature Décision, titre long « Décision n° … » (relevé D09).
            (
                record(
                    Some("MED-2024-150"),
                    CivilDate::new(2024, 11, 15),
                    Some("Décision"),
                    None,
                    Some("Décision MED-2024-150 du 15 novembre 2024"),
                    Some(
                        "Décision n° MED-2024-150 du 15 novembre 2024 mettant en demeure la société X",
                    ),
                ),
                "CNIL, décision n° MED-2024-150 du 15 novembre 2024",
            ),
            // Mise en demeure d'ancienne série, `nature_delib` « Mise en demeure » : pas un indice.
            (
                record(
                    Some("MED-2018-007"),
                    CivilDate::new(2018, 3, 5),
                    Some("Décision"),
                    Some("Mise en demeure"),
                    None,
                    Some("Décision n° MED-2018-007 du 5 mars 2018 mettant en demeure la société Y"),
                ),
                "CNIL, décision n° MED-2018-007 du 5 mars 2018",
            ),
            // Décision DF à date confirmée.
            (
                record(
                    Some("DF-2014-288"),
                    CivilDate::new(2014, 3, 12),
                    Some("Décision"),
                    Some("Autorisation de transfert"),
                    None,
                    Some("décision DF-2014-288 du 12 mars 2014 autorisant la société Z"),
                ),
                "CNIL, décision n° DF-2014-288 du 12 mars 2014",
            ),
        ];
        for (record, expected) in cases {
            assert_eq!(citation_of(&record), expected);
        }
    }

    #[test]
    fn bureau_deliberation_stays_deliberation() {
        // MEDP-2024-001 : `nature` vaut « Décision » mais le titre long dit « Délibération du
        // bureau » ; le mot vient du titre long, jamais de `nature` (garde contre le correctif naïf).
        let bureau = record(
            Some("MEDP-2024-001"),
            CivilDate::new(2024, 11, 18),
            Some("Décision"),
            None,
            Some("Décision MEDP-2024-001 du 18 novembre 2024"),
            Some("Délibération du bureau de la Commission n° MEDP-2024-001 du 18 novembre 2024"),
        );
        assert_eq!(citation_of(&bureau), "CNIL, délibération n° MEDP-2024-001 du 18 novembre 2024");
        // Un titre long dont le premier mot n'est ni l'un ni l'autre ne laisse pas la nature décider.
        let avis = record(
            Some("2017-301"),
            CivilDate::new(2017, 12, 7),
            Some("Décision"),
            None,
            None,
            Some("Avis n° 2017-301 du 7 décembre 2017 sur un projet"),
        );
        assert_eq!(citation_of(&avis), "CNIL, délibération n° 2017-301 du 7 décembre 2017");
    }

    #[test]
    fn long_title_word_wins_over_nature() {
        // La nature « Délibération » avec un titre long qui dit « Décision » : le titre long l'emporte.
        let reverse = record(
            Some("X-2024-001"),
            CivilDate::new(2024, 11, 18),
            Some("Délibération"),
            None,
            None,
            Some("Décision n° X-2024-001 du 18 novembre 2024"),
        );
        assert_eq!(citation_of(&reverse), "CNIL, décision n° X-2024-001 du 18 novembre 2024");
    }

    #[test]
    fn bureau_deliberation_without_long_title_is_pinned() {
        // Sans titre long, la nature exacte « Décision » (casse et accent repliés) décide ; toute
        // autre nature, ou l'absence de nature, garde « délibération » (AM-80).
        let cases = [
            (Some("Décision"), "décision"),
            (Some(" décision "), "décision"),
            (Some("DECISION"), "décision"),
            (Some("Délibération"), "délibération"),
            (Some("Décision du bureau"), "délibération"),
            (Some(""), "délibération"),
            (None, "délibération"),
        ];
        for (nature, word) in cases {
            let d = record(
                Some("MEDP-2024-001"),
                CivilDate::new(2024, 11, 18),
                nature,
                None,
                None,
                None,
            );
            assert_eq!(
                citation_of(&d),
                format!("CNIL, {word} n° MEDP-2024-001 du 18 novembre 2024"),
                "{nature:?}"
            );
        }
        // Un titre long blanc vaut absent.
        let blank = record(
            Some("MEDP-2024-001"),
            CivilDate::new(2024, 11, 18),
            Some("Décision"),
            None,
            None,
            Some("   "),
        );
        assert_eq!(citation_of(&blank), "CNIL, décision n° MEDP-2024-001 du 18 novembre 2024");
    }

    #[test]
    fn cloture_nature_but_deliberation_title_keeps_deliberation_citation() {
        // CNILTEXT000023602913 : nature Délibération, nature_delib Cloture, titre long qui dit
        // « Délibération » : ce n'est pas une clôture, la citation actuelle est juste.
        let d = record(
            Some("2010-117"),
            CivilDate::new(2010, 4, 8),
            Some("Délibération"),
            Some("Cloture"),
            Some("Délibération 2010-117 du 8 avril 2010"),
            Some("Délibération n° 2010-117 du 8 avril 2010 portant clôture de la procédure"),
        );
        assert_eq!(citation_of(&d), "CNIL, délibération n° 2010-117 du 8 avril 2010");
    }

    #[test]
    fn closure_cites_its_long_title() {
        let cases = [
            // MED-2022-060 : la date de la lettre (16 septembre 2022) n'est jamais citée.
            (
                "MED-2022-060",
                CivilDate::new(2022, 9, 16),
                "Clôture de la mise en demeure n° MED-2022-060 du 25 avril 2022",
            ),
            (
                "MED-2023-018",
                CivilDate::new(2024, 1, 3),
                "Clôture de la décision n°MED-2023-018 du 3 avril 2023",
            ),
            (
                "MED-2017-053",
                CivilDate::new(2018, 1, 22),
                "Clôture de la décision n° MED-2017-053 du 30 août 2017",
            ),
            (
                "MED-2019-001",
                CivilDate::new(2019, 5, 2),
                "Courrier de clôture de la mise en demeure",
            ),
        ];
        for (num, date, long) in cases {
            for (nature, nature_delib) in [
                (Some("Décision"), Some("Cloture")),
                (Some("Décision"), None),
                (Some("Délibération"), Some("Mise en demeure")),
            ] {
                let d = record(
                    Some(num),
                    date,
                    nature,
                    nature_delib,
                    Some("Décision MED-2022-060 du 16 septembre 2022"),
                    Some(long),
                );
                assert_eq!(citation_of(&d), long, "{num} {nature_delib:?}");
            }
        }
        // Sans titre long, une clôture (nature_delib Cloture) reçoit la forme neutre, sans date ;
        // jamais le titre court (qui porte la date de la lettre), jamais « délibération … du … ».
        let short = Some("Décision MED-2022-060 du 16 septembre 2022");
        let neutral = record(
            Some("MED-2022-060"),
            CivilDate::new(2022, 9, 16),
            Some("Décision"),
            Some("Cloture"),
            short,
            None,
        );
        assert_eq!(citation_of(&neutral), "CNIL, clôture de la décision n° MED-2022-060");
        let folded = Deliberation { nature_delib: Some(" CLÔTURE ".into()), ..neutral.clone() };
        assert_eq!(citation_of(&folded), "CNIL, clôture de la décision n° MED-2022-060");
        // Sans titre long ni numéro : l'identifiant, jamais le titre court.
        let nothing = Deliberation { num: None, ..neutral };
        assert_eq!(citation_of(&nothing), "Légifrance CNILTEXT000053352643");
    }

    #[test]
    fn closure_forms_detected_by_long_title_start() {
        // Le début du titre long fait la clôture, quelles que soient la nature et la nature de
        // délibération.
        for long in
            ["Clôture de la décision n° X", "cloture de la mise en demeure", "COURRIER DE CLÔTURE"]
        {
            let d = record(
                Some("MED-2022-060"),
                CivilDate::new(2022, 9, 16),
                None,
                None,
                None,
                Some(long),
            );
            assert_eq!(citation_of(&d), long, "{long}");
        }
        // « clôture » plus loin dans le titre, ou nature_delib Cloture avec un titre long qui ne
        // commence pas ainsi : pas une clôture.
        for long in
            ["Décision n° MED-2022-060 du 25 avril 2022 de clôture", "Décision portant clôture"]
        {
            let d = record(
                Some("MED-2022-060"),
                CivilDate::new(2022, 4, 25),
                Some("Décision"),
                Some("Cloture"),
                None,
                Some(long),
            );
            assert_eq!(
                citation_of(&d),
                "CNIL, décision n° MED-2022-060 du 25 avril 2022",
                "{long}"
            );
        }
    }

    #[test]
    fn legacy_unprefixed_decision_cites_decision() {
        // Ancienne numérotation sans série (2014-001, 2014-029) : « décision n° 2014-001 du … ».
        let d = record(
            Some("2014-001"),
            CivilDate::new(2014, 1, 15),
            Some("Décision"),
            None,
            None,
            Some("Décision n° 2014-001 du 15 janvier 2014 portant autorisation"),
        );
        assert_eq!(citation_of(&d), "CNIL, décision n° 2014-001 du 15 janvier 2014");
    }

    #[test]
    fn number_without_date_is_number_only() {
        // Date retirée (valeur de repli de l'amont) : « décision n° DF-2017-1448 », sans date.
        let df = record(
            Some("DF-2017-1448"),
            None,
            Some("Décision"),
            Some("Autorisation de transfert"),
            Some("Décision DF-2017-1448 du 31 décembre 2017"),
            Some("Décision DF-2017-1448 autorisant la société KEYMANTICS à transférer des données"),
        );
        assert_eq!(citation_of(&df), "CNIL, décision n° DF-2017-1448");
        let plain = record(
            Some("2014-029"),
            None,
            Some("Délibération"),
            None,
            Some("Délibération 2014-029 du 31 décembre 2014"),
            Some("Délibération n° 2014-029 portant avis"),
        );
        assert_eq!(citation_of(&plain), "CNIL, délibération n° 2014-029");
    }

    #[test]
    fn a_number_ending_with_the_title_year_keeps_it_and_adds_no_date() {
        // Cas trouvé par la propriété ci-dessous : le numéro finit par l'année du titre ; la
        // citation garde le numéro entier et n'ajoute pas la date du titre.
        let d = record(
            Some("AA-2000-2017"),
            None,
            None,
            None,
            Some("Décision AA-2000-2017 du 31 décembre 2017"),
            None,
        );
        assert_eq!(citation_of(&d), "CNIL, délibération n° AA-2000-2017");
    }

    #[test]
    fn short_title_with_placeholder_date_is_never_the_fallback() {
        // Sans numéro et sans date : le titre court est fabriqué avec la date de repli ; il n'est
        // pas cité. Le titre long (s'il y en a un) puis l'identifiant prennent le relais.
        let short = Some("Décision DF-2017-1448 du 31 décembre 2017");
        let with_long =
            record(None, None, Some("Décision"), None, short, Some("Décision autorisant X"));
        assert_eq!(citation_of(&with_long), "Décision autorisant X");
        let bare = record(None, None, Some("Décision"), None, short, None);
        assert_eq!(citation_of(&bare), "Légifrance CNILTEXT000053352643");
        // Numéro vide : même chose.
        let empty = record(Some(""), None, None, None, short, None);
        assert_eq!(citation_of(&empty), "Légifrance CNILTEXT000053352643");
    }

    mod property {
        use proptest::prelude::*;

        use super::*;

        proptest! {
            /// Sans date dans la fiche, la citation d'un acte qui n'est pas une clôture n'en
            /// contient aucune, quels que soient les titres (même datés) et la nature.
            #[test]
            fn citation_never_has_a_date_the_record_lacks(
                num in "[A-Z]{2,4}-20[0-1][0-6]-[0-9]{3,4}",
                kind in 0usize..4,
                nature in prop::option::of(prop::sample::select(vec!["Décision", "Délibération", "Avis"])),
                nature_delib in prop::option::of(prop::sample::select(vec!["Cloture", "Mise en demeure", "Sanction"])),
                with_long in any::<bool>(),
            ) {
                let opening = ["Décision", "Délibération", "Avis", "Autorisation"][kind];
                let long = format!("{opening} n° {num} du 31 décembre 2017 relative à X");
                let short = format!("Décision {num} du 31 décembre 2017");
                let d = record(
                    Some(&num),
                    None,
                    nature,
                    nature_delib,
                    Some(&short),
                    with_long.then_some(long.as_str()),
                );
                let citation = citation_of(&d);
                prop_assert!(citation.starts_with("CNIL, "), "{citation}");
                prop_assert!(citation.ends_with(&num), "{citation}");
                prop_assert!(!citation.contains(" du "), "{citation}");
                prop_assert!(!citation.contains("décembre"), "{citation}");
                // L'année se cherche hors du numéro, qui peut lui-même finir par « 2017 »
                // (« AA-2000-2017 », trouvé par la mutation finale du 2026-11-05).
                let head = citation.strip_suffix(num.as_str()).unwrap_or_default();
                prop_assert!(!head.contains("2017"), "{citation}");
            }
        }
    }
}
