//! Cas d'usage de `fr_get_related` (v0.3 (M7)) : les liens d'un article ou d'une décision
//! (`GetRelatedUseCase`, 05 §5.11, annexe B §7.11).
//!
//! # Étapes
//!
//! 1. Validation sans réseau : `id` requis, puis un `LEGIARTI` (article) ou un `JURITEXT`,
//!    `CONSTEXT`, `CETATEXT` (décision). Les autres familles d'identifiants valides, `KALIARTI`
//!    compris (V-136, **NON VÉRIFIÉ**), sont refusées en `invalid_arg` avec l'outil à employer.
//! 2. **Article** : `get_article` (la citation et la page de l'article) et `related_links` sont
//!    demandés ensemble. Un article inconnu rend le `not_found` de `fr_get_text`, et c'est
//!    l'erreur de la lecture qui l'emporte sur celle des liens.
//! 3. **Décision** : `get_decision`, puis `cited_case_law` (`citationJp`) et `applied_texts` (les
//!    titres des liens de citation) de la décision passent par le parseur (`decision_links`). Un
//!    seul appel au port : les liens n'ont pas d'identifiant et ne sont pas résolus. Une décision
//!    sans références rend `links: []`, mais cette liste vide ne prouve pas que la décision ne cite
//!    rien : pour les fonds CETAT et CONSTIT, les captures `consult_juri_cetat.json` et
//!    `consult_juri_constit.json` n'ont ni `citationJp` ni `liens` (vides ou nuls), alors que
//!    leur texte intégral fait plusieurs milliers de caractères. La réponse porte alors la `note`
//!    [`Note::NoDecisionReferences`] (« l'API ne donne aucune référence pour cette décision »),
//!    et seulement alors : une décision dont les références existent mais ne contiennent aucune
//!    citation lisible n'a pas de note. Un code qui n'était pas écrit à côté de son article n'est pas
//!    repris (« articles 1382 et 700 du code civil » donne « article 700 »), et la `note`
//!    [`Note::DecisionLinkCodes`] dit que les codes des liens d'une décision peuvent être inexacts
//!    dès qu'un lien en porte un, gardé ou refusé (V-117, L4-13). Les références analysées sont bornées à
//!    `MAX_ANALYSED_CHARS` caractères : au-delà, elles sont ignorées et `truncated` le dit.
//!
//! # Plafond de réponse (05 §5.11, 05 §6.4)
//!
//! Au plus `MAX_LINKS` liens, sans suite : `total` compte les liens avant troncature et
//! `truncated` le dit. Le paramètre facultatif `relation` (D-05) écarte les liens des autres sens
//! **avant** la troncature : `total` compte alors la relation demandée, et sans `relation` la
//! réponse est celle de la 1.0.1. L'estimation de la réponse ([`estimate_list_chars`], 24 par lien et par champ) reste sous la cible, en
//! retirant des liens par la fin ([`shrink_from_end`]) ; la vue mesure ensuite la taille exacte.

use futures::future::join;
use lf_domain::{
    contract::{Citable, CitationContract},
    id::{ArticleId, DecisionId, DocumentId, LegiArtiId},
};

use crate::{
    budget::{ResponseBudget, escaped_chars, estimate_chars, estimate_list_chars, shrink_from_end},
    citation::{
        Note,
        related::{DecisionLinks, DecisionReferences, MAX_LINKS, article_links, decision_links},
    },
    error::AppError,
    failure::port_failure,
    input::GetRelatedInput,
    messages,
    output::get_related::{GetRelatedOutput, LinkOut, Relation},
    ports::LegalCorpus,
    tools::ToolName,
};

/// Outil que ce cas d'usage sert : nom des messages et de l'audit.
const TOOL: ToolName = ToolName::FR_GET_RELATED;

/// Champs estimés de l'enveloppe : `id`, `citation`, `source_url`, `total`, `links`, `truncated`.
const ENVELOPE_FIELDS: usize = 6;

/// Les liens d'un article ou d'une décision : analyse de l'identifiant, un ou deux appels au port,
/// politique de réponse.
pub(crate) struct GetRelatedUseCase<'a, C: LegalCorpus> {
    corpus: &'a C,
}

impl<'a, C: LegalCorpus> GetRelatedUseCase<'a, C> {
    /// Cas d'usage qui lit `corpus`.
    pub(crate) fn new(corpus: &'a C) -> Self {
        Self { corpus }
    }

    /// Valide `input`, lit la page demandée et ses liens.
    ///
    /// # Errors
    ///
    /// `invalid_arg` si `id` est absent, mal formé ou d'une famille que l'outil ne lit pas, ou si
    /// `relation` n'est pas l'un des quatre sens (sans appel au port) ; `not_found` si le corpus ne connaît pas l'article ou la décision (message
    /// de l'outil de lecture correspondant, 05 §7.4) ; sinon l'erreur du port, traduite par
    /// [`port_failure`].
    pub(crate) async fn execute(
        &self,
        input: GetRelatedInput,
    ) -> Result<GetRelatedOutput, AppError> {
        let raw = input.id.ok_or_else(|| messages::required(messages::id_field(TOOL)))?;
        let parsed = DocumentId::parse(&raw).map_err(|error| {
            // Un préfixe inconnu : la phrase de l'outil, non la liste des dix familles du domaine
            // dont six sont refusées juste après (L4-10). Un préfixe connu aux chiffres faux garde
            // le message du domaine.
            messages::unknown_id_family(&error).map_or_else(
                || messages::domain(TOOL, &error),
                |shown| messages::invalid_argument(wanted(&shown, "")),
            )
        })?;
        let relation = relation_of(input.relation.as_deref())?;
        match parsed {
            DocumentId::LegiArti(id) => self.article(id, relation).await,
            DocumentId::JuriText(id) => self.decision(DecisionId::Juri(id), relation).await,
            DocumentId::ConsText(id) => self.decision(DecisionId::Constit(id), relation).await,
            DocumentId::CetaText(id) => self.decision(DecisionId::Cetat(id), relation).await,
            other => Err(unsupported(&other)),
        }
    }

    /// Un article : sa citation et ses liens `cites` et `cited_by`, ceux de `relation` seuls si elle
    /// est donnée.
    async fn article(
        &self,
        id: LegiArtiId,
        relation: Option<Relation>,
    ) -> Result<GetRelatedOutput, AppError> {
        let id = ArticleId::Legi(id);
        let (article, links) =
            join(self.corpus.get_article(&id), self.corpus.related_links(&id)).await;
        let failure = |error| port_failure(ToolName::FR_GET_TEXT, id.as_str(), &error);
        let article = article.map_err(failure)?;
        let links = links.map_err(failure)?;
        Ok(output(id.as_str(), &article.contract(), article_links(&links), relation))
    }

    /// Une décision : sa citation et les liens que le parseur lit dans ses références, ceux de
    /// `relation` seuls si elle est donnée.
    async fn decision(
        &self,
        id: DecisionId,
        relation: Option<Relation>,
    ) -> Result<GetRelatedOutput, AppError> {
        let decision = self
            .corpus
            .get_decision(&id)
            .await
            .map_err(|error| port_failure(ToolName::FR_GET_DECISION, id.as_str(), &error))?;
        let references = DecisionReferences::of(&decision);
        let DecisionLinks { links, code_involved } = decision_links(&references);
        let found = output(id.as_str(), &decision.contract(), links, relation);
        // Des références que l'API ne donne pas du tout : lues sur la décision, non sur les
        // références analysées, que `MAX_ANALYSED_CHARS` peut avoir vidées.
        let unreferenced = decision.cited_case_law.is_none() && decision.applied_texts.is_empty();
        Ok(GetRelatedOutput {
            // Des références ignorées faute de place : la liste est incomplète.
            truncated: found.truncated || references.clipped,
            note: if unreferenced {
                Some(Note::NoDecisionReferences)
            } else {
                code_involved.then_some(Note::DecisionLinkCodes)
            },
            ..found
        })
    }
}

/// `relation` : absente, tous les liens ; sinon l'un des quatre sens, insensible à la casse et aux
/// blancs autour (D-05). Une autre valeur est un `invalid_arg` qui liste les quatre.
fn relation_of(raw: Option<&str>) -> Result<Option<Relation>, AppError> {
    raw.map(|raw| {
        Relation::parse(raw).ok_or_else(|| {
            messages::not_one_of("relation", raw, &Relation::ALL.map(Relation::as_str))
        })
    })
    .transpose()
}

/// La réponse de la page `id` (`contract` : sa citation et sa page) pour `links`, ramenée aux
/// bornes : cent liens au plus, estimation sous la cible. Quand `relation` est donnée, les autres
/// liens sont écartés **avant** la troncature : `total` compte la relation demandée, et sans
/// `relation` la liste est celle de la 1.0.1.
fn output(
    id: &str,
    contract: &CitationContract,
    mut links: Vec<LinkOut>,
    relation: Option<Relation>,
) -> GetRelatedOutput {
    links.retain(|link| relation.is_none_or(|wanted| link.relation == wanted));
    let total = links.len();
    links.truncate(MAX_LINKS);
    let citation = contract.citation.as_str();
    let source_url = contract.source_url.as_str();
    let fixed = estimate_chars(
        ENVELOPE_FIELDS,
        chars(id).saturating_add(chars(citation)).saturating_add(chars(source_url)),
    );
    shrink_from_end(&mut links, |all| {
        fixed.saturating_add(all.iter().map(cost).fold(0, usize::saturating_add))
            > ResponseBudget::ESTIMATE_TARGET_CHARS
    });
    GetRelatedOutput {
        id: id.to_owned(),
        citation: citation.to_owned(),
        source_url: source_url.to_owned(),
        total: u32::try_from(total).unwrap_or(u32::MAX),
        truncated: links.len() < total,
        links,
        note: None,
    }
}

/// Coût estimé d'un lien (24 par champ, textes échappés comme en JSON) : l'élément, ses trois champs toujours présents (`relation`, `kind`,
/// `citation`) et les deux facultatifs (`id`, `source_url`) quand ils y sont.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "estimation de taille : sommes de longueurs de textes et de compteurs déjà en mémoire, donc sous `usize::MAX`"
)]
fn cost(link: &LinkOut) -> usize {
    let optional = [&link.id, &link.source_url];
    let present = optional.iter().filter(|field| field.is_some()).count();
    let text = [link.relation.as_str(), link.kind.snake_case(), link.citation.as_str()]
        .into_iter()
        .chain(optional.into_iter().flatten().map(String::as_str))
        .map(escaped_chars)
        .fold(0, usize::saturating_add);
    estimate_list_chars(4 + present, text)
}

/// Longueur en caractères (points de code) : l'unité des plafonds.
fn chars(text: &str) -> usize {
    text.chars().count()
}

/// `invalid_arg` : un identifiant valide, mais d'une famille que l'outil ne lit pas, avec l'outil
/// qui la lit (03 §2.7). `id` est déjà validé : sa forme normalisée est citée telle quelle.
fn unsupported(id: &DocumentId) -> AppError {
    let hint = match id {
        DocumentId::LegiText(_) => " LEGITEXT... identifies a whole text: use fr_get_act.",
        DocumentId::KaliArti(_) => {
            " KALIARTI articles are not supported: read one with fr_get_text."
        }
        DocumentId::KaliText(_) => {
            " KALITEXT... identifies a collective agreement: use fr_get_convention."
        }
        DocumentId::CnilText(_) => {
            " CNILTEXT... identifies a CNIL deliberation: use fr_get_deliberation."
        }
        DocumentId::AccoText(_) => {
            " ACCOTEXT... identifies a company agreement: use fr_get_company_agreement."
        }
        _ => {
            " JORFTEXT... is the original Journal officiel text: search it with fr_search \
             fond=LODA and use the LEGITEXT id of the hit."
        }
    };
    messages::invalid_argument(wanted(id.as_str(), hint))
}

/// Ce que l'outil lit, pour l'identifiant `shown` (déjà normalisé ou échappé), suivi de `hint`.
fn wanted(shown: &str, hint: &str) -> String {
    format!(
        "{}='{shown}' must be a LEGIARTI... article id or a JURITEXT..., CONSTEXT... or CETATEXT... \
         decision id.{hint}",
        messages::id_field(TOOL),
    )
}

#[cfg(test)]
mod tests {
    use lf_domain::{
        citation::CitationKind,
        entity::{ArticleText, Decision, ParentText, RelatedLink, RelatedLinks, RelatedNature},
        text::LongText,
    };
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        citation::related::MAX_ANALYSED_CHARS,
        error::ErrorCode,
        ports::CorpusError,
        testing::{FakeCorpus, PortRequest},
        usecase::get_decision::fixtures::decision,
    };

    const ARTI: &str = "LEGIARTI000006419288";
    const JURI: &str = "JURITEXT000051743650";
    const CONSTIT: &str = "CONSTEXT000052555275";
    const CETAT: &str = "CETATEXT000051655317";

    fn input(id: Option<&str>) -> GetRelatedInput {
        GetRelatedInput { id: id.map(str::to_owned), ..GetRelatedInput::default() }
    }

    fn article(id: &str) -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(id).unwrap(),
            num: Some("9".to_owned()),
            etat: None,
            code_title: Some("Code civil".to_owned()),
            parent: Some(ParentText::new(
                DocumentId::parse("LEGITEXT000006070721").unwrap(),
                Some("CODE"),
            )),
            path: vec![],
            full_sections_titre: None,
            date_debut: None,
            date_fin: None,
            text: LongText::from_plain("Chacun a droit au respect de sa vie privée.").unwrap(),
            nota: None,
        }
    }

    /// Un lien « cite » vers un article du Code civil, identifié par `n`.
    fn cited(n: u32) -> RelatedLink {
        RelatedLink {
            target: DocumentId::parse(&format!("LEGIARTI{n:012}")).ok(),
            nature: RelatedNature::Code,
            title: Some(format!("Code civil, art. {n}")),
            date: None,
        }
    }

    /// Un lien sans cible typée (un `JORFCONT`) : ni `id` ni `source_url`, donc léger.
    fn bare(n: u32) -> RelatedLink {
        RelatedLink {
            target: None,
            nature: RelatedNature::JorfCont,
            title: Some(format!("Loi n° {n}")),
            date: None,
        }
    }

    fn serving(links: RelatedLinks) -> FakeCorpus {
        let corpus = FakeCorpus::new();
        corpus.push_get_article(Ok(article(ARTI)));
        corpus.push_related_links(Ok(links));
        corpus
    }

    async fn run(corpus: &FakeCorpus, id: Option<&str>) -> Result<GetRelatedOutput, AppError> {
        GetRelatedUseCase::new(corpus).execute(input(id)).await
    }

    // ---- article -------------------------------------------------------------------------

    #[tokio::test]
    async fn an_article_answers_with_its_citation_page_and_both_directions() {
        let corpus = serving(RelatedLinks {
            cites: vec![cited(1240)],
            cited_by: vec![RelatedLink {
                target: None,
                nature: RelatedNature::JorfCont,
                title: Some("Loi du 7 octobre 2016".to_owned()),
                date: None,
            }],
        });

        let out = run(&corpus, Some(" legiarti000006419288 ")).await.unwrap();

        assert_eq!(out.id, ARTI);
        assert_eq!(out.citation, "Code civil, art. 9");
        assert_eq!(
            out.source_url,
            format!("https://www.legifrance.gouv.fr/codes/article_lc/{ARTI}")
        );
        assert_eq!(out.total, 2);
        assert!(!out.truncated);
        let relations: Vec<_> = out.links.iter().map(|link| (link.relation, link.kind)).collect();
        assert_eq!(
            relations,
            [(Relation::Cites, CitationKind::CodeArticle), (Relation::CitedBy, CitationKind::Jorf)]
        );
        assert_eq!(out.links[0].id.as_deref(), Some("LEGIARTI000000001240"));
        assert_eq!(out.links[1].id, None);
        let id = ArticleId::parse(ARTI).unwrap();
        let mut seen = corpus.seen_requests();
        seen.sort_by_key(PortRequest::method);
        assert_eq!(seen, [PortRequest::GetArticle(id.clone()), PortRequest::RelatedLinks(id)]);
    }

    #[tokio::test]
    async fn related_capped_at_100_with_truncated() {
        let many =
            |count: u32| RelatedLinks { cites: (1..=count).map(bare).collect(), cited_by: vec![] };
        // 150 liens : cent rendus, `total` garde le compte d'avant la troncature.
        let out = run(&serving(many(150)), Some(ARTI)).await.unwrap();
        assert_eq!(out.links.len(), 100);
        assert_eq!(out.total, 150);
        assert!(out.truncated);
        assert_eq!(out.links[99].citation, "Loi n° 100", "les premiers sont gardés");
        // Exactement cent : rien n'est retiré, rien n'est dit.
        let out = run(&serving(many(100)), Some(ARTI)).await.unwrap();
        assert_eq!((out.links.len(), out.total, out.truncated), (100, 100, false));
        // Aucun lien : une liste vide, pas une erreur.
        let out = run(&serving(RelatedLinks::default()), Some(ARTI)).await.unwrap();
        assert_eq!((out.links.len(), out.total, out.truncated), (0, 0, false));
    }

    #[tokio::test]
    async fn a_hundred_short_links_are_all_served() {
        // 254 liens identifiés (cas réel LEGIARTI000035644154) : cent servis, `truncated`, et rien
        // n'est retiré par l'estimation, qui comptait 64 par champ et coupait à 79-86 liens.
        let links = RelatedLinks { cited_by: (1..=254).map(cited).collect(), cites: vec![] };

        let out = run(&serving(links), Some(ARTI)).await.unwrap();

        assert_eq!((out.links.len(), out.total, out.truncated), (100, 254, true));
        // Les cent premiers, dans l'ordre de l'API.
        assert_eq!(out.links[99].citation, "Code civil, art. 100");
    }

    #[tokio::test]
    async fn cites_come_before_cited_by_and_the_cut_falls_on_the_tail() {
        // Ordre documenté (05 §5.11) : les liens « cites » d'abord, puis les « cited_by ».
        let links = RelatedLinks {
            cites: (1..=3).map(cited).collect(),
            cited_by: (100..=300).map(cited).collect(),
        };

        let out = run(&serving(links), Some(ARTI)).await.unwrap();

        assert_eq!((out.links.len(), out.total, out.truncated), (100, 204, true));
        assert!(out.links[..3].iter().all(|link| link.relation == Relation::Cites));
        assert!(out.links[3..].iter().all(|link| link.relation == Relation::CitedBy));
    }

    #[test]
    fn the_cost_of_a_link_counts_24_per_field_and_the_json_escapes() {
        let link = |citation: &str, id: Option<&str>| LinkOut {
            relation: Relation::Cites,
            kind: CitationKind::CodeArticle,
            citation: citation.to_owned(),
            id: id.map(str::to_owned),
            source_url: id.map(|id| format!("u/{id}")),
        };
        let text = Relation::Cites.as_str().len() + CitationKind::CodeArticle.snake_case().len();
        assert_eq!(cost(&link("abc", None)), estimate_list_chars(4, text + 3));
        assert_eq!(cost(&link("abc", Some("L1"))), estimate_list_chars(6, text + 3 + 2 + 4));
        // Un guillemet pèse deux caractères, un contrôle six.
        assert_eq!(cost(&link("a\"\u{1}", None)), estimate_list_chars(4, text + 1 + 2 + 6));
    }

    #[tokio::test]
    async fn the_estimate_cap_removes_links_from_the_end_and_says_so() {
        // Cent liens de 800 caractères : 80 000 caractères, au-delà de la cible de 42 000.
        let big =
            |n: u32| RelatedLink { title: Some(format!("{n} {}", "x".repeat(800))), ..cited(n) };
        let links = RelatedLinks { cites: (1..=100).map(big).collect(), cited_by: vec![] };

        let out = run(&serving(links), Some(ARTI)).await.unwrap();

        assert_eq!(out.total, 100);
        assert!(out.truncated);
        assert!(out.links.len() < 100 && !out.links.is_empty());
        let kept: usize = out.links.iter().map(cost).sum();
        assert!(kept <= ResponseBudget::ESTIMATE_TARGET_CHARS, "{kept}");
        // Ce sont les derniers qui partent.
        assert!(out.links[0].citation.starts_with("1 x"));
    }

    #[tokio::test]
    async fn an_unknown_article_is_the_not_found_of_fr_get_text_and_the_read_wins() {
        let corpus = FakeCorpus::new();
        corpus.push_get_article(Err(CorpusError::NotFound { what: "/consult/getArticle".into() }));
        corpus.push_related_links(Err(CorpusError::Upstream {
            status: Some(500),
            retryable: true,
            detail: "boom".into(),
        }));

        let error = run(&corpus, Some(ARTI)).await.unwrap_err();

        assert_eq!(error.code(), Some(ErrorCode::NotFound));
        assert_eq!(
            error.message(),
            "No article found for LEGIARTI000006419288. Check the id, or find the article with \
             fr_get_act or fr_search."
        );
    }

    #[tokio::test]
    async fn a_failing_links_call_is_translated_like_any_port_error() {
        let corpus = FakeCorpus::new();
        corpus.push_get_article(Ok(article(ARTI)));
        corpus.push_related_links(Err(CorpusError::Timeout));

        let error = run(&corpus, Some(ARTI)).await.unwrap_err();

        assert_eq!(error.code(), Some(ErrorCode::UpstreamError));
        assert!(error.message().starts_with("Legifrance/PISTE did not answer in time"));
    }

    // ---- relation (D-05) -----------------------------------------------------------------

    /// Comme [`run`], pour la seule relation `relation`.
    async fn run_relation(
        corpus: &FakeCorpus,
        id: &str,
        relation: &str,
    ) -> Result<GetRelatedOutput, AppError> {
        let input =
            GetRelatedInput { id: Some(id.to_owned()), relation: Some(relation.to_owned()) };
        GetRelatedUseCase::new(corpus).execute(input).await
    }

    #[tokio::test]
    async fn the_filter_is_applied_before_the_cap_of_100_links() {
        // Synthétique, sur le modèle du cas réel LEGIARTI000035644154 (254 liens, tous
        // `cited_by`) : 150 liens « cites » passent devant 104 « cited_by » ; sans filtre, les
        // cent rendus sont tous des « cites » et aucun « cited_by » n'est joignable.
        let links = || RelatedLinks {
            cites: (1..=150).map(cited).collect(),
            cited_by: (1_000..1_104).map(cited).collect(),
        };
        let out = run(&serving(links()), Some(ARTI)).await.unwrap();
        assert_eq!((out.links.len(), out.total, out.truncated), (100, 254, true));
        assert!(out.links.iter().all(|link| link.relation == Relation::Cites));

        // Filtré : `total` compte la relation demandée, la coupe tombe sur elle.
        let out = run_relation(&serving(links()), ARTI, "cited_by").await.unwrap();
        assert_eq!((out.links.len(), out.total, out.truncated), (100, 104, true));
        assert!(out.links.iter().all(|link| link.relation == Relation::CitedBy));
        assert_eq!(out.links[0].citation, "Code civil, art. 1000", "l'ordre de l'API est gardé");
        assert_eq!(out.links[99].citation, "Code civil, art. 1099");

        // Jumeau : l'autre relation, 150 liens, cent rendus.
        let out = run_relation(&serving(links()), ARTI, "cites").await.unwrap();
        assert_eq!((out.links.len(), out.total, out.truncated), (100, 150, true));
        assert!(out.links.iter().all(|link| link.relation == Relation::Cites));
    }

    #[tokio::test]
    async fn a_filter_that_keeps_everything_or_nothing_says_so() {
        let links = || RelatedLinks { cites: vec![], cited_by: (1..=3).map(cited).collect() };
        // Tout est gardé : même réponse que sans filtre.
        let all = run(&serving(links()), Some(ARTI)).await.unwrap();
        let kept = run_relation(&serving(links()), ARTI, "cited_by").await.unwrap();
        assert_eq!(kept, all);
        // Aucun lien de cette relation : une liste vide, pas une erreur ni une note.
        let none = run_relation(&serving(links()), ARTI, "cites").await.unwrap();
        assert_eq!((none.links.len(), none.total, none.truncated, none.note), (0, 0, false, None));
        assert_eq!(none.id, ARTI);
        assert_eq!(none.citation, "Code civil, art. 9");
    }

    #[tokio::test]
    async fn the_relation_is_matched_without_case_or_surrounding_spaces() {
        for raw in ["cited_by", "CITED_BY", " Cited_By\t"] {
            let links = RelatedLinks { cites: vec![cited(1)], cited_by: vec![cited(2)] };
            let out = run_relation(&serving(links), ARTI, raw).await.unwrap();
            let relations: Vec<_> = out.links.iter().map(|link| link.relation).collect();
            assert_eq!(relations, [Relation::CitedBy], "{raw:?}");
            assert_eq!(out.total, 1);
        }
    }

    #[tokio::test]
    async fn the_relation_applies_to_a_decision_too() {
        let referenced = || {
            let corpus = FakeCorpus::new();
            corpus.push_get_decision(Ok(referencing(JURI, Some(CITATION_JP), applied_texts())));
            corpus
        };
        let out = run_relation(&referenced(), JURI, "applies_text").await.unwrap();
        let relations: Vec<_> = out.links.iter().map(|link| link.relation).collect();
        assert_eq!((relations, out.total), (vec![Relation::AppliesText], 1));
        let out = run_relation(&referenced(), JURI, "CITES_CASE_LAW").await.unwrap();
        let relations: Vec<_> = out.links.iter().map(|link| link.relation).collect();
        assert_eq!((relations, out.total), (vec![Relation::CitesCaseLaw], 1));
        // Une relation d'article n'existe pas pour une décision : liste vide.
        let out = run_relation(&referenced(), JURI, "cited_by").await.unwrap();
        assert_eq!((out.links.len(), out.total, out.truncated), (0, 0, false));
        // Sans filtre, les deux sont rendus (jumeau).
        let out = run(&referenced(), Some(JURI)).await.unwrap();
        assert_eq!(out.total, 2);
    }

    #[tokio::test]
    async fn an_unknown_relation_is_refused_with_the_list_and_without_network() {
        for raw in ["foo", "", "cited-by", "cites cited_by"] {
            let corpus = FakeCorpus::new();
            let error = run_relation(&corpus, ARTI, raw).await.unwrap_err();
            assert_eq!(error.code(), Some(ErrorCode::InvalidArg), "{raw:?}");
            assert_eq!(
                error.message(),
                format!(
                    "relation='{raw}' must be one of ['cites', 'cited_by', 'cites_case_law', \
                     'applies_text']."
                )
            );
            assert_eq!(corpus.calls(), 0, "un rejet local n'appelle pas le port");
        }
    }

    // ---- décision ------------------------------------------------------------------------

    /// `citationJp` de `JURITEXT000051743650` (`consult_juri.json`, mojibake réparé), tel que
    /// l'adapter le range dans `Decision::cited_case_law`.
    const CITATION_JP: &str = "Sur l'interprétation du droit interne à la lumière de la directive \
        85/374/CEE du Conseil du 25 juillet 1985, à rapprocher : 1re Civ., 25 mai 2023, pourvoi \
        n° 21-23.174 (cassation partielle) et l'arrêt cité.";

    /// Les deux titres de `liens` de la même décision, tels que l'adapter les range dans
    /// `Decision::applied_texts`.
    fn applied_texts() -> Vec<String> {
        vec![
            "1 du code civil.".to_owned(),
            "Article 10 de la directive 85/374/CEE du Conseil, du 25 juillet 1985, relative au \
             rapprochement des dispositions législatives, réglementaires et administratives des \
             Etats membres en matière de responsabilité du fait des produits défectueux ; article \
             2270-"
                .to_owned(),
        ]
    }

    /// Une décision `id` qui porte ces références.
    fn referencing(id: &str, cited: Option<&str>, applied: Vec<String>) -> Decision {
        Decision {
            cited_case_law: cited.map(str::to_owned),
            applied_texts: applied,
            ..decision(id, "texte")
        }
    }

    #[tokio::test]
    async fn a_decision_needs_one_port_call_and_gets_its_citation_and_page() {
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(decision(CONSTIT, "Considérant que…")));

        let out = run(&corpus, Some(CONSTIT)).await.unwrap();

        assert_eq!(out.id, CONSTIT);
        assert_eq!(out.source_url, format!("https://www.legifrance.gouv.fr/cons/id/{CONSTIT}"));
        assert_eq!(out.citation, format!("Légifrance {CONSTIT}"), "repli sans titre ni numéro");
        assert_eq!(corpus.calls(), 1);
        assert_eq!(
            corpus.seen_requests(),
            [PortRequest::GetDecision(DecisionId::parse(CONSTIT).unwrap())]
        );
        // Les trois préfixes de décision sont lus, et chacun a sa page.
        for (id, page) in [(JURI, "juri"), (CETAT, "ceta")] {
            let corpus = FakeCorpus::new();
            corpus.push_get_decision(Ok(decision(id, "texte")));
            let out = run(&corpus, Some(id)).await.unwrap();
            assert_eq!(out.id, id);
            assert_eq!(out.source_url, format!("https://www.legifrance.gouv.fr/{page}/id/{id}"));
            assert_eq!(corpus.calls(), 1);
        }
    }

    #[tokio::test]
    async fn an_unknown_decision_names_the_fond_of_its_prefix() {
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Err(CorpusError::NotFound { what: "/consult/juri".into() }));
        let error = run(&corpus, Some(CETAT)).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::NotFound));
        assert_eq!(
            error.message(),
            "No CETAT decision found for CETATEXT000051655317. Check the id, or search with \
             fr_search."
        );
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Err(CorpusError::Timeout));
        let error = run(&corpus, Some(JURI)).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::UpstreamError));
    }

    #[tokio::test]
    async fn decision_related_from_citationjp() {
        // Ce que le port rend d'une décision JURI dont `citationJp` et `liens` sont renseignés
        // (fixture consult_juri.json, mojibake réparé) : `cited_case_law` et `applied_texts` de la
        // vraie `Decision`, passés par la même chaîne que la lecture (parseur, dédoublonnage,
        // plafond), de bout en bout par le cas d'usage.
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(referencing(JURI, Some(CITATION_JP), applied_texts())));

        let out = run(&corpus, Some(JURI)).await.unwrap();

        assert_eq!(out.total, 2);
        assert!(!out.truncated);
        let shape: Vec<_> = out
            .links
            .iter()
            .map(|link| (link.relation.as_str(), link.kind.snake_case(), link.citation.as_str()))
            .collect();
        // « 1 du code civil. » n'a pas d'ancre : aucun lien. La directive de `citationJp` et celle de
        // `liens` visent le même acte : la plus précise (l'article 10) est gardée.
        assert_eq!(
            shape,
            [
                ("cites_case_law", "decision", "Cass. 1re civ., 25 mai 2023, n° 21-23.174"),
                (
                    "applies_text",
                    "unsupported",
                    "Article 10 de la directive 85/374/CEE du Conseil, du 25 juillet 1985"
                ),
            ]
        );
        assert_eq!(out.source_url, format!("https://www.legifrance.gouv.fr/juri/id/{JURI}"));
        assert!(
            out.links.iter().all(|link| link.id.is_none() && link.source_url.is_none()),
            "les liens de décision ne sont pas résolus"
        );
        assert_eq!(corpus.calls(), 1);
    }

    #[tokio::test]
    async fn each_reference_of_the_decision_counts_on_its_own() {
        // Seul `citationJp` : la jurisprudence citée et la directive ; seuls les `liens` : des
        // textes appliqués.
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(referencing(JURI, Some(CITATION_JP), vec![])));
        let out = run(&corpus, Some(JURI)).await.unwrap();
        let relations: Vec<_> = out.links.iter().map(|link| link.relation).collect();
        assert_eq!(relations, [Relation::CitesCaseLaw, Relation::AppliesText]);

        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(referencing(JURI, None, applied_texts())));
        let out = run(&corpus, Some(JURI)).await.unwrap();
        let relations: Vec<_> = out.links.iter().map(|link| link.relation).collect();
        assert_eq!(relations, [Relation::AppliesText]);
    }

    #[tokio::test]
    async fn a_decision_without_references_has_an_empty_list() {
        // CETAT : `citationJp: ""` et `liens: []` dans la capture. L'API ne donne aucune
        // référence : la liste est vide, sans suite, sans que cela prouve que la décision ne cite
        // rien (voir la doc du module).
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(decision(CETAT, "texte")));
        let out = run(&corpus, Some(CETAT)).await.unwrap();
        assert_eq!((out.links.len(), out.total, out.truncated), (0, 0, false));
        // La réponse le dit : l'absence de lien n'est pas l'absence de citation.
        assert_eq!(out.note, Some(Note::NoDecisionReferences));
    }

    #[tokio::test]
    async fn the_no_references_note_is_only_for_a_decision_the_api_gives_no_references_for() {
        // Des références existent mais le parseur n'y lit aucune citation : liste vide, sans note
        // (l'API en a donné, ce n'est pas elle qui s'est tue).
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(referencing(JURI, Some("néant"), vec![])));
        let out = run(&corpus, Some(JURI)).await.unwrap();
        assert_eq!((out.links.len(), out.note), (0, None));
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(referencing(JURI, None, vec!["néant".to_owned()])));
        assert_eq!(run(&corpus, Some(JURI)).await.unwrap().note, None);

        // Des références qui dépassent la place d'analyse : ignorées, mais l'API en a donné.
        let corpus = FakeCorpus::new();
        let too_long = "a".repeat(MAX_ANALYSED_CHARS + 1);
        corpus.push_get_decision(Ok(referencing(JURI, None, vec![too_long])));
        let out = run(&corpus, Some(JURI)).await.unwrap();
        assert_eq!((out.links.len(), out.truncated, out.note), (0, true, None));

        // Des liens trouvés : pas de note. Un article : jamais de note.
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(referencing(JURI, Some(CITATION_JP), applied_texts())));
        assert_eq!(run(&corpus, Some(JURI)).await.unwrap().note, None);
        let corpus = serving(RelatedLinks::default());
        assert_eq!(run(&corpus, Some(ARTI)).await.unwrap().note, None);
    }

    #[tokio::test]
    async fn a_decision_link_with_a_code_carries_the_note_and_only_then() {
        // related_diff-2 : « articles 1382 et 700 du code civil » ne rend plus « Code civil, art.
        // 700 » ; la réponse dit que les codes des liens d'une décision peuvent être inexacts.
        let corpus = FakeCorpus::new();
        let texts = vec!["articles 1382 et 700 du code civil".to_owned()];
        corpus.push_get_decision(Ok(referencing(JURI, None, texts)));
        let out = run(&corpus, Some(JURI)).await.unwrap();
        let shown: Vec<_> = out.links.iter().map(|link| link.citation.as_str()).collect();
        assert_eq!(shown, ["article 1382", "article 700"]);
        assert_eq!(out.note, Some(Note::DecisionLinkCodes));

        // Un code écrit à côté de son article est gardé, mais la note est livrée aussi : le titre
        // brut vient d'un fragment amont (V-117).
        let corpus = FakeCorpus::new();
        let texts = vec!["article 700 du code de procédure civile".to_owned()];
        corpus.push_get_decision(Ok(referencing(JURI, None, texts)));
        let out = run(&corpus, Some(JURI)).await.unwrap();
        assert_eq!(out.links[0].citation, "Code de procédure civile, art. 700");
        assert_eq!(out.note, Some(Note::DecisionLinkCodes));

        // Jumeau : aucun lien ne porte de code, pas de note.
        let corpus = FakeCorpus::new();
        let texts = vec!["article 1382, devenu 1240 du code civil".to_owned()];
        corpus.push_get_decision(Ok(referencing(JURI, None, texts)));
        assert_eq!(run(&corpus, Some(JURI)).await.unwrap().note, None);
    }

    #[tokio::test]
    async fn decision_links_are_capped_like_article_links() {
        let texts: Vec<String> = (1..=150).map(|n| format!("article {n} du code civil")).collect();
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(referencing(JURI, None, texts)));
        let out = run(&corpus, Some(JURI)).await.unwrap();
        assert_eq!((out.links.len(), out.total, out.truncated), (100, 150, true));
        assert_eq!(out.links[0].citation, "Code civil, art. 1");
        assert_eq!(out.links[99].citation, "Code civil, art. 100");

        // Exactement cent : rien n'est retiré, rien n'est dit.
        let texts: Vec<String> = (1..=100).map(|n| format!("article {n} du code civil")).collect();
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(referencing(JURI, None, texts)));
        let out = run(&corpus, Some(JURI)).await.unwrap();
        assert_eq!((out.links.len(), out.total, out.truncated), (100, 100, false));
    }

    #[tokio::test]
    async fn the_longest_decision_links_still_fit_the_estimate_target() {
        // Une citation de décision tient en quelques dizaines de caractères (le parseur n'en garde
        // que la partie reconnue) : cent liens parmi les plus longs, des règlements européens,
        // restent sous la cible de 42 000 de l'estimation, sans rien retirer.
        let texts: Vec<String> = (1..=100)
            .map(|n| {
                format!(
                    "Règlement (UE) 2016/{n} du Parlement européen et du Conseil du 27 avril 2016"
                )
            })
            .collect();
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(referencing(JURI, None, texts)));

        let out = run(&corpus, Some(JURI)).await.unwrap();

        assert_eq!((out.links.len(), out.total, out.truncated), (100, 100, false));
        let kept: usize = out.links.iter().map(cost).sum();
        assert!(kept <= ResponseBudget::ESTIMATE_TARGET_CHARS, "{kept}");
        assert!(out.links.iter().all(|link| link.citation.chars().count() < 100));
    }

    #[tokio::test]
    async fn a_decision_with_a_long_reference_list_stays_bounded() {
        // Mille textes appliqués : cent liens rendus, `total` garde le compte d'avant la
        // troncature, estimation sous la cible, quelle que soit la taille de la liste de l'API.
        let texts: Vec<String> =
            (1..=1_000).map(|n| format!("article {n} du code civil")).collect();
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(referencing(JURI, Some(CITATION_JP), texts)));

        let out = run(&corpus, Some(JURI)).await.unwrap();

        assert_eq!(out.total, 1_002);
        assert!(out.truncated);
        assert_eq!(out.links.len(), 100);
        assert_eq!(out.links[0].relation, Relation::CitesCaseLaw, "la jurisprudence passe devant");
        let kept: usize = out.links.iter().map(cost).sum();
        assert!(kept <= ResponseBudget::ESTIMATE_TARGET_CHARS, "{kept}");
    }

    #[tokio::test]
    async fn references_beyond_the_analysis_budget_are_ignored_and_truncated_says_so() {
        // Quatre titres de 20 000 caractères (`a`, `b`, `c`, `d` répétés) après une citation courte :
        // seuls les deux premiers tiennent dans les 50 000 caractères analysés. Deux liens
        // seulement sont trouvés, bien en dessous de cent : `truncated` vient de l'analyse
        // partielle, non du plafond de liens.
        let long = |article: u32| format!("article {article} du code civil {}", "x".repeat(19_950));
        let texts: Vec<String> = (1..=4).map(long).collect();
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(referencing(JURI, None, texts)));

        let out = run(&corpus, Some(JURI)).await.unwrap();

        let citations: Vec<_> = out.links.iter().map(|link| link.citation.as_str()).collect();
        assert_eq!(citations, ["Code civil, art. 1", "Code civil, art. 2"]);
        assert_eq!(out.total, 2);
        assert!(out.truncated, "des références n'ont pas été lues");

        // Dans la limite, rien n'est dit.
        let texts: Vec<String> = (1..=2).map(long).collect();
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(referencing(JURI, None, texts)));
        let out = run(&corpus, Some(JURI)).await.unwrap();
        assert_eq!((out.links.len(), out.total, out.truncated), (2, 2, false));
    }

    // ---- validation ----------------------------------------------------------------------

    /// Message d'un `invalid_arg`, après avoir vérifié qu'aucune requête n'est partie.
    async fn rejected(id: Option<&str>) -> String {
        let corpus = FakeCorpus::new();
        let error = run(&corpus, id).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(corpus.calls(), 0, "un rejet local n'appelle pas le port");
        error.message().to_owned()
    }

    #[tokio::test]
    async fn the_id_is_required_and_must_be_well_formed() {
        assert_eq!(rejected(None).await, "id is required.");
        // L4-10 (related_diff-8) : un préfixe inconnu cite ce que l'outil lit, non les dix
        // familles du domaine dont six sont refusées ensuite.
        for raw in ["x", "foo", "", "FOOTEXT000000000001"] {
            assert_eq!(
                rejected(Some(raw)).await,
                format!(
                    "id='{raw}' must be a LEGIARTI... article id or a JURITEXT..., CONSTEXT... \
                     or CETATEXT... decision id."
                ),
                "{raw:?}"
            );
        }
        // Un préfixe connu aux chiffres faux garde le message du domaine.
        assert_eq!(
            rejected(Some("LEGIARTI123")).await,
            "id='LEGIARTI123' must be LEGIARTI followed by exactly 12 digits \
             (e.g. LEGIARTI000006419288)."
        );
        let message = rejected(Some("JURITEXT12")).await;
        assert!(message.contains("must be JURITEXT followed by exactly 12 digits"), "{message}");
    }

    #[tokio::test]
    async fn other_families_are_refused_with_the_tool_that_reads_them() {
        let cases = [
            (
                "KALIARTI000049886059",
                "KALIARTI articles are not supported: read one with fr_get_text.",
            ),
            ("LEGITEXT000006070721", "LEGITEXT... identifies a whole text: use fr_get_act."),
            (
                "KALITEXT000005652329",
                "KALITEXT... identifies a collective agreement: use fr_get_convention.",
            ),
            (
                "CNILTEXT000053352643",
                "CNILTEXT... identifies a CNIL deliberation: use fr_get_deliberation.",
            ),
            (
                "ACCOTEXT000037731479",
                "ACCOTEXT... identifies a company agreement: use fr_get_company_agreement.",
            ),
            (
                "JORFTEXT000019109412",
                "JORFTEXT... is the original Journal officiel text: search it with fr_search \
                 fond=LODA and use the LEGITEXT id of the hit.",
            ),
        ];
        for (id, hint) in cases {
            assert_eq!(
                rejected(Some(id)).await,
                format!(
                    "id='{id}' must be a LEGIARTI... article id or a JURITEXT..., CONSTEXT... or \
                     CETATEXT... decision id. {hint}"
                )
            );
        }
    }
}
