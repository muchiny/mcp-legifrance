//! Découpage d'une décision par ses `zones` (ADR-0035, décision 5 ; 05 §5.19).
//!
//! Mesure du 2026-11-27 (V-237) :
//! - les indices sont en unités UTF-16, comme le `substring` JavaScript de l'exemple officiel ;
//! - le dernier `end` dépasse la longueur du texte de 1 ;
//! - un ou deux caractères séparent parfois deux zones ;
//! - une partie peut avoir plusieurs fragments non contigus (moyens et motivations alternent) ;
//! - les zones manquent sur une décision ancienne (source `dila`).
//!
//! Les indices sont donc bornés au texte et ramenés à la limite de caractère précédente ; les
//! fragments d'une partie sont pris par début croissant, ceux qui se chevauchent fusionnés, et
//! joints par une ligne vide : une partie n'est jamais plus longue que le texte, séparateurs
//! mis à part. Le découpage se fait sur le texte brut, avant sa mise en forme.

use super::DecisionPart;

/// Plus grand nombre de fragments gardés pour une partie : la plus découpée des décisions lues
/// en a huit en tout. Au-delà, les fragments de trop de cette partie sont ignorés ; les autres
/// parties gardent les leurs.
pub const ZONE_SPANS_MAX: usize = 256;

/// Séparateur des fragments d'une partie.
const FRAGMENT_SEPARATOR: &str = "\n\n";

/// Un fragment de zone : indices de début (inclus) et de fin (exclue), en unités UTF-16.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ZoneSpan {
    /// Début, en unités UTF-16 depuis le début du texte.
    pub start: u32,
    /// Fin exclue, en unités UTF-16.
    pub end: u32,
}

/// Zones d'une décision : les fragments de chaque partie, au plus [`ZONE_SPANS_MAX`] par partie.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Zones(Vec<(DecisionPart, ZoneSpan)>);

impl Zones {
    /// Zones faites des fragments non vides donnés, dans l'ordre reçu ; un fragment vide ou
    /// inversé est ignoré, et au-delà de [`ZONE_SPANS_MAX`] fragments d'une même partie, les
    /// suivants de cette partie aussi.
    pub fn of(spans: impl IntoIterator<Item = (DecisionPart, ZoneSpan)>) -> Self {
        // Un compteur par partie ; chaque partie n'y figure qu'une fois.
        let mut kept = DecisionPart::ALL.map(|part| (part, 0_usize));
        let mut zones = Vec::new();
        for (part, span) in spans.into_iter().filter(|(_, span)| span.start < span.end) {
            for (of, count) in &mut kept {
                if *of == part && *count < ZONE_SPANS_MAX {
                    *count = count.saturating_add(1);
                    zones.push((part, span));
                }
            }
        }
        Self(zones)
    }

    /// Vrai si la décision n'a aucune zone.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Fragments non vides de `part`, par début croissant, ceux qui se chevauchent fusionnés.
    fn spans(&self, part: DecisionPart) -> Vec<ZoneSpan> {
        let mut spans: Vec<ZoneSpan> =
            self.0.iter().filter(|(of, _)| *of == part).map(|(_, span)| *span).collect();
        spans.sort_unstable();
        let mut merged: Vec<ZoneSpan> = Vec::with_capacity(spans.len());
        for span in spans {
            match merged.last_mut() {
                Some(last) if span.start < last.end => last.end = last.end.max(span.end),
                _ => merged.push(span),
            }
        }
        merged
    }
}

/// Texte de `part` dans `text` : ses fragments, bornés au texte, pris par début croissant (ceux
/// qui se chevauchent fusionnés), sans blancs de bord et joints par une ligne vide. `None` si la
/// partie n'a aucun fragment non blanc.
///
/// ```
/// use lf_domain::judilibre::{DecisionPart, ZoneSpan, Zones, part_text};
///
/// let text = "Entête\nMotifs : é\u{1F600}x\nPAR CES MOTIFS";
/// let zones = Zones::of([
///     (DecisionPart::Motivations, ZoneSpan { start: 7, end: 20 }),
///     (DecisionPart::Dispositif, ZoneSpan { start: 20, end: 99 }),
/// ]);
/// assert_eq!(part_text(text, &zones, DecisionPart::Motivations).as_deref(), Some("Motifs : é\u{1F600}x"));
/// assert_eq!(part_text(text, &zones, DecisionPart::Dispositif).as_deref(), Some("PAR CES MOTIFS"));
/// assert_eq!(part_text(text, &zones, DecisionPart::Moyens), None);
/// ```
pub fn part_text(text: &str, zones: &Zones, part: DecisionPart) -> Option<String> {
    let spans = zones.spans(part);
    let offsets: Vec<u32> = spans.iter().flat_map(|span| [span.start, span.end]).collect();
    let bytes = byte_offsets(text, &offsets);
    let (pairs, _) = bytes.as_chunks::<2>();
    let pieces: Vec<&str> = pairs
        .iter()
        .filter_map(|[start, end]| text.get(*start..*end))
        .map(str::trim)
        .filter(|piece| !piece.is_empty())
        .collect();
    (!pieces.is_empty()).then(|| pieces.join(FRAGMENT_SEPARATOR))
}

/// Indice en octets de chaque indice UTF-16 de `offsets`, en un seul passage sur `text` : borné
/// à la longueur du texte, et ramené au début du caractère qu'il coupe (moitié d'une paire de
/// substitution).
fn byte_offsets(text: &str, offsets: &[u32]) -> Vec<usize> {
    let mut order: Vec<(u32, usize)> = offsets.iter().copied().zip(0_usize..).collect();
    order.sort_unstable();
    // (rang dans `offsets`, octet), remis dans l'ordre de `offsets` à la fin.
    let mut found = Vec::with_capacity(offsets.len());
    let mut chars = text.char_indices().peekable();
    // Unités UTF-16 avant le caractère que `chars` montre.
    let mut units: u64 = 0;
    for (wanted, slot) in order {
        let wanted = u64::from(wanted);
        while let Some((_, next)) = chars.peek() {
            let after = units.saturating_add(len_utf16(*next));
            if after > wanted {
                break;
            }
            units = after;
            chars.next();
        }
        found.push((slot, chars.peek().map_or(text.len(), |(index, _)| *index)));
    }
    found.sort_unstable();
    found.into_iter().map(|(_, byte)| byte).collect()
}

/// Longueur d'un caractère en unités UTF-16 : 1 ou 2.
fn len_utf16(c: char) -> u64 {
    if u32::from(c) > 0xFFFF { 2 } else { 1 }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    use super::*;

    fn span(start: u32, end: u32) -> ZoneSpan {
        ZoneSpan { start, end }
    }

    #[test]
    fn zone_offsets_are_utf16() {
        // « 😀 » vaut deux unités UTF-16 et quatre octets ; « é », une unité et deux octets.
        let text = "a😀é|b";
        assert_eq!(byte_offsets(text, &[0, 1, 3, 4, 5, 6]), [0, 1, 5, 7, 8, 9]);
        let zones = Zones::of([(DecisionPart::Expose, span(3, 5))]);
        assert_eq!(part_text(text, &zones, DecisionPart::Expose).as_deref(), Some("é|"));
    }

    #[test]
    fn an_offset_inside_a_surrogate_pair_falls_back_to_the_char_start() {
        let text = "a😀b";
        assert_eq!(byte_offsets(text, &[2]), [1]);
        let zones = Zones::of([(DecisionPart::Expose, span(2, 4))]);
        assert_eq!(part_text(text, &zones, DecisionPart::Expose).as_deref(), Some("😀b"));
    }

    #[test]
    fn zone_end_past_text_is_clamped() {
        let text = "Entête. PAR CES MOTIFS : casse.";
        let length = u32::try_from(text.encode_utf16().count()).unwrap();
        // Le dernier `end` des décisions lues dépasse la longueur de 1.
        let zones = Zones::of([
            (DecisionPart::Dispositif, span(8, length + 1)),
            (DecisionPart::Annexes, span(length + 5, length + 9)),
        ]);
        assert_eq!(
            part_text(text, &zones, DecisionPart::Dispositif).as_deref(),
            Some("PAR CES MOTIFS : casse.")
        );
        assert_eq!(part_text(text, &zones, DecisionPart::Annexes), None);
        assert_eq!(byte_offsets(text, &[u32::MAX, 0]), [text.len(), 0]);
    }

    #[test]
    fn fragments_are_taken_by_increasing_start_and_joined_by_a_blank_line() {
        let text = "0123456789ABCDEFGHIJ";
        let zones = Zones::of([
            (DecisionPart::Motivations, span(15, 18)),
            (DecisionPart::Moyens, span(0, 4)),
            (DecisionPart::Motivations, span(4, 8)),
            (DecisionPart::Moyens, span(8, 15)),
        ]);
        assert_eq!(
            part_text(text, &zones, DecisionPart::Motivations).as_deref(),
            Some("4567\n\nFGH")
        );
        assert_eq!(
            part_text(text, &zones, DecisionPart::Moyens).as_deref(),
            Some("0123\n\n89ABCDE")
        );
    }

    #[test]
    fn blank_or_inverted_fragments_are_skipped() {
        let text = "abc   def";
        let zones = Zones::of([
            (DecisionPart::Expose, span(3, 6)),
            (DecisionPart::Expose, span(8, 2)),
            (DecisionPart::Introduction, span(3, 6)),
            (DecisionPart::Introduction, span(5, 9)),
        ]);
        assert_eq!(part_text(text, &zones, DecisionPart::Expose), None);
        assert_eq!(part_text(text, &zones, DecisionPart::Introduction).as_deref(), Some("def"));
    }

    #[test]
    fn missing_zones_give_no_part() {
        let zones = Zones::default();
        assert!(zones.is_empty());
        for part in DecisionPart::ALL {
            assert_eq!(part_text("Un texte.", &zones, part), None);
        }
        assert!(!Zones::of([(DecisionPart::Annexes, span(0, 1))]).is_empty());
    }

    #[test]
    fn zones_keep_at_most_the_bound_per_part() {
        // Des fragments vides ou inversés ne comptent pas dans la borne.
        let junk = (0..300).map(|_| (DecisionPart::Annexes, span(5, 5)));
        let zones = Zones::of(
            junk.chain([(DecisionPart::Annexes, span(8, 2)), (DecisionPart::Annexes, span(0, 1))]),
        );
        assert_eq!(zones.spans(DecisionPart::Annexes), [span(0, 1)]);
        // 300 fragments d'annexes reçus avant le dispositif : le dispositif garde le sien.
        let many = (0..300_u32).map(|n| {
            let start = n.saturating_mul(2);
            (DecisionPart::Annexes, span(start, start.saturating_add(1)))
        });
        let zones = Zones::of(many.chain([(DecisionPart::Dispositif, span(0, 1))]));
        assert_eq!(zones.spans(DecisionPart::Annexes).len(), ZONE_SPANS_MAX);
        assert_eq!(zones.spans(DecisionPart::Dispositif), [span(0, 1)]);
        // Trois parties mêlées : chacune garde la borne entière, quels que soient les fragments
        // des autres (un compteur mal apparié en garderait la moitié).
        let parts = [DecisionPart::Moyens, DecisionPart::Motivations, DecisionPart::Dispositif];
        let mixed = (0..300_u32).flat_map(|n| {
            let start = n.saturating_mul(2);
            parts.map(|part| (part, span(start, start.saturating_add(1))))
        });
        let zones = Zones::of(mixed);
        for part in parts {
            assert_eq!(zones.spans(part).len(), ZONE_SPANS_MAX, "{part:?}");
        }
    }

    #[test]
    fn overlapping_fragments_are_merged_never_repeated() {
        let text = "0123456789";
        // Le même fragment répété jusqu'à la borne ne donne qu'une copie du texte.
        let same =
            Zones::of((0..ZONE_SPANS_MAX).map(|_| (DecisionPart::Dispositif, span(0, u32::MAX))));
        assert_eq!(part_text(text, &same, DecisionPart::Dispositif).as_deref(), Some(text));
        // Chevauchement fusionné ; fragments contigus gardés à part ; fragment inclus absorbé.
        let zones = Zones::of([
            (DecisionPart::Moyens, span(5, 8)),
            (DecisionPart::Moyens, span(0, 3)),
            (DecisionPart::Moyens, span(2, 5)),
            (DecisionPart::Moyens, span(6, 7)),
        ]);
        assert_eq!(zones.spans(DecisionPart::Moyens), [span(0, 5), span(5, 8)]);
        assert_eq!(part_text(text, &zones, DecisionPart::Moyens).as_deref(), Some("01234\n\n567"));
    }

    #[test]
    fn utf16_length_changes_after_the_basic_plane() {
        assert_eq!(len_utf16('\u{FFFF}'), 1);
        assert_eq!(len_utf16('\u{10000}'), 2);
        let text = "\u{FFFF}a\u{10000}b";
        assert_eq!(byte_offsets(text, &[1, 2, 4]), [3, 4, 8]);
    }

    proptest! {
        /// Sur tout texte et tous indices, le découpage ne panique pas, rend des limites de
        /// caractère, chaque fragment est un morceau du texte, et la partie ne répète rien : sans
        /// ses séparateurs, elle n'est pas plus longue que le texte.
        #[test]
        fn slicing_never_panics_and_stays_inside_the_text(
            text in "\\PC{0,40}[😀é]{0,3}\\PC{0,10}",
            spans in proptest::collection::vec((0_u32..80, 0_u32..80), 0..6),
        ) {
            let offsets: Vec<u32> = spans.iter().flat_map(|(a, b)| [*a, *b]).collect();
            for byte in byte_offsets(&text, &offsets) {
                prop_assert!(text.is_char_boundary(byte));
            }
            let zones = Zones::of(spans.iter().map(|(a, b)| (DecisionPart::Motivations, span(*a, *b))));
            if let Some(part) = part_text(&text, &zones, DecisionPart::Motivations) {
                let mut length = 0_usize;
                for piece in part.split(FRAGMENT_SEPARATOR) {
                    prop_assert!(text.contains(piece));
                    length = length.saturating_add(piece.len());
                }
                prop_assert!(length <= text.len());
            }
        }
    }
}
