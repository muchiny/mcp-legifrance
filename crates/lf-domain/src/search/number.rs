//! Recherche par numéro (M6, `find_by_number`) : nature du numéro, valeur et requête (03 §4.11,
//! annexe B §6.6, qui fait foi pour la forme et pour les couples (fond, nature) autorisés).
//!
//! Ces types sont du domaine, et non de l'application : la clé de cache `Op::FindByNumber` et
//! l'adapter PISTE les emploient sans dépendre de `lf-application`, et le constructeur rend une
//! [`DomainError`]. Ils sont réexportés par [`search`](crate::search).

use crate::{
    date::{DateRange, VersionDate},
    error::{DomainError, echo},
    fond::Fond,
};

/// Longueur maximale, en caractères, d'une valeur de numéro (annexe B §6.6).
pub const NUMBER_VALUE_MAX_CHARS: usize = 128;

/// Nature du numéro cherché (annexe B §6.6). Chaque nature n'existe que pour certains fonds
/// ([`NumberQuery::new`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NumberKind {
    /// `NUM_ARTICLE` (fond `CODE` ou `LODA`).
    Article,
    /// `NUM` (fond `LODA`).
    Text,
    /// `NUM_AFFAIRE` (fond `JURI`).
    Case,
    /// `NUM_DEC` (fond `CONSTIT` ou `CETAT`).
    Decision,
    /// `NUM_DELIB` (fond `CNIL`).
    Deliberation,
    /// `/consult/kaliContIdcc` (fond `KALI`) ; IDCC en recherche (`KALI`, `ACCO`).
    Idcc,
    /// `/consult/getJoWithNor` (fond `LODA`).
    Nor,
    /// `/search` filtré par la facette `ECLI`, valeur exacte (fond `JURI` ou `CETAT`).
    Ecli,
    /// ELI cité : `/consult/getArticleWithIdEliOrAlias` ou `/consult/eliAndAliasRedirectionTexte`
    /// (fond `LODA`).
    Eli,
}

impl NumberKind {
    /// Les neuf natures, dans l'ordre de l'annexe B §6.6.
    pub const ALL: [Self; 9] = [
        Self::Article,
        Self::Text,
        Self::Case,
        Self::Decision,
        Self::Deliberation,
        Self::Idcc,
        Self::Nor,
        Self::Ecli,
        Self::Eli,
    ];

    /// Nom de la nature, tel que le message d'erreur la cite (`ecli`, `article`, ...).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Article => "article",
            Self::Text => "text",
            Self::Case => "case",
            Self::Decision => "decision",
            Self::Deliberation => "deliberation",
            Self::Idcc => "idcc",
            Self::Nor => "nor",
            Self::Ecli => "ecli",
            Self::Eli => "eli",
        }
    }

    /// La nature existe-t-elle pour ce fond ? Les couples autorisés sont ceux de l'annexe B
    /// §6.6 : (`CODE`|`LODA`, article), (`LODA`, texte), (`JURI`, affaire), (`CONSTIT`|`CETAT`,
    /// décision), (`CNIL`, délibération), (`KALI`|`ACCO`, IDCC), (`LODA`, NOR), (`JURI`|`CETAT`,
    /// ECLI) et (`LODA`, ELI).
    #[must_use]
    pub const fn applies_to(self, fond: Fond) -> bool {
        matches!(
            (fond, self),
            (Fond::Code | Fond::Loda, Self::Article)
                | (Fond::Loda, Self::Text | Self::Nor | Self::Eli)
                | (Fond::Juri, Self::Case)
                | (Fond::Constit | Fond::Cetat, Self::Decision)
                | (Fond::Cnil, Self::Deliberation)
                | (Fond::Kali | Fond::Acco, Self::Idcc)
                | (Fond::Juri | Fond::Cetat, Self::Ecli)
        )
    }

    /// Le fond filtre-t-il par `DATE_VERSION` pour cette nature ? Vrai pour (`CODE`|`LODA`,
    /// article) et (`LODA`, texte) : la date de version est alors obligatoire.
    const fn needs_version_date(self, fond: Fond) -> bool {
        matches!((fond, self), (Fond::Code | Fond::Loda, Self::Article) | (Fond::Loda, Self::Text))
    }
}

/// Valeur d'un numéro, envoyée telle quelle à l'API : 1 à [`NUMBER_VALUE_MAX_CHARS`] caractères
/// parmi `[A-Za-z0-9.*/:_ -]` (annexe B §6.6).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NumberValue(String);

impl NumberValue {
    /// Valide `input` sans le modifier.
    ///
    /// Le jeu de caractères est celui de l'annexe B §6.6 : lettres et chiffres ASCII, le point,
    /// l'étoile, la barre oblique, les deux-points, le tiret bas, le tiret et l'espace. Il couvre les numéros de pourvoi (`24-13.470`), de décision
    /// (`2025-1173 QPC`), de délibération, les NOR, les IDCC, les ECLI (`ECLI:FR:CCASS:2025:C100399`)
    /// et les chemins ELI cités (`/eli/loi/2016/10/7/ECFI1524250L/jo/texte`), et exclut tout ce qui
    /// ne sert qu'à fabriquer une requête (guillemets, barre oblique inverse, retours à la ligne,
    /// caractères de contrôle, lettres accentuées).
    ///
    /// # Errors
    ///
    /// [`DomainError::EmptyQuery`] si `input` est vide, [`DomainError::QueryTooLong`] au-delà de
    /// [`NUMBER_VALUE_MAX_CHARS`] caractères, puis [`DomainError::InvalidId`] (`expected` :
    /// `number value`) pour un caractère hors du jeu : aucune variante de [`DomainError`] n'est
    /// dédiée à cette faute, `InvalidId` en tient lieu.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        match input.chars().count() {
            0 => return Err(DomainError::EmptyQuery),
            len if len > NUMBER_VALUE_MAX_CHARS => {
                return Err(DomainError::QueryTooLong { len, max: NUMBER_VALUE_MAX_CHARS });
            }
            _ => {}
        }
        if input.chars().all(is_number_char) {
            Ok(Self(input.to_owned()))
        } else {
            Err(forbidden_character(input))
        }
    }

    /// La valeur, telle que reçue.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Vrai pour un caractère du jeu `[A-Za-z0-9.*/:_ -]` de l'annexe B §6.6.
const fn is_number_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '*' | '/' | ':' | '_' | ' ' | '-')
}

/// Erreur d'un caractère hors du jeu : en un seul endroit, pour qu'une variante dédiée du domaine
/// (demandée en `shared_requests`) la remplace sans toucher au reste.
fn forbidden_character(input: &str) -> DomainError {
    DomainError::InvalidId { input: echo(input), expected: "number value" }
}

/// Requête de recherche par numéro, entrée de `LegalCorpus::find_by_number`.
///
/// `Eq + Hash` : la valeur sert de clé de cache (`Op::FindByNumber`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NumberQuery {
    /// Fond interrogé.
    pub fond: Fond,
    /// Nature du numéro.
    pub kind: NumberKind,
    /// Valeur du numéro.
    pub value: NumberValue,
    /// Période (facette de date du fond), le cas échéant.
    pub within: Option<DateRange>,
    /// `DATE_VERSION` : `Some` pour (`CODE`|`LODA`, article|texte), `None` ailleurs.
    pub version_date: Option<VersionDate>,
}

impl NumberQuery {
    /// Assemble la requête. `at` devient `version_date` pour les recherches `CODE` et `LODA` par
    /// article ou par texte (l'adapter n'a pas d'horloge) ; elle est ignorée pour les autres
    /// couples, ce qui garde une clé de cache stable.
    ///
    /// ```
    /// use lf_domain::{
    ///     date::VersionDate,
    ///     fond::Fond,
    ///     search::{NumberKind, NumberQuery, NumberValue},
    /// };
    ///
    /// let at = VersionDate::parse("2026-10-15")?;
    /// let article = NumberQuery::new(Fond::Code, NumberKind::Article, NumberValue::parse("L36-11")?, at)?;
    /// assert_eq!(article.version_date, Some(at));
    /// let case = NumberQuery::new(Fond::Juri, NumberKind::Case, NumberValue::parse("24-13.470")?, at)?;
    /// assert_eq!(case.version_date, None);
    /// # Ok::<(), lf_domain::DomainError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`DomainError::NumberKindNotApplicable`] si `kind` n'existe pas pour `fond`.
    pub fn new(
        fond: Fond,
        kind: NumberKind,
        value: NumberValue,
        at: VersionDate,
    ) -> Result<Self, DomainError> {
        Self::build(fond, kind, value, at)
    }

    /// Corps de [`NumberQuery::new`] : la décision vit ici, car cargo-mutants ne mute jamais une
    /// fonction nommée `new` (`CONTRIBUTING.md`).
    fn build(
        fond: Fond,
        kind: NumberKind,
        value: NumberValue,
        at: VersionDate,
    ) -> Result<Self, DomainError> {
        if !kind.applies_to(fond) {
            return Err(DomainError::NumberKindNotApplicable { fond, kind: kind.as_str() });
        }
        let version_date = kind.needs_version_date(fond).then_some(at);
        Ok(Self { fond, kind, value, within: None, version_date })
    }

    /// Ajoute la période de la facette de date.
    #[must_use]
    pub fn with_within(self, within: DateRange) -> Self {
        Self { within: Some(within), ..self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ECHO_MAX_CHARS;
    use pretty_assertions::assert_eq;

    fn at() -> VersionDate {
        VersionDate::parse("2026-10-15").unwrap()
    }

    fn value(raw: &str) -> NumberValue {
        NumberValue::parse(raw).unwrap()
    }

    #[test]
    fn kind_names_are_unique_and_lowercase() {
        let names: Vec<&str> = NumberKind::ALL.iter().map(|kind| kind.as_str()).collect();
        assert_eq!(
            names,
            ["article", "text", "case", "decision", "deliberation", "idcc", "nor", "ecli", "eli"]
        );
    }

    #[test]
    fn exactly_thirteen_fond_and_kind_pairs_are_allowed() {
        let allowed: Vec<(Fond, NumberKind)> = Fond::ALL
            .into_iter()
            .flat_map(|fond| NumberKind::ALL.into_iter().map(move |kind| (fond, kind)))
            .filter(|(fond, kind)| kind.applies_to(*fond))
            .collect();
        assert_eq!(
            allowed,
            [
                (Fond::Loda, NumberKind::Article),
                (Fond::Loda, NumberKind::Text),
                (Fond::Loda, NumberKind::Nor),
                (Fond::Loda, NumberKind::Eli),
                (Fond::Code, NumberKind::Article),
                (Fond::Juri, NumberKind::Case),
                (Fond::Juri, NumberKind::Ecli),
                (Fond::Constit, NumberKind::Decision),
                (Fond::Cetat, NumberKind::Decision),
                (Fond::Cetat, NumberKind::Ecli),
                (Fond::Cnil, NumberKind::Deliberation),
                (Fond::Kali, NumberKind::Idcc),
                (Fond::Acco, NumberKind::Idcc),
            ]
        );
    }

    #[test]
    fn unlisted_pair_is_not_applicable() {
        let error =
            NumberQuery::new(Fond::Cnil, NumberKind::Ecli, value("ECLI:FR:X"), at()).unwrap_err();
        assert_eq!(error, DomainError::NumberKindNotApplicable { fond: Fond::Cnil, kind: "ecli" });
    }

    #[test]
    fn version_date_only_for_article_and_text_of_code_and_loda() {
        for (fond, kind) in [
            (Fond::Code, NumberKind::Article),
            (Fond::Loda, NumberKind::Article),
            (Fond::Loda, NumberKind::Text),
        ] {
            let query = NumberQuery::new(fond, kind, value("L36-11"), at()).unwrap();
            assert_eq!(query.version_date, Some(at()), "{fond} {kind:?}");
        }
        for (fond, kind) in [
            (Fond::Loda, NumberKind::Nor),
            (Fond::Loda, NumberKind::Eli),
            (Fond::Juri, NumberKind::Case),
            (Fond::Cetat, NumberKind::Ecli),
            (Fond::Kali, NumberKind::Idcc),
        ] {
            let query = NumberQuery::new(fond, kind, value("275"), at()).unwrap();
            assert_eq!(query.version_date, None, "{fond} {kind:?}");
        }
    }

    #[test]
    fn with_within_adds_the_period_and_keeps_the_rest() {
        let range = DateRange::new(Some(at()), None).unwrap().unwrap();
        let query =
            NumberQuery::new(Fond::Juri, NumberKind::Case, value("24-13.470"), at()).unwrap();
        assert_eq!(query.within, None);
        let with = query.clone().with_within(range);
        assert_eq!(with.within, Some(range));
        assert_eq!((with.fond, with.kind, &with.value), (query.fond, query.kind, &query.value));
    }

    #[test]
    fn value_keeps_the_text_as_received() {
        assert_eq!(value("2025-1173 QPC").as_str(), "2025-1173 QPC");
    }

    #[test]
    fn value_length_is_one_to_128_characters() {
        assert_eq!(NumberValue::parse(""), Err(DomainError::EmptyQuery));
        assert!(NumberValue::parse(&"9".repeat(NUMBER_VALUE_MAX_CHARS)).is_ok());
        assert_eq!(
            NumberValue::parse(&"9".repeat(NUMBER_VALUE_MAX_CHARS + 1)),
            Err(DomainError::QueryTooLong { len: 129, max: 128 })
        );
    }

    #[test]
    fn value_length_is_checked_before_the_charset() {
        // 129 lettres accentuées : la longueur l'emporte, comme pour la valeur ASCII de 129 « 9 ».
        let accented = "é".repeat(NUMBER_VALUE_MAX_CHARS + 1);
        assert_eq!(
            NumberValue::parse(&accented),
            Err(DomainError::QueryTooLong { len: 129, max: 128 })
        );
        // 128 lettres accentuées (256 octets) : la longueur se compte en caractères, puis le
        // jeu de caractères refuse la valeur.
        let accented = "é".repeat(NUMBER_VALUE_MAX_CHARS);
        assert_eq!(NumberValue::parse(&accented), Err(forbidden_character(&accented)));
    }

    #[test]
    fn value_charset_covers_the_numbers_of_annex_b_6_6() {
        for accepted in [
            "24-13.470",
            "2025-1173 QPC",
            "SAN-2026-002",
            "2121",
            "ECFI1524250L",
            "ECLI:FR:CCASS:2025:C100399",
            "/eli/loi/2016/10/7/ECFI1524250L/jo/texte",
            "L1242-2",
            "R. 4624-31",
            "a_b",
            "*",
        ] {
            assert_eq!(value(accepted).as_str(), accepted, "{accepted:?}");
        }
    }

    #[test]
    fn value_charset_refuses_everything_else() {
        for refused in [
            "é",
            "24-13,470",
            "a\"b",
            "a\\b",
            "a\nb",
            "a\tb",
            "a\0b",
            "a;b",
            "a<b>",
            "a(b)",
            "a%20b",
            "a\u{a0}b",
            "a\u{2011}b",
            "ａ",
            "12\u{200b}",
        ] {
            assert_eq!(
                NumberValue::parse(refused),
                Err(DomainError::InvalidId { input: refused.to_owned(), expected: "number value" }),
                "{refused:?}"
            );
        }
    }

    #[test]
    fn a_refused_value_is_echoed_truncated() {
        let long = format!("{}!", "9".repeat(100));
        assert_eq!(
            NumberValue::parse(&long),
            Err(DomainError::InvalidId {
                input: format!("{}…", "9".repeat(ECHO_MAX_CHARS)),
                expected: "number value"
            })
        );
    }

    #[test]
    fn number_kind_not_applicable() {
        // Les 13 couples autorisés sont fixés par `exactly_thirteen_fond_and_kind_pairs_are_allowed` ;
        // les 59 autres (8 fonds x 9 natures = 72) sont refusés par `NumberQuery::new`, avec le nom
        // de la nature tel que le message d'erreur le cite.
        let mut refused = 0;
        for fond in Fond::ALL {
            for kind in NumberKind::ALL {
                let result = NumberQuery::new(fond, kind, value("275"), at());
                if kind.applies_to(fond) {
                    assert!(result.is_ok(), "{fond} {kind:?}");
                } else {
                    refused += 1;
                    assert_eq!(
                        result,
                        Err(DomainError::NumberKindNotApplicable { fond, kind: kind.as_str() }),
                        "{fond} {kind:?}"
                    );
                }
            }
        }
        assert_eq!(refused, 59);
    }

    #[test]
    fn a_refused_pair_names_the_kind_and_the_fond_in_the_message() {
        let error = NumberQuery::new(Fond::Juri, NumberKind::Decision, value("2025-1173"), at())
            .unwrap_err();
        assert_eq!(error.to_string(), "decision is not a number kind of fond JURI");
    }
}
