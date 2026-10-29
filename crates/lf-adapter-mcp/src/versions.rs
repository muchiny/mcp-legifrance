//! Versions du protocole MCP annoncées par le serveur (00 §6, Q7).

use rmcp::model::ProtocolVersion;

/// Versions servies, de la plus récente à la plus ancienne : la version moderne 2026-07-28, puis
/// les deux versions « legacy » 2025-11-25 et 2025-06-18. 2025-03-26 est exclue (Q7). Liste
/// explicite : jamais `ProtocolVersion::KNOWN_VERSIONS`, qui suit rmcp et non nos engagements.
pub(crate) const SUPPORTED: &[ProtocolVersion] =
    &[ProtocolVersion::V_2026_07_28, ProtocolVersion::V_2025_11_25, ProtocolVersion::V_2025_06_18];

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn supported_lists_the_modern_version_then_the_two_legacy_ones() {
        let listed: Vec<&str> = SUPPORTED.iter().map(ProtocolVersion::as_str).collect();
        assert_eq!(listed, ["2026-07-28", "2025-11-25", "2025-06-18"]);
    }

    #[test]
    fn the_oldest_version_of_the_sdk_is_not_supported() {
        assert!(!SUPPORTED.contains(&ProtocolVersion::V_2025_03_26));
    }
}
