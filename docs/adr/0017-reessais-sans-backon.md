# ADR-0017 — Réessais sans `backon` : boucle maison

- **Statut** : **Proposé** — accepté à la fusion de la PR du plan 2c.
- **Date** : 2026-10-22
- **Jalons concernés** : M2 (plan 2c, T-M2-04)
- **Questions liées** : —
- Amende [00-decisions.md](../plan/00-decisions.md) §8 (bibliothèque des réessais) et §2 (tableau des crates de `lf-adapter-piste`) : la politique de réessai de 00 §8 reste inchangée, seule la mise en œuvre change.

## Contexte

- **Décision de départ.** 00 §8 retenait `backon` 1.6 pour les réessais de l'adapter PISTE : 3 tentatives au plus sur 429, 502, 503, 504 et erreurs réseau, 2 sur 500, backoff exponentiel avec gigue, `Retry-After`, le tout plafonné par le délai global de l'outil. 04 §5.2 prévoyait déjà un repli, un itérateur `Backoff` maison, si l'API de `backon` ne convenait pas.
- **Spike V-06** (plan 2c, tâche 9, Step 3 ; source de `backon` 1.6.0 relu, rien ajouté au dépôt). L'API est conforme à l'esquisse : `ExponentialBuilder`, `Retry::when`, `adjust`, `notify` et `sleep` existent, et la gigue **s'ajoute** (`délai + délai × f32 ∈ [0, 1)`), sans jamais retrancher. La politique tiendrait donc avec `backon`. Mais cinq coûts, dont aucun n'est rédhibitoire seul :
  1. la gigue n'est injectable que par `with_jitter_seed(u64)` : les bornes de la gigue ne se testent pas de façon déterministe ;
  2. son `Sleeper` est `'static` : il faudrait un `Arc` du `Sleeper` du crate dans une fermeture `move` ;
  3. le budget propre au 500 (deux requêtes au plus) passerait par un prédicat `when` à effet de bord, lié à l'ordre d'appel interne de la bibliothèque ;
  4. l'erreur doit être le `E` d'un `Result` (donc `Outcome` deviendrait un `Result<_, Transient>`), alors que le renouvellement du jeton sur 401 et le permis de débit restent dans la fermeture de tentative ;
  5. c'est une dépendance de plus à âge-vérifier (00 §2).
- **Nuance sur l'`unsafe`.** `backon` contient de l'`unsafe` (projection de pin de `Retry::poll` : `get_unchecked_mut` et `Pin::new_unchecked`, trois occurrences). Son aléa vient de `fastrand` 2.5.0 (déjà verrouillé), qui est `#![forbid(unsafe_code)]` et n'en contient pas : l'`unsafe` est dans `backon`, pas dans `fastrand`. Le projet interdit `unsafe_code` dans ses propres crates (`forbid`) ; une dépendance qui en contient reste admise, mais ce n'est pas un argument pour en ajouter une.
- **Décision de cadrage du 2026-10-21** (propriétaire) : abandonner `backon`, conserver le ban `backoff` (non maintenu).

## Décision

1. **Boucle maison `retry::run`** (`crates/lf-adapter-piste/src/retry.rs`, ~25 lignes) : borne dure de 3 tentatives (une boucle `for`, même mutée), fermeture de tentative qui rend un `Outcome` (`Success`, `Unauthorized`, `Transient { error, retry_after, budget }`, `Fatal`), `RetryContext { clock, sleeper, deadline, jitter }`, délai exponentiel `backoff` dont la gigue passe par le trait `Jitter` (`SystemJitter` en production : `RandomState::new().build_hasher().finish()` passé à `scale`, calcul entier sur 128 bits, strictement inférieur à la borne ; une source déterministe en test).
2. **Politique de 00 §8 inchangée** : 3 tentatives au plus ; 2 sur un 500, puis `Upstream { status: 500, retryable: false }` ; attente avant la 2e tentative = max(500 ms + gigue dans [0, 500 ms), `Retry-After`), avant la 3e = max(1 s + gigue dans [0, 1 s), `Retry-After`) ; `Retry-After` > 30 s : **pas d'attente**, échec immédiat avec l'erreur classée (`RateLimited { retry_after: Some(valeur) }` pour un 429), exactement 30 s est attendu ; plafonné par le délai global.
3. **Ban `backoff` conservé** dans `deny.toml` (raison : « non maintenu ; réessais maison (ADR-0017) »). **`backon` reste interdit à `lf-application`** (R2, liste fermée) et reste dans la table des crates confinés de R6 comme **porte inerte** : `xtask/src/arch.rs` le rattache à `lf-adapter-piste`, qui ne l'emploie pas ; la table décrit la porte, pas l'usage.
4. Toute attente passe par le `Sleeper` du crate et tout instant par sa `MonoClock` (aucune lecture d'horloge murale) ; `chrono` ne sert qu'à lire les dates HTTP de `Retry-After` et de `Date`.

## Conséquences

### Positives

- **Aucune dépendance de plus** : rien à âge-vérifier, rien à confiner, pas de nouvelle surface `unsafe`.
- **Bornes de gigue testables** de façon déterministe (le trait `Jitter`), budget du 500 écrit en clair dans la boucle plutôt que dans un prédicat à effet de bord, attentes bornées par le délai global sans couche supplémentaire.

### Négatives

- **~25 lignes à maintenir** (plus le calcul de délai et l'analyse de `Retry-After`), couvertes à 100 % et à 0 mutant survivant.
- Pas de bénéfice des correctifs futurs de `backon` ; la politique est figée par 00 §8.

### À surveiller

- **NON VÉRIFIÉ** [V-63 · retry-after-piste] : présence et format réels de `Retry-After` et de `Date` chez PISTE ; V-69 (500 transitoires) peut ramener le budget du 500 à 3 tentatives, une constante à changer.
- Si la politique se complique (réessais par endpoint, budget global partagé), réexaminer une bibliothèque : ce serait un nouvel ADR.

### Tests qui la gardent

`backoff_adds_a_jitter_below_the_base_and_never_subtracts`, `a_second_500_stops_the_loop_after_two_requests`, `retry_after_beyond_thirty_seconds_fails_fast_even_with_time_to_spare`, `the_503_three_times_maps_to_upstream_retryable` (`retry.rs`) ; `check-arch` R2 (`backon` interdit à `lf-application`) et le ban `backoff` de `cargo deny`. Documents amendés : 00 §2 et §8 ; 02 §3.0.1, §3.0.2, §3.3, §4.3 ; 04 §1.4, §5.2 ; `deny.toml`.

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| `backon` 1.6.0 | API conforme mais gigue non injectable sauf par graine, `Sleeper` `'static`, budget du 500 par prédicat à effet de bord, dépendance de plus (spike V-06, ci-dessus). |
| `backoff` | Non maintenu ; banni par `deny.toml`. |
| Réessai de reqwest (`retry`) | Seuls les NACK de protocole sûrs sont rejoués : il ne porte pas la politique applicative (04 §4.2). |

## Références et sources

- Plan : [00-decisions.md](../plan/00-decisions.md) §2, §8 ; [04-adapter-piste.md](../plan/04-adapter-piste.md) §5.2 (mise en œuvre, repli prévu) ; [02-architecture.md](../plan/02-architecture.md) §3.0.1, §4.1 (R2, R6) ; [09-risques-et-questions.md](../plan/09-risques-et-questions.md) §2.20 (V-06) ; [06-qualite.md](../plan/06-qualite.md) §13.2 (gabarit).
- Code : `crates/lf-adapter-piste/src/retry.rs` ; `backon-1.6.0/src/{retry.rs,sleep.rs,backoff/exponential.rs}` (index crates.io).
- Plan d'implémentation : `docs/superpowers/plans/2026-10-21-m2c-adapter-piste.md` (tâche 9).
