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
//!    citation lisible n'a pas de note. Les références analysées sont bornées à
//!    `MAX_ANALYSED_CHARS` caractères : au-delà, elles sont ignorées et `truncated` le dit.
//!
//! # Plafond de réponse (05 §5.11, 05 §6.4)
//!
//! Au plus `MAX_LINKS` liens, sans suite : `total` compte les liens avant troncature et
//! `truncated` le dit. L'estimation de la réponse ([`estimate_chars`]) reste sous la cible, en
//! retirant des liens par la fin ([`shrink_from_end`]) ; la vue mesure ensuite la taille exacte.

use futures::future::join;
use lf_domain::{
    contract::{Citable, CitationContract},
    id::{ArticleId, DecisionId, DocumentId, LegiArtiId},
};

use crate::{
    budget::{ResponseBudget, estimate_chars, shrink_from_end},
    citation::{
        Note,
        related::{DecisionReferences, MAX_LINKS, article_links, decision_links},
    },
    error::AppError,
    failure::port_failure,
    input::GetRelatedInput,
    messages,
    output::get_related::{GetRelatedOutput, LinkOut},
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
    /// `invalid_arg` si `id` est absent, mal formé ou d'une famille que l'outil ne lit pas (sans
    /// appel au port) ; `not_found` si le corpus ne connaît pas l'article ou la décision (message
    /// de l'outil de lecture correspondant, 05 §7.4) ; sinon l'erreur du port, traduite par
    /// [`port_failure`].
    pub(crate) async fn execute(
        &self,
        input: GetRelatedInput,
    ) -> Result<GetRelatedOutput, AppError> {
        let raw = input.id.ok_or_else(|| messages::required(messages::id_field(TOOL)))?;
        match DocumentId::parse(&raw).map_err(|error| messages::domain(TOOL, &error))? {
            DocumentId::LegiArti(id) => self.article(id).await,
            DocumentId::JuriText(id) => self.decision(DecisionId::Juri(id)).await,
            DocumentId::ConsText(id) => self.decision(DecisionId::Constit(id)).await,
            DocumentId::CetaText(id) => self.decision(DecisionId::Cetat(id)).await,
            other => Err(unsupported(&other)),
        }
    }

    /// Un article : sa citation et ses liens `cites` et `cited_by`.
    async fn article(&self, id: LegiArtiId) -> Result<GetRelatedOutput, AppError> {
        let id = ArticleId::Legi(id);
        let (article, links) =
            join(self.corpus.get_article(&id), self.corpus.related_links(&id)).await;
        let failure = |error| port_failure(ToolName::FR_GET_TEXT, id.as_str(), &error);
        let article = article.map_err(failure)?;
        let links = links.map_err(failure)?;
        Ok(output(id.as_str(), &article.contract(), article_links(&links)))
    }

    /// Une décision : sa citation et les liens que le parseur lit dans ses références.
    async fn decision(&self, id: DecisionId) -> Result<GetRelatedOutput, AppError> {
        let decision = self
            .corpus
            .get_decision(&id)
            .await
            .map_err(|error| port_failure(ToolName::FR_GET_DECISION, id.as_str(), &error))?;
        let references = DecisionReferences::of(&decision);
        let found = output(id.as_str(), &decision.contract(), decision_links(&references));
        // Des références que l'API ne donne pas du tout : lues sur la décision, non sur les
        // références analysées, que `MAX_ANALYSED_CHARS` peut avoir vidées.
        let unreferenced = decision.cited_case_law.is_none() && decision.applied_texts.is_empty();
        Ok(GetRelatedOutput {
            // Des références ignorées faute de place : la liste est incomplète.
            truncated: found.truncated || references.clipped,
            note: unreferenced.then_some(Note::NoDecisionReferences),
            ..found
        })
    }
}

/// La réponse de la page `id` (`contract` : sa citation et sa page) pour `links`, ramenée aux
/// bornes : cent liens au plus, estimation sous la cible.
fn output(id: &str, contract: &CitationContract, mut links: Vec<LinkOut>) -> GetRelatedOutput {
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

/// Coût estimé d'un lien : l'élément, ses trois champs toujours présents (`relation`, `kind`,
/// `citation`) et les deux facultatifs (`id`, `source_url`) quand ils y sont.
fn cost(link: &LinkOut) -> usize {
    let optional = [&link.id, &link.source_url];
    let present = optional.iter().filter(|field| field.is_some()).count();
    let text = [link.relation.as_str(), link.kind.snake_case(), link.citation.as_str()]
        .into_iter()
        .chain(optional.into_iter().flatten().map(String::as_str))
        .map(chars)
        .fold(0, usize::saturating_add);
    estimate_chars(4 + present, text)
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
    messages::invalid_argument(format!(
        "{}='{}' must be a LEGIARTI... article id or a JURITEXT..., CONSTEXT... or CETATEXT... \
         decision id.{hint}",
        messages::id_field(TOOL),
        id.as_str()
    ))
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
        output::get_related::Relation,
        ports::CorpusError,
        testing::{FakeCorpus, PortRequest},
        usecase::get_decision::fixtures::decision,
    };

    const ARTI: &str = "LEGIARTI000006419288";
    const JURI: &str = "JURITEXT000051743650";
    const CONSTIT: &str = "CONSTEXT000052555275";
    const CETAT: &str = "CETATEXT000051655317";

    fn input(id: Option<&str>) -> GetRelatedInput {
        GetRelatedInput { id: id.map(str::to_owned) }
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
    async fn links_with_an_id_and_a_page_cost_more_so_the_estimate_may_trim_before_100() {
        // Chaque lien identifié compte cinq champs et une page : l'estimation, volontairement
        // large (64 par champ), retire les derniers liens bien avant les 45 000 caractères réels.
        let links = RelatedLinks { cites: (1..=100).map(cited).collect(), cited_by: vec![] };

        let out = run(&serving(links), Some(ARTI)).await.unwrap();

        assert_eq!(out.total, 100);
        assert!(out.truncated);
        let kept = out.links.len();
        assert!((1..100).contains(&kept), "{kept}");
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
        assert_eq!(
            rejected(Some("x")).await,
            "id='x' must start with one of ('LEGITEXT', 'LEGIARTI', 'JORFTEXT', 'KALITEXT', \
             'KALIARTI', 'JURITEXT', 'CONSTEXT', 'CETATEXT', 'CNILTEXT', 'ACCOTEXT')."
        );
        let message = rejected(Some("FOOTEXT000000000001")).await;
        assert!(
            message.starts_with("id='FOOTEXT000000000001' must start with one of ("),
            "{message}"
        );
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
