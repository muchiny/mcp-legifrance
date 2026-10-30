//! Cas d'usage de la complétion de la variable `code` du gabarit
//! `legifrance://code/{code}/article/{num}` (v0.3, M7), son limiteur de débit et le calcul des
//! slugs de codes (05 §8.2, §8.4 ; 02 §6.2 ; 00 §7).
//!
//! # Règles
//!
//! - Seule la variable `code` est complétée ; toute autre variable donne une liste vide, sans
//!   appel au port ni jeton du limiteur.
//! - Débit : au plus 10 complétions par seconde et par processus ([`CompleteGate`], seau à jetons,
//!   MUST de `completion.mdx:207-214`). Au-delà, liste vide.
//! - Source : `list_codes()`, que l'adapter de cache sert 7 jours (au plus un appel amont par
//!   semaine, coalescé). Une erreur du port ou un délai dépassé donne une liste vide et un
//!   `warn!`, jamais une erreur.
//! - Valeurs : les slugs de codes dont le slug contient la saisie (le slug est le titre replié,
//!   donc la correspondance vaut « slug ou titre replié »), préfixes d'abord, puis ordre
//!   alphabétique ; 100 au plus (`CompletionInfo::MAX_VALUES`), avec le total des correspondances
//!   et `has_more`.
//!
//! La complétion n'est pas auditée (00 §7).

use std::{
    collections::{BTreeMap, btree_map::Entry},
    sync::{Mutex, PoisonError},
    time::Duration,
};

use lf_domain::{entity::CodeListing, status::LegalStatus};
use tokio::time::Instant;

use crate::{
    deadline::Deadline, failure::port_failure, input::CompleteInput,
    output::complete::CompleteOutput, ports::LegalCorpus, tools::ToolName,
};

/// Seule variable de gabarit complétée (00 §7).
pub(crate) const CODE_VARIABLE: &str = "code";

/// Nombre maximal de valeurs rendues (`CompletionInfo::MAX_VALUES`, `completion.mdx:142-147`).
pub(crate) const MAX_VALUES: usize = 100;

/// Complétions accordées par seconde et par processus (00 §7).
pub(crate) const COMPLETIONS_PER_SECOND: u64 = 10;

/// Caractères de la saisie qui comptent : la saisie vient du client, et un slug de code est court.
const MAX_TYPED_CHARS: usize = 200;

/// Coût d'une complétion, en millièmes de jeton.
const COST: u64 = 1_000;

/// Capacité du seau : une seconde de débit, en millièmes de jeton.
const CAPACITY: u64 = COMPLETIONS_PER_SECOND * COST;

/// Millièmes de jeton rendus par milliseconde écoulée (10 jetons par seconde).
const REFILL_PER_MS: u64 = COMPLETIONS_PER_SECOND;

/// Limiteur de débit des complétions : un seau à jetons de 10, rempli à 10 par seconde, un état que
/// la façade tient pour toute la vie du serveur (02 §6.2).
///
/// Les jetons se comptent en millièmes, sans flottant ; le seau est plein au premier appel.
#[derive(Debug)]
pub struct CompleteGate {
    bucket: Mutex<Bucket>,
}

/// État du seau.
#[derive(Debug)]
struct Bucket {
    /// Millièmes de jeton disponibles, au plus [`CAPACITY`].
    milli_tokens: u64,
    /// Instant jusqu'où le seau est rempli ; `None` avant le premier appel.
    refilled_until: Option<Instant>,
}

impl Default for CompleteGate {
    fn default() -> Self {
        Self::new()
    }
}

impl CompleteGate {
    /// Limiteur neuf : le seau est plein.
    #[must_use]
    pub fn new() -> Self {
        Self { bucket: Mutex::new(Bucket { milli_tokens: CAPACITY, refilled_until: None }) }
    }

    /// Accorde une complétion maintenant, ou la refuse.
    pub(crate) fn allow(&self) -> bool {
        self.allow_at(Instant::now())
    }

    /// Accorde une complétion à l'instant `now`, ou la refuse. Le reste d'une milliseconde non
    /// comptée est gardé pour l'appel suivant : des appels très rapprochés ne privent pas le seau
    /// de son remplissage.
    pub(crate) fn allow_at(&self, now: Instant) -> bool {
        let mut bucket = self.bucket.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(until) = bucket.refilled_until {
            let elapsed = now.saturating_duration_since(until);
            let elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
            let refilled = elapsed_ms.saturating_mul(REFILL_PER_MS);
            bucket.milli_tokens = bucket.milli_tokens.saturating_add(refilled).min(CAPACITY);
            bucket.refilled_until = until.checked_add(Duration::from_millis(elapsed_ms));
        } else {
            bucket.refilled_until = Some(now);
        }
        match bucket.milli_tokens.checked_sub(COST) {
            Some(left) => {
                bucket.milli_tokens = left;
                true
            }
            None => false,
        }
    }
}

/// Cas d'usage de la complétion de `code`.
pub(crate) struct CompleteUseCase<'a, C: LegalCorpus> {
    corpus: &'a C,
    gate: &'a CompleteGate,
    deadline: Duration,
}

impl<'a, C: LegalCorpus> CompleteUseCase<'a, C> {
    /// Cas d'usage sur `corpus`, limité par `gate` ; `deadline` borne l'appel au port.
    pub(crate) fn new(corpus: &'a C, gate: &'a CompleteGate, deadline: Duration) -> Self {
        Self { corpus, gate, deadline }
    }

    /// Les slugs de codes qui correspondent à la saisie. Jamais d'erreur : une variable autre que
    /// `code`, un débit dépassé, une erreur du port ou un délai donnent la liste vide.
    pub(crate) async fn execute(&self, input: CompleteInput) -> CompleteOutput {
        if input.argument.as_deref() != Some(CODE_VARIABLE) {
            return CompleteOutput::empty();
        }
        if !self.gate.allow() {
            tracing::debug!("completion rate limit reached; answering an empty list");
            return CompleteOutput::empty();
        }
        let listing = Deadline::after(self.deadline).scope(self.corpus.list_codes());
        match tokio::time::timeout(self.deadline, listing).await {
            Ok(Ok(codes)) => ranked(&codes, input.value.as_deref().unwrap_or_default()),
            Ok(Err(error)) => {
                drop(port_failure(ToolName::RESOURCE_CODE_ARTICLE, "", &error));
                CompleteOutput::empty()
            }
            Err(_elapsed) => {
                tracing::warn!("list_codes timed out; answering an empty completion");
                CompleteOutput::empty()
            }
        }
    }
}

/// Les slugs correspondant à `typed`, préfixes d'abord.
fn ranked(codes: &[CodeListing], typed: &str) -> CompleteOutput {
    let needle = slug(&typed.chars().take(MAX_TYPED_CHARS).collect::<String>());
    // `code_slugs` est trié par slug ; le tri par rang est stable, donc l'ordre alphabétique reste
    // celui de chaque rang.
    let mut matches: Vec<(bool, String)> = code_slugs(codes)
        .into_keys()
        .filter(|candidate| candidate.contains(&needle))
        .map(|candidate| (!candidate.starts_with(&needle), candidate))
        .collect();
    matches.sort_by_key(|(not_a_prefix, _)| *not_a_prefix);
    let total = matches.len();
    let values = matches.into_iter().take(MAX_VALUES).map(|(_, candidate)| candidate).collect();
    CompleteOutput { values, total: Some(total), has_more: total > MAX_VALUES }
}

/// Les codes de la liste par slug, triés par slug. Deux titres de même slug : celui qui est en
/// vigueur l'emporte, sinon le premier de la liste (l'unicité des slugs sur la liste réelle est
/// non vérifiée, V-123). Un titre sans slug est écarté.
pub(crate) fn code_slugs(codes: &[CodeListing]) -> BTreeMap<String, &CodeListing> {
    let mut by_slug: BTreeMap<String, &CodeListing> = BTreeMap::new();
    for code in codes {
        let key = slug(&code.title);
        if key.is_empty() {
            continue;
        }
        match by_slug.entry(key) {
            Entry::Vacant(slot) => {
                slot.insert(code);
            }
            Entry::Occupied(mut slot) => {
                if in_force(code) && !in_force(slot.get()) {
                    slot.insert(code);
                }
            }
        }
    }
    by_slug
}

/// Le code est en vigueur.
fn in_force(code: &CodeListing) -> bool {
    code.etat == Some(LegalStatus::Vigueur)
}

/// Slug d'un titre de code : minuscules, accents français repliés (`œ` en `oe`), tout caractère
/// qui n'est pas une lettre ou un chiffre ASCII devient un tiret, les tirets se confondent et
/// ceux de bord tombent. `Code général des impôts` donne `code-general-des-impots`, `Code de
/// l'environnement` donne `code-de-l-environnement` (05 §8.2).
pub(crate) fn slug(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut separate = false;
    for c in title.chars().flat_map(char::to_lowercase) {
        let mut buffer = [0_u8; 4];
        let piece =
            if c.is_ascii_alphanumeric() { Some(&*c.encode_utf8(&mut buffer)) } else { folded(c) };
        match piece {
            Some(piece) => {
                if separate && !out.is_empty() {
                    out.push('-');
                }
                separate = false;
                out.push_str(piece);
            }
            None => separate = true,
        }
    }
    out
}

/// Repli ASCII d'une minuscule accentuée du français.
const fn folded(c: char) -> Option<&'static str> {
    Some(match c {
        'à' | 'â' | 'ä' => "a",
        'ç' => "c",
        'é' | 'è' | 'ê' | 'ë' => "e",
        'î' | 'ï' => "i",
        'ô' | 'ö' => "o",
        'ù' | 'û' | 'ü' => "u",
        'ÿ' => "y",
        'œ' => "oe",
        'æ' => "ae",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use lf_domain::id::LegiTextId;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        ports::{ConfigKind, CorpusError},
        testing::FakeCorpus,
    };

    const DEADLINE: Duration = Duration::from_secs(60);

    fn listing(title: &str, etat: Option<LegalStatus>) -> CodeListing {
        CodeListing {
            id: LegiTextId::parse("LEGITEXT000006070721").unwrap(),
            title: title.into(),
            etat,
        }
    }

    fn listings(titles: &[&str]) -> Vec<CodeListing> {
        titles.iter().map(|title| listing(title, Some(LegalStatus::Vigueur))).collect()
    }

    fn input(argument: &str, value: &str) -> CompleteInput {
        CompleteInput { argument: Some(argument.to_owned()), value: Some(value.to_owned()) }
    }

    async fn run(corpus: &FakeCorpus, gate: &CompleteGate, input: CompleteInput) -> CompleteOutput {
        CompleteUseCase::new(corpus, gate, DEADLINE).execute(input).await
    }

    // ---- slug -------------------------------------------------------------------------------

    #[test]
    fn slug_folds_french_titles() {
        let expected = [
            ("Code civil", "code-civil"),
            ("Code général des impôts", "code-general-des-impots"),
            ("Code de l'environnement", "code-de-l-environnement"),
            ("Code général des impôts, annexe I", "code-general-des-impots-annexe-i"),
            ("Code de la sécurité sociale", "code-de-la-securite-sociale"),
            ("Nouveau Code de procédure civile", "nouveau-code-de-procedure-civile"),
            ("Œuvres — Æther ÀÂÄ ÇÉÈÊË ÎÏ ÔÖ ÙÛÜ Ÿ", "oeuvres-aether-aaa-ceeee-ii-oo-uuu-y"),
            ("  --Code--  du   travail ?!", "code-du-travail"),
            ("Livre 2e", "livre-2e"),
        ];
        for (title, wanted) in expected {
            assert_eq!(slug(title), wanted, "{title}");
        }
        // Rien de latin dans le titre : pas de slug.
        for title in ["", "   ", "—«»", "日本"] {
            assert_eq!(slug(title), "", "{title:?}");
        }
    }

    #[test]
    fn code_slugs_prefer_the_code_in_force_and_skip_empty_slugs() {
        let abroge = listing("Code civil", Some(LegalStatus::Abroge));
        let vigueur = listing("Code Civil", Some(LegalStatus::Vigueur));
        let unknown = listing("Code civil", None);
        let nameless = listing("—", Some(LegalStatus::Vigueur));
        let codes = [abroge.clone(), vigueur.clone(), unknown.clone(), nameless];

        let slugs = code_slugs(&codes);

        assert_eq!(slugs.len(), 1);
        assert_eq!(slugs["code-civil"], &vigueur);
        // Deux codes hors vigueur : le premier reste.
        let pair = [abroge.clone(), unknown];
        let first = code_slugs(&pair);
        assert_eq!(first["code-civil"], &abroge);
        // Un code en vigueur déjà retenu n'est pas remplacé par un autre en vigueur.
        let other = listing("Code civil", Some(LegalStatus::Vigueur));
        let both = [vigueur.clone(), other];
        let kept = code_slugs(&both);
        assert_eq!(kept["code-civil"], &vigueur);
    }

    // ---- ranking ----------------------------------------------------------------------------

    #[test]
    fn prefixes_come_first_then_alphabetical() {
        let codes = listings(&[
            "Code de procédure civile",
            "Code civil",
            "Code du travail",
            "Livre des procédures fiscales",
            "Code des civils",
        ]);

        let by_prefix = ranked(&codes, "code d");
        assert_eq!(
            by_prefix.values,
            ["code-de-procedure-civile", "code-des-civils", "code-du-travail"]
        );

        // « civ » : aucun préfixe, trois correspondances, par ordre alphabétique.
        let by_word = ranked(&codes, "civ");
        assert_eq!(by_word.values, ["code-civil", "code-de-procedure-civile", "code-des-civils"]);

        // Préfixe d'abord, même quand l'ordre alphabétique dirait le contraire.
        let mixed = listings(&["Annexe du code civil", "Code civil de test"]);
        assert_eq!(ranked(&mixed, "code").values, ["code-civil-de-test", "annexe-du-code-civil"]);
    }

    #[test]
    fn the_typed_text_is_folded_like_the_slug() {
        let codes = listings(&["Code général des impôts", "Code civil"]);
        for typed in ["général", "GENERAL", "code-gen", "code gén", "  impôts  "] {
            assert_eq!(ranked(&codes, typed).values, ["code-general-des-impots"], "{typed:?}");
        }
        // Une saisie sans lettre ni chiffre propose tout.
        assert_eq!(ranked(&codes, "").total, Some(2));
        assert_eq!(ranked(&codes, " - ").total, Some(2));
        assert_eq!(
            ranked(&codes, "zzz"),
            CompleteOutput { values: vec![], total: Some(0), has_more: false }
        );
    }

    #[test]
    fn a_very_long_typed_text_is_cut() {
        let codes = listings(&["Code civil"]);
        let typed = format!("code civil{}", "x".repeat(10_000));
        // La saisie est coupée à 200 caractères : « xxx… » ne correspond à aucun slug.
        assert_eq!(ranked(&codes, &typed).values, Vec::<String>::new());
        let typed = format!("code civil{}", " ".repeat(10_000));
        assert_eq!(ranked(&codes, &typed).values, ["code-civil"]);
    }

    // ---- use case ---------------------------------------------------------------------------

    #[tokio::test]
    async fn completion_code_only_max_100_values() {
        let corpus = FakeCorpus::new();
        let titles: Vec<String> = (0..150).map(|n| format!("Code numéro {n:03}")).collect();
        let many: Vec<CodeListing> =
            titles.iter().map(|title| listing(title, Some(LegalStatus::Vigueur))).collect();
        corpus.push_list_codes(Ok(many.clone()));
        let gate = CompleteGate::new();

        // `code` : au plus 100 valeurs, le total complet et l'indication qu'il en reste.
        let output = run(&corpus, &gate, input("code", "code")).await;
        assert_eq!(output.values.len(), 100);
        assert_eq!(output.values[0], "code-numero-000");
        assert_eq!(output.values[99], "code-numero-099");
        assert_eq!(output.total, Some(150));
        assert!(output.has_more);

        // Une saisie plus précise rend moins que 100 valeurs : plus rien en réserve.
        corpus.push_list_codes(Ok(many));
        let narrow = run(&corpus, &gate, input("code", "numero-14")).await;
        assert_eq!(narrow.values.len(), 10);
        assert_eq!(narrow.total, Some(10));
        assert!(!narrow.has_more);

        // Toute autre variable : liste vide, sans appel au port.
        let calls = corpus.calls();
        for argument in ["num", "article_id", "decision_id", "text_id", "fond", "Code", ""] {
            assert_eq!(run(&corpus, &gate, input(argument, "c")).await, CompleteOutput::empty());
        }
        let anonymous = CompleteInput { argument: None, value: Some("c".to_owned()) };
        assert_eq!(run(&corpus, &gate, anonymous).await, CompleteOutput::empty());
        assert_eq!(corpus.calls(), calls);
    }

    #[tokio::test]
    async fn a_missing_value_proposes_every_code() {
        let corpus = FakeCorpus::new();
        corpus.push_list_codes(Ok(listings(&["Code du travail", "Code civil"])));
        let blank = CompleteInput { argument: Some("code".to_owned()), value: None };

        let output = run(&corpus, &CompleteGate::new(), blank).await;

        assert_eq!(output.values, ["code-civil", "code-du-travail"]);
        assert_eq!(output.total, Some(2));
    }

    #[tokio::test(start_paused = true)]
    async fn completion_rate_limited_10_per_second() {
        let corpus = FakeCorpus::new();
        let gate = CompleteGate::new();
        for _ in 0..25 {
            corpus.push_list_codes(Ok(listings(&["Code civil"])));
        }

        // Dix complétions passent dans la même seconde, la onzième est refusée sans appel amont.
        for attempt in 0..10 {
            let output = run(&corpus, &gate, input("code", "")).await;
            assert_eq!(output.values, ["code-civil"], "complétion {attempt}");
        }
        assert_eq!(run(&corpus, &gate, input("code", "")).await, CompleteOutput::empty());
        assert_eq!(run(&corpus, &gate, input("code", "")).await, CompleteOutput::empty());
        assert_eq!(corpus.calls_of("list_codes"), 10);

        // Cent millisecondes rendent un jeton (10 par seconde), pas deux.
        tokio::time::advance(Duration::from_millis(100)).await;
        assert_eq!(run(&corpus, &gate, input("code", "")).await.values, ["code-civil"]);
        assert_eq!(run(&corpus, &gate, input("code", "")).await, CompleteOutput::empty());

        // Une seconde pleine remplit le seau : dix de plus, pas onze.
        tokio::time::advance(Duration::from_secs(1)).await;
        for _ in 0..10 {
            assert_eq!(run(&corpus, &gate, input("code", "")).await.values, ["code-civil"]);
        }
        assert_eq!(run(&corpus, &gate, input("code", "")).await, CompleteOutput::empty());
        assert_eq!(corpus.calls_of("list_codes"), 21);
    }

    #[tokio::test]
    async fn an_other_variable_does_not_spend_a_token() {
        let corpus = FakeCorpus::new();
        let gate = CompleteGate::new();
        for _ in 0..50 {
            run(&corpus, &gate, input("num", "9")).await;
        }
        assert!(gate.allow_at(Instant::now()));
    }

    #[test]
    fn the_gate_keeps_the_sub_millisecond_remainder() {
        let gate = CompleteGate::new();
        let start = Instant::now();
        for _ in 0..10 {
            assert!(gate.allow_at(start));
        }
        assert!(!gate.allow_at(start));
        // 100 appels espacés de 0,5 ms : 50 ms en tout, soit un demi-jeton. Aucune milliseconde
        // n'est perdue à l'arrondi, donc le seau est à 500 millièmes de jeton au bout.
        for step in 1..=100_u32 {
            assert!(!gate.allow_at(start + Duration::from_micros(500) * step));
        }
        // 50 ms de plus complètent le premier jeton, pas avant.
        assert!(!gate.allow_at(start + Duration::from_millis(99)));
        assert!(gate.allow_at(start + Duration::from_millis(100)));
        assert!(!gate.allow_at(start + Duration::from_millis(100)));
    }

    #[test]
    fn the_bucket_never_holds_more_than_ten_tokens_and_survives_extremes() {
        let gate = CompleteGate::new();
        let start = Instant::now();
        assert!(gate.allow_at(start));
        // Une heure d'inactivité : dix complétions, pas plus.
        let later = start + Duration::from_secs(3_600);
        for _ in 0..10 {
            assert!(gate.allow_at(later));
        }
        assert!(!gate.allow_at(later));
        // Un instant antérieur au dernier (horloge qui recule) : rien ne se remplit, rien ne panique.
        assert!(!gate.allow_at(start));
        // Un instant démesurément lointain : saturation, seau plein.
        let far = later + Duration::from_secs(10 * 365 * 24 * 3_600);
        assert!(gate.allow_at(far));
    }

    #[expect(clippy::panic, reason = "empoisonne le mutex du seau pour éprouver sa reprise")]
    fn poison(gate: &CompleteGate) {
        let _guard = gate.bucket.lock().unwrap();
        panic!("poison the bucket");
    }

    #[test]
    fn the_gate_survives_a_poisoned_mutex() {
        let gate = CompleteGate::default();
        let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| poison(&gate)));
        assert!(poisoned.is_err());
        assert!(gate.bucket.is_poisoned());
        assert!(gate.allow());
    }

    #[test]
    fn the_gate_is_shareable_between_tasks() {
        fn assert_shareable<T: Send + Sync + 'static>(_: &T) {}
        assert_shareable(&CompleteGate::new());
        assert!(format!("{:?}", CompleteGate::default()).starts_with("CompleteGate"));
    }

    #[tokio::test]
    async fn a_port_error_gives_an_empty_list_not_an_error() {
        let corpus = FakeCorpus::new();
        let errors = [
            CorpusError::Timeout,
            CorpusError::NotFound { what: "/list/code".to_owned() },
            CorpusError::Config { kind: ConfigKind::CredentialsMissing, detail: String::new() },
        ];
        for error in errors {
            corpus.push_list_codes(Err(error));
            let output = run(&corpus, &CompleteGate::new(), input("code", "c")).await;
            assert_eq!(output, CompleteOutput::empty());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_slow_port_gives_an_empty_list_after_the_deadline() {
        let corpus = FakeCorpus::new().with_delay(Duration::from_secs(3_600));
        corpus.push_list_codes(Ok(listings(&["Code civil"])));
        let gate = CompleteGate::new();

        let started = Instant::now();
        let output = CompleteUseCase::new(&corpus, &gate, Duration::from_secs(5))
            .execute(input("code", ""))
            .await;

        assert_eq!(output, CompleteOutput::empty());
        assert_eq!(started.elapsed(), Duration::from_secs(5));
        // Le port a vu l'échéance : un adapter qui la lit ne programme rien au-delà.
        assert_eq!(corpus.seen_deadlines(), [Duration::from_secs(5)]);
    }
}
