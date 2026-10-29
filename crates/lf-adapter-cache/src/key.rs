//! Clé de cache : environnement et opération typée (ADR-0007 §3, 02 §7.2).
//!
//! La clé est construite à partir des types du domaine, déjà normalisés (identifiants validés,
//! `VersionDate` au jour, `QueryText` sans espaces de bord), et **jamais** à partir du JSON envoyé
//! à PISTE : la `singleDate` de cette requête change à chaque appel (D-01), la `VersionDate` non.
//! Le texte de la requête n'est pas replié (ni casse ni accents) : la clé reflète exactement ce
//! qui part vers PISTE.

use lf_application::ports::ArticleAt;
use lf_domain::{
    coverage::PisteEnvironment,
    date::VersionDate,
    id::{AccoTextId, ArticleId, CnilTextId, DecisionId, KaliTextId, LegiTextId},
    search::{NumberQuery, SearchQuery},
};

/// Famille de durée de vie d'une opération (00 §9) : `CachePolicy` en donne la durée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TtlClass {
    /// Recherches : `Search` et `FindByNumber`.
    Search,
    /// Consultations d'un document ou d'un article.
    Consultation,
    /// Listes qui changent rarement : la liste des codes.
    List,
}

/// Opération du port, avec ses arguments normalisés. Une variante par méthode du port : en
/// ajouter une au port oblige à compléter ce type (02 §12, liste de contrôle « Port »).
///
/// `Eq + Hash` : les types du domaine le sont tous, et lisibles en diagnostic (pas de SHA-256).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Op {
    /// `search`.
    Search(SearchQuery),
    /// `get_act` : la date de version fait partie de la clé.
    Act(LegiTextId, VersionDate),
    /// `get_article`.
    Article(ArticleId),
    /// `get_decision`.
    Decision(DecisionId),
    /// `get_deliberation`.
    Deliberation(CnilTextId),
    /// `get_convention`.
    Convention(KaliTextId),
    /// `get_company_agreement`.
    Agreement(AccoTextId),
    /// `find_article` : le signal `Today` ou `On` et sa date sont dans la clé, donc la clé d'un
    /// article « du jour » change à minuit heure de Paris (02 §7.2, bascule de jour).
    FindArticle(LegiTextId, String, ArticleAt),
    /// `find_by_number`.
    FindByNumber(NumberQuery),
    /// `list_codes`.
    Codes,
    /// `article_versions`.
    Versions(ArticleId),
    /// `related_links`.
    Related(ArticleId),
}

impl Op {
    /// Nom de la méthode du port, celui des journaux (champ `op`). Jamais un argument : ni texte
    /// de requête ni identifiant dans un journal.
    pub(crate) const fn name(&self) -> &'static str {
        match self {
            Self::Search(_) => "search",
            Self::Act(..) => "get_act",
            Self::Article(_) => "get_article",
            Self::Decision(_) => "get_decision",
            Self::Deliberation(_) => "get_deliberation",
            Self::Convention(_) => "get_convention",
            Self::Agreement(_) => "get_company_agreement",
            Self::FindArticle(..) => "find_article",
            Self::FindByNumber(_) => "find_by_number",
            Self::Codes => "list_codes",
            Self::Versions(_) => "article_versions",
            Self::Related(_) => "related_links",
        }
    }

    /// Famille de durée de vie : recherches, consultations ou listes (00 §9).
    pub(crate) const fn ttl_class(&self) -> TtlClass {
        match self {
            Self::Search(_) | Self::FindByNumber(_) => TtlClass::Search,
            Self::Codes => TtlClass::List,
            Self::Act(..)
            | Self::Article(_)
            | Self::Decision(_)
            | Self::Deliberation(_)
            | Self::Convention(_)
            | Self::Agreement(_)
            | Self::FindArticle(..)
            | Self::Versions(_)
            | Self::Related(_) => TtlClass::Consultation,
        }
    }
}

/// Clé d'une entrée : l'environnement préfixe toujours l'opération, pour qu'une réponse du bac à
/// sable ne soit jamais resservie en production (D-15).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct CacheKey {
    /// Environnement PISTE interrogé.
    pub(crate) env: PisteEnvironment,
    /// Opération et arguments.
    pub(crate) op: Op,
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        hash::{BuildHasher, RandomState},
        thread::sleep,
        time::Duration,
    };

    use lf_domain::fond::Fond;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::fixtures::{article_id, day, every_op, search_query, text_id};

    fn key(env: PisteEnvironment, op: Op) -> CacheKey {
        CacheKey { env, op }
    }

    #[test]
    fn cache_key_is_stable_across_calls_same_day() {
        let today = day("2026-10-29");
        let search = |env| key(env, Op::Search(search_query(Fond::Loda, "bail", Some(today))));
        let first = search(PisteEnvironment::Sandbox);
        // Deux requêtes construites à 10 ms d'écart, avec la même date de version (ce que
        // l'application tire de `clock.today_paris()`) : même clé, même empreinte.
        sleep(Duration::from_millis(10));
        let second = search(PisteEnvironment::Sandbox);
        assert_eq!(first, second);
        let hasher = RandomState::new();
        assert_eq!(hasher.hash_one(&first), hasher.hash_one(&second));
    }

    #[test]
    fn cache_key_changes_when_paris_day_changes() {
        let (today, tomorrow) = (day("2026-10-29"), day("2026-10-30"));
        let search = |d| {
            key(PisteEnvironment::Sandbox, Op::Search(search_query(Fond::Code, "bail", Some(d))))
        };
        assert_ne!(search(today), search(tomorrow));
        let act = |d| key(PisteEnvironment::Sandbox, Op::Act(text_id(), d));
        assert_ne!(act(today), act(tomorrow));
    }

    #[test]
    fn article_at_today_key_changes_daily() {
        let (today, tomorrow) = (day("2026-10-29"), day("2026-10-30"));
        let find = |num: &str, at| {
            key(PisteEnvironment::Sandbox, Op::FindArticle(text_id(), num.into(), at))
        };
        // Le signal « aujourd'hui » porte sa date : la clé de l'article en vigueur change à
        // minuit heure de Paris, pas seulement à l'expiration du TTL.
        assert_ne!(find("9", ArticleAt::Today(today)), find("9", ArticleAt::Today(tomorrow)));
        assert_eq!(find("9", ArticleAt::Today(today)), find("9", ArticleAt::Today(today)));
        // Même date, signal différent (version en vigueur ou recherche exacte) : deux clés.
        assert_ne!(find("9", ArticleAt::Today(today)), find("9", ArticleAt::On(today)));
        // Le numéro fait partie de la clé.
        assert_ne!(find("9", ArticleAt::Today(today)), find("10", ArticleAt::Today(today)));
    }

    #[test]
    fn keys_differ_by_env() {
        let sandbox = key(PisteEnvironment::Sandbox, Op::Codes);
        let production = key(PisteEnvironment::Production, Op::Codes);
        assert_ne!(sandbox, production);
        let hasher = RandomState::new();
        assert_ne!(hasher.hash_one(&sandbox), hasher.hash_one(&production));
    }

    #[test]
    fn keys_differ_by_query_text_without_case_or_accent_folding() {
        let today = day("2026-10-29");
        let of = |text: &str| {
            key(PisteEnvironment::Sandbox, Op::Search(search_query(Fond::Loda, text, Some(today))))
        };
        assert_eq!(of("bail"), of("  bail  "), "QueryText normalise déjà les blancs");
        assert_ne!(of("bail"), of("Bail"));
        assert_ne!(of("grève"), of("greve"));
    }

    #[test]
    fn every_operation_has_its_own_key() {
        let keys: HashSet<CacheKey> =
            every_op().into_iter().map(|op| key(PisteEnvironment::Sandbox, op)).collect();
        assert_eq!(keys.len(), 12);
        // Un même identifiant sur trois opérations d'article : trois clés.
        let by_op =
            [Op::Article(article_id()), Op::Versions(article_id()), Op::Related(article_id())];
        let distinct: HashSet<Op> = by_op.into_iter().collect();
        assert_eq!(distinct.len(), 3);
    }

    #[test]
    fn op_names_follow_the_port_methods() {
        let names: Vec<&str> = every_op().iter().map(Op::name).collect();
        assert_eq!(
            names,
            [
                "search",
                "get_act",
                "get_article",
                "get_decision",
                "get_deliberation",
                "get_convention",
                "get_company_agreement",
                "find_article",
                "find_by_number",
                "list_codes",
                "article_versions",
                "related_links",
            ]
        );
    }

    #[test]
    fn ttl_classes_follow_00_section_9() {
        let classes: Vec<TtlClass> = every_op().iter().map(Op::ttl_class).collect();
        let (s, c, l) = (TtlClass::Search, TtlClass::Consultation, TtlClass::List);
        assert_eq!(classes, [s, c, c, c, c, c, c, c, s, l, c, c]);
    }
}
