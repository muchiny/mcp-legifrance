//! Moteur de résolution d'une citation (annexe B §7.1 à §7.6), partagé par `fr_verify_citations`
//! et, en V4, par `fr_resolve`.
//!
//! [`Resolver::resolve_ref`] prend une citation reconnue par le parseur et rend une
//! [`Resolution`] : un [`Outcome`] (vérifiée, en écart, plusieurs enregistrements, absente,
//! non résoluble, échec amont) et, quand le texte officiel a été lu, ce texte, que l'appelant
//! compare à l'extrait cité. Le moteur ne sait rien du transport ni du format de sortie.
//!
//! # Règles
//!
//! - **Identifiant lu, jamais construit** (§7.1) : tout [`ResolvedRecord`] vient d'une réponse du
//!   corpus. Une citation `LegifranceId` consulte l'identifiant tel quel.
//! - **Pas d'absence sans preuve** (§7.2) : [`Outcome::Absent`] n'est rendu que si la table
//!   [`policy::POLICY`](super::policy::POLICY) l'autorise pour la famille ; ailleurs, l'absence est
//!   `Unresolvable`. En bac à sable, [`Outcome::status`] la rend `unresolvable` (jamais `not_found`).
//! - **Hors ligne d'abord** : [`offline`] tranche sans appel ce que les drapeaux, les ambiguïtés et
//!   la résolvabilité du parseur permettent de trancher (N02, N20, N32…).
//!
//! # Limites de cette version
//!
//! L'article d'un texte est cherché par `find_article` sur le `LEGITEXT` résolu, sans parcourir
//! la table des matières. Un texte cité par sa seule date, la chambre et la formation d'une
//! décision, les avenants et les alinéas du Préambule ne sont pas contrôlés (`NotSupported`).

use lf_domain::{
    citation::{
        Ambiguity, ArticleNum, CaseNumber, CitationRef, CnilCite, CnilNumber, CodeRef, Confidence,
        ConventionCite, Court, DecisionCite, Flag, LegifranceIdRef, Resolvability, Spanned,
        TextCite, TextNature, UnresolvableWhy, UnsupportedFamily, VersionQualifier,
        dict::courts::CcType,
    },
    contract::{Citable, Nor},
    date::{CivilDate, VersionDate},
    ecli::Ecli,
    entity::{CodeListing, Decision, SearchHit},
    fond::Fond,
    id::{AccoTextId, ArticleId, CnilTextId, DecisionId, DocumentId, KaliTextId, LegiTextId},
    search::{NumberKind, NumberQuery, NumberValue},
    status::LegalStatus,
};

use super::{
    Candidate, CandidateKind, CheckStatus, Field, NotCheckedReason, Note, ResolvedRecord,
    policy::{self, Absence, CONSULT_LIMIT, Family, MAX_CANDIDATES, fold_line, fold_words},
};
use crate::ports::{ArticleAt, CorpusError, LegalCorpus, PisteEnvironment};

/// Ce que la résolution d'une citation a établi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Enregistrement trouvé, champs cités cohérents.
    Verified {
        /// Le document.
        record: ResolvedRecord,
        /// Confiance de la correspondance (§7.8).
        confidence: Confidence,
        /// Information ou réserve à rendre avec le verdict.
        note: Option<Note>,
    },
    /// Enregistrement trouvé, mais un champ dur cité diffère.
    Mismatch {
        /// Le document réel.
        record: ResolvedRecord,
        /// Le champ en écart.
        note: Note,
    },
    /// Syntaxe impossible : aucune recherche.
    Invalid {
        /// Le fait relevé.
        note: Note,
    },
    /// Plusieurs lectures ou plusieurs enregistrements possibles.
    Candidates {
        /// Les enregistrements (au plus [`MAX_CANDIDATES`]), vide si la citation n'a pas pu être
        /// cherchée.
        items: Vec<Candidate>,
        /// Pourquoi.
        note: Note,
    },
    /// Preuve d'absence réunie (§7.2) : `not_found` en production.
    Absent {
        /// Note de production (`NotInForce`, `NoRecord`).
        prod: Note,
        /// Note de bac à sable (`NotInSandbox`).
        sandbox: Note,
    },
    /// Contrôle impossible, pour la raison de la note.
    Unresolvable {
        /// Pourquoi.
        note: Note,
    },
    /// Le corpus a échoué : la citation n'a pas été contrôlée.
    Failed(CorpusError),
}

impl Outcome {
    /// Statut rendu pour cette issue dans l'environnement `env` (§7.6) ; en bac à sable,
    /// l'absence est `unresolvable`.
    pub(crate) fn status(&self, env: PisteEnvironment) -> CheckStatus {
        match self {
            Self::Verified { .. } => CheckStatus::Verified,
            Self::Mismatch { .. } | Self::Invalid { .. } => CheckStatus::Invalid,
            Self::Candidates { .. } => CheckStatus::Ambiguous,
            Self::Absent { .. } if env == PisteEnvironment::Production => CheckStatus::NotFound,
            Self::Absent { .. } | Self::Unresolvable { .. } | Self::Failed(_) => {
                CheckStatus::Unresolvable
            }
        }
    }

    /// Document trouvé, y compris quand il contredit la citation.
    pub(crate) fn record(&self) -> Option<&ResolvedRecord> {
        match self {
            Self::Verified { record, .. } | Self::Mismatch { record, .. } => Some(record),
            _ => None,
        }
    }

    /// Note de l'issue dans l'environnement `env`.
    pub(crate) fn note(&self, env: PisteEnvironment) -> Option<Note> {
        match self {
            Self::Verified { note, .. } => note.clone(),
            Self::Mismatch { note, .. }
            | Self::Invalid { note }
            | Self::Candidates { note, .. }
            | Self::Unresolvable { note } => Some(note.clone()),
            Self::Absent { prod, sandbox } => Some(if env == PisteEnvironment::Production {
                prod.clone()
            } else {
                sandbox.clone()
            }),
            Self::Failed(_) => Some(Note::NotChecked { reason: NotCheckedReason::Upstream }),
        }
    }
}

/// Résultat de la résolution d'une citation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resolution {
    /// Ce qui a été établi.
    pub(crate) outcome: Outcome,
    /// Texte officiel lu (article, décision), pour comparer l'extrait cité (§7.9).
    pub(crate) official: Option<String>,
}

impl Resolution {
    fn of(outcome: Outcome) -> Self {
        Self { outcome, official: None }
    }

    fn unresolvable(note: Note) -> Self {
        Self::of(Outcome::Unresolvable { note })
    }

    fn failed(error: CorpusError) -> Self {
        Self::of(Outcome::Failed(error))
    }

    fn ambiguous(note: Note) -> Self {
        Self::of(Outcome::Candidates { items: Vec::new(), note })
    }

    fn mismatch(record: ResolvedRecord, field: Field, cited: String, actual: String) -> Self {
        Self::of(Outcome::Mismatch { record, note: Note::Mismatch { field, cited, actual } })
    }

    /// Budget de temps épuisé avant la fin de la résolution.
    pub(crate) fn out_of_time() -> Self {
        Self::unresolvable(Note::NotChecked { reason: NotCheckedReason::TimeBudget })
    }
}

/// Liste des codes (`/list/code`), chargée une fois par appel.
#[derive(Debug)]
pub(crate) struct CodeTable(Result<Vec<CodeListing>, CorpusError>);

impl CodeTable {
    /// Table vide, pour les appels qui ne citent aucun code.
    pub(crate) const fn empty() -> Self {
        Self(Ok(Vec::new()))
    }

    /// Table obtenue du corpus (ou son erreur, rendue à chaque citation qui en dépend).
    pub(crate) fn from_result(result: Result<Vec<CodeListing>, CorpusError>) -> Self {
        Self(result)
    }

    /// Entrée dont le titre replié égale celui de `title` ; à titre égal, la version en vigueur.
    fn find(&self, title: &str) -> Result<Option<&CodeListing>, &CorpusError> {
        let listing = self.0.as_ref()?;
        let wanted = fold_line(title);
        let mut matching = listing.iter().filter(|code| fold_line(&code.title) == wanted);
        let first = matching.next();
        let preferred = first
            .filter(|code| code.etat == Some(LegalStatus::Vigueur))
            .or_else(|| matching.find(|code| code.etat == Some(LegalStatus::Vigueur)));
        Ok(preferred.or(first))
    }
}

/// La résolution de cette citation a-t-elle besoin de la liste des codes ?
pub(crate) fn needs_codes(item: &Spanned<CitationRef>) -> bool {
    matches!(
        item.value,
        CitationRef::CodeArticle { code: CodeRef::Known(_) | CodeRef::Unknown(_), .. }
    ) && offline(item).is_none()
}

/// Fait relevé par un drapeau `Impossible`, en vocabulaire fermé.
const fn fact_of(flag: &Flag) -> &'static str {
    match flag {
        Flag::DateInvalide => "The date does not exist",
        Flag::AnneeFuture => "The year is in the future",
        Flag::ChambreInexistante(_) => "There is no such civil chamber at the Cour de cassation",
        Flag::ChambreCassInconnue(_) => "There is no such chamber at the Cour de cassation",
        Flag::EcliOrdinalNonConforme(_) => {
            "The ECLI ordinal does not follow the form of that court"
        }
        Flag::EcliAnneeAnterieureNumero => "The ECLI year is earlier than the year of the number",
        Flag::AnneePourvoiPosterieure => "The year of the appeal number is after the decision year",
        _ => "The citation is impossible",
    }
}

/// Famille hors périmètre, en vocabulaire fermé.
const fn out_of_scope(family: UnsupportedFamily) -> &'static str {
    match family {
        UnsupportedFamily::Ue => "EU law",
        UnsupportedFamily::Cjue => "CJEU case law",
        UnsupportedFamily::Cedh => "ECHR case law",
        UnsupportedFamily::Doctrine => "Legal scholarship",
        UnsupportedFamily::Recueil => "A law report",
        UnsupportedFamily::ForeignEcli => "A foreign ECLI",
    }
}

/// Numéro d'article d'une citation, pour les notes.
fn article_key(value: &CitationRef) -> String {
    match value {
        CitationRef::CodeArticle { article, .. } | CitationRef::TextArticle { article, .. } => {
            article.num.num_key()
        }
        _ => String::new(),
    }
}

/// Ce que le parseur permet de trancher sans appel au corpus (§7.6), ou `None` si la citation doit
/// être cherchée.
pub(crate) fn offline(item: &Spanned<CitationRef>) -> Option<Resolution> {
    match item.effective_resolvability() {
        Resolvability::Unresolvable(why) => return Some(unresolvable_why(item, why)),
        Resolvability::Resolvable => {}
    }
    let ambiguity_note = match (&item.ambiguity, &item.value) {
        (
            Some(Ambiguity::NumeroManquant),
            CitationRef::Decision(DecisionCite { court, date, .. }),
        ) => Some(Note::NoNumber { fond: fond_of(court.as_ref()), date: *date }),
        (Some(Ambiguity::JuridictionManquante | Ambiguity::JuridictionEtTypeManquants), _) => {
            Some(Note::NoCourt)
        }
        (Some(Ambiguity::ConventionInconnue | Ambiguity::ConventionNonIdentifiee), _) => {
            Some(Note::NoAgreement)
        }
        _ => None,
    };
    if let Some(note) = ambiguity_note {
        return Some(Resolution::ambiguous(note));
    }
    if item.ambiguity == Some(Ambiguity::Appellation) {
        return Some(Resolution::unresolvable(Note::UseSearch));
    }
    match &item.value {
        CitationRef::Jorf(_) => Some(not_supported("JORF")),
        CitationRef::Decision(DecisionCite { court: None, .. }) => {
            Some(Resolution::ambiguous(Note::NoCourt))
        }
        CitationRef::Decision(decision) if decision.numbers.is_empty() => {
            Some(Resolution::ambiguous(Note::NoNumber {
                fond: fond_of(decision.court.as_ref()),
                date: decision.date,
            }))
        }
        CitationRef::Convention(ConventionCite { idcc: None, .. }) => {
            Some(Resolution::ambiguous(Note::NoAgreement))
        }
        CitationRef::Convention(ConventionCite { avenant: Some(_), .. }) => {
            Some(not_supported("amendment"))
        }
        CitationRef::TextArticle { text: Some(TextCite { number: None, .. }), .. }
        | CitationRef::Text(TextCite { number: None, .. }) => {
            Some(not_supported("text cited without a number"))
        }
        CitationRef::CodeArticle { article, .. } | CitationRef::TextArticle { article, .. }
            if article.num.num_key().is_empty() =>
        {
            Some(not_supported("preamble paragraph"))
        }
        _ => None,
    }
}

fn not_supported(what: &'static str) -> Resolution {
    Resolution::unresolvable(Note::NotSupported { what })
}

/// Fond où chercher une décision de cette juridiction (celui des notes `NoNumber`).
const fn fond_of(court: Option<&Court>) -> Fond {
    match court {
        Some(Court::ConseilEtat { .. } | Court::Caa { .. } | Court::Ta { .. }) => Fond::Cetat,
        Some(Court::ConseilConstitutionnel) => Fond::Constit,
        _ => Fond::Juri,
    }
}

/// Issue hors ligne d'une citation que `effective_resolvability` refuse.
fn unresolvable_why(item: &Spanned<CitationRef>, why: UnresolvableWhy) -> Resolution {
    match why {
        UnresolvableWhy::Impossible(flag) => {
            Resolution::of(Outcome::Invalid { note: Note::Impossible { fact: fact_of(&flag) } })
        }
        UnresolvableWhy::HorsPerimetre(family) => {
            Resolution::unresolvable(Note::OutOfScope { family: out_of_scope(family) })
        }
        UnresolvableWhy::SigleSansContexte(sigle) => {
            Resolution::ambiguous(Note::AmbiguousSigle { sigle })
        }
        UnresolvableWhy::SansCode => {
            Resolution::ambiguous(Note::MissingTarget { num: article_key(&item.value) })
        }
        UnresolvableWhy::AnaphoreNonResolue => {
            Resolution::unresolvable(Note::MissingTarget { num: article_key(&item.value) })
        }
        UnresolvableWhy::PublicationSeule => not_supported("publication reference"),
    }
}

/// Moteur de résolution : un corpus, le jour et la liste des codes.
pub(crate) struct Resolver<'a, C: LegalCorpus> {
    corpus: &'a C,
    today: VersionDate,
    codes: &'a CodeTable,
}

/// Filtre sur l'intitulé d'une décision (tribunal administratif, cour administrative d'appel…).
type TitleFilter = fn(&str) -> bool;

/// Recherche d'une décision par numéro.
struct Lookup {
    fond: Fond,
    kind: NumberKind,
    forms: Vec<String>,
    title_filter: Option<TitleFilter>,
    date: Option<CivilDate>,
    cc_type: Option<CcType>,
    absent: Note,
}

impl<'a, C: LegalCorpus> Resolver<'a, C> {
    /// Moteur pour un appel : `today` est le jour de Paris, `codes` la liste des codes déjà chargée.
    pub(crate) const fn new(corpus: &'a C, today: VersionDate, codes: &'a CodeTable) -> Self {
        Self { corpus, today, codes }
    }

    /// Résout une citation. `at` est la date de version voulue par l'appelant (la date de
    /// `fr_resolve`), à défaut d'une date citée ; `None` : la version en vigueur aujourd'hui.
    pub(crate) async fn resolve_ref(
        &self,
        item: &Spanned<CitationRef>,
        at: Option<VersionDate>,
    ) -> Resolution {
        if let Some(done) = offline(item) {
            return done;
        }
        match &item.value {
            CitationRef::CodeArticle { code, article, version, .. } => {
                let resolution = self.code_article(code, &article.num, version.as_ref(), at).await;
                with_sigle_reading(item, code, resolution)
            }
            CitationRef::TextArticle { text: Some(text), article, at_date } => {
                self.text_article(text, &article.num, *at_date, at).await
            }
            CitationRef::Text(text) => self.text(text).await.into_resolution(),
            CitationRef::Decision(decision) => self.decision(decision).await,
            CitationRef::Ecli(ecli) => self.ecli(ecli).await,
            CitationRef::CnilDeliberation(cnil) => self.cnil(cnil).await,
            CitationRef::Convention(convention) => self.convention(convention).await,
            CitationRef::Nor(nor) => self.by_nor(nor).await,
            CitationRef::Eli(eli) => self.by_index(NumberKind::Eli, eli.as_str(), "ELI").await,
            CitationRef::LegifranceId(id) => self.legifrance_id(id).await,
            CitationRef::TextArticle { text: None, .. }
            | CitationRef::Jorf(_)
            | CitationRef::Unsupported(_) => not_supported("this kind of citation"),
        }
    }

    /// Date de version explicite d'une citation d'article (qualificatif, date citée, date de
    /// l'appelant), `None` pour la version en vigueur. L'erreur est le motif de non-prise en
    /// charge (`Note::NotSupported`).
    fn explicit_date(
        version: Option<&VersionQualifier>,
        cited: Option<CivilDate>,
        tool: Option<VersionDate>,
    ) -> Result<Option<VersionDate>, &'static str> {
        let from_qualifier = match version {
            Some(
                VersionQualifier::EnVigueurAu(date)
                | VersionQualifier::RedactionEnVigueur(Some(date)),
            ) => Some(*date),
            Some(
                VersionQualifier::Ancien
                | VersionQualifier::RedactionApplicableAuLitige
                | VersionQualifier::RedactionApplicableDateDesFaits
                | VersionQualifier::RedactionIssueDe(_)
                | VersionQualifier::RedactionAnterieureA(_),
            ) => return Err("version date unknown"),
            Some(VersionQualifier::Nouveau | VersionQualifier::RedactionEnVigueur(None)) | None => {
                None
            }
        };
        match from_qualifier.or(cited) {
            Some(date) => VersionDate::try_from(date).map(Some).map_err(|_| "date out of range"),
            None => Ok(tool),
        }
    }

    /// Article d'un code.
    async fn code_article(
        &self,
        code: &CodeRef,
        num: &ArticleNum,
        version: Option<&VersionQualifier>,
        tool: Option<VersionDate>,
    ) -> Resolution {
        let when = match Self::explicit_date(version, None, tool) {
            Ok(when) => when,
            Err(why) => return not_supported(why),
        };
        let (label, listing) = match code {
            CodeRef::Known(key) => (key.title().to_owned(), self.codes.find(key.title())),
            CodeRef::Unknown(title) => (title.clone(), self.codes.find(title)),
            CodeRef::Anaphora(_) | CodeRef::Missing => {
                return Resolution::ambiguous(Note::MissingTarget { num: num.num_key() });
            }
        };
        let listing = match listing {
            Ok(Some(listing)) => listing,
            Ok(None) => return Resolution::unresolvable(Note::CodeNotListed),
            Err(error) => return Resolution::failed(error.clone()),
        };
        let label =
            if matches!(code, CodeRef::Known(_)) { label } else { listing.id.as_str().to_owned() };
        self.article_in(&listing.id, &label, num, when, true).await
    }

    /// Article de `text`, essayé sous chacune des variantes de sa clé.
    async fn article_in(
        &self,
        text: &LegiTextId,
        label: &str,
        num: &ArticleNum,
        when: Option<VersionDate>,
        absence_proven: bool,
    ) -> Resolution {
        let at = when.map_or(ArticleAt::Today(self.today), ArticleAt::On);
        let variants = num.key_variants();
        for variant in &variants {
            match self.corpus.find_article(text, variant, at).await {
                Ok(article) => return article_found(&article, &variants, at),
                Err(CorpusError::NotFound { .. }) => {}
                Err(error) => return Resolution::failed(error),
            }
        }
        let key = num.num_key();
        let prod = Note::NotInForce { code: label.to_owned(), num: key.clone(), date: at.date() };
        let sandbox = Note::NotInSandbox {
            family: "article",
            value: format!("{label}, art. {key}"),
            date: Some(at.date()),
        };
        let family = if absence_proven { Family::CodeArticle } else { Family::Text };
        Resolution::of(Self::when_absent(
            family,
            prod,
            sandbox,
            Note::CouldNotConfirm { index: "LODA" },
        ))
    }

    /// Issue d'une absence selon la table de la politique (§7.2).
    fn when_absent(family: Family, prod: Note, sandbox: Note, fallback: Note) -> Outcome {
        match policy::absence(family) {
            Absence::Allowed => Outcome::Absent { prod, sandbox },
            Absence::AfterVerification | Absence::Never => Outcome::Unresolvable { note: fallback },
        }
    }

    /// Article d'un texte non codifié : résout le texte, puis l'article dans ce texte.
    async fn text_article(
        &self,
        text: &TextCite,
        num: &ArticleNum,
        at_date: Option<CivilDate>,
        tool: Option<VersionDate>,
    ) -> Resolution {
        let when = match Self::explicit_date(None, at_date, tool) {
            Ok(when) => when,
            Err(why) => return not_supported(why),
        };
        match self.text(text).await {
            TextStep::Found { id, confidence, .. } => {
                let sure = confidence == Confidence::High;
                self.article_in(&id, id.as_str(), num, when, sure).await
            }
            TextStep::Done(done) => done,
        }
    }

    /// Texte numéroté (LODA) : `find_by_number`, puis comparaison de la nature et de la date.
    async fn text(&self, cite: &TextCite) -> TextStep {
        let Some(number) = &cite.number else {
            return TextStep::Done(not_supported("text cited without a number"));
        };
        let key = number
            .year
            .map_or_else(|| number.seq.to_string(), |year| format!("{year}-{}", number.seq));
        let hits = match self.by_number(Fond::Loda, NumberKind::Text, &key).await {
            Ok(hits) => hits,
            Err(error) => return TextStep::Done(Resolution::failed(error)),
        };
        let hits: Vec<SearchHit> =
            hits.into_iter().filter(|hit| matches!(hit.id, DocumentId::LegiText(_))).collect();
        if hits.is_empty() {
            return TextStep::Done(Resolution::unresolvable(Note::CouldNotConfirm {
                index: "LODA",
            }));
        }
        let fits: Vec<TextFit> = hits.iter().map(|hit| text_fit(cite, hit)).collect();
        let exact: Vec<usize> = fits
            .iter()
            .enumerate()
            .filter(|(_, fit)| **fit == TextFit::Exact)
            .map(|(i, _)| i)
            .collect();
        let confidence = if cite.date.is_some() { Confidence::High } else { Confidence::Medium };
        match (exact.as_slice(), hits.as_slice()) {
            ([only], _) => {
                let hit = &hits[*only];
                let DocumentId::LegiText(id) = &hit.id else {
                    return TextStep::Done(not_supported("this kind of citation"));
                };
                let record = ResolvedRecord::of(hit.id.clone(), hit);
                TextStep::Found { id: id.clone(), record, confidence }
            }
            ([], [only]) => TextStep::Done(text_mismatch(cite, only, &fits[0])),
            _ => TextStep::Done(Resolution::of(candidates_of_hits(&hits))),
        }
    }

    /// Décision de justice.
    async fn decision(&self, cite: &DecisionCite) -> Resolution {
        let Some(court) = &cite.court else {
            return Resolution::ambiguous(Note::NoCourt);
        };
        let Some((fond, kind, forms)) = number_forms(court, &cite.numbers) else {
            return Resolution::ambiguous(Note::NoNumber {
                fond: fond_of(Some(court)),
                date: cite.date,
            });
        };
        let (title_filter, absent): (Option<TitleFilter>, Note) = match court {
            Court::CourDeCassation { .. } => (None, Note::CouldNotConfirm { index: "JURI" }),
            Court::CourAppel { .. } => (None, Note::OutOfCoverage { court: "Cour d'appel" }),
            Court::TribunalJudiciaire { .. } => {
                (None, Note::OutOfCoverage { court: "tribunal judiciaire" })
            }
            Court::AutreJudiciaire { .. } => (None, Note::OutOfCoverage { court: "lower court" }),
            Court::ConseilEtat { .. } => (
                Some(is_conseil_etat as fn(&str) -> bool),
                Note::CouldNotConfirm { index: "CETAT" },
            ),
            Court::Caa { .. } => {
                (Some(is_caa as fn(&str) -> bool), Note::OutOfCoverage { court: "CAA" })
            }
            Court::Ta { .. } => {
                (Some(is_ta as fn(&str) -> bool), Note::OutOfCoverage { court: "TA" })
            }
            Court::ConseilConstitutionnel => (None, Note::CouldNotConfirm { index: "CONSTIT" }),
        };
        let cc_type = cite.numbers.iter().find_map(|number| match number {
            CaseNumber::Cc(cc) => Some(cc.kind),
            _ => None,
        });
        self.lookup(Lookup { fond, kind, forms, title_filter, date: cite.date, cc_type, absent })
            .await
    }

    /// Cherche une décision par numéro, la consulte et compare la date (et le type du Conseil
    /// constitutionnel).
    async fn lookup(&self, spec: Lookup) -> Resolution {
        let mut hits = Vec::new();
        for form in &spec.forms {
            match self.by_number(spec.fond, spec.kind, form).await {
                Ok(found) if found.is_empty() => {}
                Ok(found) => {
                    hits = found;
                    break;
                }
                Err(error) => return Resolution::failed(error),
            }
        }
        if let Some(filter) = spec.title_filter {
            hits.retain(|hit| hit.title.as_deref().is_none_or(filter));
        }
        if hits.is_empty() {
            return Resolution::unresolvable(spec.absent);
        }
        if hits.len() > CONSULT_LIMIT {
            return Resolution::of(candidates_of_hits(&hits));
        }
        let mut found: Vec<(DocumentId, Decision)> = Vec::new();
        for hit in &hits {
            let Ok(id) = DecisionId::parse(hit.id.as_str()) else { continue };
            match self.corpus.get_decision(&id).await {
                Ok(decision) => found.push((hit.id.clone(), decision)),
                Err(CorpusError::NotFound { .. }) => {}
                Err(error) => return Resolution::failed(error),
            }
        }
        if found.is_empty() {
            return Resolution::unresolvable(spec.absent);
        }
        choose_decision(&found, spec.date, spec.cc_type)
    }

    /// ECLI cité : numéro tiré de l'ordinal (CC, CE) ou facette ECLI (Cour de cassation), puis
    /// égalité stricte avec l'ECLI natif du document consulté.
    async fn ecli(&self, cited: &Ecli) -> Resolution {
        let parts: Vec<&str> = cited.as_str().split(':').collect();
        let (Some(court), Some(ordinal)) = (parts.get(2).copied(), parts.get(4).copied()) else {
            return not_supported("ECLI of this court");
        };
        let mut pieces = ordinal.split('.');
        let (first, second) = (pieces.next().unwrap_or_default(), pieces.next());
        let lookup = |fond, kind, form: String, filter, absent| Lookup {
            fond,
            kind,
            forms: vec![form],
            title_filter: filter,
            date: None,
            cc_type: None,
            absent,
        };
        let (resolution, index, strict) = match (court, second) {
            ("CC", Some(seq)) => {
                let spec = lookup(
                    Fond::Constit,
                    NumberKind::Decision,
                    format!("{first}-{seq}"),
                    None,
                    Note::CouldNotConfirm { index: "CONSTIT" },
                );
                (self.lookup(spec).await, "CONSTIT", false)
            }
            (court, _) if court.starts_with("CE") => {
                let spec = lookup(
                    Fond::Cetat,
                    NumberKind::Decision,
                    first.to_owned(),
                    Some(is_conseil_etat as fn(&str) -> bool),
                    Note::CouldNotConfirm { index: "CETAT" },
                );
                (self.lookup(spec).await, "CETAT", false)
            }
            ("CCASS", _) => {
                let spec = lookup(
                    Fond::Juri,
                    NumberKind::Ecli,
                    cited.as_str().to_owned(),
                    None,
                    Note::CouldNotConfirm { index: "JURI" },
                );
                (self.lookup(spec).await, "JURI", true)
            }
            _ => return not_supported("ECLI of this court"),
        };
        check_ecli(resolution, cited, index, strict)
    }

    /// Délibération de la CNIL.
    async fn cnil(&self, cite: &CnilCite) -> Resolution {
        let number = CnilNumber { series: cite.series.clone(), year: cite.year, seq: cite.seq };
        let hits =
            match self.by_number(Fond::Cnil, NumberKind::Deliberation, &number.to_string()).await {
                Ok(hits) => hits,
                Err(error) => return Resolution::failed(error),
            };
        let absent = Note::CouldNotConfirm { index: "CNIL" };
        match hits.as_slice() {
            [] => Resolution::unresolvable(absent),
            [hit] => {
                let Ok(id) = CnilTextId::parse(hit.id.as_str()) else {
                    return Resolution::unresolvable(absent);
                };
                let deliberation = match self.corpus.get_deliberation(&id).await {
                    Ok(deliberation) => deliberation,
                    Err(CorpusError::NotFound { .. }) => return Resolution::unresolvable(absent),
                    Err(error) => return Resolution::failed(error),
                };
                let record = ResolvedRecord::of(hit.id.clone(), &deliberation);
                match (cite.date, deliberation.date_texte) {
                    (Some(cited), Some(actual)) if cited != actual => Resolution::mismatch(
                        record,
                        Field::Date,
                        cited.to_string(),
                        actual.to_string(),
                    ),
                    (Some(_), Some(_)) => verified(record, Confidence::High, None),
                    _ => verified(record, Confidence::Medium, None),
                }
            }
            many => Resolution::of(candidates_of_hits(many)),
        }
    }

    /// Convention collective par son IDCC.
    async fn convention(&self, cite: &ConventionCite) -> Resolution {
        let Some(idcc) = cite.idcc else {
            return Resolution::ambiguous(Note::NoAgreement);
        };
        self.by_index(NumberKind::Idcc, &idcc.get().to_string(), "KALI").await
    }

    /// NOR.
    async fn by_nor(&self, nor: &Nor) -> Resolution {
        self.by_index(NumberKind::Nor, nor.as_str(), "LODA").await
    }

    /// Index d'un fond interrogé par une valeur exacte (NOR, ELI, IDCC) : un hit unique est
    /// l'enregistrement, aucun est `unresolvable` (index incomplet, §7.2), plusieurs sont ambigus.
    async fn by_index(&self, kind: NumberKind, value: &str, index: &'static str) -> Resolution {
        let fond = match kind {
            NumberKind::Idcc => Fond::Kali,
            _ => Fond::Loda,
        };
        let hits = match self.by_number(fond, kind, value).await {
            Ok(hits) => hits,
            Err(error) => return Resolution::failed(error),
        };
        match hits.as_slice() {
            [] => Resolution::unresolvable(Note::CouldNotConfirm { index }),
            [hit] => verified(ResolvedRecord::of(hit.id.clone(), hit), Confidence::Medium, None),
            many => Resolution::of(candidates_of_hits(many)),
        }
    }

    /// Identifiant Légifrance : consultation exacte, selon le préfixe.
    async fn legifrance_id(&self, cited: &LegifranceIdRef) -> Resolution {
        let Some(id) = cited.to_document_id() else {
            return not_supported("this kind of identifier");
        };
        let label = id.as_str().to_owned();
        let read = self.consult(&id, cited).await;
        match read {
            Ok(resolution) => resolution,
            Err(CorpusError::NotFound { .. }) => {
                let prod =
                    Note::NoRecord { family: "identifier", value: label.clone(), date: None };
                let sandbox = Note::NotInSandbox { family: "identifier", value: label, date: None };
                Resolution::of(Self::when_absent(
                    Family::LegifranceId,
                    prod,
                    sandbox,
                    Note::CouldNotConfirm { index: "LEGI" },
                ))
            }
            Err(error) => Resolution::failed(error),
        }
    }

    /// Lit le document d'identifiant `id` avec la méthode de sa famille.
    async fn consult(
        &self,
        id: &DocumentId,
        cited: &LegifranceIdRef,
    ) -> Result<Resolution, CorpusError> {
        let invalid = || CorpusError::NotFound { what: id.as_str().to_owned() };
        Ok(match id {
            DocumentId::LegiArti(_) | DocumentId::KaliArti(_) => {
                let article = self
                    .corpus
                    .get_article(&ArticleId::parse(id.as_str()).map_err(|_| invalid())?)
                    .await?;
                found_with_text(id, &article, article.text.as_str())
            }
            DocumentId::LegiText(_) => {
                let date = cited
                    .version_start
                    .and_then(|date| VersionDate::try_from(date).ok())
                    .unwrap_or(self.today);
                let act = self
                    .corpus
                    .get_act(&LegiTextId::parse(id.as_str()).map_err(|_| invalid())?, date)
                    .await?;
                found_with_text(id, &act, "")
            }
            DocumentId::JuriText(_) | DocumentId::ConsText(_) | DocumentId::CetaText(_) => {
                let decision = self
                    .corpus
                    .get_decision(&DecisionId::parse(id.as_str()).map_err(|_| invalid())?)
                    .await?;
                found_with_text(id, &decision, decision.text.as_str())
            }
            DocumentId::CnilText(_) => {
                let deliberation = self
                    .corpus
                    .get_deliberation(&CnilTextId::parse(id.as_str()).map_err(|_| invalid())?)
                    .await?;
                found_with_text(id, &deliberation, "")
            }
            DocumentId::KaliText(_) => {
                let convention = self
                    .corpus
                    .get_convention(&KaliTextId::parse(id.as_str()).map_err(|_| invalid())?)
                    .await?;
                found_with_text(id, &convention, "")
            }
            DocumentId::AccoText(_) => {
                let agreement = self
                    .corpus
                    .get_company_agreement(&AccoTextId::parse(id.as_str()).map_err(|_| invalid())?)
                    .await?;
                found_with_text(id, &agreement, "")
            }
            DocumentId::JorfText(_) => not_supported("JORF"),
        })
    }

    /// `find_by_number` pour la valeur `value`, à la date du jour.
    async fn by_number(
        &self,
        fond: Fond,
        kind: NumberKind,
        value: &str,
    ) -> Result<Vec<SearchHit>, CorpusError> {
        let unusable =
            |detail: &str| CorpusError::InvalidRequest { status: None, detail: detail.to_owned() };
        let value = NumberValue::parse(value).map_err(|_| unusable("number value"))?;
        let query = NumberQuery::new(fond, kind, value, self.today)
            .map_err(|_| unusable("number query"))?;
        self.corpus.find_by_number(&query).await
    }
}

/// Étape de résolution d'un texte.
enum TextStep {
    /// Un texte, sûr ou non (`confidence`).
    Found { id: LegiTextId, record: ResolvedRecord, confidence: Confidence },
    /// Résolution terminée sans texte (écart, plusieurs candidats, introuvable).
    Done(Resolution),
}

impl TextStep {
    fn into_resolution(self) -> Resolution {
        match self {
            Self::Found { record, confidence, .. } => verified(record, confidence, None),
            Self::Done(done) => done,
        }
    }
}

/// Accord d'un hit LODA avec le texte cité.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TextFit {
    Exact,
    Date(CivilDate),
    Nature(String),
}

/// Nature de l'API correspondant aux natures citées dont le nom est sans ambiguïté.
const fn plain_nature(nature: TextNature) -> Option<&'static str> {
    match nature {
        TextNature::Loi => Some("LOI"),
        TextNature::Ordonnance => Some("ORDONNANCE"),
        TextNature::Decret => Some("DECRET"),
        TextNature::Arrete => Some("ARRETE"),
        _ => None,
    }
}

fn text_fit(cite: &TextCite, hit: &SearchHit) -> TextFit {
    if let (Some(wanted), Some(actual)) = (plain_nature(cite.nature), hit.nature.as_deref())
        && fold_line(actual) != fold_line(wanted)
    {
        return TextFit::Nature(fold_line(actual).to_uppercase());
    }
    match (cite.date, hit.date) {
        (Some(cited), Some(actual)) if cited != actual => TextFit::Date(actual),
        _ => TextFit::Exact,
    }
}

fn text_mismatch(cite: &TextCite, hit: &SearchHit, fit: &TextFit) -> Resolution {
    let record = ResolvedRecord::of(hit.id.clone(), hit);
    match fit {
        TextFit::Date(actual) => Resolution::mismatch(
            record,
            Field::Date,
            cite.date.map(|date| date.to_string()).unwrap_or_default(),
            actual.to_string(),
        ),
        TextFit::Nature(actual) => Resolution::mismatch(
            record,
            Field::Nature,
            plain_nature(cite.nature).unwrap_or_default().to_owned(),
            actual.clone(),
        ),
        TextFit::Exact => verified(record, Confidence::Medium, None),
    }
}

fn verified(record: ResolvedRecord, confidence: Confidence, note: Option<Note>) -> Resolution {
    Resolution::of(Outcome::Verified { record, confidence, note })
}

/// Issue d'un article trouvé : cohérence du numéro, texte officiel, début de la version courante.
fn article_found(
    article: &lf_domain::entity::ArticleText,
    variants: &[String],
    at: ArticleAt,
) -> Resolution {
    let Ok(id) = DocumentId::parse(article.article_id.as_str()) else {
        return not_supported("this kind of identifier");
    };
    let record = ResolvedRecord::of(id, article);
    let official = Some(article.text.as_str().to_owned());
    if let Some(actual) = article.num.as_deref() {
        let squashed = squash(actual);
        if !variants.iter().any(|variant| squash(variant) == squashed) {
            let cited = variants.first().cloned().unwrap_or_default();
            return Resolution {
                official,
                ..Resolution::mismatch(record, Field::ArticleNum, cited, actual.to_owned())
            };
        }
    }
    let note = match at {
        ArticleAt::Today(_) => article.date_debut.map(Note::VersionSince),
        ArticleAt::On(_) => None,
    };
    Resolution { official, ..verified(record, Confidence::High, note) }
}

/// Document consulté par son identifiant : trouvé, de confiance haute.
fn found_with_text<T: Citable>(id: &DocumentId, entity: &T, text: &str) -> Resolution {
    let official = (!text.is_empty()).then(|| text.to_owned());
    Resolution {
        official,
        ..verified(ResolvedRecord::of(id.clone(), entity), Confidence::High, None)
    }
}

/// Forme compacte d'un numéro : lettres et chiffres repliés, rien d'autre.
fn squash(text: &str) -> String {
    fold_words(text).concat()
}

/// Candidats d'une liste de hits : les cinq premiers, de confiance basse.
fn candidates_of_hits(hits: &[SearchHit]) -> Outcome {
    let items = hits
        .iter()
        .take(MAX_CANDIDATES)
        .map(|hit| Candidate {
            record: ResolvedRecord::of(hit.id.clone(), hit),
            kind: CandidateKind::of(&hit.id),
            confidence: Confidence::Low,
        })
        .collect();
    Outcome::Candidates { items, note: Note::Candidates { n: hits.len() } }
}

/// Formes essayées pour le numéro d'une décision de cette juridiction, et le fond à interroger.
fn number_forms(court: &Court, numbers: &[CaseNumber]) -> Option<(Fond, NumberKind, Vec<String>)> {
    let case = |extract: fn(&CaseNumber) -> Option<Vec<String>>| numbers.iter().find_map(extract);
    match court {
        Court::CourDeCassation { .. } => case(|number| match number {
            CaseNumber::Pourvoi(p) => {
                Some(vec![p.to_string(), format!("{:02}-{:02}{:03}", p.yy, p.mid, p.seq)])
            }
            CaseNumber::Unknown(raw) => Some(vec![raw.clone()]),
            _ => None,
        })
        .map(|forms| (Fond::Juri, NumberKind::Case, forms)),
        Court::CourAppel { .. }
        | Court::TribunalJudiciaire { .. }
        | Court::AutreJudiciaire { .. } => case(|number| match number {
            CaseNumber::Rg(rg) => Some(vec![rg.to_string()]),
            CaseNumber::Portalis(raw) | CaseNumber::Unknown(raw) => Some(vec![raw.clone()]),
            _ => None,
        })
        .map(|forms| (Fond::Juri, NumberKind::Case, forms)),
        Court::ConseilEtat { .. } => case(|number| match number {
            CaseNumber::CeRequete(n) => Some(vec![n.to_string()]),
            CaseNumber::Unknown(raw) => Some(vec![raw.clone()]),
            _ => None,
        })
        .map(|forms| (Fond::Cetat, NumberKind::Decision, forms)),
        Court::Caa { .. } => case(|number| match number {
            CaseNumber::Caa(n) => Some(vec![n.to_string()]),
            _ => None,
        })
        .map(|forms| (Fond::Cetat, NumberKind::Decision, forms)),
        Court::Ta { .. } => case(|number| match number {
            CaseNumber::Ta(n) => Some(vec![n.to_string()]),
            _ => None,
        })
        .map(|forms| (Fond::Cetat, NumberKind::Decision, forms)),
        Court::ConseilConstitutionnel => case(|number| match number {
            CaseNumber::Cc(cc) => {
                let mut forms = vec![cc.to_string()];
                let first = format!("{}-{}", cc.year, cc.seq);
                if !forms.contains(&first) {
                    forms.push(first);
                }
                Some(forms)
            }
            _ => None,
        })
        .map(|forms| (Fond::Constit, NumberKind::Decision, forms)),
    }
}

fn is_conseil_etat(title: &str) -> bool {
    fold_line(title).starts_with("conseil d etat")
}

fn is_caa(title: &str) -> bool {
    fold_line(title).contains("cour administrative d appel")
}

fn is_ta(title: &str) -> bool {
    fold_line(title).contains("tribunal administratif")
}

/// Choisit parmi les décisions consultées celle que la citation désigne (date citée, type du
/// Conseil constitutionnel).
fn choose_decision(
    found: &[(DocumentId, Decision)],
    date: Option<CivilDate>,
    cc_type: Option<CcType>,
) -> Resolution {
    let candidates = |found: &[(DocumentId, Decision)]| {
        let items = found
            .iter()
            .take(MAX_CANDIDATES)
            .map(|(id, decision)| Candidate {
                record: ResolvedRecord::of(id.clone(), decision),
                kind: CandidateKind::of(id),
                confidence: Confidence::Low,
            })
            .collect();
        Resolution::of(Outcome::Candidates { items, note: Note::Candidates { n: found.len() } })
    };
    let same_day: Vec<usize> = match date {
        Some(cited) => found
            .iter()
            .enumerate()
            .filter(|(_, (_, decision))| decision.date_texte == Some(cited))
            .map(|(index, _)| index)
            .collect(),
        None => Vec::new(),
    };
    let (index, confidence) = match (date, same_day.as_slice(), found.len()) {
        (Some(_), [only], _) => (*only, Confidence::High),
        (Some(_), [], 1) | (None, _, 1) => (0, Confidence::Medium),
        _ => return candidates(found),
    };
    let (id, decision) = &found[index];
    let record = ResolvedRecord::of(id.clone(), decision);
    let official = Some(decision.text.as_str().to_owned());
    let mismatch = match (date, decision.date_texte, cc_type, decision.nature.as_deref()) {
        (Some(cited), Some(actual), _, _) if cited != actual => {
            Some((Field::Date, cited.to_string(), actual.to_string()))
        }
        (_, _, Some(kind), Some(actual)) if fold_line(actual) != fold_line(kind.as_str()) => {
            Some((Field::CcType, kind.as_str().to_owned(), fold_line(actual).to_uppercase()))
        }
        _ => None,
    };
    match mismatch {
        Some((field, cited, actual)) => {
            Resolution { official, ..Resolution::mismatch(record, field, cited, actual) }
        }
        None => Resolution { official, ..verified(record, confidence, None) },
    }
}

/// Égalité de l'ECLI natif du document avec l'ECLI cité (§7.3). `strict` : un ECLI différent ou
/// absent laisse la citation `unresolvable` (Cour de cassation, dont le numéro n'est pas dérivé) ;
/// sinon un ECLI différent donne `invalid` avec l'enregistrement réel (N14).
fn check_ecli(
    resolution: Resolution,
    cited: &Ecli,
    index: &'static str,
    strict: bool,
) -> Resolution {
    let Resolution { outcome, official } = resolution;
    let outcome = match outcome {
        Outcome::Verified { record, .. } => match record.ecli.clone() {
            Some(native) if native == *cited => Outcome::Verified {
                record,
                confidence: Confidence::High,
                note: Some(Note::NativeEcli(native)),
            },
            Some(native) if !strict => Outcome::Mismatch { record, note: Note::NativeEcli(native) },
            _ => Outcome::Unresolvable { note: Note::CouldNotConfirm { index } },
        },
        other => other,
    };
    Resolution { outcome, official }
}

/// Note `ReadAs` d'une citation lue avec le code par défaut d'un sigle ambigu (N05) ; si le
/// document n'est pas trouvé, la lecture reste ambiguë.
fn with_sigle_reading(
    item: &Spanned<CitationRef>,
    code: &CodeRef,
    resolution: Resolution,
) -> Resolution {
    let (Some(Ambiguity::SigleAmbigu(sigle)), CodeRef::Known(key)) = (&item.ambiguity, code) else {
        return resolution;
    };
    let Resolution { outcome, official } = resolution;
    let outcome = match outcome {
        Outcome::Verified { record, confidence, .. } => Outcome::Verified {
            record,
            confidence: confidence.min(Confidence::Medium),
            note: Some(Note::ReadAs { sigle: *sigle, code: key.title() }),
        },
        Outcome::Absent { .. } | Outcome::Mismatch { .. } | Outcome::Unresolvable { .. } => {
            Outcome::Candidates { items: Vec::new(), note: Note::AmbiguousSigle { sigle: *sigle } }
        }
        other => other,
    };
    Resolution { outcome, official }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeCorpus, PortRequest};
    use lf_domain::{
        citation::{IdPrefix, Span, parse},
        contract::Nor,
        entity::{Act, ArticleText, Deliberation, TextBody},
        id::{ArticleId, CnilTextId},
        text::LongText,
    };
    use pretty_assertions::assert_eq;

    const CIVIL: &str = "LEGITEXT000006070721";
    const PENAL: &str = "LEGITEXT000006070719";

    fn today() -> VersionDate {
        VersionDate::parse("2026-10-29").unwrap()
    }

    fn civil(y: u16, m: u8, d: u8) -> CivilDate {
        CivilDate::new(y, m, d).unwrap()
    }

    fn listing(id: &str, title: &str, etat: LegalStatus) -> CodeListing {
        CodeListing {
            id: LegiTextId::parse(id).unwrap(),
            title: title.to_owned(),
            etat: Some(etat),
        }
    }

    fn codes() -> CodeTable {
        CodeTable::from_result(Ok(vec![
            listing(CIVIL, "Code civil", LegalStatus::Vigueur),
            listing(PENAL, "Code pénal", LegalStatus::Vigueur),
        ]))
    }

    fn not_found() -> CorpusError {
        CorpusError::NotFound { what: "/consult".to_owned() }
    }

    fn upstream() -> CorpusError {
        CorpusError::Upstream { status: Some(503), retryable: true, detail: "down".to_owned() }
    }

    fn article(num: Option<&str>) -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse("LEGIARTI000006419288").unwrap(),
            num: num.map(str::to_owned),
            etat: Some(LegalStatus::Vigueur),
            code_title: Some("Code civil".to_owned()),
            parent: None,
            path: vec![],
            full_sections_titre: None,
            date_debut: Some(civil(2016, 10, 1)),
            date_fin: None,
            text: LongText::from_plain("Texte officiel.").unwrap(),
            nota: None,
        }
    }

    fn hit(fond: Fond, id: &str) -> SearchHit {
        SearchHit::new(fond, DocumentId::parse(id).unwrap(), None).unwrap()
    }

    fn decision(id: &str, date: CivilDate, nature: Option<&str>) -> Decision {
        Decision {
            decision_id: DecisionId::parse(id).unwrap(),
            ecli: None,
            num: None,
            juridiction: None,
            formation: None,
            solution: None,
            numero_affaire: vec![],
            date_texte: Some(date),
            nature: nature.map(str::to_owned),
            title: None,
            text: LongText::from_plain("Texte de la décision.").unwrap(),
            official_summary: lf_domain::entity::OfficialSummary::default(),
        }
    }

    fn first(text: &str) -> Spanned<CitationRef> {
        let mut items = parse(text);
        assert!(!items.is_empty(), "aucune citation dans {text:?}");
        items.remove(0)
    }

    async fn resolve_with(
        corpus: &FakeCorpus,
        codes: &CodeTable,
        item: &Spanned<CitationRef>,
        at: Option<VersionDate>,
    ) -> Resolution {
        Resolver::new(corpus, today(), codes).resolve_ref(item, at).await
    }

    async fn resolve(corpus: &FakeCorpus, text: &str) -> Resolution {
        resolve_with(corpus, &codes(), &first(text), None).await
    }

    fn status(resolution: &Resolution) -> CheckStatus {
        resolution.outcome.status(PisteEnvironment::Production)
    }

    fn note(resolution: &Resolution) -> String {
        resolution.outcome.note(PisteEnvironment::Production).map(|n| n.text()).unwrap_or_default()
    }

    fn candidates_of(outcome: Outcome) -> Option<(Vec<Candidate>, Note)> {
        match outcome {
            Outcome::Candidates { items, note } => Some((items, note)),
            _ => None,
        }
    }

    fn standalone(value: CitationRef) -> Spanned<CitationRef> {
        Spanned::new(value, Span { start: 0, end: 1 })
    }

    #[tokio::test]
    async fn today_date_uses_article_at_today() {
        let corpus = FakeCorpus::new();
        corpus.push_find_article(Err(not_found()));
        corpus.push_find_article(Err(not_found()));
        let item = first("article 9 du code civil");
        let on = VersionDate::parse("2018-01-01").unwrap();
        resolve_with(&corpus, &codes(), &item, None).await;
        resolve_with(&corpus, &codes(), &item, Some(on)).await;
        let civil_code = LegiTextId::parse(CIVIL).unwrap();
        assert_eq!(
            corpus.seen_requests(),
            vec![
                PortRequest::FindArticle(civil_code.clone(), "9".into(), ArticleAt::Today(today())),
                PortRequest::FindArticle(civil_code, "9".into(), ArticleAt::On(on)),
            ]
        );
    }

    #[tokio::test]
    async fn key_variants_are_tried_in_order_until_one_answers() {
        let corpus = FakeCorpus::new();
        corpus.push_find_article(Err(not_found()));
        corpus.push_find_article(Ok(article(Some("1"))));
        let resolution = resolve(&corpus, "article 1er du code civil").await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        let asked: Vec<String> = corpus
            .seen_requests()
            .into_iter()
            .filter_map(|request| match request {
                PortRequest::FindArticle(_, num, _) => Some(num),
                _ => None,
            })
            .collect();
        assert_eq!(asked, ["1er", "1"]);
    }

    #[tokio::test]
    async fn an_article_with_another_number_is_a_mismatch_carrying_the_real_record() {
        let corpus = FakeCorpus::new();
        corpus.push_find_article(Ok(article(Some("10"))));
        let resolution = resolve(&corpus, "article 9 du code civil").await;
        assert_eq!(status(&resolution), CheckStatus::Invalid);
        assert_eq!(
            note(&resolution),
            "Cited article number 9 differs from the official record (10)."
        );
        assert_eq!(resolution.outcome.record().unwrap().id.as_str(), "LEGIARTI000006419288");
        assert_eq!(resolution.official.as_deref(), Some("Texte officiel."));
    }

    #[tokio::test]
    async fn a_default_sigle_reading_is_verified_with_a_note_or_stays_ambiguous() {
        let corpus = FakeCorpus::new();
        corpus.push_find_article(Ok(article(Some("121-3"))));
        let resolution = resolve(&corpus, "article 121-3 CP").await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert_eq!(note(&resolution), "Read 'CP' as Code pénal.");
        assert!(matches!(
            resolution.outcome,
            Outcome::Verified { confidence: Confidence::Medium, .. }
        ));
        let corpus = FakeCorpus::new();
        corpus.push_find_article(Err(not_found()));
        let resolution = resolve(&corpus, "article 121-3 CP").await;
        assert_eq!(status(&resolution), CheckStatus::Ambiguous);
        assert!(note(&resolution).starts_with("'CP' is ambiguous"));
    }

    #[tokio::test]
    async fn a_code_missing_from_the_list_or_a_failing_list_is_not_checked() {
        let corpus = FakeCorpus::new();
        let item = first("article 9 du code civil");
        let empty = CodeTable::from_result(Ok(vec![]));
        let resolution = resolve_with(&corpus, &empty, &item, None).await;
        assert_eq!(status(&resolution), CheckStatus::Unresolvable);
        assert_eq!(note(&resolution), "Code not found in the list of codes.");
        let failing = CodeTable::from_result(Err(upstream()));
        let resolution = resolve_with(&corpus, &failing, &item, None).await;
        assert_eq!(resolution.outcome, Outcome::Failed(upstream()));
        assert_eq!(corpus.calls(), 0);
    }

    #[test]
    fn the_code_table_prefers_the_code_in_force_among_equal_titles() {
        let table = CodeTable::from_result(Ok(vec![
            listing("LEGITEXT000000000001", "Code civil", LegalStatus::Abroge),
            listing(CIVIL, "CODE  civil", LegalStatus::Vigueur),
        ]));
        assert_eq!(table.find("Code civil").unwrap().unwrap().id.as_str(), CIVIL);
        assert!(table.find("Code inconnu").unwrap().is_none());
        assert!(CodeTable::from_result(Err(upstream())).find("Code civil").is_err());
        assert!(CodeTable::empty().find("Code civil").unwrap().is_none());
    }

    #[tokio::test]
    async fn a_version_without_a_date_is_not_supported_and_makes_no_call() {
        let corpus = FakeCorpus::new();
        let resolution = resolve(&corpus, "l'article 2270-1 ancien du code civil").await;
        assert_eq!(status(&resolution), CheckStatus::Unresolvable);
        assert_eq!(note(&resolution), "Not checkable in this version (version date unknown).");
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn a_cassation_appeal_number_is_searched_with_and_without_the_dot() {
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![]));
        corpus.push_find_by_number(Ok(vec![hit(Fond::Juri, "JURITEXT000051743650")]));
        corpus.push_get_decision(Ok(decision("JURITEXT000051743650", civil(2025, 6, 4), None)));
        let text = "Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470, Publié au bulletin";
        let resolution = resolve(&corpus, text).await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert!(matches!(
            resolution.outcome,
            Outcome::Verified { confidence: Confidence::High, .. }
        ));
        let values: Vec<String> = corpus
            .seen_requests()
            .into_iter()
            .filter_map(|request| match request {
                PortRequest::FindByNumber(query) => Some(query.value.as_str().to_owned()),
                _ => None,
            })
            .collect();
        assert_eq!(values, ["24-13.470", "24-13470"]);
    }

    #[tokio::test]
    async fn a_cassation_decision_not_found_is_not_confirmed_and_a_wrong_date_is_a_mismatch() {
        let text = "Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470, Publié au bulletin";
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![]));
        corpus.push_find_by_number(Ok(vec![]));
        let resolution = resolve(&corpus, text).await;
        assert_eq!(status(&resolution), CheckStatus::Unresolvable);
        assert_eq!(note(&resolution), "Could not confirm existence; nothing was found in JURI.");

        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![hit(Fond::Juri, "JURITEXT000051743650")]));
        corpus.push_get_decision(Ok(decision("JURITEXT000051743650", civil(2025, 6, 5), None)));
        let resolution = resolve(&corpus, text).await;
        assert_eq!(status(&resolution), CheckStatus::Invalid);
        assert_eq!(
            note(&resolution),
            "Cited date 2025-06-04 differs from the official record (2025-06-05)."
        );
    }

    #[tokio::test]
    async fn more_than_five_hits_are_ambiguous_with_five_candidates() {
        let corpus = FakeCorpus::new();
        let hits =
            (1..=6).map(|n| hit(Fond::Juri, &format!("JURITEXT{n:012}"))).collect::<Vec<_>>();
        corpus.push_find_by_number(Ok(hits));
        let resolution = resolve(
            &corpus,
            "Cour d'appel de Paris, Pôle 1 - Chambre 12, 23 juin 2025, n°24/15215",
        )
        .await;
        let (items, note) =
            candidates_of(resolution.outcome).expect("des candidats étaient attendus");
        assert_eq!(items.len(), 5);
        assert!(
            items.iter().all(|c| c.kind == CandidateKind::Juri && c.confidence == Confidence::Low)
        );
        assert_eq!(note.text(), "6 records match; call fr_resolve to list them.");
        assert_eq!(corpus.calls_of("get_decision"), 0);
    }

    #[tokio::test]
    async fn a_constitutional_decision_of_another_type_is_a_mismatch() {
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![hit(Fond::Constit, "CONSTEXT000052555275")]));
        corpus.push_get_decision(Ok(decision(
            "CONSTEXT000052555275",
            civil(2025, 11, 7),
            Some("DC"),
        )));
        let text = "Cons. const., decision n° 2025-1173 QPC du 7 novembre 2025";
        let resolution = resolve(&corpus, text).await;
        assert_eq!(status(&resolution), CheckStatus::Invalid);
        assert_eq!(
            note(&resolution),
            "Cited decision type QPC differs from the official record (DC)."
        );

        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![hit(Fond::Constit, "CONSTEXT000052555275")]));
        corpus.push_get_decision(Ok(decision(
            "CONSTEXT000052555275",
            civil(2025, 11, 7),
            Some("QPC"),
        )));
        assert_eq!(status(&resolve(&corpus, text).await), CheckStatus::Verified);
        assert!(matches!(
            &corpus.seen_requests()[0],
            PortRequest::FindByNumber(query)
                if query.fond == Fond::Constit && query.value.as_str() == "2025-1173"
        ));
    }

    fn deliberation(date: CivilDate) -> Deliberation {
        Deliberation {
            deliberation_id: CnilTextId::parse("CNILTEXT000053352643").unwrap(),
            num: Some("SAN-2026-002".to_owned()),
            nature: None,
            nature_delib: None,
            etat: None,
            date_texte: Some(date),
            date_publication: None,
            title: None,
            title_long: None,
            text: LongText::default(),
        }
    }

    #[tokio::test]
    async fn a_cnil_deliberation_is_checked_on_its_date() {
        let text = "Délibération SAN-2026-002 du 8 janvier 2026";
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![hit(Fond::Cnil, "CNILTEXT000053352643")]));
        corpus.push_get_deliberation(Ok(deliberation(civil(2026, 1, 8))));
        let resolution = resolve(&corpus, text).await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert!(matches!(
            &corpus.seen_requests()[0],
            PortRequest::FindByNumber(query)
                if query.fond == Fond::Cnil && query.value.as_str() == "SAN-2026-002"
        ));

        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![hit(Fond::Cnil, "CNILTEXT000053352643")]));
        corpus.push_get_deliberation(Ok(deliberation(civil(2026, 1, 9))));
        assert_eq!(status(&resolve(&corpus, text).await), CheckStatus::Invalid);

        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![]));
        let resolution = resolve(&corpus, text).await;
        assert_eq!(status(&resolution), CheckStatus::Unresolvable);
        assert_eq!(note(&resolution), "Could not confirm existence; nothing was found in CNIL.");
    }

    #[tokio::test]
    async fn an_idcc_nor_or_eli_is_searched_in_its_index() {
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![hit(Fond::Kali, "KALITEXT000005635096")]));
        let resolution = resolve(&corpus, "CCN Syntec (IDCC 1486, brochure n° 3018)").await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert!(matches!(
            &corpus.seen_requests()[0],
            PortRequest::FindByNumber(query)
                if query.fond == Fond::Kali && query.value.as_str() == "1486"
        ));

        let nor = standalone(CitationRef::Nor(Nor::parse("ECFI1524250L").unwrap()));
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![]));
        let resolution = resolve_with(&corpus, &codes(), &nor, None).await;
        assert_eq!(note(&resolution), "Could not confirm existence; nothing was found in LODA.");
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![hit(Fond::Loda, "LEGITEXT000033202746")]));
        assert_eq!(
            status(&resolve_with(&corpus, &codes(), &nor, None).await),
            CheckStatus::Verified
        );
    }

    fn id_citation(
        prefix: IdPrefix,
        digits: u64,
        version_start: Option<CivilDate>,
    ) -> Spanned<CitationRef> {
        standalone(CitationRef::LegifranceId(LegifranceIdRef {
            prefix,
            digits,
            version_start,
            url_kind: None,
        }))
    }

    #[tokio::test]
    async fn a_cited_identifier_is_consulted_by_its_family() {
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(decision("JURITEXT000051743650", civil(2025, 6, 4), None)));
        let item = id_citation(IdPrefix::JuriText, 51_743_650, None);
        let resolution = resolve_with(&corpus, &codes(), &item, None).await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert_eq!(resolution.official.as_deref(), Some("Texte de la décision."));

        let corpus = FakeCorpus::new();
        corpus.push_get_act(Ok(Act {
            text_id: LegiTextId::parse("LEGITEXT000033205014").unwrap(),
            body: TextBody::default(),
        }));
        let item = id_citation(IdPrefix::LegiText, 33_205_014, Some(civil(2023, 9, 8)));
        assert_eq!(
            status(&resolve_with(&corpus, &codes(), &item, None).await),
            CheckStatus::Verified
        );
        assert_eq!(
            corpus.seen_requests(),
            vec![PortRequest::GetAct(
                LegiTextId::parse("LEGITEXT000033205014").unwrap(),
                VersionDate::parse("2023-09-08").unwrap()
            )]
        );

        // Sans version citée : le jour de Paris.
        let corpus = FakeCorpus::new();
        corpus.push_get_act(Err(not_found()));
        let item = id_citation(IdPrefix::LegiText, 33_205_014, None);
        let resolution = resolve_with(&corpus, &codes(), &item, None).await;
        assert!(matches!(resolution.outcome, Outcome::Absent { .. }));
        assert!(
            matches!(&corpus.seen_requests()[0], PortRequest::GetAct(_, date) if *date == today())
        );
    }

    #[tokio::test]
    async fn identifiers_without_a_consultation_endpoint_are_not_supported() {
        let corpus = FakeCorpus::new();
        for item in [
            id_citation(IdPrefix::JorfText, 33_202_746, None),
            id_citation(IdPrefix::LegiScta, 6_070_721, None),
            id_citation(IdPrefix::KaliCont, 5_635_096, None),
        ] {
            let resolution = resolve_with(&corpus, &codes(), &item, None).await;
            assert_eq!(status(&resolution), CheckStatus::Unresolvable);
            assert!(note(&resolution).starts_with("Not checkable in this version"));
        }
        assert_eq!(corpus.calls(), 0);
    }

    const LAW: &str = "article 6 de la loi n° 2016-1321 du 7 octobre 2016";

    fn law_hit() -> SearchHit {
        let mut law = hit(Fond::Loda, "LEGITEXT000033202746");
        law.nature = Some("LOI".to_owned());
        law.date = Some(civil(2016, 10, 7));
        law
    }

    #[tokio::test]
    async fn an_article_of_a_resolved_text_is_searched_in_that_text() {
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![law_hit()]));
        corpus.push_find_article(Ok(article(Some("6"))));
        let resolution = resolve(&corpus, LAW).await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert_eq!(
            corpus.seen_requests()[1],
            PortRequest::FindArticle(
                LegiTextId::parse("LEGITEXT000033202746").unwrap(),
                "6".to_owned(),
                ArticleAt::Today(today())
            )
        );
    }

    #[tokio::test]
    async fn absence_in_a_text_needs_a_sure_text() {
        // Texte sûr (date citée et concordante) : l'absence est une preuve.
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![law_hit()]));
        corpus.push_find_article(Err(not_found()));
        let resolution = resolve(&corpus, LAW).await;
        assert!(matches!(resolution.outcome, Outcome::Absent { .. }));
        assert_eq!(
            note(&resolution),
            "LEGITEXT000033202746 has no article 6 in force on 2026-10-29."
        );
        // Texte sans date citée : confiance moyenne, l'absence ne prouve rien.
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![law_hit()]));
        corpus.push_find_article(Err(not_found()));
        let resolution = resolve(&corpus, "article 6 de la loi n° 2016-1321").await;
        assert_eq!(status(&resolution), CheckStatus::Unresolvable);
    }

    #[tokio::test]
    async fn a_text_of_another_nature_or_several_texts_are_not_confirmed() {
        let corpus = FakeCorpus::new();
        let mut decree = law_hit();
        decree.nature = Some("DECRET".to_owned());
        corpus.push_find_by_number(Ok(vec![decree]));
        let resolution = resolve(&corpus, "loi n° 2016-1321 du 7 octobre 2016").await;
        assert_eq!(status(&resolution), CheckStatus::Invalid);
        assert_eq!(
            note(&resolution),
            "Cited nature LOI differs from the official record (DECRET)."
        );

        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![law_hit(), law_hit()]));
        let resolution = resolve(&corpus, "loi n° 2016-1321 du 7 octobre 2016").await;
        assert_eq!(status(&resolution), CheckStatus::Ambiguous);

        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![]));
        let resolution = resolve(&corpus, "loi n° 2016-1321 du 7 octobre 2016").await;
        assert_eq!(status(&resolution), CheckStatus::Unresolvable);
    }

    #[test]
    fn the_parser_verdicts_are_decided_offline() {
        let cases = [
            (
                "Cass. civ. 4e, 3 mars 2020, n° 18-12.345",
                CheckStatus::Invalid,
                "There is no such civil chamber at the Cour de cassation.",
            ),
            ("CE, 31 février 2020, n° 123456", CheckStatus::Invalid, "The date does not exist."),
            (
                "art. L. 52-8 CE",
                CheckStatus::Ambiguous,
                "'CE' is ambiguous (Conseil d'État, Code électoral); name the code in full.",
            ),
            (
                "conformément à l'article 12",
                CheckStatus::Ambiguous,
                "No code or text given for article 12; add it or call fr_resolve.",
            ),
            (
                "Décision 2025-1173",
                CheckStatus::Ambiguous,
                "No court given: name it, or call fr_resolve to list the matching decisions.",
            ),
            (
                "Crim., 28 mars 2018",
                CheckStatus::Ambiguous,
                "No case number: use fr_search (fond=JURI, date_from=2018-03-28, date_to=2018-03-28).",
            ),
            (
                "l'avenant n° 18",
                CheckStatus::Ambiguous,
                "No collective agreement identified; give its IDCC or its title.",
            ),
            (
                "Bull. civ. I, n° 270",
                CheckStatus::Unresolvable,
                "Not checkable in this version (publication reference).",
            ),
            (
                "note sous D. 2016, p. 1234",
                CheckStatus::Unresolvable,
                "Legal scholarship is not on Légifrance (see EUR-Lex / HUDOC).",
            ),
            (
                "la loi Macron",
                CheckStatus::Unresolvable,
                "A popular name is not a citation; find the text with fr_search.",
            ),
        ];
        for (text, expected, expected_note) in cases {
            let item = first(text);
            let done = offline(&item);
            assert!(done.is_some(), "{text:?} devait être tranchée hors ligne");
            let done = done.unwrap();
            assert_eq!(status(&done), expected, "{text}");
            assert_eq!(note(&done), expected_note, "{text}");
        }
        // Une citation cherchable n'est pas tranchée hors ligne.
        assert!(offline(&first("article 9 du code civil")).is_none());
        assert!(!needs_codes(&first("la loi Macron")));
        assert!(needs_codes(&first("article 9 du code civil")));
        assert!(!needs_codes(&first("Crim., 28 mars 2018")));
    }

    #[test]
    fn jorf_and_preamble_paragraphs_are_not_supported_offline() {
        use lf_domain::citation::JorfCite;
        let jorf = standalone(CitationRef::Jorf(JorfCite {
            number: Some(263),
            date: None,
            texte: Some(152),
            page: None,
        }));
        assert_eq!(note(&offline(&jorf).unwrap()), "Not checkable in this version (JORF).");
    }

    #[test]
    fn statuses_follow_the_environment_for_an_absence() {
        let absent = Outcome::Absent {
            prod: Note::NoRecord { family: "text", value: "1".to_owned(), date: None },
            sandbox: Note::NotInSandbox { family: "text", value: "1".to_owned(), date: None },
        };
        assert_eq!(absent.status(PisteEnvironment::Production), CheckStatus::NotFound);
        assert_eq!(absent.status(PisteEnvironment::Sandbox), CheckStatus::Unresolvable);
        assert_eq!(
            absent.note(PisteEnvironment::Production).unwrap().text(),
            "No text record matches 1."
        );
        assert!(absent.note(PisteEnvironment::Sandbox).unwrap().text().contains("sandbox"));
        let failed = Outcome::Failed(upstream());
        assert_eq!(failed.status(PisteEnvironment::Production), CheckStatus::Unresolvable);
        assert_eq!(
            failed.note(PisteEnvironment::Production).unwrap().text(),
            "Not checked: upstream error."
        );
        assert!(failed.record().is_none());
    }
}
