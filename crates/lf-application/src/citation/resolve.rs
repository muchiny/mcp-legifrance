//! Moteur de résolution d'une citation (annexe B §7.1 à §7.6), partagé par `fr_verify_citations`
//! et, en V4, par `fr_resolve`.
//!
//! `Resolver::resolve_ref` prend une citation reconnue par le parseur et rend une
//! `Resolution` : un `Outcome` (vérifiée, en écart, plusieurs enregistrements, absente,
//! non résoluble, échec amont) et, quand le texte officiel a été lu, ce texte, que l'appelant
//! compare à l'extrait cité. Le moteur ne sait rien du transport ni du format de sortie.
//!
//! # Règles
//!
//! - **Identifiant lu, jamais construit** (§7.1) : tout [`ResolvedRecord`] vient d'une réponse du
//!   corpus. Une citation `LegifranceId` consulte l'identifiant tel quel.
//! - **Pas d'absence sans preuve** (§7.2) : `Outcome::Absent` n'est rendu que si la table
//!   `policy::POLICY` l'autorise pour la famille ; ailleurs, l'absence est
//!   `Unresolvable`. Dans une source qui ne prouve rien (bac à sable de PISTE, instantané local de
//!   l'index DILA, voir `Proof`), `Outcome::status` la rend `unresolvable` (jamais `not_found`).
//! - **Hors ligne d'abord** : `offline` tranche sans appel ce que les drapeaux, les ambiguïtés et
//!   la résolvabilité du parseur permettent de trancher (N02, N20, N32…).
//!
//! # Limites de cette version
//!
//! L'article d'un texte est cherché par `find_article` sur le `LEGITEXT` résolu, sans parcourir
//! la table des matières. Un texte cité par sa seule date, la formation du Conseil d'État, le
//! siège d'une cour d'appel ou d'un tribunal, les avenants et les alinéas du Préambule ne sont pas
//! contrôlés (`NotSupported` ou ignorés). La chambre de la Cour de cassation l'est, sur les seules
//! chambres modernes (`check_chamber`).

use lf_domain::{
    citation::{
        Ambiguity, ArticleNum, ArticleRef, CaseNumber, CitationRef, CnilCite, CnilNumber, CodeRef,
        Confidence, ConventionCite, Court, DecisionCite, Flag, LegifranceIdRef, Resolvability,
        Spanned, TextCite, TextNature, TextNumber, UnresolvableWhy, UnsupportedFamily,
        VersionQualifier,
        dict::courts::{CassChamber, CcType},
        parse,
    },
    contract::{Citable, Eli, Nor},
    date::{CivilDate, VersionDate},
    ecli::Ecli,
    entity::{CodeListing, Decision, SearchHit},
    fond::Fond,
    id::{ArticleId, CnilTextId, DecisionId, DocumentId, LegiTextId},
    search::{NUMBER_VALUE_MAX_CHARS, NumberKind, NumberQuery, NumberValue},
    status::LegalStatus,
    url::WEB_BASE,
};

use super::{
    Candidate, CandidateKind, CheckStatus, Field, NotCheckedReason, Note, ResolvedRecord,
    notes::FOUND_DATES_MAX,
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
    /// Preuve d'absence réunie (§7.2) : `not_found` quand la source la prouve (production).
    Absent {
        /// Note de production (`NotInForce`, `NoRecord`).
        prod: Note,
        /// Ce qui a été cherché en vain : sert les notes des sources qui ne prouvent rien.
        miss: Miss,
    },
    /// Contrôle impossible, pour la raison de la note.
    Unresolvable {
        /// Pourquoi.
        note: Note,
    },
    /// Le corpus a échoué : la citation n'a pas été contrôlée.
    Failed(CorpusError),
}

/// Ce que prouve l'absence d'un enregistrement, selon la source interrogée (§7.2, §7.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Proof {
    /// PISTE en production, interrogée en direct : l'absence établit l'inexistence (`not_found`).
    Production,
    /// Bac à sable de PISTE : ses données diffèrent de la production (`unresolvable`).
    Sandbox,
    /// Index local DILA : un instantané peut être périmé, l'absence ne prouve rien
    /// (`unresolvable`).
    Snapshot,
}

impl Proof {
    /// La source que désignent l'environnement de la politique d'audit et le drapeau d'instantané
    /// local de la configuration. L'index local se déclare `production` dans l'audit : le drapeau
    /// l'emporte, sinon un dump périmé ferait déclarer inexistant un texte récent.
    pub(crate) const fn of(env: PisteEnvironment, local_snapshot: bool) -> Self {
        match (local_snapshot, env) {
            (true, _) => Self::Snapshot,
            (false, PisteEnvironment::Production) => Self::Production,
            (false, PisteEnvironment::Sandbox) => Self::Sandbox,
        }
    }
}

/// Ce qui a été cherché en vain : la famille (vocabulaire fermé), la valeur canonique et la date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Miss {
    family: &'static str,
    value: String,
    date: Option<VersionDate>,
}

impl Miss {
    /// Note d'une absence constatée dans le bac à sable de PISTE.
    fn in_sandbox(&self) -> Note {
        Note::NotInSandbox { family: self.family, value: self.value.clone(), date: self.date }
    }

    /// Note d'une absence constatée dans l'instantané local.
    fn in_snapshot(&self) -> Note {
        Note::NotInSnapshot { family: self.family, value: self.value.clone(), date: self.date }
    }
}

impl Outcome {
    /// Statut rendu pour cette issue pour la source `proof` (§7.6) : une absence n'est `not_found`
    /// que si la source la prouve, sinon elle est `unresolvable`.
    pub(crate) fn status(&self, proof: Proof) -> CheckStatus {
        match self {
            Self::Verified { .. } => CheckStatus::Verified,
            Self::Mismatch { .. } | Self::Invalid { .. } => CheckStatus::Invalid,
            Self::Candidates { .. } => CheckStatus::Ambiguous,
            Self::Absent { .. } if proof == Proof::Production => CheckStatus::NotFound,
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

    /// Note de l'issue pour la source `proof`.
    pub(crate) fn note(&self, proof: Proof) -> Option<Note> {
        match self {
            Self::Verified { note, .. } => note.clone(),
            Self::Mismatch { note, .. }
            | Self::Invalid { note }
            | Self::Candidates { note, .. }
            | Self::Unresolvable { note } => Some(note.clone()),
            Self::Absent { prod, miss } => Some(match proof {
                Proof::Production => prod.clone(),
                Proof::Sandbox => miss.in_sandbox(),
                Proof::Snapshot => miss.in_snapshot(),
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
        UnsupportedFamily::Cedh => "ECHR law and case law",
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
    chamber: Option<CassChamber>,
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
        self.search(item, at).await
    }

    /// Cherche dans le corpus la citation que `offline` n'a pas tranchée. Les familles que
    /// `offline` tranche toujours (JORF, hors périmètre, article sans texte) rendent ici
    /// `NotSupported` : la garde reste sûre si `offline` change.
    async fn search(&self, item: &Spanned<CitationRef>, at: Option<VersionDate>) -> Resolution {
        match &item.value {
            CitationRef::CodeArticle { code, article, version, .. } => {
                let first = self.code_article(code, &article.num, version.as_ref(), at).await;
                let resolution = close_range(first, article.range_to.as_ref(), async |end| {
                    self.code_article(code, end, version.as_ref(), at).await
                })
                .await;
                with_sigle_reading(item, code, resolution)
            }
            CitationRef::TextArticle { text: Some(text), article, at_date } => {
                self.text_article(text, article, *at_date, at).await
            }
            CitationRef::Text(text) => self.text(text).await.into_resolution(),
            CitationRef::Decision(decision) => self.decision(decision).await,
            CitationRef::Ecli(ecli) => self.ecli(ecli).await,
            CitationRef::CnilDeliberation(cnil) => self.cnil(cnil).await,
            CitationRef::Convention(convention) => self.convention(convention).await,
            CitationRef::Nor(nor) => self.by_nor(nor).await,
            CitationRef::Eli(eli) => self.eli(eli).await,
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
        // D5 : une date explicite qui vaut le jour de Paris est la version en vigueur.
        let at = match when {
            Some(date) if date != self.today => ArticleAt::On(date),
            _ => ArticleAt::Today(self.today),
        };
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
        let miss = Miss {
            family: "article",
            value: format!("{label}, art. {key}"),
            date: Some(at.date()),
        };
        let family = if absence_proven { Family::CodeArticle } else { Family::Text };
        Resolution::of(Self::when_absent(
            family,
            prod,
            miss,
            Note::CouldNotConfirm { index: "LODA" },
        ))
    }

    /// Issue d'une absence selon la table de la politique (§7.2).
    fn when_absent(family: Family, prod: Note, miss: Miss, fallback: Note) -> Outcome {
        match policy::absence(family) {
            Absence::Allowed => Outcome::Absent { prod, miss },
            Absence::AfterVerification | Absence::Never => Outcome::Unresolvable { note: fallback },
        }
    }

    /// Article d'un texte non codifié (ou plage d'articles) : résout le texte une fois, puis
    /// l'article dans ce texte, et la dernière borne d'une plage.
    async fn text_article(
        &self,
        text: &TextCite,
        article: &ArticleRef,
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
                let first = self.article_in(&id, id.as_str(), &article.num, when, sure).await;
                close_range(first, article.range_to.as_ref(), async |end| {
                    self.article_in(&id, id.as_str(), end, when, sure).await
                })
                .await
            }
            TextStep::Done(done) => done,
        }
    }

    /// Texte numéroté (LODA) : `find_by_number`, puis comparaison de la nature et de la date.
    #[expect(
        clippy::indexing_slicing,
        reason = "les indices viennent des motifs de tranche ou de l'énumération des mêmes listes"
    )]
    async fn text(&self, cite: &TextCite) -> TextStep {
        let Some(number) = &cite.number else {
            return TextStep::Done(not_supported("text cited without a number"));
        };
        // Premier jeu de résultats LEGI parmi les formes du numéro (`89-462`, puis `1989-462`) :
        // la sandbox ne connaît que la forme à deux chiffres des textes d'avant 2000.
        let mut found: Vec<(LegiTextId, SearchHit)> = Vec::new();
        for key in text_keys(number) {
            let hits = match self.by_number(Fond::Loda, NumberKind::Text, &key).await {
                Ok(hits) => hits,
                Err(error) => return TextStep::Done(Resolution::failed(error)),
            };
            found = hits
                .into_iter()
                .filter_map(|hit| match &hit.id {
                    DocumentId::LegiText(id) => Some((id.clone(), hit)),
                    _ => None,
                })
                .collect();
            if !found.is_empty() {
                break;
            }
        }
        let (ids, hits): (Vec<LegiTextId>, Vec<SearchHit>) = found.into_iter().unzip();
        if hits.is_empty() {
            return TextStep::Done(Resolution::unresolvable(Note::CouldNotConfirm {
                index: "LODA",
            }));
        }
        // La date de signature se lit une fois par hit : l'ajustement et la confiance du hit
        // retenu partent de la même lecture.
        let signed: Vec<Option<CivilDate>> =
            hits.iter().map(|hit| signed_on(hit, number)).collect();
        let fits: Vec<TextFit> =
            hits.iter().zip(&signed).map(|(hit, &signed)| text_fit(cite, hit, signed)).collect();
        let exact: Vec<usize> = fits
            .iter()
            .enumerate()
            .filter(|(_, fit)| **fit == TextFit::Exact)
            .map(|(i, _)| i)
            .collect();
        match (exact.as_slice(), hits.as_slice()) {
            ([only], _) => {
                let hit = &hits[*only];
                // Sûr seulement si la date citée a été comparée à celle du titre, et égale.
                let compared = cite.date.is_some() && signed[*only] == cite.date;
                let confidence = if compared { Confidence::High } else { Confidence::Medium };
                let record = ResolvedRecord::of(hit.id.clone(), hit);
                TextStep::Found { id: ids[*only].clone(), record, confidence }
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
            Court::ConseilEtat { .. } => {
                (Some(is_conseil_etat), Note::CouldNotConfirm { index: "CETAT" })
            }
            Court::Caa { .. } => (Some(is_caa), Note::OutOfCoverage { court: "CAA" }),
            Court::Ta { .. } => (Some(is_ta), Note::OutOfCoverage { court: "TA" }),
            Court::ConseilConstitutionnel => (None, Note::CouldNotConfirm { index: "CONSTIT" }),
        };
        let cc_type = cite.numbers.iter().find_map(|number| match number {
            CaseNumber::Cc(cc) => Some(cc.kind),
            _ => None,
        });
        let chamber = match court {
            Court::CourDeCassation { chamber } => *chamber,
            _ => None,
        };
        self.lookup(Lookup {
            fond,
            kind,
            forms,
            title_filter,
            date: cite.date,
            cc_type,
            chamber,
            absent,
        })
        .await
    }

    /// Cherche une décision par numéro, la consulte et compare la date (et le type du Conseil
    /// constitutionnel).
    async fn lookup(&self, spec: Lookup) -> Resolution {
        let mut hits = Vec::new();
        let mut number = String::new();
        for form in &spec.forms {
            match self.by_number(spec.fond, spec.kind, form).await {
                Ok(found) if found.is_empty() => {}
                Ok(found) => {
                    hits = found;
                    number.clone_from(form);
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
            if let Some(cited) = spec.date
                && let Some(dates) = hits.iter().map(judged_on).collect::<Option<Vec<_>>>()
                && !dates.contains(&cited)
            {
                return no_record_at_date(&number, cited, dates);
            }
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
        if let Some(cited) = spec.date
            && none_on_the_day(&found, cited)
        {
            let dates = found.iter().filter_map(|(_, decision)| decision.date_texte).collect();
            return no_record_at_date(&number, cited, dates);
        }
        choose_decision(&found, spec.date, spec.cc_type, spec.chamber)
    }

    /// ECLI cité : numéro tiré de l'ordinal (CC, CE) ou facette ECLI (Cour de cassation), puis
    /// égalité stricte avec l'ECLI natif du document consulté.
    async fn ecli(&self, cited: &Ecli) -> Resolution {
        // Un ECLI validé a cinq éléments `ECLI:FR:<juridiction>:<année>:<ordinal>` ; la valeur par
        // défaut (vide) ne correspond à aucune juridiction prise en charge.
        let mut parts = cited.as_str().split(':');
        let court = parts.nth(2).unwrap_or_default();
        let ordinal = parts.nth(1).unwrap_or_default();
        let mut pieces = ordinal.split('.');
        let (first, second) = (pieces.next().unwrap_or_default(), pieces.next());
        let lookup = |fond, kind, form: String, filter, absent| Lookup {
            fond,
            kind,
            forms: vec![form],
            title_filter: filter,
            date: None,
            cc_type: None,
            chamber: None,
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
                    Some(is_conseil_etat),
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

    /// ELI cité : l'API reçoit le chemin `/eli/...` sans hôte (04 §7.2), jamais l'URL absolue.
    /// Un chemin plus long que `NUMBER_VALUE_MAX_CHARS` ne tient pas dans une requête par numéro.
    async fn eli(&self, eli: &Eli) -> Resolution {
        let absolute = eli.as_str();
        let path = absolute.strip_prefix(WEB_BASE).unwrap_or(absolute);
        if path.chars().count() > NUMBER_VALUE_MAX_CHARS {
            return not_supported("an ELI this long");
        }
        self.by_index(NumberKind::Eli, path, "ELI").await
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
                let miss = Miss { family: "identifier", value: label, date: None };
                Resolution::of(Self::when_absent(
                    Family::LegifranceId,
                    prod,
                    miss,
                    Note::CouldNotConfirm { index: "LEGI" },
                ))
            }
            Err(error) => Resolution::failed(error),
        }
    }

    /// Lit le document d'identifiant `id` avec la méthode de sa famille. L'identifiant porte déjà
    /// son type : aucune relecture de sa forme textuelle, donc aucun échec de validation possible.
    async fn consult(
        &self,
        id: &DocumentId,
        cited: &LegifranceIdRef,
    ) -> Result<Resolution, CorpusError> {
        Ok(match id {
            DocumentId::LegiArti(arti) => {
                let article = self.corpus.get_article(&ArticleId::Legi(arti.clone())).await?;
                found_with_text(id, &article, article.text.as_str())
            }
            DocumentId::KaliArti(arti) => {
                let article = self.corpus.get_article(&ArticleId::Kali(arti.clone())).await?;
                found_with_text(id, &article, article.text.as_str())
            }
            DocumentId::LegiText(text) => {
                let date = cited
                    .version_start
                    .and_then(|date| VersionDate::try_from(date).ok())
                    .unwrap_or(self.today);
                let act = self.corpus.get_act(text, date).await?;
                found_with_text(id, &act, "")
            }
            DocumentId::JuriText(text) => {
                let decision = self.corpus.get_decision(&DecisionId::Juri(text.clone())).await?;
                found_with_text(id, &decision, decision.text.as_str())
            }
            DocumentId::ConsText(text) => {
                let decision = self.corpus.get_decision(&DecisionId::Constit(text.clone())).await?;
                found_with_text(id, &decision, decision.text.as_str())
            }
            DocumentId::CetaText(text) => {
                let decision = self.corpus.get_decision(&DecisionId::Cetat(text.clone())).await?;
                found_with_text(id, &decision, decision.text.as_str())
            }
            DocumentId::CnilText(text) => {
                let deliberation = self.corpus.get_deliberation(text).await?;
                found_with_text(id, &deliberation, "")
            }
            DocumentId::KaliText(text) => {
                let convention = self.corpus.get_convention(text).await?;
                found_with_text(id, &convention, "")
            }
            DocumentId::AccoText(text) => {
                let agreement = self.corpus.get_company_agreement(text).await?;
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

/// Formes du numéro à chercher dans l'index NUM, dans l'ordre : `AA-N` puis `AAAA-N` avant 2000
/// (le `AA` est celui que la sandbox indexe), `AAAA-N` après, `N` sans année.
fn text_keys(number: &TextNumber) -> Vec<String> {
    match number.year {
        None => vec![number.seq.to_string()],
        Some(year @ 1900..=1999) => {
            vec![format!("{:02}-{}", year % 100, number.seq), format!("{year}-{}", number.seq)]
        }
        Some(year) => vec![format!("{year}-{}", number.seq)],
    }
}

/// Date de signature d'un texte, lue dans le titre du hit (« Loi n° 2016-1321 du 7 octobre
/// 2016 … ») : le titre cite d'autres textes, seule la citation du même numéro compte. `hit.date`
/// n'est jamais lue (entrée en vigueur ou publication selon le fond). `None` si le titre manque
/// ou ne se lit pas.
fn signed_on(hit: &SearchHit, number: &TextNumber) -> Option<CivilDate> {
    let title = hit.title.as_deref()?;
    parse(title).into_iter().find_map(|item| match item.value {
        CitationRef::Text(text)
            if text
                .number
                .as_ref()
                .is_some_and(|read| read.year == number.year && read.seq == number.seq) =>
        {
            text.date
        }
        _ => None,
    })
}

/// Ajustement du hit à la citation, `signed` étant la date de signature lue dans son titre
/// (`signed_on`).
fn text_fit(cite: &TextCite, hit: &SearchHit, signed: Option<CivilDate>) -> TextFit {
    if let (Some(wanted), Some(actual)) = (plain_nature(cite.nature), hit.nature.as_deref())
        && fold_line(actual) != fold_line(wanted)
    {
        return TextFit::Nature(fold_line(actual).to_uppercase());
    }
    match (cite.date, signed) {
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

/// Plage d'articles (« articles 1240 à 1244 », L2-07) : la dernière borne `end` est lue par le même
/// chemin que la première, mais seulement quand la première est vérifiée (sinon son issue est
/// rendue, sans second appel). Les deux bornes vérifiées donnent `verified` de confiance moyenne,
/// avec la note qui dit que le milieu n'est pas lu, sans comparaison d'extrait. Une dernière borne
/// absente (preuve d'absence, ou « existence non confirmée » d'un texte sans date comparée) donne
/// `ambiguous`, jamais `invalid` ni `not_found` : la plage peut être juste pour une autre date.
/// Toute autre issue de la dernière borne est rendue telle quelle. Sans `end`, la première
/// résolution est rendue inchangée.
async fn close_range(
    first: Resolution,
    end: Option<&ArticleNum>,
    resolve_end: impl AsyncFnOnce(&ArticleNum) -> Resolution,
) -> Resolution {
    let (Some(end), Outcome::Verified { record, .. }) = (end, &first.outcome) else {
        return first;
    };
    let record = record.clone();
    let outcome = match resolve_end(end).await.outcome {
        Outcome::Verified { .. } => Outcome::Verified {
            record,
            confidence: Confidence::Medium,
            note: Some(Note::RangeEndpointsOnly),
        },
        Outcome::Absent { .. } | Outcome::Unresolvable { note: Note::CouldNotConfirm { .. } } => {
            Outcome::Candidates {
                items: Vec::new(),
                note: Note::RangeEndNotFound { num: end.num_key() },
            }
        }
        other => other,
    };
    Resolution::of(outcome)
}

/// Issue d'un article trouvé : cohérence du numéro, texte officiel, début de la version courante.
fn article_found(
    article: &lf_domain::entity::ArticleText,
    variants: &[String],
    at: ArticleAt,
) -> Resolution {
    let id = match &article.article_id {
        ArticleId::Legi(id) => DocumentId::LegiArti(id.clone()),
        ArticleId::Kali(id) => DocumentId::KaliArti(id.clone()),
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
    Resolution {
        official: official_text(text),
        ..verified(ResolvedRecord::of(id.clone(), entity), Confidence::High, None)
    }
}

/// Texte officiel lu, absent quand le document n'en porte pas. Hors de `found_with_text`, pour
/// que ses deux branches ne se dédoublent pas en une instance par type d'entité.
fn official_text(text: &str) -> Option<String> {
    if text.is_empty() { None } else { Some(text.to_owned()) }
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

/// Date de la décision, lue dans le titre du hit (« Cour de cassation, civile, Chambre sociale,
/// 21 mai 2025, 22-11.901 », « Conseil d'État, …, 30/01/2020, 419837 »). Les hits JURI, CETAT et
/// CONSTIT n'ont aucune date propre : `date`, `datePublication` et `dateSignature` valent null.
/// Seule la première décision du titre compte (celui du Conseil constitutionnel cite des lois).
/// `None` si le titre manque ou ne se lit pas.
fn judged_on(hit: &SearchHit) -> Option<CivilDate> {
    let title = hit.title.as_deref()?;
    parse(title).into_iter().find_map(|item| match item.value {
        CitationRef::Decision(decision) => Some(decision.date),
        _ => None,
    })?
}

/// Plusieurs décisions portent ce numéro et toutes sont datées d'un autre jour que `cited`. Une
/// décision sans date ne peut rien écarter : elle laisse la question ouverte.
fn none_on_the_day(found: &[(DocumentId, Decision)], cited: CivilDate) -> bool {
    found.len() > 1
        && found.iter().all(|(_, decision)| decision.date_texte.is_some_and(|date| date != cited))
}

/// Des enregistrements portent ce numéro, aucun la date citée : `unresolvable`, et non « rien
/// trouvé » (`absent`), puisque des enregistrements existent. `dates` est la date de chacun
/// (un enregistrement par élément) : la note donne leur nombre et leurs dates distinctes
/// (croissantes, bornées), pour que l'appelant ne conclue pas que le numéro n'existe pas.
fn no_record_at_date(number: &str, cited: CivilDate, mut dates: Vec<CivilDate>) -> Resolution {
    let n = dates.len();
    dates.sort_unstable();
    dates.dedup();
    let more = dates.len().saturating_sub(FOUND_DATES_MAX);
    dates.truncate(FOUND_DATES_MAX);
    Resolution::unresolvable(Note::NoRecordAtDate {
        n,
        number: number.to_owned(),
        date: cited,
        found: dates,
        more,
    })
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

/// Chambres que la formation du corpus désigne sans ambiguïté (annexe B §4.5) : seules elles se
/// comparent. Les chambres historiques (`Reunies`, `CivUnique`, `Requetes`) n'ont pas de valeur
/// de facette établie ou changent de sens selon l'époque.
const fn is_modern(chamber: CassChamber) -> bool {
    matches!(
        chamber,
        CassChamber::Civ1
            | CassChamber::Civ2
            | CassChamber::Civ3
            | CassChamber::Com
            | CassChamber::Soc
            | CassChamber::Crim
            | CassChamber::AssPlen
            | CassChamber::Mixte
            | CassChamber::PremierPresident
    )
}

/// Ce que la comparaison de la chambre citée avec la formation officielle a établi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChamberCheck {
    /// Rien à comparer, ou chambres identiques.
    Fine,
    /// Les deux côtés sont des chambres modernes et diffèrent : (citée, officielle).
    Differs(CassChamber, CassChamber),
    /// Chambre moderne citée, mais la formation officielle n'en est pas une.
    NotComparable,
}

/// Compare la chambre citée (Cour de cassation) à la `formation` de la décision consultée. Jamais
/// d'écart si un côté est absent ou n'est pas une chambre moderne (Q7) : une formation inattendue
/// (juridiction du fond, code inconnu) ne donne qu'une note.
fn check_chamber(cited: Option<CassChamber>, formation: Option<&str>) -> ChamberCheck {
    let Some(cited) = cited.filter(|chamber| is_modern(*chamber)) else {
        return ChamberCheck::Fine;
    };
    let Some(formation) = formation.map(str::trim).filter(|value| !value.is_empty()) else {
        return ChamberCheck::Fine;
    };
    match CassChamber::from_facet(formation).filter(|chamber| is_modern(*chamber)) {
        Some(actual) if actual == cited => ChamberCheck::Fine,
        Some(actual) => ChamberCheck::Differs(cited, actual),
        None => ChamberCheck::NotComparable,
    }
}

/// Choisit parmi les décisions consultées celle que la citation désigne (date citée, type du
/// Conseil constitutionnel, chambre de la Cour de cassation). Tous les champs durs en écart sont
/// rendus ensemble.
#[expect(
    clippy::indexing_slicing,
    reason = "les indices viennent des motifs de tranche ou de l'énumération des mêmes listes"
)]
fn choose_decision(
    found: &[(DocumentId, Decision)],
    date: Option<CivilDate>,
    cc_type: Option<CcType>,
    chamber: Option<CassChamber>,
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
    let mut gaps: Vec<(Field, String, String)> = Vec::new();
    if let (Some(cited), Some(actual)) = (date, decision.date_texte)
        && cited != actual
    {
        gaps.push((Field::Date, cited.to_string(), actual.to_string()));
    }
    let chamber_check = check_chamber(chamber, decision.formation.as_deref());
    if let ChamberCheck::Differs(cited, actual) = chamber_check {
        gaps.push((Field::Chamber, cited.code().to_owned(), actual.code().to_owned()));
    }
    if let (Some(kind), Some(actual)) = (cc_type, decision.nature.as_deref())
        && fold_line(actual) != fold_line(kind.as_str())
    {
        gaps.push((Field::CcType, kind.as_str().to_owned(), fold_line(actual).to_uppercase()));
    }
    let note = match gaps.len() {
        0 => None,
        1 => gaps.pop().map(|(field, cited, actual)| Note::Mismatch { field, cited, actual }),
        _ => Some(Note::Mismatches(gaps)),
    };
    if let Some(note) = note {
        return Resolution { official, ..Resolution::of(Outcome::Mismatch { record, note }) };
    }
    let note = (chamber_check == ChamberCheck::NotComparable).then_some(Note::ChamberNotCompared);
    Resolution { official, ..verified(record, confidence, note) }
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

#[expect(
    clippy::arithmetic_side_effects,
    reason = "tests : valeurs bornées par les données du test"
)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeCorpus, PortRequest};
    use lf_domain::{
        citation::{
            Anaphora, ArticleRef, EcliFamily, IdPrefix, Sigle, Span, SpecialNum, UnsupportedCite,
        },
        contract::{Idcc, Nor},
        entity::{Act, ArticleText, CompanyAgreement, Convention, Deliberation, TextBody},
        id::{AccoTextId, ArticleId, CnilTextId, KaliTextId},
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
            cited_case_law: None,
            applied_texts: vec![],
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
        resolution.outcome.status(Proof::Production)
    }

    fn note(resolution: &Resolution) -> String {
        resolution.outcome.note(Proof::Production).map(|n| n.text()).unwrap_or_default()
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
        for _ in 0..3 {
            corpus.push_find_article(Err(not_found()));
        }
        let item = first("article 9 du code civil");
        let on = VersionDate::parse("2018-01-01").unwrap();
        resolve_with(&corpus, &codes(), &item, None).await;
        resolve_with(&corpus, &codes(), &item, Some(on)).await;
        // D5 : une date explicite égale au jour de Paris est la version en vigueur.
        resolve_with(&corpus, &codes(), &item, Some(today())).await;
        let civil_code = LegiTextId::parse(CIVIL).unwrap();
        assert_eq!(
            corpus.seen_requests(),
            vec![
                PortRequest::FindArticle(civil_code.clone(), "9".into(), ArticleAt::Today(today())),
                PortRequest::FindArticle(civil_code.clone(), "9".into(), ArticleAt::On(on)),
                PortRequest::FindArticle(civil_code, "9".into(), ArticleAt::Today(today())),
            ]
        );
    }

    #[tokio::test]
    async fn qualifier_date_equal_to_today_uses_article_at_today() {
        let corpus = FakeCorpus::new();
        corpus.push_find_article(Err(not_found()));
        let item = first("article 9 du code civil en vigueur au 29 octobre 2026");
        resolve_with(&corpus, &codes(), &item, None).await;
        let civil_code = LegiTextId::parse(CIVIL).unwrap();
        assert_eq!(
            corpus.seen_requests(),
            vec![PortRequest::FindArticle(civil_code, "9".into(), ArticleAt::Today(today()))]
        );
    }

    #[tokio::test]
    async fn eli_is_sent_as_a_path_without_host() {
        const PATH: &str = "/eli/loi/2016/10/7/ECFI1524250L/jo/texte";
        let eli = standalone(CitationRef::Eli(Eli::parse(PATH).unwrap()));
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![hit(Fond::Loda, "LEGITEXT000033202746")]));
        let resolution = resolve_with(&corpus, &codes(), &eli, None).await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        let requests = corpus.seen_requests();
        assert!(
            matches!(
                requests.as_slice(),
                [PortRequest::FindByNumber(query)]
                    if query.fond == Fond::Loda
                        && query.kind == NumberKind::Eli
                        && query.value.as_str() == PATH
            ),
            "{requests:?}"
        );
    }

    #[tokio::test]
    async fn eli_longer_than_a_number_value_is_not_sent() {
        let path = format!("/eli/jo/{}", "a".repeat(NUMBER_VALUE_MAX_CHARS));
        let eli = Eli::parse(&path).expect("an ELI of 134 characters is valid");
        let corpus = FakeCorpus::new();
        let resolution =
            resolve_with(&corpus, &codes(), &standalone(CitationRef::Eli(eli)), None).await;
        assert_eq!(status(&resolution), CheckStatus::Unresolvable);
        assert_eq!(note(&resolution), "Not checkable in this version (an ELI this long).");
        assert_eq!(corpus.seen_requests(), vec![]);
    }

    #[tokio::test]
    async fn key_variants_are_tried_in_order_until_one_answers() {
        let corpus = FakeCorpus::new();
        corpus.push_find_article(Err(not_found()));
        corpus.push_find_article(Ok(article(Some("1"))));
        let resolution = resolve(&corpus, "article 1er du code civil").await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        let civil_code = LegiTextId::parse(CIVIL).unwrap();
        let at = ArticleAt::Today(today());
        assert_eq!(
            corpus.seen_requests(),
            vec![
                PortRequest::FindArticle(civil_code.clone(), "1er".into(), at),
                PortRequest::FindArticle(civil_code, "1".into(), at),
            ]
        );
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
        assert_eq!(confidence_of(&resolution.outcome), Some(Confidence::Medium));
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
        assert_eq!(confidence_of(&resolution.outcome), Some(Confidence::High));
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
        assert_eq!(
            note(&resolution),
            "Could not confirm existence: nothing was found in JURI, which does not prove that it does not exist."
        );

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

    /// Pourvoi relevé (capture C4) : texte cité, identifiant, date officielle, formation lue.
    struct Recorded {
        text: &'static str,
        id: &'static str,
        date: (u16, u8, u8),
        formation: &'static str,
    }

    /// Les huit arrêts réels dont la chambre citée est la bonne (relevés C4).
    const FAITHFUL: [Recorded; 8] = [
        Recorded {
            text: "Cass. civ. 1re, 13 mars 2019, n° 18-12.605",
            id: "JURITEXT000038322046",
            date: (2019, 3, 13),
            formation: "CHAMBRE_CIVILE_1",
        },
        Recorded {
            text: "Cass. civ. 2e, 24 septembre 2009, n° 08-17.523",
            id: "JURITEXT000021080678",
            date: (2009, 9, 24),
            formation: "CHAMBRE_CIVILE_2",
        },
        Recorded {
            text: "Cass. civ. 3e, 11 février 2016, n° 14-26.866",
            id: "JURITEXT000032056669",
            date: (2016, 2, 11),
            formation: "CHAMBRE_CIVILE_3",
        },
        Recorded {
            text: "Cass. com., 3 juin 2026, n° 24-14.748",
            id: "JURITEXT000054218381",
            date: (2026, 6, 3),
            formation: "CHAMBRE_COMMERCIALE",
        },
        Recorded {
            text: "Cass. crim., 26 janvier 2010, n° 08-88.134",
            id: "JURITEXT000021854686",
            date: (2010, 1, 26),
            formation: "CHAMBRE_CRIMINELLE",
        },
        Recorded {
            text: "Cass. soc., 9 septembre 2026, n° 25-14.356",
            id: "JURITEXT000054929168",
            date: (2026, 9, 9),
            formation: "CHAMBRE_SOCIALE",
        },
        Recorded {
            text: "Cass. soc., 25 novembre 2015, n° 14-24.444",
            id: "JURITEXT000031539712",
            date: (2015, 11, 25),
            formation: "CHAMBRE_SOCIALE",
        },
        Recorded {
            text: "Cass. ch. mixte, 21 décembre 2007, n° 06-12.769",
            id: "JURITEXT000017739957",
            date: (2007, 12, 21),
            formation: "CHAMBRE_MIXTE",
        },
    ];

    /// Résout `text` contre une décision de la Cour de cassation dont la formation est `formation`.
    async fn resolve_cassation(
        text: &str,
        id: &str,
        date: (u16, u8, u8),
        formation: Option<&str>,
    ) -> Resolution {
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![hit(Fond::Juri, id)]));
        let mut official = decision(id, civil(date.0, date.1, date.2), None);
        official.formation = formation.map(str::to_owned);
        corpus.push_get_decision(Ok(official));
        resolve(&corpus, text).await
    }

    #[tokio::test]
    async fn a_wrong_cassation_chamber_is_a_mismatch() {
        // Les quatre pourvois de l'E2E (verify_citations-2), cités avec une autre chambre.
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
            let resolution = resolve_cassation(text, id, date, Some(formation)).await;
            assert_eq!(status(&resolution), CheckStatus::Invalid, "{text}");
            assert_eq!(note(&resolution), expected, "{text}");
            assert_eq!(record_id(&resolution).as_deref(), Some(id));
        }
    }

    #[tokio::test]
    async fn the_right_cassation_chamber_stays_verified_on_the_recorded_decisions() {
        for case in &FAITHFUL {
            let resolution =
                resolve_cassation(case.text, case.id, case.date, Some(case.formation)).await;
            assert_eq!(status(&resolution), CheckStatus::Verified, "{}", case.text);
            assert_eq!(resolution.outcome.note(Proof::Production), None, "{}", case.text);
            assert_eq!(confidence_of(&resolution.outcome), Some(Confidence::High), "{}", case.text);
        }
    }

    #[tokio::test]
    async fn a_wrong_date_and_a_wrong_chamber_are_both_reported() {
        let resolution = resolve_cassation(
            "Cass. soc., 5 mai 2021, n° 99-12.345",
            "JURITEXT000000000001",
            (2000, 12, 20),
            Some("CHAMBRE_CIVILE_1"),
        )
        .await;
        assert_eq!(status(&resolution), CheckStatus::Invalid);
        assert_eq!(
            note(&resolution),
            "Cited date 2021-05-05 differs from the official record (2000-12-20); chamber SOC \
             differs (CIV1)."
        );
    }

    #[tokio::test]
    async fn a_formation_that_cannot_be_compared_gives_a_note_never_a_mismatch() {
        let expected = "The cited chamber could not be compared: the official record gives a \
                        formation that is not a modern Cour de cassation chamber.";
        // Valeurs réelles des relevés C4 (décisions de juridictions du fond), puis une chambre
        // historique et la chambre unique : jamais `invalid`.
        for formation in ["CT0196", "I7", "CHAMBRES_REUNIES", "CHAMBRE_CIVILE"] {
            let resolution = resolve_cassation(
                "Cass. soc., 25 novembre 2015, n° 14-24.444",
                "JURITEXT000031539712",
                (2015, 11, 25),
                Some(formation),
            )
            .await;
            assert_eq!(status(&resolution), CheckStatus::Verified, "{formation}");
            assert_eq!(note(&resolution), expected, "{formation}");
        }
    }

    #[tokio::test]
    async fn a_missing_or_blank_formation_is_not_compared_and_not_noted() {
        for formation in [None, Some(""), Some("  ")] {
            let resolution = resolve_cassation(
                "Cass. soc., 25 novembre 2015, n° 14-24.444",
                "JURITEXT000031539712",
                (2015, 11, 25),
                formation,
            )
            .await;
            assert_eq!(status(&resolution), CheckStatus::Verified, "{formation:?}");
            assert_eq!(resolution.outcome.note(Proof::Production), None, "{formation:?}");
        }
    }

    #[tokio::test]
    async fn a_cited_chamber_that_is_absent_or_historical_is_not_compared() {
        // Chambre non citée.
        let resolution = resolve_cassation(
            "Cour de cassation, 25 novembre 2015, 14-24.444",
            "JURITEXT000031539712",
            (2015, 11, 25),
            Some("CHAMBRE_SOCIALE"),
        )
        .await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert_eq!(resolution.outcome.note(Proof::Production), None);
        // Chambre civile sans numéro (citée, historique) contre une chambre moderne.
        let resolution = resolve_cassation(
            "Cass. civ., 25 novembre 1925, n° 14-24.444",
            "JURITEXT000031539712",
            (1925, 11, 25),
            Some("CHAMBRE_SOCIALE"),
        )
        .await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert_eq!(resolution.outcome.note(Proof::Production), None);
    }

    #[tokio::test]
    async fn the_chamber_of_a_non_cassation_decision_is_never_compared() {
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![hit(Fond::Juri, "JURITEXT000046652043")]));
        let mut official = decision("JURITEXT000046652043", civil(2022, 7, 5), None);
        official.formation = Some("CHAMBRE_SOCIALE".to_owned());
        corpus.push_get_decision(Ok(official));
        let text = "Cour d'appel de Paris, Pôle 1 - Chambre 12, 5 juillet 2022, n°14/10026";
        let resolution = resolve(&corpus, text).await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert_eq!(resolution.outcome.note(Proof::Production), None);
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

    const SOC_CITED: &str = "Cass. soc., 17 mars 2021, n° 00-00.000";

    fn juri_ids(count: usize) -> Vec<String> {
        (1..=count).map(|n| format!("JURITEXT{n:012}")).collect()
    }

    /// errors-6 : plusieurs enregistrements portent le numéro, aucun à la date citée.
    #[tokio::test]
    async fn records_of_the_number_none_dated_the_cited_day_are_unresolvable() {
        let ids = juri_ids(3);
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(ids.iter().map(|id| hit(Fond::Juri, id)).collect()));
        for (id, year) in ids.iter().zip([1989, 2003, 2004]) {
            corpus.push_get_decision(Ok(decision(id, civil(year, 3, 17), None)));
        }
        let resolution = resolve(&corpus, SOC_CITED).await;
        assert_eq!(status(&resolution), CheckStatus::Unresolvable);
        assert_eq!(
            note(&resolution),
            "3 records share number 00-00.000, none dated 2021-03-17; dates found: 1989-03-17, \
             2003-03-17, 2004-03-17."
        );
        assert_eq!(corpus.calls_of("get_decision"), 3);
    }

    /// Jumeaux « vrai positif conservé » : un enregistrement du jour cité reste le choix sûr ; une
    /// décision sans date ne peut rien écarter ; sans date citée, rien n'est écarté.
    #[tokio::test]
    async fn a_record_of_the_cited_day_or_an_undated_one_keeps_the_choice() {
        let ids = juri_ids(2);
        let run = |dates: [Option<CivilDate>; 2], text: &'static str| {
            let ids = ids.clone();
            async move {
                let corpus = FakeCorpus::new();
                corpus.push_find_by_number(Ok(ids.iter().map(|id| hit(Fond::Juri, id)).collect()));
                for (id, date) in ids.iter().zip(dates) {
                    let mut record = decision(id, civil(2000, 1, 1), None);
                    record.date_texte = date;
                    corpus.push_get_decision(Ok(record));
                }
                resolve(&corpus, text).await
            }
        };
        let day = Some(civil(2021, 3, 17));
        let other = Some(civil(2003, 3, 17));
        let chosen = run([other, day], SOC_CITED).await;
        assert_eq!(status(&chosen), CheckStatus::Verified);
        assert_eq!(confidence_of(&chosen.outcome), Some(Confidence::High));
        assert_eq!(record_id(&chosen).as_deref(), Some("JURITEXT000000000002"));
        let undated = run([other, None], SOC_CITED).await;
        assert_eq!(status(&undated), CheckStatus::Ambiguous);
        let no_date = run([other, other], "Cass. soc., n° 00-00.000").await;
        assert_eq!(status(&no_date), CheckStatus::Ambiguous);
    }

    /// errors-6, au-delà de cinq hits : les hits JURI n'ont pas de date, c'est le titre qui la dit
    /// (comme en bac à sable).
    #[tokio::test]
    async fn more_than_five_hits_titled_with_other_days_are_unresolvable_without_a_consultation() {
        let ids = juri_ids(8);
        let titles = [
            "Cour de cassation, civile, Chambre sociale, 9 décembre 1989, 00-00.000, Publié au bulletin",
            "Cour de cassation, Chambre sociale, du 12 mars 2003, 00-00.000",
            "Cour de cassation, civile, Chambre sociale, 4 mai 2004, 00-00.000",
            "Cour de cassation, civile, Chambre sociale, 1 mai 2004, 00-00.000",
            "Cour de cassation, civile, Chambre sociale, 2 mai 2004, 00-00.000",
            "Cour de cassation, civile, Chambre sociale, 3 mai 2004, 00-00.000",
            "Cour de cassation, civile, Chambre sociale, 5 mai 2004, 00-00.000",
            "Cour de cassation, civile, Chambre sociale, 6 mai 2004, 00-00.000",
        ];
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(ids
            .iter()
            .zip(titles)
            .map(|(id, title)| titled(Fond::Juri, id, title))
            .collect()));
        let resolution = resolve(&corpus, SOC_CITED).await;
        assert_eq!(status(&resolution), CheckStatus::Unresolvable);
        assert_eq!(
            note(&resolution),
            "8 records share number 00-00.000, none dated 2021-03-17; dates found: 1989-12-09, \
             2003-03-12, 2004-05-01, 2004-05-02, 2004-05-03 and 3 more."
        );
        assert_eq!(corpus.calls_of("get_decision"), 0);
    }

    /// Jumeaux : un titre à la date citée garde les candidats ; un titre illisible n'écarte rien ;
    /// sans date citée, rien n'est filtré.
    #[tokio::test]
    async fn more_than_five_hits_keep_their_candidates_when_a_title_fits_or_cannot_be_read() {
        let other = "Cour de cassation, civile, Chambre sociale, 4 mai 2004, 00-00.000";
        let day = "Cour de cassation, civile, Chambre sociale, 17 mars 2021, 00-00.000";
        let ids = juri_ids(7);
        let run = |odd: Option<&'static str>, text: &'static str| {
            let ids = ids.clone();
            async move {
                let mut hits: Vec<SearchHit> =
                    ids.iter().map(|id| titled(Fond::Juri, id, other)).collect();
                hits[3].title = odd.map(str::to_owned);
                let corpus = FakeCorpus::new();
                corpus.push_find_by_number(Ok(hits));
                resolve(&corpus, text).await
            }
        };
        for (odd, text) in [
            (Some(day), SOC_CITED),
            (Some("Chambre sociale"), SOC_CITED),
            (None, SOC_CITED),
            (Some(other), "Cass. soc., n° 00-00.000"),
        ] {
            let resolution = run(odd, text).await;
            let (items, note) = candidates_of(resolution.outcome).expect("des candidats");
            assert_eq!(items.len(), MAX_CANDIDATES, "{odd:?} {text}");
            assert_eq!(note, Note::Candidates { n: 7 }, "{odd:?} {text}");
        }
    }

    #[test]
    fn a_decision_title_gives_its_own_date_and_never_a_cited_text() {
        let dated = |title: &str| judged_on(&titled(Fond::Juri, "JURITEXT000000000001", title));
        assert_eq!(
            dated("Cour de cassation, civile, Chambre sociale, 21 mai 2025, 22-11.901"),
            Some(civil(2025, 5, 21))
        );
        assert_eq!(
            dated("Conseil d'État, 10ème - 9ème chambres réunies, 30/01/2020, 419837"),
            Some(civil(2020, 1, 30))
        );
        assert_eq!(
            dated(
                "Décision 2000-433 DC - 27 juillet 2000 - Loi modifiant la loi n° 86-1067 du 30 septembre 1986"
            ),
            Some(civil(2000, 7, 27))
        );
        // Un titre sans date, sans décision (un texte seul) ou absent ne se lit pas.
        assert_eq!(dated("Cour de cassation, Chambre sociale, 22-11.901"), None);
        assert_eq!(dated("Loi n° 2016-1321 du 7 octobre 2016"), None);
        assert_eq!(judged_on(&hit(Fond::Juri, "JURITEXT000000000001")), None);
    }

    #[test]
    fn no_decision_is_on_the_day_only_when_several_are_all_dated_another_day() {
        let day = civil(2025, 6, 4);
        let other = [civil(2025, 6, 5), civil(2025, 6, 6)];
        assert!(none_on_the_day(&found_of(&other, None), day));
        // Une seule décision : c'est un écart de date, pas une absence.
        assert!(!none_on_the_day(&found_of(&other[..1], None), day));
        assert!(!none_on_the_day(&found_of(&[other[0], day], None), day));
        let mut undated = found_of(&other, None);
        undated[1].1.date_texte = None;
        assert!(!none_on_the_day(&undated, day));
    }

    #[test]
    fn the_no_record_at_date_note_counts_records_and_lists_distinct_sorted_dates() {
        let day = civil(1400, 1, 1);
        let dates = vec![civil(2004, 1, 2), civil(1989, 1, 1), civil(2004, 1, 2)];
        let resolution = no_record_at_date("00-00.000", day, dates);
        assert_eq!(status(&resolution), CheckStatus::Unresolvable);
        assert_eq!(
            note(&resolution),
            "3 records share number 00-00.000, none dated 1400-01-01; dates found: 1989-01-01, \
             2004-01-02."
        );
        let many = (1..=7).map(|d| civil(2000, 1, d)).collect();
        assert_eq!(
            note(&no_record_at_date("1", day, many)),
            "7 records share number 1, none dated 1400-01-01; dates found: 2000-01-01, \
             2000-01-02, 2000-01-03, 2000-01-04, 2000-01-05 and 2 more."
        );
    }

    /// Reports de W3 (possibles depuis L1-08) : « loi 89-462 » et « loi 78-17 », sans « n° » ni
    /// date, cherchent la forme courte et rendent le candidat en confiance moyenne (L2-02).
    #[tokio::test]
    async fn a_pre_2000_law_cited_without_n_degree_is_searched_by_its_short_number() {
        for (cited, number, id, title) in [
            (
                "loi 89-462",
                "89-462",
                "LEGITEXT000006069108",
                "Loi n° 89-462 du 6 juillet 1989 tendant à améliorer les rapports locatifs",
            ),
            (
                "loi 78-17",
                "78-17",
                "LEGITEXT000006068624",
                "Loi n° 78-17 du 6 janvier 1978 relative à l'informatique, aux fichiers et aux libertés",
            ),
        ] {
            let corpus = FakeCorpus::new();
            corpus.push_find_by_number(Ok(vec![loda_hit(id, "LOI", title, None)]));
            let resolution = resolve(&corpus, cited).await;
            assert_eq!(record_id(&resolution).as_deref(), Some(id), "{cited}");
            assert_eq!(confidence_of(&resolution.outcome), Some(Confidence::Medium), "{cited}");
            assert_eq!(
                corpus.seen_requests(),
                vec![find_request(Fond::Loda, NumberKind::Text, number)],
                "{cited}"
            );
        }
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
        assert_eq!(
            note(&resolution),
            "Could not confirm existence: nothing was found in CNIL, which does not prove that it does not exist."
        );
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
        assert_eq!(
            note(&resolution),
            "Could not confirm existence: nothing was found in LODA, which does not prove that it does not exist."
        );
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
        // Seule une absence prouvée en production est `not_found`.
        assert_eq!(status(&resolution), CheckStatus::NotFound);
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
        // Comme en bac à sable : `date` est la date d'entrée en vigueur, pas la signature.
        law.date = Some(civil(2016, 10, 9));
        law.title =
            Some("LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique (1)".into());
        law
    }

    /// Un texte LODA tel que la recherche par numéro le rend : nature, titre et `date` (celle de
    /// l'entrée en vigueur, qui n'est jamais la signature).
    fn loda_hit(id: &str, nature: &str, title: &str, date: Option<CivilDate>) -> SearchHit {
        let mut found = titled(Fond::Loda, id, title);
        found.nature = Some(nature.to_owned());
        found.date = date;
        found
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
        assert_eq!(status(&resolution), CheckStatus::NotFound);
        assert_eq!(
            note(&resolution),
            "LEGITEXT000033202746 has no article 6 in force on 2026-10-29. The article may exist in earlier versions; retry with a date."
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
                "No recognised code or text for article 12 (an unknown abbreviation counts as missing); \
                 name the code in full, for example \"article 12 du code civil\".",
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
                "No collective agreement identified; give its IDCC (e.g. 'IDCC 1486').",
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
    fn statuses_follow_the_source_for_an_absence() {
        let absent = Outcome::Absent {
            prod: Note::NoRecord { family: "text", value: "1".to_owned(), date: None },
            miss: Miss { family: "text", value: "1".to_owned(), date: None },
        };
        // Seule la production prouve l'absence ; le bac à sable et l'instantané local ne prouvent rien.
        assert_eq!(absent.status(Proof::Production), CheckStatus::NotFound);
        assert_eq!(absent.status(Proof::Sandbox), CheckStatus::Unresolvable);
        assert_eq!(absent.status(Proof::Snapshot), CheckStatus::Unresolvable);
        assert_eq!(absent.note(Proof::Production).unwrap().text(), "No text record matches 1.");
        assert_eq!(
            absent.note(Proof::Sandbox).unwrap().text(),
            "No text record matches 1 in the PISTE sandbox; sandbox data differ from production."
        );
        assert_eq!(
            absent.note(Proof::Snapshot).unwrap().text(),
            "No text record matches 1 in the local DILA snapshot, which may be out of date."
        );
        let failed = Outcome::Failed(upstream());
        assert_eq!(failed.status(Proof::Production), CheckStatus::Unresolvable);
        assert_eq!(failed.note(Proof::Production).unwrap().text(), "Not checked: upstream error.");
        assert!(failed.record().is_none());
    }

    /// Le drapeau d'instantané local l'emporte sur l'environnement : l'index local se déclare
    /// `production` dans l'audit, et c'est précisément pour cela que le drapeau existe.
    #[test]
    fn the_local_snapshot_flag_overrides_the_declared_environment() {
        assert_eq!(Proof::of(PisteEnvironment::Production, false), Proof::Production);
        assert_eq!(Proof::of(PisteEnvironment::Sandbox, false), Proof::Sandbox);
        assert_eq!(Proof::of(PisteEnvironment::Production, true), Proof::Snapshot);
        assert_eq!(Proof::of(PisteEnvironment::Sandbox, true), Proof::Snapshot);
    }

    // ----------------------------------------------------------------------------------------
    // Constructeurs des tests de couverture et de mutation
    // ----------------------------------------------------------------------------------------

    fn art(text: &str) -> ArticleRef {
        ArticleRef { num: text.parse().unwrap(), sub: vec![], range_to: None }
    }

    /// Alinéa du Préambule : un article sans numéro, de clé vide.
    fn preamble_article() -> ArticleRef {
        let num = ArticleNum {
            prefix: None,
            special: Some(SpecialNum::SansNumero),
            base: vec![],
            latin: None,
            latin_sub: None,
            letters: None,
            latin_after: None,
        };
        ArticleRef { num, sub: vec![], range_to: None }
    }

    fn cassation() -> Court {
        Court::CourDeCassation { chamber: None }
    }

    fn conseil_etat() -> Court {
        Court::ConseilEtat { formation: None, chambers: vec![] }
    }

    fn pourvoi() -> CaseNumber {
        CaseNumber::Pourvoi("24-13.470".parse().unwrap())
    }

    fn decision_cite(
        court: Option<Court>,
        numbers: Vec<CaseNumber>,
        date: Option<CivilDate>,
    ) -> DecisionCite {
        DecisionCite {
            court,
            court_via: None,
            date,
            numbers,
            kind: None,
            publication: None,
            solution: None,
            parties: None,
            subject: None,
            arret_num: None,
            formation_code: None,
            publication_codes: vec![],
        }
    }

    fn decision_item(
        court: Option<Court>,
        numbers: Vec<CaseNumber>,
        date: Option<CivilDate>,
    ) -> Spanned<CitationRef> {
        standalone(CitationRef::Decision(decision_cite(court, numbers, date)))
    }

    fn with_ambiguity(
        mut item: Spanned<CitationRef>,
        ambiguity: Ambiguity,
    ) -> Spanned<CitationRef> {
        item.ambiguity = Some(ambiguity);
        item
    }

    fn convention_cite(idcc: Option<u16>, avenant: Option<u32>) -> ConventionCite {
        ConventionCite {
            kind: None,
            idcc: idcc.and_then(|n| Idcc::parse(&n.to_string())),
            brochure: None,
            avenant,
            title_hint: None,
            date: None,
        }
    }

    fn convention_item(idcc: Option<u16>, avenant: Option<u32>) -> Spanned<CitationRef> {
        standalone(CitationRef::Convention(convention_cite(idcc, avenant)))
    }

    fn text_cite(
        nature: TextNature,
        number: Option<(u16, u32)>,
        date: Option<CivilDate>,
    ) -> TextCite {
        TextCite {
            nature,
            number: number.map(|(year, seq)| TextNumber {
                year: Some(year),
                seq,
                extra: None,
                raw: format!("{year}-{seq}"),
            }),
            date,
            date_raw: None,
            title_hint: None,
            anaphora: None,
            appellation: None,
            emitter: None,
        }
    }

    fn nor_item() -> Spanned<CitationRef> {
        standalone(CitationRef::Nor(Nor::parse("ECFI1524250L").unwrap()))
    }

    fn code_item(code: CodeRef, article: ArticleRef) -> Spanned<CitationRef> {
        standalone(CitationRef::CodeArticle { code, article, version: None, code_via: None })
    }

    fn ambiguous_outcome(note: Note) -> Outcome {
        Outcome::Candidates { items: Vec::new(), note }
    }

    fn unresolvable_outcome(note: Note) -> Outcome {
        Outcome::Unresolvable { note }
    }

    fn confidence_of(outcome: &Outcome) -> Option<Confidence> {
        if let Outcome::Verified { confidence, .. } = outcome { Some(*confidence) } else { None }
    }

    fn find_request(fond: Fond, kind: NumberKind, value: &str) -> PortRequest {
        let value = NumberValue::parse(value).unwrap();
        PortRequest::FindByNumber(NumberQuery::new(fond, kind, value, today()).unwrap())
    }

    fn resolver<'a>(corpus: &'a FakeCorpus, codes: &'a CodeTable) -> Resolver<'a, FakeCorpus> {
        Resolver::new(corpus, today(), codes)
    }

    fn titled(fond: Fond, id: &str, title: &str) -> SearchHit {
        let mut record = hit(fond, id);
        record.title = Some(title.to_owned());
        record
    }

    fn with_ecli(mut found: Decision, ecli: Option<&str>) -> Decision {
        found.ecli = ecli.map(|ecli| Ecli::parse(ecli).unwrap());
        found
    }

    // ----------------------------------------------------------------------------------------
    // Issues hors ligne
    // ----------------------------------------------------------------------------------------

    #[test]
    fn every_impossible_flag_states_its_own_fact() {
        let cases = [
            (Flag::DateInvalide, "The date does not exist"),
            (Flag::AnneeFuture, "The year is in the future"),
            (
                Flag::ChambreInexistante("civ4".to_owned()),
                "There is no such civil chamber at the Cour de cassation",
            ),
            (
                Flag::ChambreCassInconnue("XX".to_owned()),
                "There is no such chamber at the Cour de cassation",
            ),
            (
                Flag::EcliOrdinalNonConforme(EcliFamily::Cc),
                "The ECLI ordinal does not follow the form of that court",
            ),
            (
                Flag::EcliAnneeAnterieureNumero,
                "The ECLI year is earlier than the year of the number",
            ),
            (
                Flag::AnneePourvoiPosterieure,
                "The year of the appeal number is after the decision year",
            ),
            (Flag::EcliCodeCeSeul, "The citation is impossible"),
        ];
        for (flag, fact) in cases {
            assert_eq!(fact_of(&flag), fact, "{flag:?}");
        }
    }

    #[test]
    fn every_unsupported_family_has_its_own_name() {
        let cases = [
            (UnsupportedFamily::Ue, "EU law"),
            (UnsupportedFamily::Cjue, "CJEU case law"),
            (UnsupportedFamily::Cedh, "ECHR law and case law"),
            (UnsupportedFamily::Doctrine, "Legal scholarship"),
            (UnsupportedFamily::Recueil, "A law report"),
            (UnsupportedFamily::ForeignEcli, "A foreign ECLI"),
        ];
        for (family, name) in cases {
            assert_eq!(out_of_scope(family), name, "{family:?}");
        }
    }

    #[test]
    fn each_reason_of_the_parser_gives_its_own_offline_outcome() {
        let code = code_item(CodeRef::Missing, art("12"));
        let cases = [
            (
                UnresolvableWhy::Impossible(Flag::AnneeFuture),
                Outcome::Invalid { note: Note::Impossible { fact: "The year is in the future" } },
            ),
            (
                UnresolvableWhy::HorsPerimetre(UnsupportedFamily::Cedh),
                unresolvable_outcome(Note::OutOfScope { family: "ECHR law and case law" }),
            ),
            (
                UnresolvableWhy::SigleSansContexte(Sigle::Ce),
                ambiguous_outcome(Note::AmbiguousSigle { sigle: Sigle::Ce }),
            ),
            (
                UnresolvableWhy::SansCode,
                ambiguous_outcome(Note::MissingTarget { num: "12".into() }),
            ),
            (
                UnresolvableWhy::AnaphoreNonResolue,
                unresolvable_outcome(Note::MissingTarget { num: "12".into() }),
            ),
            (
                UnresolvableWhy::PublicationSeule,
                unresolvable_outcome(Note::NotSupported { what: "publication reference" }),
            ),
        ];
        for (why, expected) in cases {
            let label = format!("{why:?}");
            assert_eq!(unresolvable_why(&code, why).outcome, expected, "{label}");
        }
        // Le numéro d'article se lit aussi dans un texte, et reste vide ailleurs.
        let text_article = standalone(CitationRef::TextArticle {
            text: None,
            article: art("L. 761-1"),
            at_date: None,
        });
        assert_eq!(article_key(&text_article.value), "L761-1");
        assert_eq!(article_key(&nor_item().value), "");
        assert_eq!(
            unresolvable_why(&nor_item(), UnresolvableWhy::SansCode).outcome,
            ambiguous_outcome(Note::MissingTarget { num: String::new() })
        );
    }

    #[test]
    fn the_fond_of_a_decision_follows_its_court() {
        let caa = Court::Caa { seat: None, chamber: None };
        let ta = Court::Ta { seat: None, chamber: None };
        assert_eq!(fond_of(Some(&conseil_etat())), Fond::Cetat);
        assert_eq!(fond_of(Some(&caa)), Fond::Cetat);
        assert_eq!(fond_of(Some(&ta)), Fond::Cetat);
        assert_eq!(fond_of(Some(&Court::ConseilConstitutionnel)), Fond::Constit);
        assert_eq!(fond_of(Some(&cassation())), Fond::Juri);
        assert_eq!(fond_of(Some(&Court::CourAppel { seat: None, chamber: None })), Fond::Juri);
        assert_eq!(fond_of(Some(&Court::TribunalJudiciaire { seat: None })), Fond::Juri);
        assert_eq!(fond_of(Some(&Court::AutreJudiciaire { label: "CPH".to_owned() })), Fond::Juri);
        assert_eq!(fond_of(None), Fond::Juri);
    }

    #[test]
    fn a_missing_case_number_names_the_fond_of_the_court() {
        let date = Some(civil(2018, 3, 28));
        // L'ambiguïté `numero_manquant` l'emporte sur l'absence de juridiction : la note porte la
        // date et le fond, pas « no court ».
        let item = with_ambiguity(decision_item(None, vec![], date), Ambiguity::NumeroManquant);
        assert_eq!(
            offline(&item).unwrap().outcome,
            ambiguous_outcome(Note::NoNumber { fond: Fond::Juri, date })
        );
        // ... et même si un numéro est écrit.
        let item = with_ambiguity(
            decision_item(Some(conseil_etat()), vec![CaseNumber::CeRequete(1)], date),
            Ambiguity::NumeroManquant,
        );
        assert_eq!(
            offline(&item).unwrap().outcome,
            ambiguous_outcome(Note::NoNumber { fond: Fond::Cetat, date })
        );
        // Sans l'ambiguïté, une décision sans numéro est tranchée de même, avec le fond de sa cour.
        for (court, fond) in [
            (cassation(), Fond::Juri),
            (conseil_etat(), Fond::Cetat),
            (Court::ConseilConstitutionnel, Fond::Constit),
        ] {
            let item = decision_item(Some(court), vec![], date);
            assert_eq!(
                offline(&item).unwrap().outcome,
                ambiguous_outcome(Note::NoNumber { fond, date })
            );
        }
        // Avec un numéro, elle se cherche.
        assert!(offline(&decision_item(Some(cassation()), vec![pourvoi()], date)).is_none());
    }

    #[test]
    fn a_missing_court_or_agreement_is_decided_offline_for_any_family() {
        let no_court = ambiguous_outcome(Note::NoCourt);
        for ambiguity in [Ambiguity::JuridictionManquante, Ambiguity::JuridictionEtTypeManquants] {
            let item = with_ambiguity(
                decision_item(Some(cassation()), vec![pourvoi()], None),
                ambiguity.clone(),
            );
            assert_eq!(offline(&item).unwrap().outcome, no_court, "{ambiguity:?}");
        }
        // Sans ambiguïté, c'est l'absence de juridiction elle-même qui tranche.
        let item = decision_item(None, vec![pourvoi()], None);
        assert_eq!(offline(&item).unwrap().outcome, no_court);
        let item = decision_item(None, vec![], Some(civil(2018, 3, 28)));
        assert_eq!(offline(&item).unwrap().outcome, no_court);

        let no_agreement = ambiguous_outcome(Note::NoAgreement);
        for ambiguity in [Ambiguity::ConventionInconnue, Ambiguity::ConventionNonIdentifiee] {
            let item = with_ambiguity(convention_item(Some(1486), None), ambiguity.clone());
            assert_eq!(offline(&item).unwrap().outcome, no_agreement, "{ambiguity:?}");
        }
        assert_eq!(offline(&convention_item(None, None)).unwrap().outcome, no_agreement);
        assert_eq!(offline(&convention_item(None, Some(3))).unwrap().outcome, no_agreement);
    }

    #[test]
    fn amendments_and_numberless_texts_are_not_supported_offline() {
        let amendment = offline(&convention_item(Some(1486), Some(3))).unwrap();
        assert_eq!(
            amendment.outcome,
            unresolvable_outcome(Note::NotSupported { what: "amendment" })
        );
        assert!(offline(&convention_item(Some(1486), None)).is_none());

        let numberless =
            unresolvable_outcome(Note::NotSupported { what: "text cited without a number" });
        let loi = text_cite(TextNature::Loi, None, None);
        assert_eq!(
            offline(&standalone(CitationRef::Text(loi.clone()))).unwrap().outcome,
            numberless
        );
        let article = standalone(CitationRef::TextArticle {
            text: Some(loi),
            article: art("6"),
            at_date: None,
        });
        assert_eq!(offline(&article).unwrap().outcome, numberless);
        let numbered = text_cite(TextNature::Loi, Some((2016, 1321)), None);
        assert!(offline(&standalone(CitationRef::Text(numbered))).is_none());
    }

    #[test]
    fn a_preamble_paragraph_is_not_supported_offline_but_a_numbered_article_is_searched() {
        let preamble = unresolvable_outcome(Note::NotSupported { what: "preamble paragraph" });
        let item = code_item(CodeRef::Unknown("Code civil".to_owned()), preamble_article());
        assert_eq!(offline(&item).unwrap().outcome, preamble);
        let item = standalone(CitationRef::TextArticle {
            text: Some(text_cite(TextNature::Loi, Some((2016, 1321)), None)),
            article: preamble_article(),
            at_date: None,
        });
        assert_eq!(offline(&item).unwrap().outcome, preamble);
        let item = code_item(CodeRef::Unknown("Code civil".to_owned()), art("9"));
        assert!(offline(&item).is_none());
    }

    #[tokio::test]
    async fn what_offline_always_decides_is_not_supported_by_the_search() {
        let corpus = FakeCorpus::new();
        let codes = codes();
        let resolver = resolver(&corpus, &codes);
        let not_supported_here =
            unresolvable_outcome(Note::NotSupported { what: "this kind of citation" });
        let jorf = standalone(CitationRef::Jorf(lf_domain::citation::JorfCite {
            number: None,
            date: None,
            texte: None,
            page: None,
        }));
        let eu = standalone(CitationRef::Unsupported(UnsupportedCite {
            family: UnsupportedFamily::Ue,
            key: None,
        }));
        let no_text =
            standalone(CitationRef::TextArticle { text: None, article: art("6"), at_date: None });
        for item in [jorf, eu, no_text] {
            assert_eq!(resolver.search(&item, None).await.outcome, not_supported_here);
        }
        assert_eq!(corpus.calls(), 0);
    }

    // ----------------------------------------------------------------------------------------
    // Notes et statuts des issues
    // ----------------------------------------------------------------------------------------

    #[test]
    fn every_outcome_exposes_its_record_status_and_note() {
        let record = |id: &str| ResolvedRecord::of(DocumentId::parse(id).unwrap(), &article(None));
        let verified_since = Outcome::Verified {
            record: record("LEGIARTI000006419288"),
            confidence: Confidence::High,
            note: Some(Note::VersionSince(civil(2016, 10, 1))),
        };
        let verified_plain = Outcome::Verified {
            record: record("LEGIARTI000006419288"),
            confidence: Confidence::Low,
            note: None,
        };
        let mismatch = Outcome::Mismatch {
            record: record("LEGIARTI000006419288"),
            note: Note::Mismatch { field: Field::Date, cited: "a".into(), actual: "b".into() },
        };
        let invalid = Outcome::Invalid { note: Note::NoCourt };
        let candidates = ambiguous_outcome(Note::NoAgreement);
        let unresolvable = unresolvable_outcome(Note::CodeNotListed);
        let cases = [
            (&verified_since, CheckStatus::Verified, Some(Note::VersionSince(civil(2016, 10, 1)))),
            (&verified_plain, CheckStatus::Verified, None),
            (&mismatch, CheckStatus::Invalid, mismatch.note(Proof::Sandbox)),
            (&invalid, CheckStatus::Invalid, Some(Note::NoCourt)),
            (&candidates, CheckStatus::Ambiguous, Some(Note::NoAgreement)),
            (&unresolvable, CheckStatus::Unresolvable, Some(Note::CodeNotListed)),
        ];
        for (outcome, status, note) in cases {
            for proof in [Proof::Production, Proof::Sandbox, Proof::Snapshot] {
                assert_eq!(outcome.status(proof), status, "{outcome:?}");
                assert_eq!(outcome.note(proof), note, "{outcome:?}");
            }
        }
        assert_eq!(
            mismatch.note(Proof::Production),
            Some(Note::Mismatch { field: Field::Date, cited: "a".into(), actual: "b".into() })
        );
        assert!(verified_plain.record().is_some());
        assert!(mismatch.record().is_some());
        assert!(invalid.record().is_none());
        assert!(candidates.record().is_none());
        assert!(unresolvable.record().is_none());
        assert_eq!(candidates_of(candidates).unwrap().1, Note::NoAgreement);
        assert!(candidates_of(invalid).is_none());
    }

    #[test]
    fn an_exhausted_time_budget_is_unresolvable_with_its_reason() {
        let late = Resolution::out_of_time();
        assert_eq!(
            late.outcome,
            unresolvable_outcome(Note::NotChecked { reason: NotCheckedReason::TimeBudget })
        );
        assert_eq!(late.official, None);
    }

    // ----------------------------------------------------------------------------------------
    // Formes du numéro et prédicats de titre
    // ----------------------------------------------------------------------------------------

    /// Ce que `number_forms` rend pour ces formes ; `None` quand il n'y en a aucune.
    fn forms(
        fond: Fond,
        kind: NumberKind,
        forms: &[&str],
    ) -> Option<(Fond, NumberKind, Vec<String>)> {
        let owned: Vec<String> = forms.iter().map(|form| (*form).to_owned()).collect();
        (!owned.is_empty()).then_some((fond, kind, owned))
    }

    #[test]
    fn case_numbers_are_tried_in_the_forms_of_their_court() {
        let unknown = CaseNumber::Unknown("X 1".to_owned());
        let portalis = CaseNumber::Portalis("P-1".to_owned());
        let rg = CaseNumber::Rg("24/15215".parse().unwrap());
        let ce = CaseNumber::CeRequete(506_507);
        let caa = CaseNumber::Caa("24MA02429".parse().unwrap());
        let ta = CaseNumber::Ta("2005064".parse().unwrap());

        // Cour de cassation : le pourvoi avec et sans le point, ou le numéro de forme inconnue.
        assert_eq!(
            number_forms(&cassation(), &[pourvoi()]),
            forms(Fond::Juri, NumberKind::Case, &["24-13.470", "24-13470"])
        );
        assert_eq!(
            number_forms(&cassation(), std::slice::from_ref(&unknown)),
            forms(Fond::Juri, NumberKind::Case, &["X 1"])
        );
        assert_eq!(number_forms(&cassation(), &[ce.clone(), rg.clone()]), None);
        // Juridictions du fond : RG, Portalis ou numéro de forme inconnue.
        let lower = [
            Court::CourAppel { seat: None, chamber: None },
            Court::TribunalJudiciaire { seat: None },
            Court::AutreJudiciaire { label: "CPH".to_owned() },
        ];
        for court in &lower {
            assert_eq!(
                number_forms(court, std::slice::from_ref(&rg)),
                forms(Fond::Juri, NumberKind::Case, &["24/15215"]),
                "{court:?}"
            );
            assert_eq!(
                number_forms(court, std::slice::from_ref(&portalis)),
                forms(Fond::Juri, NumberKind::Case, &["P-1"]),
                "{court:?}"
            );
            assert_eq!(
                number_forms(court, std::slice::from_ref(&unknown)),
                forms(Fond::Juri, NumberKind::Case, &["X 1"]),
                "{court:?}"
            );
            assert_eq!(number_forms(court, &[pourvoi(), ce.clone()]), None, "{court:?}");
        }
        // Conseil d'État, CAA et TA interrogent le fond CETAT par numéro de décision.
        assert_eq!(
            number_forms(&conseil_etat(), std::slice::from_ref(&ce)),
            forms(Fond::Cetat, NumberKind::Decision, &["506507"])
        );
        assert_eq!(
            number_forms(&conseil_etat(), std::slice::from_ref(&unknown)),
            forms(Fond::Cetat, NumberKind::Decision, &["X 1"])
        );
        assert_eq!(number_forms(&conseil_etat(), &[pourvoi(), rg.clone()]), None);
        let caa_court = Court::Caa { seat: None, chamber: None };
        assert_eq!(
            number_forms(&caa_court, std::slice::from_ref(&caa)),
            forms(Fond::Cetat, NumberKind::Decision, &["24MA02429"])
        );
        assert_eq!(number_forms(&caa_court, &[unknown.clone(), ce.clone()]), None);
        let ta_court = Court::Ta { seat: None, chamber: None };
        assert_eq!(
            number_forms(&ta_court, std::slice::from_ref(&ta)),
            forms(Fond::Cetat, NumberKind::Decision, &["2005064"])
        );
        assert_eq!(number_forms(&ta_court, &[unknown, caa]), None);
        // Sans numéro, aucune juridiction n'a de forme à chercher.
        for court in lower.iter().chain([&cassation(), &conseil_etat(), &caa_court, &ta_court]) {
            assert_eq!(number_forms(court, &[]), None, "{court:?}");
        }
    }

    #[test]
    fn a_constitutional_number_adds_the_form_without_the_joined_numbers_only_when_different() {
        let cc = |text: &str| CaseNumber::Cc(text.parse().unwrap());
        let court = Court::ConseilConstitutionnel;
        // Numéro simple : une seule forme (une forme en double serait cherchée deux fois).
        assert_eq!(
            number_forms(&court, &[cc("2025-1173 QPC")]),
            forms(Fond::Constit, NumberKind::Decision, &["2025-1173"])
        );
        // Numéros joints ou rectification : la forme complète, puis l'année et le premier numéro.
        assert_eq!(
            number_forms(&court, &[cc("2010-14/22 QPC")]),
            forms(Fond::Constit, NumberKind::Decision, &["2010-14/22", "2010-14"])
        );
        assert_eq!(
            number_forms(&court, &[cc("2015-491R DC")]),
            forms(Fond::Constit, NumberKind::Decision, &["2015-491R", "2015-491"])
        );
        assert_eq!(number_forms(&court, &[pourvoi()]), None);
        assert_eq!(number_forms(&court, &[]), None);
    }

    #[test]
    fn titles_are_told_apart_by_the_court_they_name() {
        let conseil = "Conseil d'État, 5ème - 6ème chambres réunies";
        let appeal = "Cour administrative d'appel de Marseille";
        let tribunal = "Tribunal administratif de Nîmes";
        assert!(is_conseil_etat(conseil));
        assert!(!is_conseil_etat(appeal));
        assert!(!is_conseil_etat(tribunal));
        assert!(!is_conseil_etat("Avis du Conseil d'État"));
        assert!(is_caa(appeal));
        assert!(!is_caa(conseil));
        assert!(!is_caa(tribunal));
        assert!(is_ta(tribunal));
        assert!(!is_ta(conseil));
        assert!(!is_ta(appeal));
    }

    // ----------------------------------------------------------------------------------------
    // Recherche d'une décision
    // ----------------------------------------------------------------------------------------

    #[tokio::test]
    async fn each_court_searches_its_fond_and_names_what_could_not_be_confirmed() {
        let caa = Court::Caa { seat: None, chamber: None };
        let ta = Court::Ta { seat: None, chamber: None };
        let rg = || CaseNumber::Rg("24/15215".parse().unwrap());
        let cases = [
            (
                cassation(),
                pourvoi(),
                Note::CouldNotConfirm { index: "JURI" },
                find_request(Fond::Juri, NumberKind::Case, "24-13.470"),
            ),
            (
                Court::CourAppel { seat: None, chamber: None },
                rg(),
                Note::OutOfCoverage { court: "Cour d'appel" },
                find_request(Fond::Juri, NumberKind::Case, "24/15215"),
            ),
            (
                Court::TribunalJudiciaire { seat: None },
                rg(),
                Note::OutOfCoverage { court: "tribunal judiciaire" },
                find_request(Fond::Juri, NumberKind::Case, "24/15215"),
            ),
            (
                Court::AutreJudiciaire { label: "CPH".to_owned() },
                rg(),
                Note::OutOfCoverage { court: "lower court" },
                find_request(Fond::Juri, NumberKind::Case, "24/15215"),
            ),
            (
                conseil_etat(),
                CaseNumber::CeRequete(506_507),
                Note::CouldNotConfirm { index: "CETAT" },
                find_request(Fond::Cetat, NumberKind::Decision, "506507"),
            ),
            (
                caa,
                CaseNumber::Caa("24MA02429".parse().unwrap()),
                Note::OutOfCoverage { court: "CAA" },
                find_request(Fond::Cetat, NumberKind::Decision, "24MA02429"),
            ),
            (
                ta,
                CaseNumber::Ta("2005064".parse().unwrap()),
                Note::OutOfCoverage { court: "TA" },
                find_request(Fond::Cetat, NumberKind::Decision, "2005064"),
            ),
            (
                Court::ConseilConstitutionnel,
                CaseNumber::Cc("2025-1173 QPC".parse().unwrap()),
                Note::CouldNotConfirm { index: "CONSTIT" },
                find_request(Fond::Constit, NumberKind::Decision, "2025-1173"),
            ),
        ];
        for (court, number, absent, first_request) in cases {
            let label = format!("{court:?}");
            let corpus = FakeCorpus::new();
            corpus.push_find_by_number(Ok(vec![]));
            corpus.push_find_by_number(Ok(vec![]));
            let item = decision_item(Some(court), vec![number], None);
            let resolution = resolve_with(&corpus, &codes(), &item, None).await;
            assert_eq!(resolution.outcome, unresolvable_outcome(absent), "{label}");
            assert_eq!(corpus.seen_requests()[0], first_request, "{label}");
        }
    }

    #[tokio::test]
    async fn a_number_of_another_family_than_the_court_is_a_missing_number() {
        let corpus = FakeCorpus::new();
        let date = Some(civil(2025, 6, 4));
        let item = decision_item(Some(conseil_etat()), vec![pourvoi()], date);
        let resolution = resolve_with(&corpus, &codes(), &item, None).await;
        assert_eq!(
            resolution.outcome,
            ambiguous_outcome(Note::NoNumber { fond: Fond::Cetat, date })
        );
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn the_decision_search_refuses_what_offline_would_have_decided() {
        let corpus = FakeCorpus::new();
        let codes = codes();
        let resolver = resolver(&corpus, &codes);
        let cite = decision_cite(None, vec![pourvoi()], None);
        assert_eq!(resolver.decision(&cite).await.outcome, ambiguous_outcome(Note::NoCourt));
        assert_eq!(corpus.calls(), 0);
    }

    fn lookup_spec(forms: &[&str]) -> Lookup {
        Lookup {
            fond: Fond::Juri,
            kind: NumberKind::Case,
            forms: forms.iter().map(|form| (*form).to_owned()).collect(),
            title_filter: None,
            date: None,
            cc_type: None,
            chamber: None,
            absent: Note::CouldNotConfirm { index: "JURI" },
        }
    }

    #[tokio::test]
    async fn the_next_form_is_tried_only_when_the_previous_one_found_nothing() {
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![hit(Fond::Juri, "JURITEXT000051743650")]));
        corpus.push_get_decision(Ok(decision("JURITEXT000051743650", civil(2025, 6, 4), None)));
        let codes = codes();
        let resolution = resolver(&corpus, &codes).lookup(lookup_spec(&["a", "b"])).await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert_eq!(
            corpus.seen_requests().iter().map(PortRequest::method).collect::<Vec<_>>(),
            ["find_by_number", "get_decision"]
        );
    }

    #[tokio::test]
    async fn every_hit_up_to_the_limit_is_consulted_and_one_more_is_not() {
        let ids: Vec<String> = (1..=CONSULT_LIMIT).map(|n| format!("JURITEXT{n:012}")).collect();
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(ids.iter().map(|id| hit(Fond::Juri, id)).collect()));
        for id in &ids {
            corpus.push_get_decision(Ok(decision(id, civil(2025, 6, 4), None)));
        }
        let codes = codes();
        let resolution = resolver(&corpus, &codes).lookup(lookup_spec(&["a"])).await;
        assert_eq!(corpus.calls_of("get_decision"), CONSULT_LIMIT);
        let (items, note) = candidates_of(resolution.outcome).expect("des candidats");
        assert_eq!(items.len(), MAX_CANDIDATES);
        assert_eq!(note, Note::Candidates { n: CONSULT_LIMIT });
    }

    #[tokio::test]
    async fn a_title_filter_drops_the_hits_of_other_courts_but_keeps_untitled_ones() {
        let spec = |filter| Lookup {
            fond: Fond::Cetat,
            kind: NumberKind::Decision,
            title_filter: Some(filter),
            absent: Note::CouldNotConfirm { index: "CETAT" },
            ..lookup_spec(&["506507"])
        };
        let tribunal =
            titled(Fond::Cetat, "CETATEXT000051655317", "Tribunal administratif de Paris");
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![tribunal.clone()]));
        let codes = codes();
        let resolution = resolver(&corpus, &codes).lookup(spec(is_conseil_etat)).await;
        assert_eq!(
            resolution.outcome,
            unresolvable_outcome(Note::CouldNotConfirm { index: "CETAT" })
        );
        assert_eq!(corpus.calls_of("get_decision"), 0);

        let untitled = hit(Fond::Cetat, "CETATEXT000051655318");
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![tribunal, untitled]));
        corpus.push_get_decision(Ok(decision("CETATEXT000051655318", civil(2026, 5, 29), None)));
        let resolution = resolver(&corpus, &codes).lookup(spec(is_conseil_etat)).await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert_eq!(
            resolution.outcome.record().map(|record| record.id.as_str().to_owned()).as_deref(),
            Some("CETATEXT000051655318")
        );
    }

    #[tokio::test]
    async fn a_hit_that_is_not_a_decision_is_skipped_and_a_failure_is_reported() {
        let mut stray = hit(Fond::Juri, "JURITEXT000051743650");
        stray.id = DocumentId::parse("LEGITEXT000033202746").unwrap();
        let codes = codes();

        let corpus = FakeCorpus::new();
        corpus
            .push_find_by_number(Ok(vec![stray.clone(), hit(Fond::Juri, "JURITEXT000051743651")]));
        corpus.push_get_decision(Ok(decision("JURITEXT000051743651", civil(2025, 6, 4), None)));
        let resolution = resolver(&corpus, &codes).lookup(lookup_spec(&["a"])).await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert_eq!(corpus.calls_of("get_decision"), 1);

        // Aucun hit consultable : introuvable, sans appel.
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![stray]));
        let resolution = resolver(&corpus, &codes).lookup(lookup_spec(&["a"])).await;
        assert_eq!(
            resolution.outcome,
            unresolvable_outcome(Note::CouldNotConfirm { index: "JURI" })
        );
        assert_eq!(corpus.calls_of("get_decision"), 0);

        // Une décision introuvable à la lecture ne compte pas ; une autre erreur est rendue.
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![hit(Fond::Juri, "JURITEXT000051743650")]));
        corpus.push_get_decision(Err(not_found()));
        let resolution = resolver(&corpus, &codes).lookup(lookup_spec(&["a"])).await;
        assert_eq!(
            resolution.outcome,
            unresolvable_outcome(Note::CouldNotConfirm { index: "JURI" })
        );
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![hit(Fond::Juri, "JURITEXT000051743650")]));
        corpus.push_get_decision(Err(upstream()));
        let resolution = resolver(&corpus, &codes).lookup(lookup_spec(&["a"])).await;
        assert_eq!(resolution.outcome, Outcome::Failed(upstream()));
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Err(upstream()));
        let resolution = resolver(&corpus, &codes).lookup(lookup_spec(&["a", "b"])).await;
        assert_eq!(resolution.outcome, Outcome::Failed(upstream()));
        assert_eq!(corpus.calls(), 1);
    }

    #[tokio::test]
    async fn a_number_the_api_cannot_take_is_an_invalid_request_without_a_call() {
        let corpus = FakeCorpus::new();
        let codes = codes();
        let resolver = resolver(&corpus, &codes);
        let invalid = |detail: &str| {
            Err(CorpusError::InvalidRequest { status: None, detail: detail.to_owned() })
        };
        let too_long = "9".repeat(NUMBER_VALUE_MAX_CHARS + 1);
        assert_eq!(
            resolver.by_number(Fond::Juri, NumberKind::Case, &too_long).await,
            invalid("number value")
        );
        assert_eq!(
            resolver.by_number(Fond::Juri, NumberKind::Case, "").await,
            invalid("number value")
        );
        assert_eq!(
            resolver.by_number(Fond::Juri, NumberKind::Case, "é").await,
            invalid("number value")
        );
        // Un fond qui n'a pas cette nature de numéro.
        assert_eq!(
            resolver.by_number(Fond::Juri, NumberKind::Nor, "ECFI1524250L").await,
            invalid("number query")
        );
        assert_eq!(corpus.calls(), 0);
        // La limite elle-même passe.
        let at_limit = "9".repeat(NUMBER_VALUE_MAX_CHARS);
        corpus.push_find_by_number(Ok(vec![]));
        assert_eq!(resolver.by_number(Fond::Juri, NumberKind::Case, &at_limit).await, Ok(vec![]));

        // Par la citation : un numéro de forme inconnue trop long n'atteint pas l'API.
        let corpus = FakeCorpus::new();
        let item = decision_item(Some(cassation()), vec![CaseNumber::Unknown(too_long)], None);
        let resolution = resolve_with(&corpus, &codes, &item, None).await;
        assert_eq!(
            resolution.outcome,
            Outcome::Failed(CorpusError::InvalidRequest {
                status: None,
                detail: "number value".to_owned()
            })
        );
        assert_eq!(corpus.calls(), 0);
    }

    // ----------------------------------------------------------------------------------------
    // Choix parmi les décisions consultées
    // ----------------------------------------------------------------------------------------

    fn found_of(dates: &[CivilDate], nature: Option<&str>) -> Vec<(DocumentId, Decision)> {
        dates
            .iter()
            .enumerate()
            .map(|(n, date)| {
                let id = format!("JURITEXT{:012}", n + 1);
                (DocumentId::parse(&id).unwrap(), decision(&id, *date, nature))
            })
            .collect()
    }

    fn record_id(resolution: &Resolution) -> Option<String> {
        resolution.outcome.record().map(|record| record.id.as_str().to_owned())
    }

    #[test]
    fn the_decision_of_the_cited_day_is_chosen_with_high_confidence_and_its_text() {
        let day = civil(2025, 6, 4);
        let found = found_of(&[civil(2025, 6, 3), day, civil(2025, 6, 5)], None);
        let chosen = choose_decision(&found, Some(day), None, None);
        assert_eq!(confidence_of(&chosen.outcome), Some(Confidence::High));
        assert_eq!(record_id(&chosen).as_deref(), Some("JURITEXT000000000002"));
        assert_eq!(chosen.official.as_deref(), Some("Texte de la décision."));
        assert_eq!(chosen.outcome.note(Proof::Production), None);
    }

    #[test]
    fn a_single_decision_of_another_day_is_a_date_mismatch_carrying_its_text() {
        let found = found_of(&[civil(2025, 6, 5)], None);
        let chosen = choose_decision(&found, Some(civil(2025, 6, 4)), None, None);
        assert_eq!(
            chosen.outcome.note(Proof::Production),
            Some(Note::Mismatch {
                field: Field::Date,
                cited: "2025-06-04".to_owned(),
                actual: "2025-06-05".to_owned()
            })
        );
        assert_eq!(confidence_of(&chosen.outcome), None);
        assert_eq!(chosen.official.as_deref(), Some("Texte de la décision."));
        assert_eq!(record_id(&chosen).as_deref(), Some("JURITEXT000000000001"));
    }

    #[test]
    fn without_a_date_a_single_decision_is_medium_and_several_are_candidates() {
        let found = found_of(&[civil(2025, 6, 5)], None);
        let chosen = choose_decision(&found, None, None, None);
        assert_eq!(confidence_of(&chosen.outcome), Some(Confidence::Medium));
        assert_eq!(chosen.official.as_deref(), Some("Texte de la décision."));

        let many = found_of(&[civil(2025, 6, 5), civil(2025, 6, 6)], None);
        let (items, note) =
            candidates_of(choose_decision(&many, None, None, None).outcome).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(note, Note::Candidates { n: 2 });
        assert!(
            items.iter().all(|c| c.kind == CandidateKind::Juri && c.confidence == Confidence::Low)
        );
    }

    #[test]
    fn several_decisions_that_fit_or_none_of_them_are_candidates() {
        let day = civil(2025, 6, 4);
        // Deux décisions du jour cité.
        let twice = found_of(&[day, day], None);
        let (_, note) =
            candidates_of(choose_decision(&twice, Some(day), None, None).outcome).unwrap();
        assert_eq!(note, Note::Candidates { n: 2 });
        // Plusieurs décisions, aucune du jour cité : `lookup` l'écarte avant (`none_on_the_day`),
        // ce choix ne voit plus que les listes qu'une décision sans date laisse ouvertes.
        let other_days = found_of(&[civil(2025, 6, 5), civil(2025, 6, 6)], None);
        let (_, note) =
            candidates_of(choose_decision(&other_days, Some(day), None, None).outcome).unwrap();
        assert_eq!(note, Note::Candidates { n: 2 });
        // Au-delà de cinq, cinq candidats et le compte exact.
        let dates: Vec<CivilDate> = (1..=6).map(|d| civil(2025, 7, d)).collect();
        let six = found_of(&dates, None);
        let (items, note) = candidates_of(choose_decision(&six, None, None, None).outcome).unwrap();
        assert_eq!(items.len(), MAX_CANDIDATES);
        assert_eq!(note, Note::Candidates { n: 6 });
    }

    #[test]
    fn a_constitutional_type_is_compared_only_when_the_record_names_one() {
        let day = civil(2025, 11, 7);
        let mismatch =
            choose_decision(&found_of(&[day], Some("DC")), Some(day), Some(CcType::Qpc), None);
        assert_eq!(
            mismatch.outcome.note(Proof::Production),
            Some(Note::Mismatch {
                field: Field::CcType,
                cited: "QPC".to_owned(),
                actual: "DC".to_owned()
            })
        );
        assert_eq!(mismatch.official.as_deref(), Some("Texte de la décision."));
        let same =
            choose_decision(&found_of(&[day], Some("QPC")), Some(day), Some(CcType::Qpc), None);
        assert_eq!(confidence_of(&same.outcome), Some(Confidence::High));
        assert_eq!(same.official.as_deref(), Some("Texte de la décision."));
        let unnamed = choose_decision(&found_of(&[day], None), Some(day), Some(CcType::Qpc), None);
        assert_eq!(confidence_of(&unnamed.outcome), Some(Confidence::High));
        // La date et le type en écart sont rendus ensemble.
        let found = found_of(&[day], Some("DC"));
        let both = choose_decision(&found, Some(civil(2025, 11, 8)), Some(CcType::Qpc), None);
        assert_eq!(
            both.outcome.note(Proof::Production),
            Some(Note::Mismatches(vec![
                (Field::Date, "2025-11-08".to_owned(), "2025-11-07".to_owned()),
                (Field::CcType, "QPC".to_owned(), "DC".to_owned()),
            ]))
        );
    }

    fn with_formation(dates: &[CivilDate], formation: Option<&str>) -> Vec<(DocumentId, Decision)> {
        let mut found = found_of(dates, None);
        for (_, official) in &mut found {
            official.formation = formation.map(str::to_owned);
        }
        found
    }

    #[test]
    fn only_the_modern_chambers_are_comparable() {
        let modern = [
            CassChamber::Civ1,
            CassChamber::Civ2,
            CassChamber::Civ3,
            CassChamber::Com,
            CassChamber::Soc,
            CassChamber::Crim,
            CassChamber::AssPlen,
            CassChamber::Mixte,
            CassChamber::PremierPresident,
        ];
        for chamber in CassChamber::ALL {
            assert_eq!(is_modern(chamber), modern.contains(&chamber), "{chamber:?}");
        }
    }

    #[test]
    fn the_cited_chamber_is_compared_with_the_official_formation() {
        let same = check_chamber(Some(CassChamber::Soc), Some("CHAMBRE_SOCIALE"));
        assert_eq!(same, ChamberCheck::Fine);
        let differs = check_chamber(Some(CassChamber::Civ1), Some("CHAMBRE_SOCIALE"));
        assert_eq!(differs, ChamberCheck::Differs(CassChamber::Civ1, CassChamber::Soc));
        // Les espaces autour de la valeur officielle ne comptent pas.
        let spaced = check_chamber(Some(CassChamber::Soc), Some(" CHAMBRE_SOCIALE "));
        assert_eq!(spaced, ChamberCheck::Fine);
        // Rien à comparer : chambre citée absente ou historique, formation absente ou vide.
        assert_eq!(check_chamber(None, Some("CHAMBRE_SOCIALE")), ChamberCheck::Fine);
        let unique = check_chamber(Some(CassChamber::CivUnique), Some("CHAMBRE_SOCIALE"));
        assert_eq!(unique, ChamberCheck::Fine);
        assert_eq!(check_chamber(Some(CassChamber::Soc), None), ChamberCheck::Fine);
        assert_eq!(check_chamber(Some(CassChamber::Soc), Some("")), ChamberCheck::Fine);
        assert_eq!(check_chamber(Some(CassChamber::Soc), Some("  ")), ChamberCheck::Fine);
        // Formation inattendue (relevés C4), historique ou casse différente : note, pas d'écart.
        for formation in ["CT0196", "I7", "CHAMBRES_REUNIES", "CHAMBRE_CIVILE", "chambre_sociale"] {
            let check = check_chamber(Some(CassChamber::Soc), Some(formation));
            assert_eq!(check, ChamberCheck::NotComparable, "{formation}");
        }
    }

    #[test]
    fn a_cassation_chamber_gap_is_reported_alone_or_with_the_date() {
        let day = civil(2025, 6, 4);
        let soc = with_formation(&[day], Some("CHAMBRE_SOCIALE"));
        // Chambre seule.
        let alone = choose_decision(&soc, Some(day), None, Some(CassChamber::Civ1));
        assert_eq!(
            alone.outcome.note(Proof::Production),
            Some(Note::Mismatch {
                field: Field::Chamber,
                cited: "CIV1".to_owned(),
                actual: "SOC".to_owned()
            })
        );
        assert_eq!(alone.official.as_deref(), Some("Texte de la décision."));
        // Date et chambre.
        let both = choose_decision(&soc, Some(civil(2025, 6, 5)), None, Some(CassChamber::Civ1));
        assert_eq!(
            both.outcome.note(Proof::Production),
            Some(Note::Mismatches(vec![
                (Field::Date, "2025-06-05".to_owned(), "2025-06-04".to_owned()),
                (Field::Chamber, "CIV1".to_owned(), "SOC".to_owned()),
            ]))
        );
        // Date seule, bonne chambre : l'ancien rendu, un seul écart.
        let date_only =
            choose_decision(&soc, Some(civil(2025, 6, 5)), None, Some(CassChamber::Soc));
        assert_eq!(
            date_only.outcome.note(Proof::Production),
            Some(Note::Mismatch {
                field: Field::Date,
                cited: "2025-06-05".to_owned(),
                actual: "2025-06-04".to_owned()
            })
        );
        // Bonne chambre et bonne date : vérifié, sans note.
        let fine = choose_decision(&soc, Some(day), None, Some(CassChamber::Soc));
        assert_eq!(confidence_of(&fine.outcome), Some(Confidence::High));
        assert_eq!(fine.outcome.note(Proof::Production), None);
    }

    #[test]
    fn an_unexpected_formation_is_verified_with_a_note_and_a_date_gap_still_wins() {
        let day = civil(2025, 6, 4);
        let odd = with_formation(&[day], Some("CT0196"));
        let noted = choose_decision(&odd, Some(day), None, Some(CassChamber::Soc));
        assert_eq!(confidence_of(&noted.outcome), Some(Confidence::High));
        assert_eq!(noted.outcome.note(Proof::Production), Some(Note::ChamberNotCompared));
        // Une chambre non citée ne produit aucune note, même devant une formation inattendue.
        let silent = choose_decision(&odd, Some(day), None, None);
        assert_eq!(silent.outcome.note(Proof::Production), None);
        // Un écart de date reste un écart de date : la note de chambre ne s'y ajoute pas.
        let late = choose_decision(&odd, Some(civil(2025, 6, 5)), None, Some(CassChamber::Soc));
        assert_eq!(
            late.outcome.note(Proof::Production),
            Some(Note::Mismatch {
                field: Field::Date,
                cited: "2025-06-05".to_owned(),
                actual: "2025-06-04".to_owned()
            })
        );
    }

    // ----------------------------------------------------------------------------------------
    // ECLI
    // ----------------------------------------------------------------------------------------

    const CC_ECLI: &str = "ECLI:FR:CC:2025:2025.1173.QPC";
    const CE_ECLI: &str = "ECLI:FR:CECHR:2026:506507.20260529";
    const CASS_ECLI: &str = "ECLI:FR:CCASS:2025:C100399";

    fn ecli_item(text: &str) -> Spanned<CitationRef> {
        standalone(CitationRef::Ecli(Ecli::parse(text).unwrap()))
    }

    fn native_note(text: &str) -> Note {
        Note::NativeEcli(Ecli::parse(text).unwrap())
    }

    #[tokio::test]
    async fn a_constitutional_ecli_is_searched_by_its_number_and_compared_with_the_native_one() {
        let id = "CONSTEXT000052555275";
        let day = civil(2025, 11, 7);
        let expected_requests = vec![
            find_request(Fond::Constit, NumberKind::Decision, "2025-1173"),
            PortRequest::GetDecision(DecisionId::parse(id).unwrap()),
        ];
        let corpus_for = |native: Option<&str>| {
            let corpus = FakeCorpus::new();
            corpus.push_find_by_number(Ok(vec![hit(Fond::Constit, id)]));
            corpus.push_get_decision(Ok(with_ecli(decision(id, day, Some("QPC")), native)));
            corpus
        };

        let corpus = corpus_for(Some(CC_ECLI));
        let resolution = resolve_with(&corpus, &codes(), &ecli_item(CC_ECLI), None).await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert_eq!(confidence_of(&resolution.outcome), Some(Confidence::High));
        assert_eq!(resolution.outcome.note(Proof::Production), Some(native_note(CC_ECLI)));
        assert_eq!(resolution.official.as_deref(), Some("Texte de la décision."));
        assert_eq!(corpus.seen_requests(), expected_requests);

        // Un autre ECLI natif : `invalid`, avec l'enregistrement réel (N14).
        let other = "ECLI:FR:CC:2025:2025.9999.QPC";
        let corpus = corpus_for(Some(other));
        let resolution = resolve_with(&corpus, &codes(), &ecli_item(CC_ECLI), None).await;
        assert_eq!(status(&resolution), CheckStatus::Invalid);
        assert_eq!(resolution.outcome.note(Proof::Production), Some(native_note(other)));
        assert_eq!(record_id(&resolution).as_deref(), Some(id));
        assert_eq!(resolution.official.as_deref(), Some("Texte de la décision."));

        // Pas d'ECLI natif : rien à comparer.
        let corpus = corpus_for(None);
        let resolution = resolve_with(&corpus, &codes(), &ecli_item(CC_ECLI), None).await;
        assert_eq!(
            resolution.outcome,
            unresolvable_outcome(Note::CouldNotConfirm { index: "CONSTIT" })
        );
        assert_eq!(resolution.official.as_deref(), Some("Texte de la décision."));
    }

    #[tokio::test]
    async fn a_conseil_d_etat_ecli_is_searched_by_the_request_number_among_its_own_decisions() {
        let id = "CETATEXT000051655317";
        let day = civil(2026, 5, 29);
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![
            titled(Fond::Cetat, id, "Conseil d'État, 5ème chambre"),
            titled(Fond::Cetat, "CETATEXT000051655318", "Tribunal administratif de Paris"),
        ]));
        corpus.push_get_decision(Ok(with_ecli(decision(id, day, None), Some(CE_ECLI))));
        let resolution = resolve_with(&corpus, &codes(), &ecli_item(CE_ECLI), None).await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert_eq!(resolution.outcome.note(Proof::Production), Some(native_note(CE_ECLI)));
        assert_eq!(
            corpus.seen_requests(),
            vec![
                find_request(Fond::Cetat, NumberKind::Decision, "506507"),
                PortRequest::GetDecision(DecisionId::parse(id).unwrap()),
            ]
        );

        // Un hit d'une autre juridiction n'est pas consulté.
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![titled(
            Fond::Cetat,
            "CETATEXT000051655318",
            "Tribunal administratif de Paris",
        )]));
        let resolution = resolve_with(&corpus, &codes(), &ecli_item(CE_ECLI), None).await;
        assert_eq!(
            resolution.outcome,
            unresolvable_outcome(Note::CouldNotConfirm { index: "CETAT" })
        );
        assert_eq!(corpus.calls_of("get_decision"), 0);

        // Un autre ECLI natif est un écart (la requête dérive de l'ordinal).
        let other = "ECLI:FR:CECHR:2026:506507.20260530";
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![titled(Fond::Cetat, id, "Conseil d'État")]));
        corpus.push_get_decision(Ok(with_ecli(decision(id, day, None), Some(other))));
        let resolution = resolve_with(&corpus, &codes(), &ecli_item(CE_ECLI), None).await;
        assert_eq!(status(&resolution), CheckStatus::Invalid);
        assert_eq!(resolution.outcome.note(Proof::Production), Some(native_note(other)));
    }

    #[tokio::test]
    async fn a_cassation_ecli_is_searched_by_facet_and_must_match_exactly() {
        let id = "JURITEXT000051743650";
        let day = civil(2025, 6, 4);
        let corpus_for = |native: Option<&str>| {
            let corpus = FakeCorpus::new();
            corpus.push_find_by_number(Ok(vec![hit(Fond::Juri, id)]));
            corpus.push_get_decision(Ok(with_ecli(decision(id, day, None), native)));
            corpus
        };

        let corpus = corpus_for(Some(CASS_ECLI));
        let resolution = resolve_with(&corpus, &codes(), &ecli_item(CASS_ECLI), None).await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert_eq!(resolution.outcome.note(Proof::Production), Some(native_note(CASS_ECLI)));
        assert_eq!(
            corpus.seen_requests()[0],
            find_request(Fond::Juri, NumberKind::Ecli, CASS_ECLI)
        );

        // Le numéro n'est pas dérivé de l'ECLI de la Cour de cassation : un ECLI différent ou
        // absent ne prouve rien, ce n'est jamais un écart.
        for native in [Some("ECLI:FR:CCASS:2025:C100400"), None] {
            let corpus = corpus_for(native);
            let resolution = resolve_with(&corpus, &codes(), &ecli_item(CASS_ECLI), None).await;
            assert_eq!(
                resolution.outcome,
                unresolvable_outcome(Note::CouldNotConfirm { index: "JURI" }),
                "{native:?}"
            );
        }

        // Introuvable : la note de l'index traverse la comparaison.
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![]));
        let resolution = resolve_with(&corpus, &codes(), &ecli_item(CASS_ECLI), None).await;
        assert_eq!(
            resolution.outcome,
            unresolvable_outcome(Note::CouldNotConfirm { index: "JURI" })
        );
    }

    #[tokio::test]
    async fn an_ecli_of_another_court_or_without_a_sequence_is_not_supported() {
        let not_supported_ecli =
            unresolvable_outcome(Note::NotSupported { what: "ECLI of this court" });
        // « CC » sans numéro d'ordre, juridictions hors périmètre, et « CE » seul n'est pas « CC ».
        for text in ["ECLI:FR:CC:2025:1", "ECLI:FR:CAA:2025:1", "ECLI:FR:XYZ:2025:ABC.1"] {
            let corpus = FakeCorpus::new();
            let resolution = resolve_with(&corpus, &codes(), &ecli_item(text), None).await;
            assert_eq!(resolution.outcome, not_supported_ecli, "{text}");
            assert_eq!(corpus.calls(), 0, "{text}");
        }
    }

    #[test]
    fn a_failure_or_a_mismatch_passes_through_the_ecli_comparison() {
        let cited = Ecli::parse(CC_ECLI).unwrap();
        let failed = Resolution::failed(upstream());
        assert_eq!(
            check_ecli(failed, &cited, "CONSTIT", false).outcome,
            Outcome::Failed(upstream())
        );
        let official = Some("texte".to_owned());
        let given = Resolution {
            outcome: Outcome::Invalid { note: Note::NoCourt },
            official: official.clone(),
        };
        let checked = check_ecli(given, &cited, "CONSTIT", true);
        assert_eq!(checked.outcome, Outcome::Invalid { note: Note::NoCourt });
        assert_eq!(checked.official, official);
    }

    // ----------------------------------------------------------------------------------------
    // Délibérations de la CNIL
    // ----------------------------------------------------------------------------------------

    fn cnil_item(date: Option<CivilDate>) -> Spanned<CitationRef> {
        standalone(CitationRef::CnilDeliberation(CnilCite {
            series: Some("SAN".to_owned()),
            year: 2026,
            seq: 2,
            date,
            formation_restreinte: false,
        }))
    }

    #[tokio::test]
    async fn a_cnil_deliberation_is_sure_only_when_the_cited_date_agrees() {
        let id = "CNILTEXT000053352643";
        let dated = |cited: Option<CivilDate>, actual: Option<CivilDate>| {
            let corpus = FakeCorpus::new();
            corpus.push_find_by_number(Ok(vec![hit(Fond::Cnil, id)]));
            let mut found = deliberation(civil(2026, 1, 8));
            found.date_texte = actual;
            corpus.push_get_deliberation(Ok(found));
            (corpus, cnil_item(cited))
        };
        let day = civil(2026, 1, 8);

        let (corpus, item) = dated(Some(day), Some(day));
        let resolution = resolve_with(&corpus, &codes(), &item, None).await;
        assert_eq!(confidence_of(&resolution.outcome), Some(Confidence::High));
        assert_eq!(
            corpus.seen_requests(),
            vec![
                find_request(Fond::Cnil, NumberKind::Deliberation, "SAN-2026-002"),
                PortRequest::GetDeliberation(CnilTextId::parse(id).unwrap()),
            ]
        );

        // Une date citée sans date officielle, ou l'inverse, ne confirme rien.
        for (cited, actual) in [(None, Some(day)), (Some(day), None), (None, None)] {
            let (corpus, item) = dated(cited, actual);
            let resolution = resolve_with(&corpus, &codes(), &item, None).await;
            assert_eq!(confidence_of(&resolution.outcome), Some(Confidence::Medium), "{cited:?}");
        }

        let (corpus, item) = dated(Some(day), Some(civil(2026, 1, 9)));
        let resolution = resolve_with(&corpus, &codes(), &item, None).await;
        assert_eq!(
            resolution.outcome.note(Proof::Production),
            Some(Note::Mismatch {
                field: Field::Date,
                cited: "2026-01-08".to_owned(),
                actual: "2026-01-09".to_owned()
            })
        );
    }

    #[tokio::test]
    async fn a_cnil_search_that_goes_wrong_is_reported_as_it_is() {
        let id = "CNILTEXT000053352643";
        let unconfirmed = unresolvable_outcome(Note::CouldNotConfirm { index: "CNIL" });
        let item = cnil_item(None);

        // Le hit n'est pas une délibération.
        let mut stray = hit(Fond::Cnil, id);
        stray.id = DocumentId::parse("JURITEXT000051743650").unwrap();
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![stray]));
        let resolution = resolve_with(&corpus, &codes(), &item, None).await;
        assert_eq!(resolution.outcome, unconfirmed);
        assert_eq!(corpus.calls_of("get_deliberation"), 0);

        // Délibération introuvable à la lecture.
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![hit(Fond::Cnil, id)]));
        corpus.push_get_deliberation(Err(not_found()));
        let resolution = resolve_with(&corpus, &codes(), &item, None).await;
        assert_eq!(resolution.outcome, unconfirmed);

        // Erreur amont à la lecture, puis à la recherche.
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![hit(Fond::Cnil, id)]));
        corpus.push_get_deliberation(Err(upstream()));
        let resolution = resolve_with(&corpus, &codes(), &item, None).await;
        assert_eq!(resolution.outcome, Outcome::Failed(upstream()));
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Err(upstream()));
        let resolution = resolve_with(&corpus, &codes(), &item, None).await;
        assert_eq!(resolution.outcome, Outcome::Failed(upstream()));

        // Plusieurs délibérations : des candidats, sans lecture.
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![
            hit(Fond::Cnil, id),
            hit(Fond::Cnil, "CNILTEXT000053352644"),
        ]));
        let resolution = resolve_with(&corpus, &codes(), &item, None).await;
        let (items, note) = candidates_of(resolution.outcome).unwrap();
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|candidate| candidate.kind == CandidateKind::Cnil));
        assert_eq!(note, Note::Candidates { n: 2 });
        assert_eq!(corpus.calls_of("get_deliberation"), 0);
    }

    // ----------------------------------------------------------------------------------------
    // Index : IDCC, NOR, ELI
    // ----------------------------------------------------------------------------------------

    #[tokio::test]
    async fn an_index_answers_with_a_record_candidates_or_a_failure() {
        let item = convention_item(Some(1486), None);
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![hit(Fond::Kali, "KALITEXT000005635096")]));
        let resolution = resolve_with(&corpus, &codes(), &item, None).await;
        assert_eq!(confidence_of(&resolution.outcome), Some(Confidence::Medium));
        assert_eq!(
            corpus.seen_requests(),
            vec![find_request(Fond::Kali, NumberKind::Idcc, "1486")]
        );

        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![]));
        let resolution = resolve_with(&corpus, &codes(), &item, None).await;
        assert_eq!(
            resolution.outcome,
            unresolvable_outcome(Note::CouldNotConfirm { index: "KALI" })
        );

        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![
            hit(Fond::Kali, "KALITEXT000005635096"),
            hit(Fond::Kali, "KALITEXT000005635097"),
        ]));
        let resolution = resolve_with(&corpus, &codes(), &item, None).await;
        let (items, note) = candidates_of(resolution.outcome).unwrap();
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|candidate| candidate.kind == CandidateKind::Kali));
        assert_eq!(note, Note::Candidates { n: 2 });

        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Err(upstream()));
        let resolution = resolve_with(&corpus, &codes(), &nor_item(), None).await;
        assert_eq!(resolution.outcome, Outcome::Failed(upstream()));
        assert_eq!(
            corpus.seen_requests(),
            vec![find_request(Fond::Loda, NumberKind::Nor, "ECFI1524250L")]
        );
    }

    #[tokio::test]
    async fn a_convention_without_an_idcc_is_ambiguous_without_a_call() {
        let corpus = FakeCorpus::new();
        let codes = codes();
        let resolution = resolver(&corpus, &codes).convention(&convention_cite(None, None)).await;
        assert_eq!(resolution.outcome, ambiguous_outcome(Note::NoAgreement));
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn an_eli_is_sent_up_to_the_length_of_a_number_value() {
        // « /eli/jo/ » compte 8 caractères : le chemin tient pile dans la limite.
        let path = format!("/eli/jo/{}", "a".repeat(NUMBER_VALUE_MAX_CHARS - "/eli/jo/".len()));
        assert_eq!(path.chars().count(), NUMBER_VALUE_MAX_CHARS);
        let eli = standalone(CitationRef::Eli(Eli::parse(&path).unwrap()));
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![hit(Fond::Loda, "LEGITEXT000033202746")]));
        let resolution = resolve_with(&corpus, &codes(), &eli, None).await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert_eq!(corpus.seen_requests(), vec![find_request(Fond::Loda, NumberKind::Eli, &path)]);
    }

    // ----------------------------------------------------------------------------------------
    // Textes numérotés
    // ----------------------------------------------------------------------------------------

    #[test]
    fn only_the_four_plain_natures_have_an_api_nature() {
        let plain = [
            (TextNature::Loi, "LOI"),
            (TextNature::Ordonnance, "ORDONNANCE"),
            (TextNature::Decret, "DECRET"),
            (TextNature::Arrete, "ARRETE"),
        ];
        for (nature, expected) in plain {
            assert_eq!(plain_nature(nature), Some(expected), "{nature:?}");
        }
        for nature in
            TextNature::ALL.into_iter().filter(|nature| plain.iter().all(|(p, _)| p != nature))
        {
            assert_eq!(plain_nature(nature), None, "{nature:?}");
        }
    }

    #[test]
    fn a_hit_of_another_nature_or_date_does_not_fit_the_cited_text() {
        let day = civil(2016, 10, 7);
        for (nature, name) in [
            (TextNature::Loi, "LOI"),
            (TextNature::Ordonnance, "ORDONNANCE"),
            (TextNature::Decret, "DECRET"),
            (TextNature::Arrete, "ARRETE"),
        ] {
            let cite = text_cite(nature, Some((2016, 1321)), Some(day));
            let mut found = law_hit();
            found.nature = Some(name.to_lowercase());
            assert_eq!(fit_of(&cite, &found), TextFit::Exact, "{name}");
            let other = if name == "LOI" { "DECRET" } else { "LOI" };
            found.nature = Some(other.to_owned());
            assert_eq!(fit_of(&cite, &found), TextFit::Nature(other.to_owned()), "{name}");
        }
        // Une nature sans nom univoque ne se compare pas ; la date, si.
        let cite = text_cite(TextNature::Circulaire, Some((2016, 1321)), Some(day));
        let mut found = law_hit();
        found.nature = Some("DECRET".to_owned());
        found.title = Some("LOI n° 2016-1321 du 8 octobre 2016 pour une République".into());
        assert_eq!(fit_of(&cite, &found), TextFit::Date(civil(2016, 10, 8)));
        // Sans titre lisible, rien ne contredit la date citée.
        found.title = None;
        assert_eq!(fit_of(&cite, &found), TextFit::Exact);
    }

    #[test]
    fn a_text_mismatch_names_the_field_and_both_values() {
        let day = civil(2016, 10, 7);
        let cite = text_cite(TextNature::Decret, Some((2016, 1321)), Some(day));
        let found = law_hit();
        let by_date = text_mismatch(&cite, &found, &TextFit::Date(civil(2016, 10, 8)));
        assert_eq!(
            by_date.outcome.note(Proof::Production),
            Some(Note::Mismatch {
                field: Field::Date,
                cited: "2016-10-07".to_owned(),
                actual: "2016-10-08".to_owned()
            })
        );
        let by_nature = text_mismatch(&cite, &found, &TextFit::Nature("LOI".to_owned()));
        assert_eq!(
            by_nature.outcome.note(Proof::Production),
            Some(Note::Mismatch {
                field: Field::Nature,
                cited: "DECRET".to_owned(),
                actual: "LOI".to_owned()
            })
        );
        let exact = text_mismatch(&cite, &found, &TextFit::Exact);
        assert_eq!(confidence_of(&exact.outcome), Some(Confidence::Medium));
        assert_eq!(record_id(&exact).as_deref(), Some("LEGITEXT000033202746"));
    }

    fn text_item(date: Option<CivilDate>) -> Spanned<CitationRef> {
        standalone(CitationRef::Text(text_cite(TextNature::Loi, Some((2016, 1321)), date)))
    }

    #[tokio::test]
    async fn a_numbered_text_is_sure_only_with_a_cited_date() {
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![law_hit()]));
        let resolution =
            resolve_with(&corpus, &codes(), &text_item(Some(civil(2016, 10, 7))), None).await;
        assert_eq!(confidence_of(&resolution.outcome), Some(Confidence::High));
        assert_eq!(
            corpus.seen_requests(),
            vec![find_request(Fond::Loda, NumberKind::Text, "2016-1321")]
        );

        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![law_hit()]));
        let resolution = resolve_with(&corpus, &codes(), &text_item(None), None).await;
        assert_eq!(confidence_of(&resolution.outcome), Some(Confidence::Medium));

        // Une date citée qui diffère de celle du texte est un écart.
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![law_hit()]));
        let resolution =
            resolve_with(&corpus, &codes(), &text_item(Some(civil(2016, 10, 8))), None).await;
        assert_eq!(
            resolution.outcome.note(Proof::Production),
            Some(Note::Mismatch {
                field: Field::Date,
                cited: "2016-10-08".to_owned(),
                actual: "2016-10-07".to_owned()
            })
        );
    }

    #[test]
    fn a_text_number_is_searched_in_the_forms_the_index_may_hold() {
        let number =
            |year: Option<u16>, seq| TextNumber { year, seq, extra: None, raw: String::new() };
        for (year, expected) in [
            (None, vec!["1419"]),
            (Some(1993), vec!["93-1419", "1993-1419"]),
            (Some(1900), vec!["00-1419", "1900-1419"]),
            (Some(1905), vec!["05-1419", "1905-1419"]),
            (Some(1999), vec!["99-1419", "1999-1419"]),
            (Some(2000), vec!["2000-1419"]),
            (Some(2016), vec!["2016-1419"]),
            (Some(1899), vec!["1899-1419"]),
            (Some(1804), vec!["1804-1419"]),
        ] {
            assert_eq!(text_keys(&number(year, 1419)), expected, "{year:?}");
        }
    }

    /// `text_fit` avec la date de signature lue dans le titre pour le numéro cité.
    fn fit_of(cite: &TextCite, hit: &SearchHit) -> TextFit {
        text_fit(cite, hit, cite.number.as_ref().and_then(|number| signed_on(hit, number)))
    }

    #[test]
    fn the_signature_date_is_read_in_the_title_for_the_cited_number_only() {
        let number =
            |year, seq| TextNumber { year: Some(year), seq, extra: None, raw: String::new() };
        let mut found = law_hit();
        found.title = Some(
            "Décret n° 2019-536 du 29 mai 2019 pris pour l'application de la loi n° 2018-493 du \
             20 juin 2018"
                .to_owned(),
        );
        // Le numéro du décret, pas celui de la loi qu'il applique.
        assert_eq!(signed_on(&found, &number(2019, 536)), Some(civil(2019, 5, 29)));
        assert_eq!(signed_on(&found, &number(2018, 493)), Some(civil(2018, 6, 20)));
        assert_eq!(signed_on(&found, &number(2019, 537)), None);
        assert_eq!(signed_on(&found, &number(2018, 536)), None);
        // Un numéro sans année ne se lit pas dans un titre à numéro daté.
        let yearless = TextNumber { year: None, seq: 536, extra: None, raw: String::new() };
        assert_eq!(signed_on(&found, &yearless), None);
        // Sans titre, ou sans date dans le titre : rien à lire.
        found.title = None;
        assert_eq!(signed_on(&found, &number(2019, 536)), None);
        found.title = Some("Décret n° 2019-536 portant diverses mesures".to_owned());
        assert_eq!(signed_on(&found, &number(2019, 536)), None);
        // La date d'entrée en vigueur du hit n'est jamais lue.
        found.date = Some(civil(2019, 6, 1));
        assert_eq!(signed_on(&found, &number(2019, 536)), None);
    }

    /// L2-01 (ground_truth-1) : la sandbox trouve `89-462` et ne trouve rien avec `1989-462`.
    #[tokio::test]
    async fn a_pre_2000_law_is_searched_by_its_two_digit_number_first() {
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![loda_hit(
            "LEGITEXT000006069108",
            "LOI",
            "Loi n° 89-462 du 6 juillet 1989 tendant à améliorer les rapports locatifs",
            Some(civil(1989, 7, 8)),
        )]));
        let resolution = resolve(&corpus, "loi n° 89-462 du 6 juillet 1989").await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert_eq!(confidence_of(&resolution.outcome), Some(Confidence::High));
        assert_eq!(record_id(&resolution).as_deref(), Some("LEGITEXT000006069108"));
        assert_eq!(
            corpus.seen_requests(),
            vec![find_request(Fond::Loda, NumberKind::Text, "89-462")]
        );
    }

    #[tokio::test]
    async fn a_pre_2000_law_falls_back_to_the_four_digit_number() {
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![]));
        corpus.push_find_by_number(Ok(vec![loda_hit(
            "LEGITEXT000006068624",
            "LOI",
            "Loi n° 78-17 du 6 janvier 1978 relative à l'informatique, aux fichiers et aux libertés",
            Some(civil(1978, 1, 7)),
        )]));
        let resolution = resolve(&corpus, "loi n° 78-17 du 6 janvier 1978").await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert_eq!(record_id(&resolution).as_deref(), Some("LEGITEXT000006068624"));
        assert_eq!(
            corpus.seen_requests(),
            vec![
                find_request(Fond::Loda, NumberKind::Text, "78-17"),
                find_request(Fond::Loda, NumberKind::Text, "1978-17"),
            ]
        );
    }

    #[tokio::test]
    async fn a_pre_2000_law_found_nowhere_is_unconfirmed_after_exactly_two_requests() {
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![]));
        corpus.push_find_by_number(Ok(vec![]));
        let resolution = resolve(&corpus, "décret n° 93-1419 du 29 décembre 1993").await;
        assert_eq!(
            resolution.outcome,
            unresolvable_outcome(Note::CouldNotConfirm { index: "LODA" })
        );
        assert_eq!(
            corpus.seen_requests(),
            vec![
                find_request(Fond::Loda, NumberKind::Text, "93-1419"),
                find_request(Fond::Loda, NumberKind::Text, "1993-1419"),
            ]
        );
    }

    /// Une forme qui ne rend que des hits hors LEGI (Journal officiel) ne compte pas comme un
    /// résultat : la forme suivante est essayée.
    #[tokio::test]
    async fn a_form_that_returns_no_legal_text_does_not_stop_the_search() {
        let mut journal = law_hit();
        journal.id = DocumentId::parse("JORFTEXT000006069108").unwrap();
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![journal]));
        corpus.push_find_by_number(Ok(vec![law_hit()]));
        let resolution = resolve(&corpus, "loi n° 89-462").await;
        assert_eq!(record_id(&resolution).as_deref(), Some("LEGITEXT000033202746"));
        assert_eq!(corpus.calls_of("find_by_number"), 2);
    }

    #[tokio::test]
    async fn a_failing_form_fails_the_resolution() {
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Err(upstream()));
        let resolution = resolve(&corpus, "loi n° 89-462").await;
        assert_eq!(resolution.outcome, Outcome::Failed(upstream()));
        assert_eq!(corpus.calls_of("find_by_number"), 1);

        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![]));
        corpus.push_find_by_number(Err(upstream()));
        let resolution = resolve(&corpus, "loi n° 89-462").await;
        assert_eq!(resolution.outcome, Outcome::Failed(upstream()));
        assert_eq!(corpus.calls_of("find_by_number"), 2);
    }

    /// Les lois de 2000 et après, et les numéros sans année, ne coûtent qu'une requête.
    #[tokio::test]
    async fn a_post_1999_law_is_searched_once() {
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![]));
        let resolution = resolve(&corpus, "loi n° 2016-1321 du 7 octobre 2016").await;
        assert_eq!(
            resolution.outcome,
            unresolvable_outcome(Note::CouldNotConfirm { index: "LODA" })
        );
        assert_eq!(
            corpus.seen_requests(),
            vec![find_request(Fond::Loda, NumberKind::Text, "2016-1321")]
        );
    }

    #[tokio::test]
    async fn an_article_of_a_pre_2000_law_is_searched_in_the_law_found_by_its_short_number() {
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![loda_hit(
            "LEGITEXT000006069108",
            "LOI",
            "Loi n° 89-462 du 6 juillet 1989 tendant à améliorer les rapports locatifs",
            Some(civil(1989, 7, 8)),
        )]));
        corpus.push_find_article(Ok(article(Some("22"))));
        let resolution = resolve(&corpus, "article 22 de la loi n° 89-462 du 6 juillet 1989").await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert_eq!(
            corpus.seen_requests(),
            vec![
                find_request(Fond::Loda, NumberKind::Text, "89-462"),
                PortRequest::FindArticle(
                    LegiTextId::parse("LEGITEXT000006069108").unwrap(),
                    "22".to_owned(),
                    ArticleAt::Today(today())
                ),
            ]
        );
    }

    /// L2-02 (resolve-2) : la date citée est la signature, lue dans le titre ; `hit.date`
    /// (entrée en vigueur) n'est jamais comparée.
    #[tokio::test]
    async fn the_cited_date_is_compared_to_the_signature_date_read_in_the_title() {
        let cases = [
            (
                "LOI",
                "LEGITEXT000033205014",
                "LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique (1)",
                civil(2016, 10, 9),
                "loi n° 2016-1321 du 7 octobre 2016",
            ),
            (
                "ORDONNANCE",
                "LEGITEXT000032004939",
                "Ordonnance n° 2016-131 du 10 février 2016 portant réforme du droit des contrats, du régime général et de la preuve des obligations",
                civil(2016, 10, 1),
                "ordonnance n° 2016-131 du 10 février 2016",
            ),
            (
                "DECRET",
                "LEGITEXT000038531569",
                "Décret n° 2019-536 du 29 mai 2019 pris pour l'application de la loi n° 2018-493 du 20 juin 2018 relative à la protection des données personnelles",
                civil(2019, 6, 1),
                "décret n° 2019-536 du 29 mai 2019",
            ),
            (
                "LOI",
                "LEGITEXT000005789847",
                "Loi n° 2004-575 du 21 juin 2004 pour la confiance dans l'économie numérique",
                civil(2004, 6, 22),
                "loi n° 2004-575 du 21 juin 2004",
            ),
        ];
        for (nature, id, title, in_force, cited) in cases {
            let corpus = FakeCorpus::new();
            corpus.push_find_by_number(Ok(vec![loda_hit(id, nature, title, Some(in_force))]));
            let resolution = resolve(&corpus, cited).await;
            assert_eq!(status(&resolution), CheckStatus::Verified, "{cited}");
            assert_eq!(confidence_of(&resolution.outcome), Some(Confidence::High), "{cited}");
            assert_eq!(resolution.outcome.note(Proof::Production), None, "{cited}");
        }
    }

    /// Jumeau « vrai positif conservé » : une date de signature fausse reste un écart, et l'écart
    /// cite la date du titre, jamais la date d'entrée en vigueur.
    #[tokio::test]
    async fn a_wrong_signature_date_is_still_a_mismatch_naming_the_title_date() {
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![law_hit()]));
        let resolution = resolve(&corpus, "loi n° 2016-1321 du 9 octobre 2016").await;
        assert_eq!(status(&resolution), CheckStatus::Invalid);
        assert_eq!(
            resolution.outcome.note(Proof::Production),
            Some(Note::Mismatch {
                field: Field::Date,
                cited: "2016-10-09".to_owned(),
                actual: "2016-10-07".to_owned()
            })
        );
    }

    /// Le résultat ne dépend pas de `hit.date` : absente ou différente, la signature décide.
    #[tokio::test]
    async fn the_hit_date_never_decides() {
        for date in [None, Some(civil(2016, 10, 7)), Some(civil(1999, 1, 1))] {
            let mut found = law_hit();
            found.date = date;
            let corpus = FakeCorpus::new();
            corpus.push_find_by_number(Ok(vec![found.clone()]));
            let resolution = resolve(&corpus, "loi n° 2016-1321 du 7 octobre 2016").await;
            assert_eq!(confidence_of(&resolution.outcome), Some(Confidence::High), "{date:?}");
            let corpus = FakeCorpus::new();
            corpus.push_find_by_number(Ok(vec![found]));
            let resolution = resolve(&corpus, "loi n° 2016-1321 du 8 octobre 2016").await;
            assert_eq!(status(&resolution), CheckStatus::Invalid, "{date:?}");
        }
    }

    /// Titre absent, sans numéro ou sans date : rien n'a été comparé, confiance moyenne.
    #[tokio::test]
    async fn an_unreadable_title_is_exact_with_medium_confidence() {
        for title in [
            None,
            Some("Loi relative à la République numérique"),
            Some("LOI n° 2016-1321 pour une République numérique"),
            Some("Loi n° 2015-990 du 6 août 2015 pour la croissance"),
        ] {
            let mut found = law_hit();
            found.title = title.map(str::to_owned);
            let corpus = FakeCorpus::new();
            corpus.push_find_by_number(Ok(vec![found]));
            let resolution = resolve(&corpus, "loi n° 2016-1321 du 7 octobre 2016").await;
            assert_eq!(status(&resolution), CheckStatus::Verified, "{title:?}");
            assert_eq!(confidence_of(&resolution.outcome), Some(Confidence::Medium), "{title:?}");
        }
    }

    /// Les deux repros de ground_truth-1 sans « n° » (si la grammaire les lit) et sans date.
    #[tokio::test]
    async fn a_pre_2000_law_without_date_is_found_with_medium_confidence() {
        for (cited, number, id, title) in [
            (
                "loi n° 89-462",
                "89-462",
                "LEGITEXT000006069108",
                "Loi n° 89-462 du 6 juillet 1989 tendant à améliorer les rapports locatifs",
            ),
            (
                "loi n° 78-17",
                "78-17",
                "LEGITEXT000006068624",
                "Loi n° 78-17 du 6 janvier 1978 relative à l'informatique, aux fichiers et aux libertés",
            ),
        ] {
            let corpus = FakeCorpus::new();
            corpus.push_find_by_number(Ok(vec![loda_hit(id, "LOI", title, None)]));
            let resolution = resolve(&corpus, cited).await;
            assert_eq!(record_id(&resolution).as_deref(), Some(id), "{cited}");
            assert_eq!(confidence_of(&resolution.outcome), Some(Confidence::Medium), "{cited}");
            assert_eq!(
                corpus.seen_requests(),
                vec![find_request(Fond::Loda, NumberKind::Text, number)],
                "{cited}"
            );
        }
    }

    #[tokio::test]
    async fn a_text_search_keeps_only_legal_texts_and_reports_a_failure() {
        let unconfirmed = unresolvable_outcome(Note::CouldNotConfirm { index: "LODA" });
        // Un hit du Journal officiel n'est pas un texte LEGI.
        let mut journal = law_hit();
        journal.id = DocumentId::parse("JORFTEXT000033202746").unwrap();
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![journal.clone()]));
        let resolution = resolve_with(&corpus, &codes(), &text_item(None), None).await;
        assert_eq!(resolution.outcome, unconfirmed);
        // Mêlé à un texte LEGI, il est écarté : le texte restant est seul.
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![journal, law_hit()]));
        let resolution = resolve_with(&corpus, &codes(), &text_item(None), None).await;
        assert_eq!(record_id(&resolution).as_deref(), Some("LEGITEXT000033202746"));
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Err(upstream()));
        let resolution = resolve_with(&corpus, &codes(), &text_item(None), None).await;
        assert_eq!(resolution.outcome, Outcome::Failed(upstream()));
    }

    #[tokio::test]
    async fn a_text_without_a_number_is_not_searched() {
        let corpus = FakeCorpus::new();
        let codes = codes();
        let step = resolver(&corpus, &codes).text(&text_cite(TextNature::Loi, None, None)).await;
        assert_eq!(
            step.into_resolution().outcome,
            unresolvable_outcome(Note::NotSupported { what: "text cited without a number" })
        );
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn an_article_of_a_text_uses_the_cited_date_or_gives_up() {
        let cited_article = |at_date| {
            standalone(CitationRef::TextArticle {
                text: Some(text_cite(
                    TextNature::Loi,
                    Some((2016, 1321)),
                    Some(civil(2016, 10, 7)),
                )),
                article: art("6"),
                at_date,
            })
        };
        let on = civil(2018, 1, 1);
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![law_hit()]));
        corpus.push_find_article(Ok(article(Some("6"))));
        let resolution = resolve_with(&corpus, &codes(), &cited_article(Some(on)), None).await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert_eq!(
            corpus.seen_requests()[1],
            PortRequest::FindArticle(
                LegiTextId::parse("LEGITEXT000033202746").unwrap(),
                "6".to_owned(),
                ArticleAt::On(VersionDate::try_from(on).unwrap())
            )
        );

        let corpus = FakeCorpus::new();
        let resolution =
            resolve_with(&corpus, &codes(), &cited_article(Some(civil(1400, 1, 1))), None).await;
        assert_eq!(
            resolution.outcome,
            unresolvable_outcome(Note::NotSupported { what: "date out of range" })
        );
        assert_eq!(corpus.calls(), 0);

        // Le texte lui-même introuvable : l'issue du texte est rendue telle quelle.
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![]));
        let resolution = resolve_with(&corpus, &codes(), &cited_article(None), None).await;
        assert_eq!(
            resolution.outcome,
            unresolvable_outcome(Note::CouldNotConfirm { index: "LODA" })
        );
        assert_eq!(corpus.calls_of("find_article"), 0);
    }

    // ----------------------------------------------------------------------------------------
    // Articles de code
    // ----------------------------------------------------------------------------------------

    #[test]
    fn the_version_date_comes_from_the_qualifier_then_the_citation_then_the_caller() {
        let explicit =
            |version, cited, tool| Resolver::<FakeCorpus>::explicit_date(version, cited, tool);
        let tool = VersionDate::parse("2022-01-01").unwrap();
        let version = |date: CivilDate| VersionDate::try_from(date).unwrap();
        let (qualified, cited) = (civil(2020, 1, 1), civil(2021, 1, 1));

        let in_force_at = VersionQualifier::EnVigueurAu(qualified);
        assert_eq!(
            explicit(Some(&in_force_at), Some(cited), Some(tool)),
            Ok(Some(version(qualified)))
        );
        let wording = VersionQualifier::RedactionEnVigueur(Some(qualified));
        assert_eq!(explicit(Some(&wording), Some(cited), Some(tool)), Ok(Some(version(qualified))));

        let newer = VersionQualifier::Nouveau;
        let wording_in_force = VersionQualifier::RedactionEnVigueur(None);
        for open in [Some(&newer), Some(&wording_in_force), None] {
            assert_eq!(explicit(open, Some(cited), Some(tool)), Ok(Some(version(cited))));
            assert_eq!(explicit(open, None, Some(tool)), Ok(Some(tool)));
            assert_eq!(explicit(open, None, None), Ok(None));
        }

        let amending = Box::new(text_cite(TextNature::Loi, Some((2016, 1321)), None));
        let unknown = [
            VersionQualifier::Ancien,
            VersionQualifier::RedactionApplicableAuLitige,
            VersionQualifier::RedactionApplicableDateDesFaits,
            VersionQualifier::RedactionIssueDe(amending.clone()),
            VersionQualifier::RedactionAnterieureA(amending),
        ];
        for qualifier in &unknown {
            assert_eq!(
                explicit(Some(qualifier), Some(cited), Some(tool)),
                Err("version date unknown"),
                "{qualifier:?}"
            );
        }
        assert_eq!(explicit(None, Some(civil(1400, 1, 1)), None), Err("date out of range"));
        assert_eq!(
            explicit(Some(&VersionQualifier::EnVigueurAu(civil(1400, 1, 1))), None, None),
            Err("date out of range")
        );
    }

    #[tokio::test]
    async fn a_code_cited_by_a_title_outside_the_dictionary_is_searched_by_its_listed_id() {
        let item = code_item(CodeRef::Unknown("code civil".to_owned()), art("9"));
        let corpus = FakeCorpus::new();
        corpus.push_find_article(Err(not_found()));
        let resolution = resolve_with(&corpus, &codes(), &item, None).await;
        assert_eq!(
            corpus.seen_requests(),
            vec![PortRequest::FindArticle(
                LegiTextId::parse(CIVIL).unwrap(),
                "9".to_owned(),
                ArticleAt::Today(today())
            )]
        );
        // L'absence est prouvée ; la note nomme le code par son identifiant listé.
        assert_eq!(status(&resolution), CheckStatus::NotFound);
        assert_eq!(
            resolution.outcome.note(Proof::Production),
            Some(Note::NotInForce { code: CIVIL.to_owned(), num: "9".to_owned(), date: today() })
        );

        let unlisted = code_item(CodeRef::Unknown("code de la voirie".to_owned()), art("9"));
        let corpus = FakeCorpus::new();
        let resolution = resolve_with(&corpus, &codes(), &unlisted, None).await;
        assert_eq!(resolution.outcome, unresolvable_outcome(Note::CodeNotListed));
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn an_article_without_a_code_to_read_is_ambiguous() {
        let corpus = FakeCorpus::new();
        let codes = codes();
        for code in [CodeRef::Missing, CodeRef::Anaphora(Anaphora::Dudit)] {
            let resolution =
                resolver(&corpus, &codes).code_article(&code, &art("12").num, None, None).await;
            assert_eq!(
                resolution.outcome,
                ambiguous_outcome(Note::MissingTarget { num: "12".to_owned() }),
                "{code:?}"
            );
        }
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn a_failing_article_search_is_reported_and_the_sigle_reading_does_not_hide_it() {
        let corpus = FakeCorpus::new();
        corpus.push_find_article(Err(upstream()));
        let resolution = resolve(&corpus, "article 9 du code civil").await;
        assert_eq!(resolution.outcome, Outcome::Failed(upstream()));

        let corpus = FakeCorpus::new();
        corpus.push_find_article(Err(upstream()));
        let resolution = resolve(&corpus, "article 121-3 CP").await;
        assert_eq!(resolution.outcome, Outcome::Failed(upstream()));
    }

    /// Article d'un code, d'identifiant propre à `n`.
    fn numbered(n: u32, num: &str) -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(&format!("LEGIARTI{n:012}")).unwrap(),
            ..article(Some(num))
        }
    }

    fn find_civil(num: &str) -> PortRequest {
        PortRequest::FindArticle(
            LegiTextId::parse(CIVIL).unwrap(),
            num.to_owned(),
            ArticleAt::Today(today()),
        )
    }

    /// L2-07 (verify_citations-9, gap3-7) : les deux bornes d'une plage sont lues ; la plage est
    /// vérifiée de confiance moyenne, avec la note qui dit que le milieu n'est pas lu.
    #[tokio::test]
    async fn a_range_resolves_both_ends_and_says_so() {
        let corpus = FakeCorpus::new();
        corpus.push_find_article(Ok(numbered(1, "1240")));
        corpus.push_find_article(Ok(numbered(2, "1244")));
        let resolution = resolve(&corpus, "articles 1240 à 1244 du code civil").await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert_eq!(confidence_of(&resolution.outcome), Some(Confidence::Medium));
        assert_eq!(resolution.outcome.note(Proof::Production), Some(Note::RangeEndpointsOnly));
        assert_eq!(record_id(&resolution).as_deref(), Some("LEGIARTI000000000001"));
        assert_eq!(resolution.official, None, "pas de comparaison d'extrait sur une plage");
        assert_eq!(corpus.seen_requests(), vec![find_civil("1240"), find_civil("1244")]);
    }

    /// Une plage lue avec le code par défaut d'un sigle ambigu : les deux bornes sont lues, la note
    /// de lecture du sigle prime.
    #[tokio::test]
    async fn a_range_with_an_ambiguous_sigle_keeps_the_sigle_reading_note() {
        let corpus = FakeCorpus::new();
        corpus.push_find_article(Ok(numbered(1, "121-3")));
        corpus.push_find_article(Ok(numbered(2, "121-5")));
        let resolution = resolve(&corpus, "articles 121-3 à 121-5 CP").await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert_eq!(note(&resolution), "Read 'CP' as Code pénal.");
        assert_eq!(corpus.calls_of("find_article"), 2);
    }

    #[tokio::test]
    async fn a_range_whose_last_article_does_not_exist_is_not_verified() {
        for proof in [Proof::Production, Proof::Sandbox, Proof::Snapshot] {
            let corpus = FakeCorpus::new();
            corpus.push_find_article(Ok(numbered(1, "1240")));
            corpus.push_find_article(Err(not_found()));
            let resolution = resolve(&corpus, "articles 1240 à 9999 du code civil").await;
            assert_eq!(
                resolution.outcome,
                ambiguous_outcome(Note::RangeEndNotFound { num: "9999".to_owned() })
            );
            assert_eq!(resolution.outcome.status(proof), CheckStatus::Ambiguous, "{proof:?}");
            assert_eq!(resolution.official, None);
            assert_eq!(corpus.seen_requests(), vec![find_civil("1240"), find_civil("9999")]);
        }
    }

    #[tokio::test]
    async fn a_range_whose_first_article_is_missing_makes_a_single_call() {
        let corpus = FakeCorpus::new();
        corpus.push_find_article(Err(not_found()));
        let resolution = resolve(&corpus, "articles 9998 à 9999 du code civil").await;
        assert_eq!(status(&resolution), CheckStatus::NotFound);
        assert_eq!(corpus.seen_requests(), vec![find_civil("9998")]);

        let corpus = FakeCorpus::new();
        corpus.push_find_article(Err(upstream()));
        let resolution = resolve(&corpus, "articles 1240 à 1244 du code civil").await;
        assert_eq!(resolution.outcome, Outcome::Failed(upstream()));
        assert_eq!(corpus.calls_of("find_article"), 1);
    }

    /// La seconde borne garde son issue quand ce n'est pas une absence : écart de numéro, échec
    /// amont.
    #[tokio::test]
    async fn a_range_end_that_is_not_absent_keeps_its_own_outcome() {
        let corpus = FakeCorpus::new();
        corpus.push_find_article(Ok(numbered(1, "1240")));
        corpus.push_find_article(Ok(numbered(2, "1245")));
        let resolution = resolve(&corpus, "articles 1240 à 1244 du code civil").await;
        assert_eq!(status(&resolution), CheckStatus::Invalid);
        assert_eq!(
            note(&resolution),
            "Cited article number 1244 differs from the official record (1245)."
        );
        assert_eq!(resolution.official, None);

        let corpus = FakeCorpus::new();
        corpus.push_find_article(Ok(numbered(1, "1240")));
        corpus.push_find_article(Err(upstream()));
        let resolution = resolve(&corpus, "articles 1240 à 1244 du code civil").await;
        assert_eq!(resolution.outcome, Outcome::Failed(upstream()));
    }

    /// Jumeau de « plage » : un article seul garde la confiance haute et son extrait ; une liste
    /// (« 1240 et 1241 ») n'est pas une plage.
    #[tokio::test]
    async fn a_single_article_and_a_list_are_not_ranges() {
        let corpus = FakeCorpus::new();
        corpus.push_find_article(Ok(numbered(1, "1240")));
        let resolution = resolve(&corpus, "article 1240 du code civil").await;
        assert_eq!(confidence_of(&resolution.outcome), Some(Confidence::High));
        assert_eq!(resolution.official.as_deref(), Some("Texte officiel."));
        assert_eq!(corpus.calls_of("find_article"), 1);

        let items = parse("articles 1240 et 1241 du code civil");
        assert_eq!(items.len(), 2);
        for (item, num) in items.iter().zip(["1240", "1241"]) {
            let corpus = FakeCorpus::new();
            corpus.push_find_article(Ok(numbered(1, num)));
            let resolution = resolve_with(&corpus, &codes(), item, None).await;
            assert_eq!(confidence_of(&resolution.outcome), Some(Confidence::High));
            assert_eq!(corpus.calls_of("find_article"), 1);
        }
    }

    /// Plage d'un texte non codifié : le texte est cherché une fois, les deux articles dans ce
    /// texte.
    #[tokio::test]
    async fn a_range_of_a_numbered_text_resolves_the_text_once() {
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![hit(Fond::Loda, "LEGITEXT000033202746")]));
        corpus.push_find_article(Ok(numbered(1, "3")));
        corpus.push_find_article(Ok(numbered(2, "5")));
        let resolution = resolve(&corpus, "articles 3 à 5 de la loi n° 2016-1321").await;
        assert_eq!(status(&resolution), CheckStatus::Verified);
        assert_eq!(confidence_of(&resolution.outcome), Some(Confidence::Medium));
        assert_eq!(resolution.outcome.note(Proof::Production), Some(Note::RangeEndpointsOnly));
        assert_eq!(corpus.calls_of("find_by_number"), 1);
        assert_eq!(corpus.calls_of("find_article"), 2);

        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![hit(Fond::Loda, "LEGITEXT000033202746")]));
        corpus.push_find_article(Ok(numbered(1, "3")));
        corpus.push_find_article(Err(not_found()));
        let resolution = resolve(&corpus, "articles 3 à 99 de la loi n° 2016-1321").await;
        assert_eq!(status(&resolution), CheckStatus::Ambiguous);
        assert_eq!(
            note(&resolution),
            "The first article of the range exists, but its last, article 99, was not found; \
             check the number or the date."
        );
    }

    #[test]
    fn an_article_is_read_from_its_own_family_and_dated_only_for_the_current_version() {
        let kali = ArticleText {
            article_id: ArticleId::parse("KALIARTI000005805295").unwrap(),
            ..article(Some("1"))
        };
        let variants = vec!["1er".to_owned(), "1".to_owned()];
        let current = article_found(&kali, &variants, ArticleAt::Today(today()));
        assert_eq!(record_id(&current).as_deref(), Some("KALIARTI000005805295"));
        assert_eq!(
            current.outcome.note(Proof::Production),
            Some(Note::VersionSince(civil(2016, 10, 1)))
        );
        assert_eq!(current.official.as_deref(), Some("Texte officiel."));
        let past = article_found(&kali, &variants, ArticleAt::On(today()));
        assert_eq!(past.outcome.note(Proof::Production), None);

        let legi = article_found(
            &article(Some("L. 121-3")),
            &["L121-3".to_owned()],
            ArticleAt::On(today()),
        );
        assert_eq!(record_id(&legi).as_deref(), Some("LEGIARTI000006419288"));
        assert_eq!(status(&legi), CheckStatus::Verified);
    }

    // ----------------------------------------------------------------------------------------
    // Identifiants Légifrance
    // ----------------------------------------------------------------------------------------

    fn convention_entity() -> Convention {
        Convention {
            text_id: KaliTextId::parse("KALITEXT000005635096").unwrap(),
            body: TextBody::default(),
            idcc: None,
            container_title: None,
            container_id: None,
            bulletin: None,
        }
    }

    fn agreement_entity() -> CompanyAgreement {
        CompanyAgreement {
            agreement_id: AccoTextId::parse("ACCOTEXT000050000001").unwrap(),
            nature: None,
            title: None,
            raison_sociale: None,
            siret: None,
            idcc: None,
            code_ape: None,
            secteur: None,
            themes: vec![],
            syndicats: vec![],
            date_texte: None,
            date_effet: None,
            date_depot: None,
        }
    }

    /// Le document cité par son identifiant est lu par une seule requête, de confiance haute.
    async fn assert_consulted(
        corpus: FakeCorpus,
        item: Spanned<CitationRef>,
        request: PortRequest,
        official: Option<&str>,
    ) {
        let resolution = resolve_with(&corpus, &codes(), &item, None).await;
        assert_eq!(status(&resolution), CheckStatus::Verified, "{request:?}");
        assert_eq!(confidence_of(&resolution.outcome), Some(Confidence::High));
        assert_eq!(resolution.official.as_deref(), official, "{request:?}");
        assert_eq!(corpus.seen_requests(), vec![request]);
    }

    #[tokio::test]
    async fn each_identifier_family_is_read_with_its_own_method_and_text() {
        let check = assert_consulted;

        let corpus = FakeCorpus::new();
        corpus.push_get_article(Ok(article(Some("9"))));
        check(
            corpus,
            id_citation(IdPrefix::LegiArti, 6_419_288, None),
            PortRequest::GetArticle(ArticleId::parse("LEGIARTI000006419288").unwrap()),
            Some("Texte officiel."),
        )
        .await;

        let corpus = FakeCorpus::new();
        corpus.push_get_article(Ok(article(Some("1"))));
        check(
            corpus,
            id_citation(IdPrefix::KaliArti, 5_805_295, None),
            PortRequest::GetArticle(ArticleId::parse("KALIARTI000005805295").unwrap()),
            Some("Texte officiel."),
        )
        .await;

        for (prefix, digits, id) in [
            (IdPrefix::JuriText, 51_743_650, "JURITEXT000051743650"),
            (IdPrefix::ConsText, 52_555_275, "CONSTEXT000052555275"),
            (IdPrefix::CetaText, 51_655_317, "CETATEXT000051655317"),
        ] {
            let corpus = FakeCorpus::new();
            corpus.push_get_decision(Ok(decision(id, civil(2025, 6, 4), None)));
            check(
                corpus,
                id_citation(prefix, digits, None),
                PortRequest::GetDecision(DecisionId::parse(id).unwrap()),
                Some("Texte de la décision."),
            )
            .await;
        }

        let corpus = FakeCorpus::new();
        corpus.push_get_deliberation(Ok(deliberation(civil(2026, 1, 8))));
        check(
            corpus,
            id_citation(IdPrefix::CnilText, 53_352_643, None),
            PortRequest::GetDeliberation(CnilTextId::parse("CNILTEXT000053352643").unwrap()),
            None,
        )
        .await;

        let corpus = FakeCorpus::new();
        corpus.push_get_convention(Ok(convention_entity()));
        check(
            corpus,
            id_citation(IdPrefix::KaliText, 5_635_096, None),
            PortRequest::GetConvention(KaliTextId::parse("KALITEXT000005635096").unwrap()),
            None,
        )
        .await;

        let corpus = FakeCorpus::new();
        corpus.push_get_company_agreement(Ok(agreement_entity()));
        check(
            corpus,
            id_citation(IdPrefix::AccoText, 50_000_001, None),
            PortRequest::GetCompanyAgreement(AccoTextId::parse("ACCOTEXT000050000001").unwrap()),
            None,
        )
        .await;
    }

    #[tokio::test]
    async fn a_failure_reading_any_identifier_family_is_reported() {
        let failed = |corpus: FakeCorpus, prefix, digits| async move {
            let item = id_citation(prefix, digits, None);
            let resolution = resolve_with(&corpus, &codes(), &item, None).await;
            assert_eq!(resolution.outcome, Outcome::Failed(upstream()), "{prefix:?}");
            assert_eq!(corpus.calls(), 1, "{prefix:?}");
        };
        for (prefix, digits) in [(IdPrefix::LegiArti, 6_419_288), (IdPrefix::KaliArti, 5_805_295)] {
            let corpus = FakeCorpus::new();
            corpus.push_get_article(Err(upstream()));
            failed(corpus, prefix, digits).await;
        }
        for (prefix, digits) in [
            (IdPrefix::JuriText, 51_743_650),
            (IdPrefix::ConsText, 52_555_275),
            (IdPrefix::CetaText, 51_655_317),
        ] {
            let corpus = FakeCorpus::new();
            corpus.push_get_decision(Err(upstream()));
            failed(corpus, prefix, digits).await;
        }
        let corpus = FakeCorpus::new();
        corpus.push_get_act(Err(upstream()));
        failed(corpus, IdPrefix::LegiText, 33_205_014).await;
        let corpus = FakeCorpus::new();
        corpus.push_get_deliberation(Err(upstream()));
        failed(corpus, IdPrefix::CnilText, 53_352_643).await;
        let corpus = FakeCorpus::new();
        corpus.push_get_convention(Err(upstream()));
        failed(corpus, IdPrefix::KaliText, 5_635_096).await;
        let corpus = FakeCorpus::new();
        corpus.push_get_company_agreement(Err(upstream()));
        failed(corpus, IdPrefix::AccoText, 50_000_001).await;
    }

    #[tokio::test]
    async fn a_text_number_without_a_year_is_searched_by_its_sequence_alone() {
        let mut cite = text_cite(TextNature::Decret, Some((1993, 1419)), None);
        cite.number.as_mut().unwrap().year = None;
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![]));
        let item = standalone(CitationRef::Text(cite));
        let resolution = resolve_with(&corpus, &codes(), &item, None).await;
        assert_eq!(
            resolution.outcome,
            unresolvable_outcome(Note::CouldNotConfirm { index: "LODA" })
        );
        assert_eq!(
            corpus.seen_requests(),
            vec![find_request(Fond::Loda, NumberKind::Text, "1419")]
        );
    }

    #[tokio::test]
    async fn an_identifier_that_cannot_be_read_is_absent_or_failed() {
        let item = id_citation(IdPrefix::JuriText, 51_743_650, None);
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Err(not_found()));
        let resolution = resolve_with(&corpus, &codes(), &item, None).await;
        assert_eq!(status(&resolution), CheckStatus::NotFound);
        assert_eq!(
            resolution.outcome.note(Proof::Production),
            Some(Note::NoRecord {
                family: "identifier",
                value: "JURITEXT000051743650".to_owned(),
                date: None
            })
        );
        assert_eq!(
            resolution.outcome.note(Proof::Sandbox),
            Some(Note::NotInSandbox {
                family: "identifier",
                value: "JURITEXT000051743650".to_owned(),
                date: None
            })
        );
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Err(upstream()));
        let resolution = resolve_with(&corpus, &codes(), &item, None).await;
        assert_eq!(resolution.outcome, Outcome::Failed(upstream()));
    }
}
