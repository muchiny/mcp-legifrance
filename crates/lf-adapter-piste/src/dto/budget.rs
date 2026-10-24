//! Budget d'éléments par document (04 §8.5, limite 1 (a)).
//!
//! Un corps de 16 Mio peut se dérouler en des millions d'éléments de listes : chaque élément
//! lu par un DTO à listes se paie sur un compteur du thread, armé par `decode` avant la lecture
//! et rendu à son état précédent par une garde à sa chute, même sur erreur ou panique. `decode`
//! est synchrone : aucun `await` ne sépare l'armement de la chute de la garde, donc aucune autre
//! tâche ne peut lire ce compteur entre-temps sur le même thread. La garde n'est pas `Send` : une
//! tâche qui la tiendrait à travers un `await` ne pourrait pas être lancée sur un runtime
//! multi-thread, donc ne changerait pas de thread en la gardant.
//!
//! Hors armement (`serde_json::from_str` direct sur un DTO à listes), toute charge est
//! refusée : un DTO dont les listes passent par `bounded_list` ne peut pas être lu sans budget
//! (le test `every_dto_list_is_bounded` de `dto/mod.rs` vérifie que toutes y passent).

use std::cell::Cell;
use std::marker::PhantomData;

/// Nombre maximal d'éléments de liste lus dans un document.
pub(crate) const MAX_ELEMENTS: u32 = 200_000;

/// Texte de l'erreur quand le budget d'un document est épuisé.
pub(crate) const TOO_MANY: &str = "too many elements";

/// Texte de l'erreur quand on lit sans que `decode` ait armé le budget.
pub(crate) const NOT_ARMED: &str = "decoding budget not armed";

thread_local! {
    /// Éléments encore permis pour le document en cours, `None` hors armement.
    static REMAINING: Cell<Option<u32>> = const { Cell::new(None) };
}

/// Garde du budget : rend au thread son état d'armement précédent quand elle tombe, y compris
/// pendant une panique.
///
/// Pas `Send` (`PhantomData<*const ()>`, figé par `the_guard_is_not_send`) : elle reste sur le
/// thread dont elle arme le compteur.
#[must_use = "le budget est rendu à son état précédent dès que la garde tombe"]
pub(crate) struct Armed {
    /// État du compteur avant cet armement : `None` si le thread n'était pas armé.
    previous: Option<u32>,
    /// Retire `Send` et `Sync` à la garde.
    _not_send: PhantomData<*const ()>,
}

impl Drop for Armed {
    /// Désarmé avant : désarmé de nouveau. Armé avant : armé avec le reste courant, qui garde les
    /// éléments payés sous cette garde (aucun remboursement) ; si une garde extérieure est déjà
    /// tombée et a désarmé le thread, il le reste.
    fn drop(&mut self) {
        REMAINING.set(self.previous.and(REMAINING.get()));
    }
}

/// Arme le budget du thread et rend la garde qui rétablit son état précédent.
///
/// Un premier armement accorde [`MAX_ELEMENTS`] éléments ; un armement imbriqué garde le reste
/// courant ([`armed_budget`]) : il ne remonte jamais le budget du document en cours.
pub(crate) fn arm() -> Armed {
    let previous = REMAINING.get();
    REMAINING.set(Some(armed_budget(previous)));
    Armed { previous, _not_send: PhantomData }
}

/// Éléments accordés par un armement : [`MAX_ELEMENTS`] sur un thread désarmé, sinon le reste
/// courant, jamais plus de [`MAX_ELEMENTS`].
fn armed_budget(previous: Option<u32>) -> u32 {
    previous.map_or(MAX_ELEMENTS, |left| left.min(MAX_ELEMENTS))
}

/// Raison d'un refus de charge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    /// Les [`MAX_ELEMENTS`] éléments du document sont consommés.
    Exhausted,
    /// Le budget n'a pas été armé par `decode`.
    NotArmed,
}

impl Refusal {
    /// Texte fixe de l'erreur : ni guillemet ni accent grave, `redact` le garde tel quel.
    fn text(self) -> &'static str {
        match self {
            Self::Exhausted => TOO_MANY,
            Self::NotArmed => NOT_ARMED,
        }
    }
}

/// Décompte un élément du budget `remaining` : le reste, ou la raison du refus.
fn spend(remaining: Option<u32>) -> Result<u32, Refusal> {
    match remaining {
        None => Err(Refusal::NotArmed),
        Some(0) => Err(Refusal::Exhausted),
        Some(left) => Ok(left - 1),
    }
}

/// Paie un élément de liste sur le budget du document en cours.
///
/// # Errors
///
/// Une erreur `serde` de texte fixe ([`TOO_MANY`]) quand le budget est épuisé, ou
/// ([`NOT_ARMED`]) quand `decode` ne l'a pas armé ; le compteur n'est alors pas modifié.
pub(crate) fn charge<E: serde::de::Error>() -> Result<(), E> {
    match spend(REMAINING.get()) {
        Ok(left) => {
            REMAINING.set(Some(left));
            Ok(())
        }
        Err(refusal) => Err(E::custom(refusal.text())),
    }
}

/// Éléments restants du budget, pour les tests.
#[cfg(test)]
pub(crate) fn remaining() -> Option<u32> {
    REMAINING.get()
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
    use std::thread;

    use super::*;

    fn charge_one() -> Result<(), serde_json::Error> {
        charge::<serde_json::Error>()
    }

    #[test]
    fn spend_decrements_and_refuses_at_zero() {
        assert_eq!(spend(Some(2)), Ok(1));
        assert_eq!(spend(Some(1)), Ok(0));
        assert_eq!(spend(Some(0)), Err(Refusal::Exhausted));
        assert_eq!(spend(None), Err(Refusal::NotArmed));
    }

    #[test]
    fn refusal_texts_are_fixed() {
        assert_eq!(Refusal::Exhausted.text(), "too many elements");
        assert_eq!(Refusal::NotArmed.text(), "decoding budget not armed");
        assert_eq!(TOO_MANY, "too many elements");
        assert_eq!(NOT_ARMED, "decoding budget not armed");
    }

    #[test]
    fn max_elements_is_200_000() {
        assert_eq!(MAX_ELEMENTS, 200_000);
    }

    #[test]
    fn a_fresh_arm_grants_exactly_max_elements() {
        let _armed = arm();
        for _ in 0..MAX_ELEMENTS {
            charge_one().unwrap();
        }
        assert_eq!(remaining(), Some(0));
        assert_eq!(charge_one().unwrap_err().to_string(), "too many elements");
        assert_eq!(remaining(), Some(0));
    }

    #[test]
    fn charging_decrements_the_remaining_budget() {
        let _armed = arm();
        for _ in 0..3 {
            charge_one().unwrap();
        }
        assert_eq!(remaining(), Some(MAX_ELEMENTS - 3));
    }

    #[test]
    fn charging_without_a_guard_is_refused() {
        assert_eq!(remaining(), None);
        assert_eq!(charge_one().unwrap_err().to_string(), "decoding budget not armed");
        assert_eq!(remaining(), None);
    }

    #[test]
    fn the_guard_disarms_on_drop() {
        {
            let _armed = arm();
            assert_eq!(remaining(), Some(MAX_ELEMENTS));
        }
        assert_eq!(remaining(), None);
    }

    #[test]
    fn the_guard_disarms_on_unwind() {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let _armed = arm();
            resume_unwind(Box::new(()))
        }));
        assert!(outcome.is_err());
        assert_eq!(remaining(), None);
    }

    #[test]
    fn arming_again_restarts_the_count() {
        {
            let _armed = arm();
            for _ in 0..5 {
                charge_one().unwrap();
            }
            assert_eq!(remaining(), Some(MAX_ELEMENTS - 5));
        }
        let _armed = arm();
        assert_eq!(remaining(), Some(MAX_ELEMENTS));
    }

    #[test]
    fn nested_arming_never_raises_the_budget() {
        let outer = arm();
        for _ in 0..5 {
            charge_one().unwrap();
        }
        let inner = arm();
        assert_eq!(remaining(), Some(MAX_ELEMENTS - 5));
        charge_one().unwrap();
        assert_eq!(remaining(), Some(MAX_ELEMENTS - 6));
        drop(inner);
        // La garde externe reste armée, sans remboursement de l'élément payé sous l'interne.
        assert_eq!(remaining(), Some(MAX_ELEMENTS - 6));
        drop(outer);
        assert_eq!(remaining(), None);
    }

    #[test]
    fn an_exhausted_budget_stays_exhausted_under_a_nested_guard() {
        let _outer = arm();
        REMAINING.set(Some(0));
        let _inner = arm();
        assert_eq!(remaining(), Some(0));
        assert_eq!(charge_one().unwrap_err().to_string(), "too many elements");
    }

    #[test]
    fn dropping_the_inner_guard_restores_the_outer_state() {
        let outer = arm();
        drop(arm());
        assert_eq!(remaining(), Some(MAX_ELEMENTS));
        charge_one().unwrap();
        assert_eq!(remaining(), Some(MAX_ELEMENTS - 1));
        drop(outer);
        assert_eq!(remaining(), None);
        assert_eq!(charge_one().unwrap_err().to_string(), "decoding budget not armed");
    }

    #[test]
    fn armed_budget_never_exceeds_the_previous_one() {
        assert_eq!(armed_budget(None), MAX_ELEMENTS);
        assert_eq!(armed_budget(Some(7)), 7);
        assert_eq!(armed_budget(Some(0)), 0);
        assert_eq!(armed_budget(Some(u32::MAX)), MAX_ELEMENTS);
    }

    /// `<T as NotSend<_>>::check` est ambigu dès que `T: Send` : les deux impls conviennent, et le
    /// compilateur refuse l'appel (technique de `static_assertions::assert_not_impl_any!`, sans la
    /// crate).
    trait NotSend<Marker> {
        fn check() {}
    }

    impl<T: ?Sized> NotSend<()> for T {}

    impl<T: ?Sized + Send> NotSend<u8> for T {}

    #[test]
    fn the_guard_is_not_send() {
        // Ne compile plus si `Armed` devient `Send`.
        <Armed as NotSend<_>>::check();
    }

    #[test]
    fn guards_dropped_out_of_order_leave_the_budget_disarmed() {
        let outer = arm();
        let inner = arm();
        drop(outer);
        assert_eq!(remaining(), None);
        drop(inner);
        assert_eq!(remaining(), None);
    }

    #[test]
    fn each_thread_has_its_own_budget() {
        let _armed = arm();
        assert!(thread::spawn(|| charge::<serde_json::Error>().is_err()).join().unwrap());
        assert_eq!(remaining(), Some(MAX_ELEMENTS));
    }
}
