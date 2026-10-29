//! Suite de contrat du port `LegalCorpus` sur `CachedCorpus` (T-M3-05, 06 §2.4) : le décorateur de
//! cache se comporte comme le corpus qu'il décore, sur les douze opérations.
//!
//! La règle R8 interdit à ce crate de dépendre de `lf-adapter-piste`, même en développement : le
//! corpus décoré est un `FakeCorpus` programmé par `reference_fake`. Il ne reçoit **qu'une** copie
//! de chaque réponse alors que la suite appelle deux fois chaque cas présent : la seconde lecture
//! doit venir du cache, sans quoi le faux corpus répondrait « aucune réponse programmée » et la
//! suite échouerait. Les cas absents (erreurs `NotFound` ou `Upstream`, listes vides) ne sont
//! jamais mis en cache, mais la suite ne les appelle qu'une fois.
//!
//! Tout le fichier est sous `cfg(test)` : les aides partagent les règles de lint des tests
//! (`unwrap` permis, `clippy.toml`).

#[cfg(test)]
mod contract {
    use lf_adapter_cache::{CachePolicy, CachedCorpus};
    use lf_application::contract::{
        ContractFixtures, corpus_contract, corpus_contract_m6_m7, reference_fake,
    };
    use lf_application::ports::PisteEnvironment;

    /// Les douze opérations derrière le cache : chaque lecture répétée vient du cache, y compris
    /// celles de `find_article`, qui a une clé par variante d'`ArticleAt`.
    #[tokio::test]
    async fn contract_on_cached_corpus() {
        let fx = ContractFixtures::reference();
        let corpus = CachedCorpus::new(
            reference_fake(&fx, 1),
            PisteEnvironment::Sandbox,
            CachePolicy::default(),
        );
        corpus_contract(&corpus, &fx).await;
    }

    /// Témoin : sans cache, la même programmation (une copie par réponse) ne suffit pas à la suite.
    /// Le test précédent prouve donc bien que la seconde lecture vient du cache.
    #[tokio::test]
    #[should_panic(expected = "is deterministic")]
    async fn one_copy_per_answer_is_not_enough_without_the_cache() {
        let fx = ContractFixtures::reference();
        corpus_contract(&reference_fake(&fx, 1), &fx).await;
    }

    /// Témoin pour M6 et M7 : sans cache, la seconde lecture de la première opération (la
    /// recherche d'un article par son numéro) trouve la file vide.
    #[tokio::test]
    #[should_panic(expected = "find_article")]
    async fn one_copy_per_answer_is_not_enough_for_m6_and_m7_without_the_cache() {
        let fx = ContractFixtures::reference();
        corpus_contract_m6_m7(&reference_fake(&fx, 1), &fx).await;
    }
}
