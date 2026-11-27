//! Parties d'une décision de Judilibre (ADR-0035, décisions 4 et 5).
//!
//! Taxonomie lue le 2026-11-27 (`/taxonomy?id=field`) : `introduction` (entête), `expose`
//! (exposé du litige), `moyens`, `motivations` (motivation), `dispositif`, `annexes` (moyens
//! annexés), `visa` (textes appliqués), `summary` (sommaire), `themes` (titre) et `text` (texte
//! entier). Les six premières sont aussi les clés des `zones` d'une décision.

/// Partie d'une décision où chercher (`part` de `fr_search_judilibre`, envoyée en `field`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SearchPart {
    /// Motivation (`motivations`) : la plus demandée, et la plus lente (6,6 s mesurées).
    Motivations,
    /// Dispositif (`dispositif`).
    Dispositif,
    /// Exposé du litige (`expose`).
    Expose,
    /// Moyens (`moyens`).
    Moyens,
    /// Sommaire de la Cour de cassation (`summary`).
    Summary,
    /// Textes appliqués (`visa`).
    Visa,
}

impl SearchPart {
    /// Les six parties, dans l'ordre de la description de l'outil.
    pub const ALL: [Self; 6] =
        [Self::Motivations, Self::Dispositif, Self::Expose, Self::Moyens, Self::Summary, Self::Visa];

    /// Valeurs admises de `part`, dans l'ordre de [`SearchPart::ALL`].
    pub const NAMES: [&'static str; 6] =
        ["motivations", "dispositif", "expose", "moyens", "summary", "visa"];

    /// Nom de la partie, qui est aussi la valeur de `field` envoyée à l'API.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Motivations => "motivations",
            Self::Dispositif => "dispositif",
            Self::Expose => "expose",
            Self::Moyens => "moyens",
            Self::Summary => "summary",
            Self::Visa => "visa",
        }
    }

    /// Partie d'un nom, insensible à la casse et aux espaces de bord.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        Self::ALL.into_iter().find(|part| part.name().eq_ignore_ascii_case(raw))
    }
}

/// Partie d'une décision à lire (`part` de `fr_get_judilibre`) : une clé de ses `zones`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DecisionPart {
    /// Entête (`introduction`).
    Introduction,
    /// Exposé du litige (`expose`).
    Expose,
    /// Moyens (`moyens`).
    Moyens,
    /// Motivation (`motivations`).
    Motivations,
    /// Dispositif (`dispositif`).
    Dispositif,
    /// Moyens annexés (`annexes`).
    Annexes,
}

impl DecisionPart {
    /// Les six parties, dans l'ordre d'une décision.
    pub const ALL: [Self; 6] = [
        Self::Introduction,
        Self::Expose,
        Self::Moyens,
        Self::Motivations,
        Self::Dispositif,
        Self::Annexes,
    ];

    /// Valeurs admises de `part`, dans l'ordre de [`DecisionPart::ALL`].
    pub const NAMES: [&'static str; 6] =
        ["introduction", "expose", "moyens", "motivations", "dispositif", "annexes"];

    /// Nom de la partie, qui est aussi sa clé dans les `zones` d'une décision.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Introduction => "introduction",
            Self::Expose => "expose",
            Self::Moyens => "moyens",
            Self::Motivations => "motivations",
            Self::Dispositif => "dispositif",
            Self::Annexes => "annexes",
        }
    }

    /// Partie d'un nom, insensible à la casse et aux espaces de bord.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        Self::ALL.into_iter().find(|part| part.name().eq_ignore_ascii_case(raw))
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn search_parts_round_trip() {
        assert_eq!(SearchPart::ALL.map(SearchPart::name), SearchPart::NAMES);
        for part in SearchPart::ALL {
            assert_eq!(SearchPart::parse(part.name()), Some(part));
            assert_eq!(SearchPart::parse(&format!(" {} ", part.name().to_uppercase())), Some(part));
        }
        for raw in ["", "text", "themes", "introduction", "annexes", "motivation"] {
            assert_eq!(SearchPart::parse(raw), None);
        }
    }

    #[test]
    fn decision_parts_round_trip() {
        assert_eq!(DecisionPart::ALL.map(DecisionPart::name), DecisionPart::NAMES);
        for part in DecisionPart::ALL {
            assert_eq!(DecisionPart::parse(part.name()), Some(part));
            assert_eq!(
                DecisionPart::parse(&format!("\t{}\n", part.name().to_uppercase())),
                Some(part)
            );
        }
        for raw in ["", "text", "summary", "visa", "motif"] {
            assert_eq!(DecisionPart::parse(raw), None);
        }
    }
}
