//! Politique du cache : durées de vie par opération, capacité en octets et poids des entrées
//! (00 §9, ADR-0007 §4 et §7, 02 §7.2).
//!
//! `moka` n'utilise pas l'horloge de `tokio` : les durées de vie ne se testent pas sans attente
//! réelle (V-08, `moka-horloge-tests`). On teste donc directement la fonction [`Expiry`] qui les
//! calcule, et les tests de capacité s'appuient sur le poids, pas sur le temps.

use std::{
    mem::{size_of, size_of_val},
    time::{Duration, Instant},
};

use lf_domain::{
    entity::{
        Act, ArticleText, ArticleVersion, CodeListing, CompanyAgreement, Convention, Decision,
        Deliberation, RelatedLink, RelatedLinks, SearchHit, SearchPage, Snippet, TextBody, TocNode,
    },
    text::LongText,
};
use moka::Expiry;

use crate::key::{CacheKey, Op, TtlClass};

/// Capacité par défaut : 64 Mio de poids d'entrées (ADR-0007 §7).
const DEFAULT_CAPACITY_BYTES: u64 = 64 * 1024 * 1024;

/// Politique du cache : durée de vie par famille d'opérations et capacité en octets.
///
/// Les valeurs de 00 §9 sont des constantes en v0.1 : aucune variable d'environnement ne les
/// règle. Les champs ne sont donc pas publics ; seul `Default` construit la politique de
/// production, et les tests du crate en font varier un champ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachePolicy {
    /// Recherches (`search`, `find_by_number`) : 1 h.
    pub(crate) search_ttl: Duration,
    /// Consultations d'un document ou d'un article : 24 h.
    pub(crate) consultation_ttl: Duration,
    /// Listes (`list_codes`) : 7 jours.
    pub(crate) list_ttl: Duration,
    /// Poids total maximal des entrées, en octets (proposition de 64 Mio).
    pub(crate) capacity_bytes: u64,
}

impl Default for CachePolicy {
    fn default() -> Self {
        Self {
            search_ttl: Duration::from_secs(60 * 60),
            consultation_ttl: Duration::from_secs(24 * 60 * 60),
            list_ttl: Duration::from_secs(7 * 24 * 60 * 60),
            capacity_bytes: DEFAULT_CAPACITY_BYTES,
        }
    }
}

impl CachePolicy {
    /// Durée de vie d'une opération.
    pub(crate) const fn ttl(&self, op: &Op) -> Duration {
        match op.ttl_class() {
            TtlClass::Search => self.search_ttl,
            TtlClass::Consultation => self.consultation_ttl,
            TtlClass::List => self.list_ttl,
        }
    }
}

/// `Expiry` de moka : la durée de vie est fixée à la création de l'entrée, d'après son opération.
///
/// Ni une lecture ni une mise à jour ne la prolonge (les méthodes par défaut gardent l'échéance).
#[derive(Debug, Clone, Copy)]
pub(crate) struct OpExpiry {
    policy: CachePolicy,
}

impl OpExpiry {
    pub(crate) const fn new(policy: CachePolicy) -> Self {
        Self { policy }
    }
}

impl<V> Expiry<CacheKey, V> for OpExpiry {
    fn expire_after_create(
        &self,
        key: &CacheKey,
        _value: &V,
        _created_at: Instant,
    ) -> Option<Duration> {
        Some(self.policy.ttl(&key.op))
    }
}

/// Poids approximatif d'une valeur, en octets : sa taille en mémoire plus celle de ses chaînes et
/// de ses textes. Les identifiants (une vingtaine d'octets) ne sont pas comptés à part.
pub(crate) trait Weigh {
    /// Poids en octets.
    fn weight(&self) -> usize;
}

/// Le poids d'une entrée est un `u32` (`builder.rs:277` de moka) : on le sature au lieu de
/// déborder (02 §3.4, piège 4).
pub(crate) fn saturating_weight(bytes: usize) -> u32 {
    u32::try_from(bytes).unwrap_or(u32::MAX)
}

/// Octets d'une chaîne optionnelle.
fn text(value: Option<&String>) -> usize {
    value.map_or(0, String::len)
}

/// Octets d'une liste de chaînes, tableau compris.
fn texts(values: &[String]) -> usize {
    size_of_val(values) + values.iter().map(String::len).sum::<usize>()
}

/// Octets d'un texte long optionnel.
fn long(value: Option<&LongText>) -> usize {
    value.map_or(0, LongText::heap_bytes)
}

fn hit_heap(hit: &SearchHit) -> usize {
    let snippet = |s: &Option<Snippet>| s.as_ref().map_or(0, |s| s.as_str().len());
    let articles = size_of_val(hit.articles.as_slice())
        + hit.articles.iter().map(|a| text(a.num.as_ref()) + snippet(&a.snippet)).sum::<usize>();
    text(hit.title.as_ref())
        + text(hit.nature.as_ref())
        + text(hit.raison_sociale.as_ref())
        + snippet(&hit.snippet)
        + articles
}

fn hits_heap(hits: &[SearchHit]) -> usize {
    size_of_val(hits) + hits.iter().map(hit_heap).sum::<usize>()
}

fn toc_heap(nodes: &[TocNode]) -> usize {
    size_of_val(nodes)
        + nodes
            .iter()
            .map(|node| match node {
                TocNode::Section(section) => {
                    text(section.title.as_ref()) + toc_heap(&section.children)
                }
                TocNode::Article(article) => {
                    text(article.num.as_ref()) + long(article.inline_text.as_ref())
                }
            })
            .sum::<usize>()
}

fn body_heap(body: &TextBody) -> usize {
    text(body.title.as_ref())
        + text(body.nature.as_ref())
        + text(body.num_parution.as_ref())
        + text(body.jo_reference.as_ref())
        + texts(&body.aliases)
        + toc_heap(body.toc.roots())
}

fn links_heap(links: &[RelatedLink]) -> usize {
    size_of_val(links) + links.iter().map(|link| text(link.title.as_ref())).sum::<usize>()
}

impl Weigh for SearchPage {
    fn weight(&self) -> usize {
        size_of::<Self>() + self.query.len() + hits_heap(&self.hits)
    }
}

impl Weigh for Vec<SearchHit> {
    fn weight(&self) -> usize {
        size_of::<Self>() + hits_heap(self)
    }
}

impl Weigh for Act {
    fn weight(&self) -> usize {
        size_of::<Self>() + body_heap(&self.body)
    }
}

impl Weigh for Convention {
    fn weight(&self) -> usize {
        size_of::<Self>()
            + body_heap(&self.body)
            + text(self.container_title.as_ref())
            + text(self.container_id.as_ref())
            + text(self.bulletin.as_ref())
    }
}

impl Weigh for ArticleText {
    fn weight(&self) -> usize {
        size_of::<Self>()
            + text(self.num.as_ref())
            + text(self.code_title.as_ref())
            + texts(&self.path)
            + text(self.full_sections_titre.as_ref())
            + self.text.heap_bytes()
            + long(self.nota.as_ref())
    }
}

impl Weigh for Decision {
    fn weight(&self) -> usize {
        size_of::<Self>()
            + text(self.num.as_ref())
            + text(self.juridiction.as_ref())
            + text(self.formation.as_ref())
            + text(self.solution.as_ref())
            + texts(&self.numero_affaire)
            + text(self.nature.as_ref())
            + text(self.title.as_ref())
            + self.text.heap_bytes()
            + texts(&self.official_summary.analyses)
            + texts(&self.official_summary.titrages)
            + text(self.cited_case_law.as_ref())
            + texts(&self.applied_texts)
    }
}

impl Weigh for Deliberation {
    fn weight(&self) -> usize {
        size_of::<Self>()
            + text(self.num.as_ref())
            + text(self.nature.as_ref())
            + text(self.nature_delib.as_ref())
            + text(self.title.as_ref())
            + text(self.title_long.as_ref())
            + self.text.heap_bytes()
    }
}

impl Weigh for CompanyAgreement {
    fn weight(&self) -> usize {
        size_of::<Self>()
            + text(self.nature.as_ref())
            + text(self.title.as_ref())
            + text(self.raison_sociale.as_ref())
            + text(self.siret.as_ref())
            + text(self.code_ape.as_ref())
            + text(self.secteur.as_ref())
            + texts(&self.themes)
            + texts(&self.syndicats)
    }
}

impl Weigh for Vec<CodeListing> {
    fn weight(&self) -> usize {
        size_of::<Self>()
            + size_of_val(self.as_slice())
            + self.iter().map(|code| code.title.len()).sum::<usize>()
    }
}

impl Weigh for Vec<ArticleVersion> {
    fn weight(&self) -> usize {
        size_of::<Self>()
            + size_of_val(self.as_slice())
            + self.iter().map(|version| text(version.num.as_ref())).sum::<usize>()
    }
}

impl Weigh for RelatedLinks {
    fn weight(&self) -> usize {
        size_of::<Self>() + links_heap(&self.cites) + links_heap(&self.cited_by)
    }
}

impl Weigh for CacheKey {
    fn weight(&self) -> usize {
        let argument = match &self.op {
            Op::Search(query) => query.text().as_str().len(),
            Op::FindArticle(_, num, _) => num.len(),
            _ => 0,
        };
        size_of::<Self>() + argument
    }
}

#[cfg(test)]
mod tests {
    use lf_domain::{coverage::PisteEnvironment, fond::Fond};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::fixtures::{
        act, agreement, article, codes, convention, decision, deliberation, every_op, hit, page,
        related, search_query, versions,
    };

    const HOUR: Duration = Duration::from_secs(3_600);
    const DAY: Duration = Duration::from_secs(24 * 3_600);
    const WEEK: Duration = Duration::from_secs(7 * 24 * 3_600);

    #[test]
    fn default_policy_follows_00_section_9() {
        let policy = CachePolicy::default();
        assert_eq!(
            (policy.search_ttl, policy.consultation_ttl, policy.list_ttl, policy.capacity_bytes),
            (HOUR, DAY, WEEK, 64 * 1024 * 1024)
        );
    }

    #[test]
    fn ttl_policy_per_operation() {
        // V-08 : on teste la fonction `Expiry`, pas l'horloge de moka.
        let expiry = OpExpiry::new(CachePolicy::default());
        let ttls: Vec<Option<Duration>> = every_op()
            .into_iter()
            .map(|op| {
                let key = CacheKey { env: PisteEnvironment::Sandbox, op };
                expiry.expire_after_create(&key, &(), Instant::now())
            })
            .collect();
        // search, act, article, decision, deliberation, convention, agreement, find_article,
        // find_by_number, codes, versions, related.
        let expected = [HOUR, DAY, DAY, DAY, DAY, DAY, DAY, DAY, HOUR, WEEK, DAY, DAY];
        assert_eq!(ttls, expected.map(Some));
    }

    #[test]
    fn m6_m7_ops_have_their_ttl() {
        let policy = CachePolicy::default();
        let ops = every_op();
        let ttl = |name: &str| {
            let op = ops.iter().find(|op| op.name() == name).unwrap();
            policy.ttl(op)
        };
        // M6 : la liste des codes vit 7 jours ; la recherche par numéro 1 h ; l'article par numéro
        // est une consultation (24 h).
        assert_eq!(ttl("list_codes"), WEEK);
        assert_eq!(ttl("find_by_number"), HOUR);
        assert_eq!(ttl("find_article"), DAY);
        // M7 : versions et liens d'un article sont des consultations.
        assert_eq!(ttl("article_versions"), DAY);
        assert_eq!(ttl("related_links"), DAY);
    }

    #[test]
    fn custom_policy_changes_each_family_independently() {
        let policy = CachePolicy {
            search_ttl: Duration::from_secs(1),
            consultation_ttl: Duration::from_secs(2),
            list_ttl: Duration::from_secs(3),
            ..CachePolicy::default()
        };
        let secs: Vec<u64> = every_op().iter().map(|op| policy.ttl(op).as_secs()).collect();
        assert_eq!(secs, [1, 2, 2, 2, 2, 2, 2, 2, 1, 3, 2, 2]);
    }

    #[test]
    fn weight_saturates_u32() {
        assert_eq!(saturating_weight(0), 0);
        assert_eq!(saturating_weight(1_234), 1_234);
        assert_eq!(saturating_weight(usize::try_from(u32::MAX).unwrap()), u32::MAX);
        assert_eq!(saturating_weight(usize::MAX), u32::MAX);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(saturating_weight(usize::try_from(u64::from(u32::MAX) + 1).unwrap()), u32::MAX);
    }

    /// Octets ajoutés par les champs textuels : passer de 1 à 1 001 octets par champ ajoute 1 000
    /// octets par champ, ni plus ni moins (aucun champ oublié ni compté deux fois).
    fn grows_by(fields: usize, weight: impl Fn(&str) -> usize) {
        let small = "x".to_owned();
        let large = "x".repeat(1_001);
        assert_eq!(weight(&large) - weight(&small), fields * 1_000);
    }

    #[test]
    fn weight_counts_every_text_field_once() {
        grows_by(8, |f| vec![hit(f), hit(f)].weight());
        grows_by(9, |f| page(f, 2).weight());
        grows_by(7, |f| article(f).weight());
        grows_by(10, |f| act(f).weight());
        grows_by(13, |f| convention(f).weight());
        grows_by(16, |f| decision(f).weight());
        grows_by(6, |f| deliberation(f).weight());
        grows_by(10, |f| agreement(f).weight());
        grows_by(3, |f| codes(f, 3).weight());
        grows_by(2, |f| versions(f, 2).weight());
        grows_by(3, |f| related(f, 1, 2).weight());
    }

    #[test]
    fn weight_includes_the_fixed_part_and_the_snippets() {
        // Un résultat vide pèse sa taille en mémoire ; l'extrait (taille fixe) s'y ajoute.
        let empty = page("x", 0);
        assert_eq!(empty.weight(), size_of::<SearchPage>() + 1);
        assert_eq!(related("x", 0, 0).weight(), size_of::<RelatedLinks>());
        assert_eq!(codes("x", 0).weight(), size_of::<Vec<CodeListing>>());
        let one = page("x", 1);
        let hit = &one.hits[0];
        assert!(one.weight() > empty.weight() + size_of::<SearchHit>());
        assert_eq!(
            hit_heap(hit),
            4 + "un extrait de taille fixe".len()
                + "extrait d'article".len()
                + size_of::<lf_domain::entity::CodeArticleRef>()
        );
    }

    #[test]
    fn key_weight_counts_the_query_text_and_the_article_number() {
        let base = size_of::<CacheKey>();
        let weights: Vec<usize> = every_op()
            .into_iter()
            .map(|op| CacheKey { env: PisteEnvironment::Sandbox, op }.weight() - base)
            .collect();
        // search « bail » (4) et find_article « 9 » (1) ; les autres opérations n'ont que la clé.
        assert_eq!(weights, [4, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
        let long = CacheKey {
            env: PisteEnvironment::Production,
            op: Op::Search(search_query(Fond::Juri, "droit de grève", None)),
        };
        // Le poids est en octets : « è » en compte deux.
        assert_eq!(long.weight(), base + "droit de grève".len());
    }
}
