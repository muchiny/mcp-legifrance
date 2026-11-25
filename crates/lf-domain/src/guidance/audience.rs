//! Public d'une fiche de Service-Public et filtre de public d'une recherche (ADR-0033,
//! décision 6 ; Q39 = A).
//!
//! La DILA publie un flux par public : Particuliers (`www.service-public.gouv.fr`) et
//! Entreprendre (`entreprendre.service-public.gouv.fr`, pour les entreprises et les
//! indépendants). Le flux Associations n'est pas servi. Le nom que le flux donne à chaque
//! public (`part`, `pro`) est l'affaire de l'adapter.

/// Public d'une fiche.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Audience {
    /// Particuliers, le public par défaut.
    #[default]
    Particuliers,
    /// Entreprendre : entreprises et indépendants.
    Entreprendre,
}

impl Audience {
    /// Les deux publics, Particuliers d'abord.
    pub const ALL: [Self; 2] = [Self::Particuliers, Self::Entreprendre];

    /// Les valeurs admises, dans l'ordre alphabétique (message d'une valeur refusée).
    pub const NAMES: [&'static str; 2] = ["entreprendre", "particuliers"];

    /// Lit une valeur de `audience`, sans tenir compte de la casse ni des espaces de bord ;
    /// `None` pour toute autre valeur, `all` compris.
    ///
    /// ```
    /// use lf_domain::guidance::Audience;
    ///
    /// assert_eq!(Audience::parse(" Entreprendre "), Some(Audience::Entreprendre));
    /// assert_eq!(Audience::parse("all"), None);
    /// ```
    pub fn parse(input: &str) -> Option<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "particuliers" => Some(Self::Particuliers),
            "entreprendre" => Some(Self::Entreprendre),
            _ => None,
        }
    }

    /// Nom côté utilisateur : `particuliers` ou `entreprendre`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Particuliers => "particuliers",
            Self::Entreprendre => "entreprendre",
        }
    }

    /// Nom du site de ce public, dans la citation et l'attribution (ADR-0033, décision 9).
    pub const fn site(self) -> &'static str {
        match self {
            Self::Particuliers => "Service-Public.gouv.fr",
            Self::Entreprendre => "Entreprendre.Service-Public.gouv.fr",
        }
    }

    /// L'autre public, que nomme le message d'un numéro absent (05 §5.17).
    #[must_use]
    pub const fn other(self) -> Self {
        match self {
            Self::Particuliers => Self::Entreprendre,
            Self::Entreprendre => Self::Particuliers,
        }
    }
}

/// Publics cherchés (paramètre `audience` de la recherche), Particuliers seul par défaut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AudienceFilter {
    /// Particuliers.
    #[default]
    Particuliers,
    /// Entreprendre.
    Entreprendre,
    /// Les deux publics.
    All,
}

impl AudienceFilter {
    /// Les valeurs admises, dans l'ordre alphabétique (message d'une valeur refusée).
    pub const NAMES: [&'static str; 3] = ["all", "entreprendre", "particuliers"];

    /// Lit une valeur de `audience`, sans tenir compte de la casse ni des espaces de bord ;
    /// `None` pour toute autre valeur.
    ///
    /// ```
    /// use lf_domain::guidance::AudienceFilter;
    ///
    /// assert_eq!(AudienceFilter::parse(" ALL "), Some(AudienceFilter::All));
    /// assert_eq!(AudienceFilter::parse("associations"), None);
    /// ```
    pub fn parse(input: &str) -> Option<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "all" => Some(Self::All),
            other => Audience::parse(other).map(Self::from),
        }
    }

    /// Les publics cherchés, Particuliers d'abord : c'est sous lui qu'un numéro commun aux deux
    /// est rendu (05 §5.16).
    pub const fn audiences(self) -> &'static [Audience] {
        match self {
            Self::Particuliers => &[Audience::Particuliers],
            Self::Entreprendre => &[Audience::Entreprendre],
            Self::All => &Audience::ALL,
        }
    }
}

impl From<Audience> for AudienceFilter {
    fn from(audience: Audience) -> Self {
        match audience {
            Audience::Particuliers => Self::Particuliers,
            Audience::Entreprendre => Self::Entreprendre,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn audiences_have_their_user_names() {
        assert_eq!(Audience::ALL.map(Audience::name), ["particuliers", "entreprendre"]);
        let mut sorted = Audience::ALL.map(Audience::name);
        sorted.sort_unstable();
        assert_eq!(sorted, Audience::NAMES);
    }

    #[test]
    fn each_audience_names_its_site() {
        assert_eq!(
            Audience::ALL.map(Audience::site),
            ["Service-Public.gouv.fr", "Entreprendre.Service-Public.gouv.fr"]
        );
    }

    #[test]
    fn audience_parses_its_names_only() {
        for (input, expected) in [
            ("particuliers", Audience::Particuliers),
            (" PARTICULIERS\t", Audience::Particuliers),
            ("Entreprendre", Audience::Entreprendre),
        ] {
            assert_eq!(Audience::parse(input), Some(expected));
        }
        for input in ["", "all", "associations", "part", "pro", "particulier", "entreprises"] {
            assert_eq!(Audience::parse(input), None, "{input:?}");
        }
    }

    #[test]
    fn particuliers_is_the_default_and_each_names_the_other() {
        assert_eq!(Audience::default(), Audience::Particuliers);
        assert_eq!(Audience::Particuliers.other(), Audience::Entreprendre);
        assert_eq!(Audience::Entreprendre.other(), Audience::Particuliers);
    }

    #[test]
    fn filter_parses_all_and_each_audience() {
        for (input, expected) in [
            ("all", AudienceFilter::All),
            (" All ", AudienceFilter::All),
            ("particuliers", AudienceFilter::Particuliers),
            ("ENTREPRENDRE", AudienceFilter::Entreprendre),
        ] {
            assert_eq!(AudienceFilter::parse(input), Some(expected));
        }
        for input in ["", "both", "tous", "associations", "a ll"] {
            assert_eq!(AudienceFilter::parse(input), None, "{input:?}");
        }
        assert_eq!(AudienceFilter::default(), AudienceFilter::Particuliers);
        let mut names: Vec<&str> =
            Audience::NAMES.iter().copied().chain(std::iter::once("all")).collect();
        names.sort_unstable();
        assert_eq!(names, AudienceFilter::NAMES);
    }

    #[test]
    fn filter_lists_particuliers_first() {
        assert_eq!(AudienceFilter::Particuliers.audiences(), [Audience::Particuliers]);
        assert_eq!(AudienceFilter::Entreprendre.audiences(), [Audience::Entreprendre]);
        assert_eq!(
            AudienceFilter::All.audiences(),
            [Audience::Particuliers, Audience::Entreprendre]
        );
        for audience in Audience::ALL {
            assert_eq!(AudienceFilter::from(audience).audiences(), [audience]);
        }
    }
}
