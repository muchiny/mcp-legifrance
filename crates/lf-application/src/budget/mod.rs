//! Budget de réponse (02 §6.1, 00 §7) : bornes et défauts des paramètres de lecture, plafond dur
//! de 45 000 caractères et estimation conservatrice qui l'assure.
//!
//! Le plafond se tient par estimation avant la sérialisation (somme des longueurs plus 64 par
//! champ, cible 42 000), dans l'ordre de dégradation de 00 §7 : la table des matières se raccourcit
//! ([`shrink_from_end`]), puis les extraits des recherches se retirent en partant du dernier
//! résultat ([`strip_from_end`]), puis les listes de citations se tronquent. L'adapter MCP
//! mesure ensuite la taille exacte de la sortie.

use crate::{error::AppError, messages};

pub mod reading;

pub use reading::{PassageOut, Reading, ReadingRequest, read};

/// Les bornes de réponse, en constantes : aucun réglage par instance (D12 du plan « fin du
/// projet » : ni `MaxChars`, ni `PassageCount`, ni `TocLimit`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResponseBudget;

impl ResponseBudget {
    /// `max_chars` par défaut : plafond du texte rendu.
    pub const MAX_CHARS_DEFAULT: usize = 4_000;
    /// `max_chars` minimal.
    pub const MAX_CHARS_MIN: usize = 500;
    /// `max_chars` maximal.
    pub const MAX_CHARS_MAX: usize = 40_000;
    /// `passages` par défaut.
    pub const PASSAGES_DEFAULT: usize = 3;
    /// `passages` maximal.
    pub const PASSAGES_MAX: usize = 10;
    /// Longueur maximale d'un extrait de recherche, `…` compris.
    pub const SNIPPET_MAX_CHARS: usize = lf_domain::entity::SNIPPET_MAX_CHARS;
    /// `toc_limit` par défaut.
    pub const TOC_LIMIT_DEFAULT: usize = 100;
    /// `toc_limit` maximal.
    pub const TOC_LIMIT_MAX: usize = 500;
    /// `max_citations` par défaut.
    pub const MAX_CITATIONS_DEFAULT: usize = 50;
    /// `max_citations` maximal.
    pub const MAX_CITATIONS_MAX: usize = 100;
    /// Plafond dur d'une réponse, en caractères de JSON compact (00 §7).
    pub const HARD_CAP_CHARS: usize = 45_000;
    /// Cible de l'estimation, sous le plafond dur : l'estimation est conservatrice mais pas exacte.
    pub const ESTIMATE_TARGET_CHARS: usize = 42_000;
    /// Poids estimé d'un champ ou d'un élément de liste (clé, guillemets, séparateurs).
    pub const ESTIMATE_PER_FIELD: usize = 64;
    /// Poids estimé d'un champ ou d'un élément dans les listes de résultats de `fr_search` et de
    /// liens de `fr_get_related` : une clé JSON courte, ses guillemets et son séparateur en pèsent
    /// 10 à 18, d'où une marge sous 24 (64 surestimait d'un facteur 2 à 3 et rognait ou refusait
    /// des pages qui tenaient).
    pub const ESTIMATE_PER_LIST_FIELD: usize = 24;
    /// Nombre de résultats de recherche atteignables (`page × page_size`).
    pub const REACHABLE_RESULTS: u64 = lf_domain::search::REACHABLE_RESULTS;
}

/// Taille estimée d'une réponse : `fields` champs ou éléments à [`ResponseBudget::ESTIMATE_PER_FIELD`]
/// caractères, plus `text_chars` caractères de texte. Les opérations saturent : une entrée
/// démesurée ne fait pas déborder l'estimation.
#[must_use]
pub const fn estimate_chars(fields: usize, text_chars: usize) -> usize {
    fields.saturating_mul(ResponseBudget::ESTIMATE_PER_FIELD).saturating_add(text_chars)
}

/// Taille estimée d'une liste de résultats ou de liens : `fields` champs ou éléments à
/// [`ResponseBudget::ESTIMATE_PER_LIST_FIELD`] caractères, plus `text_chars` caractères de texte
/// (mesurés par [`escaped_chars`]). Même saturation que [`estimate_chars`], dont le coefficient
/// global reste celui des autres outils.
#[must_use]
pub const fn estimate_list_chars(fields: usize, text_chars: usize) -> usize {
    fields.saturating_mul(ResponseBudget::ESTIMATE_PER_LIST_FIELD).saturating_add(text_chars)
}

/// Longueur de `text` une fois écrit dans un JSON compact : un point de code compte pour un
/// caractère, sauf `"` et `\` (deux), les contrôles `\b \t \n \f \r` (deux) et les autres
/// contrôles U+0000 à U+001F (six, `\u00XX`). Sature.
#[must_use]
pub fn escaped_chars(text: &str) -> usize {
    text.chars()
        .map(|c| match c {
            '"' | '\\' | '\u{8}' | '\t' | '\n' | '\u{c}' | '\r' => 2,
            c if c < ' ' => 6,
            _ => 1,
        })
        .fold(0, usize::saturating_add)
}

/// « the last hit » ou « the last N hits » : le groupe nominal des notes de dégradation.
#[must_use]
pub fn the_last_hits(count: usize) -> String {
    if count == 1 { "the last hit".to_owned() } else { format!("the last {count} hits") }
}

/// Retire des éléments en partant de la fin tant que `over` dit que la liste est trop grosse
/// (table des matières, listes de citations). Rend le nombre d'éléments retirés ; la liste peut
/// se vider si rien ne tient.
pub fn shrink_from_end<T>(items: &mut Vec<T>, mut over: impl FnMut(&[T]) -> bool) -> usize {
    let mut removed = 0_usize;
    while !items.is_empty() && over(items) {
        items.pop();
        removed = removed.saturating_add(1);
    }
    removed
}

/// Retire un champ secondaire (`strip` : l'extrait d'un résultat) en partant du dernier élément
/// tant que `over` dit que la liste est trop grosse. Aucun élément n'est jamais retiré : `strip`
/// vide le champ et dit s'il a trouvé quelque chose à retirer. Rend le nombre d'éléments dont le
/// champ a été retiré.
pub fn strip_from_end<T>(
    items: &mut [T],
    mut over: impl FnMut(&[T]) -> bool,
    mut strip: impl FnMut(&mut T) -> bool,
) -> usize {
    let mut stripped = 0_usize;
    for index in (0..items.len()).rev() {
        if !over(items) {
            break;
        }
        if let Some(item) = items.get_mut(index)
            && strip(item)
        {
            stripped = stripped.saturating_add(1);
        }
    }
    stripped
}

/// Entier optionnel d'un paramètre, ramené dans `min..=max` (bornes incluses) : `default` s'il est
/// absent, [`messages::out_of_range`] (`max_chars=100 out of range (500..40000).`) s'il sort des
/// bornes, négatif ou démesuré compris. Commun aux paramètres `max_chars`, `passages`,
/// `toc_limit`, `per_fond` et `max_citations`.
///
/// # Errors
///
/// [`AppError::InvalidArgument`] quand `value` est hors de `min..=max`.
pub fn bounded(
    field: &'static str,
    value: Option<i64>,
    default: usize,
    min: usize,
    max: usize,
) -> Result<usize, AppError> {
    let Some(value) = value else { return Ok(default) };
    match usize::try_from(value) {
        Ok(in_range) if (min..=max).contains(&in_range) => Ok(in_range),
        _ => Err(messages::out_of_range(field, value, to_i64(min), to_i64(max))),
    }
}

/// `usize` vers `i64`, saturant : les bornes du budget tiennent largement dans un `i64`.
fn to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn constants_match_the_budget_table() {
        let budget = [
            (ResponseBudget::MAX_CHARS_DEFAULT, 4_000),
            (ResponseBudget::MAX_CHARS_MIN, 500),
            (ResponseBudget::MAX_CHARS_MAX, 40_000),
            (ResponseBudget::PASSAGES_DEFAULT, 3),
            (ResponseBudget::PASSAGES_MAX, 10),
            (ResponseBudget::SNIPPET_MAX_CHARS, 280),
            (ResponseBudget::TOC_LIMIT_DEFAULT, 100),
            (ResponseBudget::TOC_LIMIT_MAX, 500),
            (ResponseBudget::MAX_CITATIONS_DEFAULT, 50),
            (ResponseBudget::MAX_CITATIONS_MAX, 100),
            (ResponseBudget::HARD_CAP_CHARS, 45_000),
            (ResponseBudget::ESTIMATE_TARGET_CHARS, 42_000),
            (ResponseBudget::ESTIMATE_PER_FIELD, 64),
            (ResponseBudget::ESTIMATE_PER_LIST_FIELD, 24),
        ];
        for (actual, expected) in budget {
            assert_eq!(actual, expected);
        }
        assert_eq!(ResponseBudget::REACHABLE_RESULTS, 10_000);
        const { assert!(ResponseBudget::ESTIMATE_TARGET_CHARS < ResponseBudget::HARD_CAP_CHARS) };
        assert_eq!(format!("{ResponseBudget:?}"), "ResponseBudget");
    }

    #[test]
    fn estimate_counts_64_per_field() {
        assert_eq!(estimate_chars(0, 0), 0);
        assert_eq!(estimate_chars(1, 0), 64);
        assert_eq!(estimate_chars(10, 250), 890);
        assert_eq!(estimate_chars(0, 4_000), 4_000);
        // Saturant : ni dépassement ni panique.
        assert_eq!(estimate_chars(usize::MAX, 1), usize::MAX);
        assert_eq!(estimate_chars(1, usize::MAX), usize::MAX);
    }

    #[test]
    fn list_estimate_counts_24_per_field_and_saturates() {
        assert_eq!(estimate_list_chars(0, 0), 0);
        assert_eq!(estimate_list_chars(1, 0), 24);
        assert_eq!(estimate_list_chars(10, 250), 490);
        assert_eq!(estimate_list_chars(0, 4_000), 4_000);
        assert_eq!(estimate_list_chars(usize::MAX, 1), usize::MAX);
        assert_eq!(estimate_list_chars(1, usize::MAX), usize::MAX);
        // Le coefficient global des autres outils ne bouge pas.
        assert_eq!(estimate_chars(1, 0), 64);
    }

    #[test]
    fn escaped_chars_matches_the_compact_json_length() {
        assert_eq!(escaped_chars(""), 0);
        assert_eq!(escaped_chars("plain é \u{1F600}"), 9);
        // Guillemet, antislash, \b \t \n \f \r : deux caractères chacun.
        assert_eq!(escaped_chars("\"\\\u{8}\t\n\u{c}\r"), 14);
        // Autres contrôles : \u00XX, six caractères, bornes U+0000 et U+001F comprises.
        assert_eq!(escaped_chars("\u{0}\u{1f}"), 12);
        // U+0020 n'est pas un contrôle.
        assert_eq!(escaped_chars(" "), 1);
        // U+007F et les C1 ne sont pas échappés par `serde_json`.
        assert_eq!(escaped_chars("\u{7f}\u{80}"), 2);
        assert_eq!(escaped_chars("a\"b"), 4);
    }

    #[test]
    fn the_last_hits_is_singular_for_one() {
        assert_eq!(the_last_hits(1), "the last hit");
        assert_eq!(the_last_hits(2), "the last 2 hits");
        assert_eq!(the_last_hits(33), "the last 33 hits");
    }

    #[test]
    fn shrink_stops_as_soon_as_under() {
        let mut items: Vec<u32> = (1..=10).collect();
        // Trop gros tant que la somme dépasse 30 : 55, 45, 36, 28 → trois retraits.
        let removed = shrink_from_end(&mut items, |all| all.iter().sum::<u32>() > 30);
        assert_eq!(removed, 3);
        assert_eq!(items, (1..=7).collect::<Vec<u32>>());
        // Déjà sous la borne : rien n'est retiré.
        assert_eq!(shrink_from_end(&mut items, |_| false), 0);
        assert_eq!(items.len(), 7);
        // Rien ne tient : la liste se vide, sans tourner dans le vide.
        assert_eq!(shrink_from_end(&mut items, |_| true), 7);
        assert_eq!(items, Vec::<u32>::new());
        assert_eq!(shrink_from_end(&mut items, |_| true), 0);
    }

    #[test]
    fn shrink_asks_over_with_the_current_list() {
        let mut seen = Vec::new();
        let mut items = vec!['a', 'b', 'c'];
        shrink_from_end(&mut items, |all| {
            seen.push(all.len());
            all.len() > 1
        });
        assert_eq!(seen, [3, 2, 1]);
        assert_eq!(items, ['a']);
    }

    #[test]
    fn strip_never_removes_items() {
        // Un résultat de recherche : (identifiant, extrait).
        let mut hits: Vec<(u8, Option<&str>)> =
            vec![(1, Some("aa")), (2, Some("bb")), (3, None), (4, Some("dd"))];
        let size = |all: &[(u8, Option<&str>)]| -> usize {
            all.iter().map(|(_, snippet)| snippet.map_or(0, str::len)).sum()
        };
        // Trop gros tant qu'il reste plus de 2 caractères d'extrait : on retire les extraits du
        // dernier résultat vers le premier (le troisième n'en a pas, il n'est pas compté).
        let stripped = strip_from_end(&mut hits, |all| size(all) > 2, |hit| hit.1.take().is_some());
        assert_eq!(stripped, 2);
        assert_eq!(hits, vec![(1, Some("aa")), (2, None), (3, None), (4, None)]);
        // Les éléments, eux, sont tous là.
        assert_eq!(hits.iter().map(|(id, _)| *id).collect::<Vec<_>>(), [1, 2, 3, 4]);
    }

    #[test]
    fn strip_stops_when_under_and_when_nothing_is_left() {
        let mut values = [1, 2, 3];
        let calls = std::cell::Cell::new(0);
        let mut strip = |_: &mut i32| {
            calls.set(calls.get() + 1);
            false
        };
        // Pas trop gros d'emblée : `strip` n'est jamais appelé.
        assert_eq!(strip_from_end(&mut values, |_| false, &mut strip), 0);
        assert_eq!(values, [1, 2, 3]);
        assert_eq!(calls.get(), 0);
        // `over` reste vrai mais `strip` ne trouve rien : on parcourt toute la liste une fois.
        assert_eq!(strip_from_end(&mut values, |_| true, &mut strip), 0);
        assert_eq!(calls.get(), 3);
        let mut empty: [i32; 0] = [];
        assert_eq!(strip_from_end(&mut empty, |_| true, &mut strip), 0);
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn bounded_defaults_when_absent_and_accepts_both_bounds() {
        assert_eq!(bounded("max_chars", None, 4_000, 500, 40_000), Ok(4_000));
        assert_eq!(bounded("max_chars", Some(500), 4_000, 500, 40_000), Ok(500));
        assert_eq!(bounded("max_chars", Some(40_000), 4_000, 500, 40_000), Ok(40_000));
        assert_eq!(bounded("passages", Some(7), 3, 1, 10), Ok(7));
    }

    #[test]
    fn bounded_rejects_values_outside_the_bounds_with_the_reference_wording() {
        for value in [100, 499, 40_001, 0, -1, i64::MIN, i64::MAX] {
            let error = bounded("max_chars", Some(value), 4_000, 500, 40_000).unwrap_err();
            assert_eq!(error.code(), Some(ErrorCode::InvalidArg), "{value}");
            assert_eq!(error.message(), format!("max_chars={value} out of range (500..40000)."));
        }
    }
}
