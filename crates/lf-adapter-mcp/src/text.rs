//! Textes du serveur : instructions et surface annoncée (05 §4, Q14, Q15).
//!
//! Source unique de ces textes. Les titres et descriptions d'outils (anglais) et les textes des
//! prompts (français) rejoignent ce module avec leurs tranches.

/// Surface d'outils annoncée par le serveur : chaque jalon étend la précédente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// v0.1 : recherche, lecture, couverture (M3 à M5).
    V01,
    /// v0.2 : plus la vérification et la résolution de citations (M6).
    V02,
    /// v0.3 : plus la recherche multi-fonds, les prompts et les ressources (M7).
    V03,
}

/// Surface servie par ce binaire. L'intégrateur la fait avancer avec les outils enregistrés
/// (`ToolName::ALL`) ; la valeur finale est fixée en F1.
pub const SURFACE: Surface = Surface::V01;

// STUB(V0) → V1-10 : V1-10 écrit les trois textes d'instructions (05 §4, 3 500 caractères au plus,
// attribution des données comprise).

/// Instructions de la surface v0.1.
#[allow(dead_code, reason = "STUB(V0) → V1-10 : texte et `get_info` de V1-10")]
pub(crate) const INSTRUCTIONS_V01: &str = "";
/// Instructions de la surface v0.2.
#[allow(dead_code, reason = "STUB(V0) → V1-10 : texte et `get_info` de V1-10")]
pub(crate) const INSTRUCTIONS_V02: &str = "";
/// Instructions de la surface v0.3.
#[allow(dead_code, reason = "STUB(V0) → V1-10 : texte et `get_info` de V1-10")]
pub(crate) const INSTRUCTIONS_V03: &str = "";

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_surface_starts_at_v01() {
        assert_eq!(SURFACE, Surface::V01);
    }

    #[test]
    fn surfaces_are_distinct_and_copyable() {
        let surface = SURFACE;
        let copy = surface;
        assert_eq!(surface, copy);
        assert_ne!(Surface::V02, Surface::V03);
        assert_eq!(format!("{:?}", Surface::V02), "V02");
    }
}
