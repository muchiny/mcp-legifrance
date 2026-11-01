//! Contrôle des citations d'un texte (annexe B §7.7, `fr_verify_citations`).
//!
//! `verify_text` enchaîne les étapes de l'algorithme : analyse du texte, tri hors ligne,
//! dédoublonnage par clé de résolution, résolution quatre à la fois sous un budget de 80 % du
//! délai de l'outil, puis composition des contrôles dans l'ordre du texte. Il ne connaît ni la
//! validation de l'entrée (le cas d'usage) ni le plafond de réponse (le cas d'usage et la vue).
//!
//! # Garanties
//!
//! - Les citations de même clé partagent leur résolution : une répétition ne coûte aucun appel.
//! - La liste des codes n'est lue qu'une fois, et seulement si une citation la demande.
//! - Au plus `CONCURRENCY` résolutions sont en vol. Passé le budget, les citations restantes
//!   sont `unresolvable` (« time budget ») sans appel au corpus.
//! - Si **toutes** les citations soumises au corpus ont échoué, l'erreur de la première est
//!   rendue : un résultat fait uniquement de « not checked » laisserait croire à une vérification.

use std::{collections::HashMap, time::Duration};

use futures::{StreamExt, stream};
use lf_domain::{
    citation::{CitationRef, ParseOptions, Spanned, adjacent_quote, parse_with},
    date::VersionDate,
};
use tokio::time::{Instant, timeout_at};

use super::{
    CheckStatus, CitationCheck, Note, QuoteCheck,
    policy::{CONCURRENCY, budget_of, check_quote},
    resolve::{CodeTable, Outcome, Proof, Resolution, Resolver, needs_codes, offline},
};
use crate::{
    messages,
    ports::{CorpusError, LegalCorpus},
};

/// Longueur maximale, en caractères, de l'extrait d'origine rendu pour une citation.
const RAW_MAX_CHARS: usize = 300;

/// Réglages d'un contrôle.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Run {
    /// Source interrogée : l'absence n'est `not_found` que si elle la prouve (ni bac à sable, ni
    /// instantané local).
    pub(crate) proof: Proof,
    /// Jour de Paris.
    pub(crate) today: VersionDate,
    /// Temps restant de l'appel : le budget en est 80 %.
    pub(crate) remaining: Duration,
    /// Nombre maximal de citations contrôlées.
    pub(crate) max_citations: usize,
}

/// Résultat du contrôle d'un texte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Verification {
    /// Citations reconnues dans le texte, y compris celles qui dépassent `max_citations`.
    pub(crate) found: usize,
    /// Les `max_citations` premières citations, contrôlées, dans l'ordre du texte.
    pub(crate) checks: Vec<CitationCheck>,
}

/// Clé de résolution d'une citation : tout ce qui peut changer le résultat de la recherche. Deux
/// citations de même clé (énumérations, répétitions) partagent leur résolution.
fn key_of(item: &Spanned<CitationRef>) -> String {
    format!("{:?}|{:?}|{:?}", item.value, item.ambiguity, item.flags)
}

/// Résout `item` avant `limit` ; passé ce délai, la citation n'est pas contrôlée.
async fn resolve_within<C: LegalCorpus>(
    resolver: &Resolver<'_, C>,
    item: &Spanned<CitationRef>,
    limit: Instant,
) -> Resolution {
    if Instant::now() >= limit {
        return Resolution::out_of_time();
    }
    timeout_at(limit, resolver.resolve_ref(item, None))
        .await
        .unwrap_or_else(|_| Resolution::out_of_time())
}

/// Contrôle les citations de `text`.
///
/// # Errors
///
/// L'erreur du corpus de la première citation résolue, si toutes les citations soumises au corpus
/// ont échoué.
#[expect(
    clippy::indexing_slicing,
    reason = "`slot` est le rang d'un élément unique : `outcomes` en compte autant"
)]
pub(crate) async fn verify_text<C: LegalCorpus>(
    corpus: &C,
    run: Run,
    text: &str,
) -> Result<Verification, CorpusError> {
    // Le garde-fou de l'analyse est la borne par défaut du domaine (1 000) : l'application borne
    // ensuite à `max_citations` (annexe B §7.7).
    let options = ParseOptions { today: Some(run.today.as_civil()), ..ParseOptions::default() };
    let parsed = parse_with(text, &options);
    let found = parsed.len();
    let kept: Vec<Spanned<CitationRef>> = parsed.into_iter().take(run.max_citations).collect();

    // Tri hors ligne (`Ok`), puis une résolution par clé (`Err` : le rang de la résolution).
    let mut slots: HashMap<String, usize> = HashMap::new();
    let mut unique: Vec<Spanned<CitationRef>> = Vec::new();
    let mut routes: Vec<Result<Resolution, usize>> = Vec::with_capacity(kept.len());
    for item in &kept {
        if let Some(done) = offline(item) {
            routes.push(Ok(done));
            continue;
        }
        let slot = *slots.entry(key_of(item)).or_insert_with(|| {
            unique.push(item.clone());
            unique.len().saturating_sub(1)
        });
        routes.push(Err(slot));
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "`run.remaining` est le délai restant de la requête (secondes), jamais proche du débordement d'`Instant`"
    )]
    let limit = Instant::now() + budget_of(run.remaining);
    let codes = if unique.iter().any(needs_codes) {
        let listed = timeout_at(limit, corpus.list_codes()).await;
        CodeTable::from_result(listed.unwrap_or(Err(CorpusError::Timeout)))
    } else {
        CodeTable::empty()
    };
    let resolver = Resolver::new(corpus, run.today, &codes);
    let resolver = &resolver;
    let mut arrived: Vec<(usize, Resolution)> = stream::iter(unique.into_iter().enumerate())
        .map(|(slot, item)| async move { (slot, resolve_within(resolver, &item, limit).await) })
        .buffer_unordered(CONCURRENCY)
        .collect()
        .await;
    arrived.sort_by_key(|(slot, _)| *slot);
    let outcomes: Vec<Resolution> = arrived.into_iter().map(|(_, resolution)| resolution).collect();

    if !outcomes.is_empty() {
        let failures: Vec<&CorpusError> = outcomes
            .iter()
            .filter_map(|resolution| match &resolution.outcome {
                Outcome::Failed(error) => Some(error),
                _ => None,
            })
            .collect();
        if let Some(first) = failures.first().filter(|_| failures.len() == outcomes.len()) {
            return Err((*first).clone());
        }
    }

    let checks = kept
        .iter()
        .zip(routes)
        .map(|(item, route)| {
            let resolution = route.unwrap_or_else(|slot| outcomes[slot].clone());
            compose(run.proof, text, item, resolution)
        })
        .collect();
    Ok(Verification { found, checks })
}

/// Note finale : l'écart d'extrait passe toujours en tête d'une note de statut `verified`
/// (version en vigueur, sigle lu par défaut, chambre non comparée), qu'elle ne masque plus. Pour un
/// autre statut, la note du statut reste en tête : c'est la vraie raison, qu'un écart d'extrait
/// ne doit ni précéder ni faire rogner par la borne de 200 caractères.
fn with_quote_gap(
    status: CheckStatus,
    note: Option<Note>,
    quote: Option<QuoteCheck>,
) -> Option<Note> {
    if quote != Some(QuoteCheck::QuoteMismatch) || status != CheckStatus::Verified {
        return note;
    }
    Some(note.map_or(Note::QuoteMismatch, |inner| Note::WithQuoteMismatch(Box::new(inner))))
}

/// Contrôle d'une citation : statut, enregistrement, extrait cité et note, span en points de code.
fn compose(
    proof: Proof,
    text: &str,
    item: &Spanned<CitationRef>,
    resolution: Resolution,
) -> CitationCheck {
    let Resolution { outcome, official } = resolution;
    let quote = official.as_deref().and_then(|official| {
        let span = adjacent_quote(text, item.span)?;
        text.get(span.start..span.end).map(|quoted| check_quote(quoted, official))
    });
    let status = outcome.status(proof);
    let note = with_quote_gap(status, outcome.note(proof), quote);
    let raw = text.get(item.span.start..item.span.end).unwrap_or_default();
    CitationCheck {
        span: item.span.to_chars(text),
        raw: messages::clip(raw, RAW_MAX_CHARS),
        kind: item.value.kind(),
        status,
        resolved: outcome.record().cloned(),
        quote,
        note,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        citation::CheckStatus,
        ports::{ArticleAt, PisteEnvironment},
        testing::{FakeCorpus, PortRequest},
    };
    use lf_domain::{
        date::CivilDate,
        ecli::Ecli,
        entity::{ArticleText, CodeListing, Decision, OfficialSummary, SearchHit},
        fond::Fond,
        id::{ArticleId, DecisionId, DocumentId, LegiTextId},
        status::LegalStatus,
        text::LongText,
    };
    use pretty_assertions::assert_eq;

    const CIVIL: &str = "LEGITEXT000006070721";
    const TRAVAIL: &str = "LEGITEXT000006072050";

    fn today() -> VersionDate {
        VersionDate::parse("2026-10-29").unwrap()
    }

    fn run(env: PisteEnvironment) -> Run {
        run_with(Proof::of(env, false))
    }

    fn run_with(proof: Proof) -> Run {
        Run { proof, today: today(), remaining: Duration::from_secs(60), max_citations: 50 }
    }

    fn listing(id: &str, title: &str) -> CodeListing {
        CodeListing {
            id: LegiTextId::parse(id).unwrap(),
            title: title.to_owned(),
            etat: Some(LegalStatus::Vigueur),
        }
    }

    fn codes(corpus: &FakeCorpus) {
        corpus.push_list_codes(Ok(vec![
            listing(CIVIL, "Code civil"),
            listing(TRAVAIL, "Code du travail"),
        ]));
    }

    fn article(n: u32, num: Option<&str>, since: Option<CivilDate>, text: &str) -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(&format!("LEGIARTI{n:012}")).unwrap(),
            num: num.map(str::to_owned),
            etat: Some(LegalStatus::Vigueur),
            code_title: Some("Code civil".to_owned()),
            parent: None,
            path: vec![],
            full_sections_titre: None,
            date_debut: since,
            date_fin: None,
            text: LongText::from_plain(text).unwrap(),
            nota: None,
        }
    }

    fn not_found() -> CorpusError {
        CorpusError::NotFound { what: "/consult".to_owned() }
    }

    fn upstream() -> CorpusError {
        CorpusError::Upstream { status: Some(503), retryable: true, detail: "down".to_owned() }
    }

    fn hit(fond: Fond, id: &str) -> SearchHit {
        SearchHit::new(fond, DocumentId::parse(id).unwrap(), None).unwrap()
    }

    fn note_text(check: &CitationCheck) -> String {
        check.note.as_ref().map(Note::text).unwrap_or_default()
    }

    fn statuses(verification: &Verification) -> Vec<CheckStatus> {
        verification.checks.iter().map(|check| check.status).collect()
    }

    async fn verify(
        corpus: &FakeCorpus,
        env: PisteEnvironment,
        text: &str,
    ) -> Result<Verification, CorpusError> {
        verify_text(corpus, run(env), text).await
    }

    #[tokio::test]
    async fn n24_production_article_9999_is_not_found_with_a_dated_note() {
        let corpus = FakeCorpus::new();
        codes(&corpus);
        corpus.push_find_article(Err(not_found()));
        let verification =
            verify(&corpus, PisteEnvironment::Production, "article 9999 du code civil")
                .await
                .unwrap();
        assert_eq!(verification.found, 1);
        let check = &verification.checks[0];
        assert_eq!(check.status, CheckStatus::NotFound);
        assert_eq!(
            note_text(check),
            "Code civil has no article 9999 in force on 2026-10-29. The article may exist in earlier versions; retry with a date."
        );
        assert!(check.resolved.is_none());
        assert_eq!(
            corpus.seen_requests(),
            vec![
                PortRequest::ListCodes,
                PortRequest::FindArticle(
                    LegiTextId::parse(CIVIL).unwrap(),
                    "9999".to_owned(),
                    ArticleAt::Today(today())
                ),
            ]
        );
    }

    /// L2-09 (gap3-11) : `verify` nomme aussi les deux codes du CPCE, sans appel au corpus.
    #[tokio::test]
    async fn an_ambiguous_cpce_names_both_codes() {
        let corpus = FakeCorpus::new();
        let verification =
            verify(&corpus, PisteEnvironment::Production, "art. L. 32 CPCE").await.unwrap();
        let check = &verification.checks[0];
        assert_eq!(check.status, CheckStatus::Ambiguous);
        let text = note_text(check);
        assert!(text.contains("Code des postes et des communications électroniques"), "{text}");
        assert!(text.contains("Code des procédures civiles d'exécution"), "{text}");
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn n24_sandbox_article_9999_is_unresolvable_with_the_sandbox_note() {
        let corpus = FakeCorpus::new();
        codes(&corpus);
        corpus.push_find_article(Err(not_found()));
        let verification =
            verify(&corpus, PisteEnvironment::Sandbox, "article 9999 du code civil").await.unwrap();
        let check = &verification.checks[0];
        assert_eq!(check.status, CheckStatus::Unresolvable);
        assert_eq!(
            note_text(check),
            "No article record matches Code civil, art. 9999 on 2026-10-29 in the PISTE sandbox; \
             sandbox data differ from production. The article may exist in earlier versions; retry with a date."
        );
    }

    /// Une absence dans l'instantané local de l'index DILA n'est jamais `not_found` : un dump
    /// périmé ne prouve pas l'inexistence (V4-02). La note le dit, sans parler de bac à sable.
    #[tokio::test]
    async fn a_local_snapshot_never_proves_an_absence() {
        let text = "article 9999 du code civil et l'identifiant LEGIARTI000006419288";
        let corpus = FakeCorpus::new();
        codes(&corpus);
        corpus.push_find_article(Err(not_found()));
        corpus.push_get_article(Err(not_found()));
        let verification = verify_text(&corpus, run_with(Proof::Snapshot), text).await.unwrap();
        assert_eq!(verification.checks.len(), 2);
        for check in &verification.checks {
            assert_eq!(check.status, CheckStatus::Unresolvable, "{}", check.raw);
            let note = note_text(check);
            assert!(
                note.contains("in the local DILA snapshot, which may be out of date"),
                "{note}"
            );
            assert!(!note.contains("sandbox"), "{note}");
        }
        assert_eq!(
            note_text(&verification.checks[0]),
            "No article record matches Code civil, art. 9999 on 2026-10-29 in the local DILA \
             snapshot, which may be out of date."
        );
    }

    #[tokio::test]
    async fn sandbox_never_not_found() {
        // Une absence prouvée (article de code, identifiant) est `not_found` en production et
        // `unresolvable` en bac à sable, jamais `not_found` (00 §7).
        let text = "article 9999 du code civil et l'identifiant LEGIARTI000006419288";
        for (env, expected) in [
            (PisteEnvironment::Production, CheckStatus::NotFound),
            (PisteEnvironment::Sandbox, CheckStatus::Unresolvable),
        ] {
            let corpus = FakeCorpus::new();
            codes(&corpus);
            corpus.push_find_article(Err(not_found()));
            corpus.push_get_article(Err(not_found()));
            let verification = verify(&corpus, env, text).await.unwrap();
            assert_eq!(verification.checks.len(), 2, "{env:?}");
            for check in &verification.checks {
                assert_eq!(check.status, expected, "{env:?} {}", check.raw);
            }
            if env == PisteEnvironment::Sandbox {
                assert!(
                    verification.checks.iter().all(|check| note_text(check).contains("sandbox"))
                );
            }
        }
    }

    #[tokio::test]
    async fn n14_ecli_one_day_off_is_invalid_with_the_native_ecli() {
        let corpus = FakeCorpus::new();
        let mut found = hit(Fond::Cetat, "CETATEXT000054178503");
        found.title =
            Some("Conseil d'État, 2ème - 7ème chambres réunies, 29/05/2026, 506507".into());
        corpus.push_find_by_number(Ok(vec![found]));
        corpus.push_get_decision(Ok(Decision {
            decision_id: DecisionId::parse("CETATEXT000054178503").unwrap(),
            ecli: Some(Ecli::parse("ECLI:FR:CECHR:2026:506507.20260529").unwrap()),
            num: Some("506507".to_owned()),
            juridiction: None,
            formation: None,
            solution: None,
            numero_affaire: vec![],
            date_texte: CivilDate::new(2026, 5, 29),
            nature: None,
            title: Some("Conseil d'État, 2ème - 7ème chambres réunies, 29/05/2026, 506507".into()),
            text: LongText::from_plain("Texte de la décision.").unwrap(),
            official_summary: OfficialSummary::default(),
            cited_case_law: None,
            applied_texts: vec![],
        }));
        let verification =
            verify(&corpus, PisteEnvironment::Production, "ECLI:FR:CECHR:2026:506507.20260530")
                .await
                .unwrap();
        let check = &verification.checks[0];
        assert_eq!(check.status, CheckStatus::Invalid);
        assert_eq!(check.resolved.as_ref().unwrap().id.as_str(), "CETATEXT000054178503");
        assert_eq!(note_text(check), "Native ECLI is ECLI:FR:CECHR:2026:506507.20260529.");
        assert_eq!(corpus.calls_of("find_by_number"), 1);
        assert!(matches!(
            &corpus.seen_requests()[0],
            PortRequest::FindByNumber(query)
                if query.fond == Fond::Cetat && query.value.as_str() == "506507"
        ));
    }

    /// Repro de verify_citations-2 et -14 : les quatre pourvois de l'E2E (relevés C4), cités avec
    /// une autre chambre, sont `invalid` avec la vraie chambre en note.
    #[tokio::test]
    async fn a_cassation_citation_with_the_wrong_chamber_is_invalid_with_the_real_one() {
        for (text, id, date, formation, expected) in [
            (
                "Cass. civ. 1re, 25 novembre 2015, n° 14-24.444",
                "JURITEXT000031539712",
                (2015, 11, 25),
                "CHAMBRE_SOCIALE",
                "Cited chamber CIV1 differs from the official record (SOC).",
            ),
            (
                "Cass. crim., 6 octobre 2006, n° 05-13.255",
                "JURITEXT000007052928",
                (2006, 10, 6),
                "ASSEMBLEE_PLENIERE",
                "Cited chamber CRIM differs from the official record (AP).",
            ),
            (
                "Cass. com., 29 mars 1991, n° 89-15.231",
                "JURITEXT000007026394",
                (1991, 3, 29),
                "ASSEMBLEE_PLENIERE",
                "Cited chamber COM differs from the official record (AP).",
            ),
            (
                "Cass. soc., 21 décembre 2007, n° 06-12.769",
                "JURITEXT000017739957",
                (2007, 12, 21),
                "CHAMBRE_MIXTE",
                "Cited chamber SOC differs from the official record (MIXTE).",
            ),
        ] {
            let corpus = FakeCorpus::new();
            corpus.push_find_by_number(Ok(vec![hit(Fond::Juri, id)]));
            corpus.push_get_decision(Ok(Decision {
                decision_id: DecisionId::parse(id).unwrap(),
                ecli: None,
                num: None,
                juridiction: None,
                formation: Some(formation.to_owned()),
                solution: None,
                numero_affaire: vec![],
                date_texte: CivilDate::new(date.0, date.1, date.2),
                nature: Some("ARRET".to_owned()),
                title: None,
                text: LongText::from_plain("Texte de la décision.").unwrap(),
                official_summary: OfficialSummary::default(),
                cited_case_law: None,
                applied_texts: vec![],
            }));
            let verification = verify(&corpus, PisteEnvironment::Production, text).await.unwrap();
            let check = &verification.checks[0];
            assert_eq!(check.status, CheckStatus::Invalid, "{text}");
            assert_eq!(note_text(check), expected, "{text}");
            assert_eq!(check.resolved.as_ref().unwrap().id.as_str(), id);
        }
    }

    #[tokio::test]
    async fn n07_year_mismatch_is_invalid_with_the_real_law() {
        let corpus = FakeCorpus::new();
        let mut law = hit(Fond::Loda, "LEGITEXT000033202746");
        law.nature = Some("LOI".to_owned());
        law.date = CivilDate::new(2016, 10, 7);
        law.title = Some("LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique".into());
        corpus.push_find_by_number(Ok(vec![law]));
        let verification =
            verify(&corpus, PisteEnvironment::Production, "loi n° 2016-1321 du 7 octobre 2015")
                .await
                .unwrap();
        let check = &verification.checks[0];
        assert_eq!(check.status, CheckStatus::Invalid);
        assert_eq!(check.resolved.as_ref().unwrap().id.as_str(), "LEGITEXT000033202746");
        assert_eq!(
            note_text(check),
            "Cited date 2015-10-07 differs from the official record (2016-10-07)."
        );
        assert!(matches!(
            &corpus.seen_requests()[0],
            PortRequest::FindByNumber(query)
                if query.fond == Fond::Loda && query.value.as_str() == "2016-1321"
        ));
    }

    /// Repros de ground_truth-1 et de resolve-2 : la loi de 1978 n'est indexée que sous
    /// `78-17` (sandbox) et sa date citée est celle de la signature, pas de l'entrée en vigueur.
    #[tokio::test]
    async fn a_pre_2000_law_is_verified_by_its_short_number_and_its_signature_date() {
        let corpus = FakeCorpus::new();
        let mut law = hit(Fond::Loda, "LEGITEXT000006068624");
        law.nature = Some("LOI".to_owned());
        law.date = CivilDate::new(1978, 1, 7);
        law.title = Some(
            "Loi n° 78-17 du 6 janvier 1978 relative à l'informatique, aux fichiers et aux libertés"
                .into(),
        );
        corpus.push_find_by_number(Ok(vec![law]));
        let verification =
            verify(&corpus, PisteEnvironment::Production, "loi n° 78-17 du 6 janvier 1978")
                .await
                .unwrap();
        let check = &verification.checks[0];
        assert_eq!(check.status, CheckStatus::Verified);
        assert_eq!(note_text(check), "");
        assert_eq!(check.resolved.as_ref().unwrap().id.as_str(), "LEGITEXT000006068624");
        assert!(matches!(
            &corpus.seen_requests()[0],
            PortRequest::FindByNumber(query)
                if query.fond == Fond::Loda && query.value.as_str() == "78-17"
        ));
    }

    #[tokio::test]
    async fn n25_renumbered_article_is_verified_but_the_quote_differs() {
        let corpus = FakeCorpus::new();
        codes(&corpus);
        corpus.push_find_article(Ok(article(
            6_419_288,
            Some("1382"),
            CivilDate::new(2016, 10, 1),
            "Chacun a droit au respect de sa vie privée.",
        )));
        let text = "la responsabilité délictuelle de l'article 1382 du code civil : « Tout fait \
                    quelconque de l'homme, qui cause à autrui un dommage, oblige celui par la \
                    faute duquel il est arrivé à le réparer »";
        let verification = verify(&corpus, PisteEnvironment::Production, text).await.unwrap();
        let check = &verification.checks[0];
        assert_eq!(check.status, CheckStatus::Verified);
        assert_eq!(check.quote, Some(QuoteCheck::QuoteMismatch));
        assert_eq!(
            note_text(check),
            "Quoted text not found in the official text. Current version in force since 2016-10-01."
        );
        assert_eq!(check.resolved.as_ref().unwrap().id.as_str(), "LEGIARTI000006419288");
    }

    /// Sans note de version, un extrait infidèle porte à lui seul la note `QuoteMismatch`, et un
    /// extrait fidèle n'en porte aucune.
    #[tokio::test]
    async fn a_quote_mismatch_is_noted_only_when_nothing_else_is() {
        for (quoted, expected_quote, expected_note) in [
            (
                "Tout fait quelconque de l'homme",
                QuoteCheck::QuoteMismatch,
                "Quoted text not found in the official text.",
            ),
            ("Chacun a droit au respect de sa vie privée", QuoteCheck::Match, ""),
        ] {
            let corpus = FakeCorpus::new();
            codes(&corpus);
            // `Today` et pas de début de version : l'issue vérifiée n'a pas de note.
            corpus.push_find_article(Ok(article(
                6_419_288,
                Some("9"),
                None,
                "Chacun a droit au respect de sa vie privée.",
            )));
            let text = format!("Selon l'article 9 du code civil, « {quoted} »");
            let verification = verify(&corpus, PisteEnvironment::Production, &text).await.unwrap();
            let check = &verification.checks[0];
            assert_eq!(check.status, CheckStatus::Verified, "{quoted}");
            assert_eq!(check.quote, Some(expected_quote), "{quoted}");
            assert_eq!(note_text(check), expected_note, "{quoted}");
        }
    }

    /// Repro de verify_citations-7 : l'art. 1240 en vigueur depuis 2016-10-01, cité avec un extrait
    /// faux, ne garde plus la seule note de version. Jumeau : l'extrait fidèle garde la note de
    /// version seule.
    #[tokio::test]
    async fn a_false_quote_on_a_current_article_leads_the_version_note() {
        for (quoted, expected_quote, expected_note) in [
            (
                "Le chat est un animal domestique",
                QuoteCheck::QuoteMismatch,
                "Quoted text not found in the official text. Current version in force since 2016-10-01.",
            ),
            (
                "Tout fait quelconque de l'homme",
                QuoteCheck::Match,
                "Current version in force since 2016-10-01.",
            ),
        ] {
            let corpus = FakeCorpus::new();
            codes(&corpus);
            corpus.push_find_article(Ok(article(
                32_041_571,
                Some("1240"),
                CivilDate::new(2016, 10, 1),
                "Tout fait quelconque de l'homme, qui cause à autrui un dommage, oblige celui par \
                 la faute duquel il est arrivé à le réparer.",
            )));
            let text = format!("Art. 1240 C. civ. : « {quoted}. »");
            let verification = verify(&corpus, PisteEnvironment::Production, &text).await.unwrap();
            let check = &verification.checks[0];
            assert_eq!(check.status, CheckStatus::Verified, "{quoted}");
            assert_eq!(check.quote, Some(expected_quote), "{quoted}");
            assert_eq!(note_text(check), expected_note, "{quoted}");
        }
    }

    fn cassation_decision(id: &str, formation: &str, date: (u16, u8, u8)) -> Decision {
        Decision {
            decision_id: DecisionId::parse(id).unwrap(),
            ecli: None,
            num: None,
            juridiction: None,
            formation: Some(formation.to_owned()),
            solution: None,
            numero_affaire: vec![],
            date_texte: CivilDate::new(date.0, date.1, date.2),
            nature: Some("ARRET".to_owned()),
            title: None,
            text: LongText::from_plain("Texte de la décision.").unwrap(),
            official_summary: OfficialSummary::default(),
            cited_case_law: None,
            applied_texts: vec![],
        }
    }

    /// Le statut `invalid` garde sa vraie raison en tête, même avec un extrait faux : la note de
    /// l'écart de chambre n'est pas enveloppée et reste dans les 200 caractères.
    #[tokio::test]
    async fn a_hard_field_gap_stays_ahead_of_the_quote_gap() {
        let corpus = FakeCorpus::new();
        let id = "JURITEXT000017739957";
        corpus.push_find_by_number(Ok(vec![hit(Fond::Juri, id)]));
        corpus.push_get_decision(Ok(cassation_decision(id, "CHAMBRE_MIXTE", (2007, 12, 21))));
        let text = "Cass. soc., 21 décembre 2007, n° 06-12.769 : « Phrase qui ne figure pas ici »";
        let verification = verify(&corpus, PisteEnvironment::Production, text).await.unwrap();
        let check = &verification.checks[0];
        assert_eq!(check.status, CheckStatus::Invalid);
        assert_eq!(check.quote, Some(QuoteCheck::QuoteMismatch));
        let note = note_text(check);
        assert_eq!(note, "Cited chamber SOC differs from the official record (MIXTE).");
        assert!(note.chars().count() <= 200);
    }

    /// `ChamberNotCompared` est une note de statut `verified` : l'écart d'extrait passe devant,
    /// et sans écart la note reste seule.
    #[tokio::test]
    async fn the_chamber_not_compared_note_does_not_hide_a_quote_gap() {
        for (quoted, expected_quote, expected_note) in [
            (
                "Phrase qui ne figure pas ici",
                QuoteCheck::QuoteMismatch,
                "Quoted text not found in the official text. The cited chamber could not be \
                 compared: the official record gives a formation that is not a modern Cour de \
                 cassation chamber.",
            ),
            (
                "Texte de la décision",
                QuoteCheck::Match,
                "The cited chamber could not be compared: the official record gives a formation \
                 that is not a modern Cour de cassation chamber.",
            ),
        ] {
            let corpus = FakeCorpus::new();
            let id = "JURITEXT000017739957";
            corpus.push_find_by_number(Ok(vec![hit(Fond::Juri, id)]));
            corpus.push_get_decision(Ok(cassation_decision(id, "CT0196", (2007, 12, 21))));
            let text = format!("Cass. soc., 21 décembre 2007, n° 06-12.769 : « {quoted} »");
            let verification = verify(&corpus, PisteEnvironment::Production, &text).await.unwrap();
            let check = &verification.checks[0];
            assert_eq!(check.status, CheckStatus::Verified, "{quoted}");
            assert_eq!(check.quote, Some(expected_quote), "{quoted}");
            let note = note_text(check);
            assert_eq!(note, expected_note, "{quoted}");
            assert!(note.chars().count() <= 200, "{note}");
        }
    }

    /// La date du jour est transmise à l'analyse : une année future est un fait impossible,
    /// décidé sans appel au corpus (drapeau `annee_future`).
    #[tokio::test]
    async fn a_future_year_is_decided_offline_from_the_run_date() {
        let corpus = FakeCorpus::new();
        let verification =
            verify(&corpus, PisteEnvironment::Production, "ECLI:FR:CCASS:2099:C100399")
                .await
                .unwrap();
        assert_eq!(statuses(&verification), vec![CheckStatus::Invalid]);
        assert!(note_text(&verification.checks[0]).contains("in the future"));
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn n25_a_faithful_quote_matches() {
        let corpus = FakeCorpus::new();
        codes(&corpus);
        corpus.push_find_article(Ok(article(
            6_419_288,
            Some("9"),
            CivilDate::new(1994, 7, 30),
            "Chacun a droit au respect de sa vie privée.",
        )));
        let text =
            "Selon l'article 9 du code civil, « Chacun a droit au respect de sa vie privée »";
        let verification = verify(&corpus, PisteEnvironment::Production, text).await.unwrap();
        assert_eq!(verification.checks[0].status, CheckStatus::Verified);
        assert_eq!(verification.checks[0].quote, Some(QuoteCheck::Match));
    }

    #[tokio::test]
    async fn n20_n02_and_n32_are_decided_without_any_call() {
        for (text, expected) in [
            ("Crim., 28 mars 2018", CheckStatus::Ambiguous),
            ("Le préavis est fixé par L. 1234-5.", CheckStatus::Ambiguous),
            (
                "à la lumière de la directive 85/374/CEE du Conseil du 25 juillet 1985",
                CheckStatus::Unresolvable,
            ),
        ] {
            let corpus = FakeCorpus::new();
            let verification = verify(&corpus, PisteEnvironment::Production, text).await.unwrap();
            assert_eq!(statuses(&verification), vec![expected], "{text}");
            assert_eq!(corpus.calls(), 0, "{text}");
        }
    }

    #[tokio::test]
    async fn p110_court_of_appeal_not_found_is_out_of_coverage_never_not_found() {
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![]));
        let verification = verify(
            &corpus,
            PisteEnvironment::Production,
            "Cour d'appel de Paris, Pôle 1 - Chambre 12, 23 juin 2025, n°24/15215",
        )
        .await
        .unwrap();
        let check = &verification.checks[0];
        assert_eq!(check.status, CheckStatus::Unresolvable);
        assert_eq!(
            note_text(check),
            "Légifrance only holds a selection of Cour d'appel decisions; absence is not proof \
             (see fr_coverage)."
        );
        assert!(matches!(
            &corpus.seen_requests()[0],
            PortRequest::FindByNumber(query)
                if query.fond == Fond::Juri && query.value.as_str() == "24/15215"
        ));
    }

    fn find_travail(num: &str) -> PortRequest {
        PortRequest::FindArticle(
            LegiTextId::parse(TRAVAIL).unwrap(),
            num.to_owned(),
            ArticleAt::Today(today()),
        )
    }

    /// L2-07 (verify_citations-9) : une plage est une seule citation, dont les deux bornes sont
    /// lues ; elle est `verified` avec la note qui dit que le milieu n'est pas lu, et l'extrait
    /// cité n'est pas comparé.
    #[tokio::test]
    async fn a_range_is_checked_at_both_ends_and_says_so() {
        let corpus = FakeCorpus::new();
        codes(&corpus);
        corpus.push_find_article(Ok(article(1, Some("L. 1234-9"), None, "Premier.")));
        corpus.push_find_article(Ok(article(2, Some("L. 1234-11"), None, "Dernier.")));
        let text = "les articles L. 1234-9 à L. 1234-11 du code du travail";
        let verification = verify(&corpus, PisteEnvironment::Production, text).await.unwrap();
        assert_eq!(verification.found, 1);
        assert_eq!(verification.checks.len(), 1);
        let check = &verification.checks[0];
        assert_eq!(check.status, CheckStatus::Verified);
        assert_eq!(
            note_text(check),
            "Only the first and last articles of the range were checked; the articles between were not."
        );
        assert_eq!(check.resolved.as_ref().unwrap().id.as_str(), "LEGIARTI000000000001");
        assert_eq!(check.quote, None);
        assert_eq!(
            corpus.seen_requests(),
            vec![PortRequest::ListCodes, find_travail("L1234-9"), find_travail("L1234-11")]
        );
    }

    /// Une dernière borne inventée n'est plus verified : `ambiguous`, jamais `not_found`.
    #[tokio::test]
    async fn a_range_ending_at_a_missing_article_is_ambiguous_not_not_found() {
        let corpus = FakeCorpus::new();
        codes(&corpus);
        corpus.push_find_article(Ok(article(1, Some("L. 1234-9"), None, "Premier.")));
        corpus.push_find_article(Err(not_found()));
        let text = "les articles L. 1234-9 à L. 9999-99 du code du travail";
        let verification = verify(&corpus, PisteEnvironment::Production, text).await.unwrap();
        let check = &verification.checks[0];
        assert_eq!(check.status, CheckStatus::Ambiguous);
        assert!(check.resolved.is_none());
        assert_eq!(
            note_text(check),
            "The first article of the range exists, but its last, article L9999-99, was not found; check the number or the date."
        );
    }

    /// Une première borne absente : un seul appel, l'issue d'un article seul (`not_found`).
    #[tokio::test]
    async fn a_range_starting_at_a_missing_article_is_not_found_after_one_call() {
        let corpus = FakeCorpus::new();
        codes(&corpus);
        corpus.push_find_article(Err(not_found()));
        let text = "les articles L. 9998-1 à L. 9999-99 du code du travail";
        let verification = verify(&corpus, PisteEnvironment::Production, text).await.unwrap();
        assert_eq!(verification.checks[0].status, CheckStatus::NotFound);
        assert_eq!(corpus.calls_of("find_article"), 1);
    }

    /// Jumeau : « 1240 et 1241 » reste une liste, une citation et un appel par numéro, chacune
    /// verified de confiance haute (sans la note des plages).
    #[tokio::test]
    async fn a_list_of_two_articles_is_not_a_range() {
        let corpus = FakeCorpus::new();
        codes(&corpus);
        corpus.push_find_article(Ok(article(1, Some("1240"), None, "Un.")));
        corpus.push_find_article(Ok(article(2, Some("1241"), None, "Deux.")));
        let text = "les articles 1240 et 1241 du code civil";
        let verification = verify(&corpus, PisteEnvironment::Production, text).await.unwrap();
        assert_eq!(statuses(&verification), vec![CheckStatus::Verified, CheckStatus::Verified]);
        assert_eq!(corpus.calls_of("find_article"), 2);
        assert!(verification.checks.iter().all(|c| !note_text(c).contains("range")));
    }

    const TWO_ARTICLES: &str = "article 9 du code civil. Voir aussi l'article 10 du code civil.";

    #[tokio::test]
    async fn upstream_error_on_one_of_two_leaves_the_other_verified() {
        let corpus = FakeCorpus::new();
        codes(&corpus);
        corpus.push_find_article(Ok(article(1, Some("9"), None, "Texte.")));
        corpus.push_find_article(Err(upstream()));
        let verification =
            verify(&corpus, PisteEnvironment::Production, TWO_ARTICLES).await.unwrap();
        assert_eq!(statuses(&verification), vec![CheckStatus::Verified, CheckStatus::Unresolvable]);
        assert_eq!(note_text(&verification.checks[1]), "Not checked: upstream error.");
    }

    #[tokio::test]
    async fn upstream_error_on_every_citation_fails_the_call() {
        let corpus = FakeCorpus::new();
        codes(&corpus);
        corpus.push_find_article(Err(upstream()));
        corpus.push_find_article(Err(upstream()));
        let error = verify(&corpus, PisteEnvironment::Production, TWO_ARTICLES).await.unwrap_err();
        assert_eq!(error, upstream());
    }

    #[tokio::test]
    async fn a_failing_code_list_fails_a_call_made_only_of_code_articles() {
        let corpus = FakeCorpus::new();
        corpus.push_list_codes(Err(upstream()));
        let error = verify(&corpus, PisteEnvironment::Production, TWO_ARTICLES).await.unwrap_err();
        assert_eq!(error, upstream());
        assert_eq!(corpus.calls_of("find_article"), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn stops_at_80_percent_of_deadline_with_note() {
        // Budget : 80 % de 10 s = 8 s. Chaque appel dure 3 s : la liste des codes arrive à 3 s, les
        // quatre premiers articles à 6 s ; les quatre suivants dépasseraient 8 s et sont
        // interrompus ; les quatre derniers ne sont jamais lancés.
        let corpus = FakeCorpus::new().with_delay(Duration::from_secs(3));
        codes(&corpus);
        for n in 1..=12 {
            corpus.push_find_article(Ok(article(n, None, None, "Texte.")));
        }
        let text =
            (1..=12).map(|n| format!("article {n} du code civil")).collect::<Vec<_>>().join(" ; ");
        let run = Run { remaining: Duration::from_secs(10), ..run(PisteEnvironment::Production) };
        let verification = verify_text(&corpus, run, &text).await.unwrap();
        assert_eq!(verification.checks.len(), 12);
        let verified = verification.checks.iter().filter(|c| c.status == CheckStatus::Verified);
        assert_eq!(verified.count(), 4);
        let late: Vec<&CitationCheck> =
            verification.checks.iter().filter(|c| c.status == CheckStatus::Unresolvable).collect();
        assert_eq!(late.len(), 8);
        assert!(late.iter().all(|check| note_text(check) == "Not checked: time budget."));
        assert_eq!(corpus.calls_of("find_article"), 8);
    }

    #[tokio::test]
    async fn duplicates_share_one_resolution_and_the_code_list_is_read_once() {
        let corpus = FakeCorpus::new();
        codes(&corpus);
        for (n, num) in [(1, "L122-4"), (2, "L122-14"), (3, "L122-14-3")] {
            corpus.push_find_article(Ok(article(n, Some(num), None, "Texte.")));
        }
        let text = "Vu les articles L. 122-4, L. 122-14 et L. 122-14-3 du Code du travail ; voir \
                    encore l'article L. 122-4 du Code du travail.";
        let verification = verify(&corpus, PisteEnvironment::Production, text).await.unwrap();
        assert_eq!(verification.found, 4);
        assert_eq!(verification.checks.len(), 4);
        assert!(verification.checks.iter().all(|check| check.status == CheckStatus::Verified));
        assert_eq!(corpus.calls_of("list_codes"), 1);
        assert_eq!(corpus.calls_of("find_article"), 3);
        assert_eq!(verification.checks[0].resolved, verification.checks[3].resolved);
    }

    #[tokio::test(start_paused = true)]
    async fn concurrency_is_buffer_unordered_4() {
        let corpus = FakeCorpus::new().with_delay(Duration::from_secs(1));
        codes(&corpus);
        for n in 1..=50 {
            corpus.push_find_article(Ok(article(n, None, None, "Texte.")));
        }
        let text =
            (1..=50).map(|n| format!("article {n} du code civil")).collect::<Vec<_>>().join(" ; ");
        let verification = verify(&corpus, PisteEnvironment::Production, &text).await.unwrap();
        assert_eq!(verification.checks.len(), 50);
        assert!(verification.checks.iter().all(|check| check.status == CheckStatus::Verified));
        assert_eq!(corpus.max_concurrency(), 4);
        assert_eq!(corpus.calls_of("find_article"), 50);
    }

    #[tokio::test]
    async fn only_the_first_max_citations_are_checked() {
        let corpus = FakeCorpus::new();
        codes(&corpus);
        corpus.push_find_article(Ok(article(1, None, None, "Texte.")));
        let run = Run { max_citations: 1, ..run(PisteEnvironment::Production) };
        let verification = verify_text(&corpus, run, TWO_ARTICLES).await.unwrap();
        assert_eq!((verification.found, verification.checks.len()), (2, 1));
        assert_eq!(corpus.calls_of("find_article"), 1);
    }

    #[tokio::test]
    async fn a_text_without_citation_makes_no_call() {
        let corpus = FakeCorpus::new();
        let verification =
            verify(&corpus, PisteEnvironment::Production, "Bonjour tout le monde.").await.unwrap();
        assert_eq!((verification.found, verification.checks.len()), (0, 0));
        assert_eq!(corpus.calls(), 0);
    }
}
