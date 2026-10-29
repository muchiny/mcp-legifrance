//! Recherche par numéro (M6, `find_by_number`) : nature du numéro, valeur et requête (03 §4.11,
//! annexe B §6.6, qui fait foi pour la forme et pour les couples (fond, nature) autorisés).
//!
//! Ces types sont du domaine, et non de l'application : la clé de cache `Op::FindByNumber` et
//! l'adapter PISTE les emploient sans dépendre de `lf-application`, et le constructeur rend une
//! [`DomainError`]. Ils sont réexportés par [`search`](crate::search).

use crate::{
    date::{DateRange, VersionDate},
    error::DomainError,
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
    /// STUB(V0) → V2-08 : seule la longueur est contrôlée. Le jeu de caractères et l'erreur qui
    /// l'accompagne (aucune variante de [`DomainError`] n'est encore dédiée) restent à écrire.
    ///
    /// # Errors
    ///
    /// [`DomainError::EmptyQuery`] si `input` est vide, [`DomainError::QueryTooLong`] au-delà de
    /// [`NUMBER_VALUE_MAX_CHARS`] caractères.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        // STUB(V0) → V2-08
        match input.chars().count() {
            0 => Err(DomainError::EmptyQuery),
            len if len > NUMBER_VALUE_MAX_CHARS => {
                Err(DomainError::QueryTooLong { len, max: NUMBER_VALUE_MAX_CHARS })
            }
            _ => Ok(Self(input.to_owned())),
        }
    }

    /// La valeur, telle que reçue.
    pub fn as_str(&self) -> &str {
        &self.0
    }
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
        // STUB(V0) → V2-08 : le jeu de caractères est éprouvé là-bas.
        assert_eq!(NumberValue::parse(""), Err(DomainError::EmptyQuery));
        assert!(NumberValue::parse(&"9".repeat(NUMBER_VALUE_MAX_CHARS)).is_ok());
        assert_eq!(
            NumberValue::parse(&"9".repeat(NUMBER_VALUE_MAX_CHARS + 1)),
            Err(DomainError::QueryTooLong { len: 129, max: 128 })
        );
    }
}
