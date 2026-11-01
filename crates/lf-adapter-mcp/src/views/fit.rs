//! Garde du plafond dur d'une réponse (05 §6.4, 02 §6.1, 00 §7).
//!
//! Les tranches d'outils mesurent ici la taille **exacte** du JSON compact, en caractères, de la
//! vue construite. Pour `fr_search`, l'application a déjà compté exactement ce JSON (échappements
//! compris) avant de construire la vue : cette mesure n'est qu'un filet. Pour les autres outils,
//! l'estimation (`budget::estimate_chars`) est conservatrice mais pas exacte : une fenêtre riche en
//! échappements JSON (`"`, `\`, U+0000 à U+001F, qui prennent 2 à 6 caractères) ou en caractères
//! hors BMP peut encore dépasser le plafond, d'où cette seconde garde. Exception :
//! `fr_verify_citations` n'estime plus (L2-13), cette mesure y est la seule à couper la liste.

use lf_application::budget::ResponseBudget;
use serde::Serialize;

/// Taille en caractères d'un JSON compact (sans espaces), échappements compris ; une vue qui ne
/// se sérialise pas compte comme démesurée. Non générique : une seule instance à couvrir.
fn chars_of(json: Result<String, serde_json::Error>) -> usize {
    match json {
        Ok(json) => json.chars().count(),
        Err(_) => usize::MAX,
    }
}

fn measure<V: Serialize>(view: &V) -> usize {
    chars_of(serde_json::to_string(view))
}

/// Mesure `view` puis la dégrade tant qu'elle dépasse [`ResponseBudget::HARD_CAP_CHARS`] et que
/// `degrade` rend `true`. Rend la taille finale, en caractères de JSON compact.
///
/// `degrade` retire un peu de contenu à chaque appel (le dernier extrait, la dernière entrée de
/// table des matières…) et rend `true` seulement s'il a modifié la vue ; il rend `false` quand il
/// n'a plus rien à retirer. Si la vue reste au-dessus du plafond, un `warn!` le signale (sans
/// jamais citer son contenu) : la réponse part telle quelle plutôt que d'être perdue.
pub(crate) fn fit_under_cap<V: Serialize>(
    view: &mut V,
    mut degrade: impl FnMut(&mut V) -> bool,
) -> usize {
    let cap = ResponseBudget::HARD_CAP_CHARS;
    let mut size = measure(view);
    while size > cap && degrade(view) {
        size = measure(view);
    }
    if size > cap {
        tracing::warn!(size, cap, "response still over the hard cap after degradation");
    }
    size
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde::{Serialize, Serializer};

    use super::*;

    const CAP: usize = ResponseBudget::HARD_CAP_CHARS;

    /// Vue témoin : une liste de textes, comme les `hits` d'une recherche.
    #[derive(Serialize)]
    struct View {
        hits: Vec<String>,
    }

    impl View {
        fn of(hits: &[&str]) -> Self {
            Self { hits: hits.iter().map(|hit| (*hit).to_owned()).collect() }
        }
    }

    /// Taille de référence, calculée à part : les caractères du JSON compact de `serde_json`.
    fn json_chars(view: &impl Serialize) -> usize {
        serde_json::to_string(view).unwrap().chars().count()
    }

    /// Dégradation témoin : retire le dernier résultat, sauf le seul qui reste. Tous les tests de
    /// la vue `View` la passent à `fit_under_cap` : llvm-cov ne retient, pour une fonction générique,
    /// que l'instance la mieux couverte, qui doit donc couvrir seule toutes les branches.
    fn trim_hits(view: &mut View) -> bool {
        if view.hits.len() > 1 {
            view.hits.pop();
            true
        } else {
            false
        }
    }

    #[test]
    fn fit_returns_size_without_degrading_when_under_cap() {
        let mut view = View::of(&["alpha", "beta"]);
        let size = fit_under_cap(&mut view, trim_hits);
        assert_eq!(size, r#"{"hits":["alpha","beta"]}"#.len());
        assert_eq!(view.hits, ["alpha", "beta"], "aucune dégradation sous le plafond");
    }

    #[test]
    fn fit_degrades_until_under_cap() {
        // 11 caractères de JSON pour 4 caractères de texte : `"` -> `\"`, `\` -> `\\`, U+0001 ->
        // `\u0001`, et un emoji hors BMP, qui reste un seul caractère.
        let unit = "\"\\\u{1}\u{1F600}";
        let hit = unit.repeat(100);
        assert_eq!(hit.chars().count(), 400);
        let mut view = View { hits: vec![hit; 45] };
        // Les 45 textes bruts tiennent largement (18 000 caractères) : seuls les échappements
        // font déborder la réponse.
        assert!(json_chars(&view) > CAP);

        let size = fit_under_cap(&mut view, trim_hits);

        // 1 103 caractères par résultat, plus 10 de structure : 40 résultats tiennent, pas 41.
        assert_eq!(view.hits.len(), 40);
        assert_eq!(size, 44_130);
        assert_eq!(size, json_chars(&view));
        assert!(size <= CAP);
    }

    #[test]
    fn fit_measures_characters_not_bytes() {
        // 12 000 emoji : 48 000 octets UTF-8 mais 12 000 caractères. Sous le plafond.
        let mut view = View { hits: vec!["\u{1F600}".repeat(12_000), "\u{1F600}".repeat(12_000)] };
        assert!(serde_json::to_string(&view).unwrap().len() > CAP);
        let size = fit_under_cap(&mut view, trim_hits);
        assert_eq!(size, 24_000 + r#"{"hits":["",""]}"#.len());
        assert_eq!(view.hits.len(), 2, "mesurée en octets, la vue aurait été dégradée");
    }

    #[test]
    fn fit_stops_when_degrade_gives_up() {
        let mut view = View { hits: vec!["x".repeat(CAP)] };
        let size = fit_under_cap(&mut view, trim_hits);
        assert_eq!(size, json_chars(&view));
        assert!(size > CAP);
        assert_eq!(view.hits.len(), 1, "une dégradation refusée ne touche pas la vue");
    }

    #[test]
    fn fit_gives_up_over_the_cap_after_some_progress() {
        // Deux résultats énormes : le premier tour retire le second, le suivant n'a plus rien à
        // retirer alors que le reste dépasse encore le plafond.
        let mut view = View { hits: vec!["x".repeat(CAP), "y".repeat(10)] };
        let size = fit_under_cap(&mut view, trim_hits);
        assert_eq!(view.hits, ["x".repeat(CAP)]);
        assert_eq!(size, json_chars(&view));
        assert!(size > CAP);
    }

    #[test]
    fn fit_keeps_degrading_until_the_view_fits() {
        // Un seul résultat énorme, que `degrade` raccourcit de moitié à chaque appel.
        let mut view = View { hits: vec!["y".repeat(3 * CAP)] };
        let size = fit_under_cap(&mut view, |view| {
            let hit = &mut view.hits[0];
            hit.truncate(hit.len() / 2);
            true
        });
        assert!(size <= CAP);
        assert_eq!(size, json_chars(&view));
        assert_eq!(view.hits[0].len(), 3 * CAP / 4, "deux moitiés suffisent, pas une de plus");
    }

    /// Vue dont la sérialisation échoue toujours.
    struct Unserializable;

    impl Serialize for Unserializable {
        fn serialize<S: Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("not serializable"))
        }
    }

    #[test]
    fn an_unserializable_view_counts_as_over_the_cap() {
        let mut calls = 0;
        let size = fit_under_cap(&mut Unserializable, |_| {
            calls += 1;
            calls < 3
        });
        assert_eq!(size, usize::MAX);
        assert_eq!(calls, 3, "elle est dégradée tant que `degrade` progresse");
    }
}
