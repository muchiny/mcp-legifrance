//! Nature d'une publication du BOFiP et filtre de nature d'une recherche (ADR-0029, décision 2).
//!
//! Le jeu `bofip-vigueur` mêle deux natures : 6 367 pages de doctrine (les commentaires) et
//! 2 806 actualités (les annonces de mise à jour), mesurées le 2026-11-15. Les actualités,
//! courtes, masquent les pages de fond : une recherche ne rend que la doctrine par défaut.
//! Le nom que le portail donne à chaque nature est l'affaire de l'adapter.

/// Nature d'une publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DoctrineKind {
    /// Page de doctrine : un commentaire de l'administration.
    Doctrine,
    /// Actualité : l'annonce d'une mise à jour.
    News,
}

impl DoctrineKind {
    /// Nom côté utilisateur : `doctrine` ou `news`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Doctrine => "doctrine",
            Self::News => "news",
        }
    }
}

/// Natures demandées à une recherche (paramètre `kind`), la doctrine seule par défaut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum KindFilter {
    /// Les pages de doctrine.
    #[default]
    Doctrine,
    /// Les actualités.
    News,
    /// Les deux natures.
    All,
}

impl KindFilter {
    /// Les valeurs admises, dans l'ordre alphabétique (message d'une valeur refusée).
    pub const NAMES: [&'static str; 3] = ["all", "doctrine", "news"];

    /// Lit une valeur de `kind`, sans tenir compte de la casse ni des espaces de bord ; `None`
    /// pour toute autre valeur.
    ///
    /// ```
    /// use lf_domain::doctrine::KindFilter;
    ///
    /// assert_eq!(KindFilter::parse(" News "), Some(KindFilter::News));
    /// assert_eq!(KindFilter::parse("actualité"), None);
    /// ```
    pub fn parse(input: &str) -> Option<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "doctrine" => Some(Self::Doctrine),
            "news" => Some(Self::News),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    /// La seule nature gardée, ou `None` quand le filtre les garde toutes.
    pub const fn kind(self) -> Option<DoctrineKind> {
        match self {
            Self::Doctrine => Some(DoctrineKind::Doctrine),
            Self::News => Some(DoctrineKind::News),
            Self::All => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn kinds_have_their_user_names() {
        assert_eq!(
            [DoctrineKind::Doctrine, DoctrineKind::News].map(DoctrineKind::name),
            ["doctrine", "news"]
        );
    }

    #[test]
    fn filter_defaults_to_doctrine() {
        assert_eq!(KindFilter::default(), KindFilter::Doctrine);
    }

    #[test]
    fn filter_reads_every_listed_name() {
        let parsed = KindFilter::NAMES.map(KindFilter::parse);
        assert_eq!(
            parsed,
            [Some(KindFilter::All), Some(KindFilter::Doctrine), Some(KindFilter::News)]
        );
    }

    #[test]
    fn filter_ignores_case_and_edge_spaces() {
        for (input, expected) in [
            ("DOCTRINE", KindFilter::Doctrine),
            (" News\t", KindFilter::News),
            ("aLL", KindFilter::All),
        ] {
            assert_eq!(KindFilter::parse(input), Some(expected));
        }
    }

    #[test]
    fn filter_rejects_other_values() {
        for input in
            ["", " ", "actualité", "doctrines", "new", "all news", "contenu", "ALL\u{212a}"]
        {
            assert_eq!(KindFilter::parse(input), None);
        }
    }

    #[test]
    fn filter_names_the_single_kind_it_keeps() {
        assert_eq!(
            [KindFilter::Doctrine, KindFilter::News, KindFilter::All].map(KindFilter::kind),
            [Some(DoctrineKind::Doctrine), Some(DoctrineKind::News), None]
        );
    }
}
