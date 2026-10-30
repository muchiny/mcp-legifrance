//! Cas d'usage de `fr_diff_article` (v0.3, M7) : la comparaison de deux versions d'un article
//! (`DiffArticleUseCase`, 05 §5.12).
//!
//! # Étapes
//!
//! 1. Validation sans réseau : `article_id` (un `LEGIARTI`), `date_a` (requise), puis `date_b`
//!    (aujourd'hui à Paris par défaut, jamais la date UTC, D-19).
//! 2. Un appel `article_versions` : la version en vigueur à chaque date. Une version couvre
//!    `date_debut..=date_fin` (fin incluse, absente = sans date de fin) ; une version sans
//!    `date_debut` n'est en vigueur à aucune date (sentinelle `2222-02-22` des entrées en vigueur
//!    différée, V-101). Si plusieurs couvrent la date, la plus récente l'emporte. Aucune version :
//!    `not_found` « No version of {id} is in force on {date}. ».
//! 3. Les deux dates dans la même version : `identical`, sans hunk et avec un seul `get_article`
//!    (la citation). Sinon les deux textes se lisent ensemble.
//! 4. Diff mot à mot ([`similar`], Myers) : un hunk par suite contiguë de mots changés, avec sa
//!    position dans A (en caractères) et jusqu'à 60 caractères de contexte avant.
//!
//! # Plafond de réponse (05 §6.4)
//!
//! Chaque passage retiré ou ajouté est coupé à 3 900 caractères ; le texte de tous les hunks tient
//! sous 8 000 caractères et l'estimation de la réponse sous la cible ([`estimate_chars`]), en
//! retirant des hunks par la fin ([`shrink_from_end`]) ; `truncated` le dit. Un diff qui dépasse
//! deux secondes (textes démesurés) s'arrête en approximation plutôt que de bloquer le processus.

use std::time::{Duration, Instant};

use futures::future::join;
use lf_domain::{
    contract::Citable,
    date::{VersionDate, open_ended},
    entity::{ArticleText, ArticleVersion},
    id::{ArticleId, LegiArtiId},
};
use similar::{Algorithm, DiffTag, capture_diff_slices_deadline};

use crate::{
    budget::{ResponseBudget, estimate_chars, shrink_from_end},
    error::AppError,
    failure::port_failure,
    input::DiffArticleInput,
    messages,
    output::diff_article::{DiffArticleOutput, HunkOut, VersionOut},
    ports::{Clock, CorpusError, LegalCorpus},
    tools::ToolName,
};

/// Outil que ce cas d'usage sert : nom des messages et de l'audit.
const TOOL: ToolName = ToolName::FR_DIFF_ARTICLE;

/// Longueur maximale du contexte d'un hunk, `… ` compris (05 §5.12).
const CONTEXT_MAX_CHARS: usize = 60;

/// Marque d'un contexte dont le début manque : `…` et une espace.
const ELLIPSIS: &str = "… ";

/// Plafond du texte de tous les hunks (contexte, retiré, ajouté), en caractères (05 §5.12).
const HUNK_TEXT_CAP: usize = 8_000;

/// Plafond d'un passage retiré ou ajouté, `…` non compris. Deux passages et un contexte tiennent
/// ainsi toujours sous [`HUNK_TEXT_CAP`] : un seul hunk ne vide jamais la réponse.
const FIELD_CLIP_CHARS: usize = 3_900;

/// Nombre de hunks au-delà duquel les suivants sont écartés sans calcul : chacun coûte au moins
/// deux champs estimés, donc plus de 330 ne tiendraient de toute façon pas sous la cible.
const MAX_HUNKS: usize = 400;

/// Durée au-delà de laquelle le diff s'arrête en approximation (textes démesurés).
const DIFF_BUDGET: Duration = Duration::from_secs(2);

/// Champs estimés de l'enveloppe : `article_id`, `citation`, deux versions de quatre champs,
/// `identical`, `hunks`, `truncated`.
const ENVELOPE_FIELDS: usize = 16;

/// Caractères estimés de l'enveloppe hors citation : l'identifiant, et pour chaque version son
/// identifiant, deux dates et un code d'état.
const ENVELOPE_TEXT_CHARS: usize = 20 + 2 * (20 + 2 * 10 + 16);

const _: () = assert!(CONTEXT_MAX_CHARS + 2 * (FIELD_CLIP_CHARS + 1) <= HUNK_TEXT_CAP);

/// Comparaison de deux versions d'un article : analyse des arguments, deux appels au port au
/// plus, diff et politique de réponse.
pub(crate) struct DiffArticleUseCase<'a, C: LegalCorpus, K: Clock> {
    corpus: &'a C,
    clock: &'a K,
}

/// Arguments validés.
struct Request {
    id: LegiArtiId,
    date_a: VersionDate,
    date_b: VersionDate,
}

impl<'a, C: LegalCorpus, K: Clock> DiffArticleUseCase<'a, C, K> {
    pub(crate) fn new(corpus: &'a C, clock: &'a K) -> Self {
        Self { corpus, clock }
    }

    /// Valide `input`, lit les versions puis les textes, et compare.
    ///
    /// # Errors
    ///
    /// `invalid_arg` pour tout argument refusé (sans appel au port), `not_found` quand aucune
    /// version n'est en vigueur à l'une des dates ; sinon l'erreur du port, traduite par
    /// [`port_failure`].
    pub(crate) async fn execute(
        &self,
        input: DiffArticleInput,
    ) -> Result<DiffArticleOutput, AppError> {
        let Request { id, date_a, date_b } = self.request_of(input)?;
        let versions = self
            .corpus
            .article_versions(&ArticleId::Legi(id.clone()))
            .await
            .map_err(|error| port_error(&id, date_a, &error))?;
        let version_a = in_force(&versions, date_a).ok_or_else(|| no_version(&id, date_a))?;
        let version_b = in_force(&versions, date_b).ok_or_else(|| no_version(&id, date_b))?;
        let identical = version_a.id == version_b.id;
        let (target, hunks) = if identical {
            (self.read(&id, version_b, date_b).await?, Vec::new())
        } else {
            let (before, after) =
                join(self.read(&id, version_a, date_a), self.read(&id, version_b, date_b)).await;
            let (before, after) = (before?, after?);
            let hunks = word_hunks(before.text.as_str(), after.text.as_str(), deadline());
            (after, hunks)
        };
        let citation = target.contract().citation.as_str().to_owned();
        let (hunks, truncated) = fit(
            hunks,
            estimate_chars(ENVELOPE_FIELDS, ENVELOPE_TEXT_CHARS.saturating_add(chars(&citation))),
        );
        Ok(DiffArticleOutput {
            article_id: id,
            citation,
            version_a: version_out(version_a),
            version_b: version_out(version_b),
            identical,
            hunks,
            truncated,
        })
    }

    /// Arguments validés, dans l'ordre : identifiant, `date_a`, `date_b`.
    fn request_of(&self, input: DiffArticleInput) -> Result<Request, AppError> {
        let DiffArticleInput { article_id, date_a, date_b } = input;
        let id = article_id.as_deref().ok_or_else(|| messages::required("article_id"))?;
        let id = LegiArtiId::parse(id).map_err(|error| messages::domain(TOOL, &error))?;
        let date_a = date_a.as_deref().ok_or_else(|| messages::required("date_a"))?;
        let date_a = date_of("date_a", date_a)?;
        let date_b =
            date_b.as_deref().map_or_else(|| Ok(self.today()), |raw| date_of("date_b", raw))?;
        Ok(Request { id, date_a, date_b })
    }

    /// Jour civil Europe/Paris de l'horloge. Une horloge au-delà de la plage des dates de
    /// version (année 2222 ou après) est ramenée à sa borne plutôt que refusée.
    fn today(&self) -> VersionDate {
        VersionDate::try_from(self.clock.today_paris()).unwrap_or(VersionDate::MAX)
    }

    /// Texte de la version `version`, ou l'erreur du port rendue pour `requested` à `date`.
    async fn read(
        &self,
        requested: &LegiArtiId,
        version: &ArticleVersion,
        date: VersionDate,
    ) -> Result<ArticleText, AppError> {
        self.corpus
            .get_article(&ArticleId::Legi(version.id.clone()))
            .await
            .map_err(|error| port_error(requested, date, &error))
    }
}

/// Date `AAAA-MM-JJ` de l'argument `name` (D-22), sans rognage.
fn date_of(name: &'static str, raw: &str) -> Result<VersionDate, AppError> {
    VersionDate::parse(raw).map_err(|error| messages::domain_arg(TOOL, Some(name), &error))
}

/// Erreur du port. Un `NotFound` dit qu'aucune version n'est en vigueur à `date` : le message de
/// `messages::corpus_error` n'a pas de ligne pour cet outil (05 §7.4), la phrase est donc écrite
/// ici ; l'erreur n'en est pas moins journalisée par [`port_failure`].
fn port_error(requested: &LegiArtiId, date: VersionDate, error: &CorpusError) -> AppError {
    let failure = port_failure(TOOL, requested.as_str(), error);
    match error {
        CorpusError::NotFound { .. } => no_version(requested, date),
        _ => failure,
    }
}

/// `not_found` : aucune version de l'article n'est en vigueur à `date` (05 §7.4).
fn no_version(id: &LegiArtiId, date: VersionDate) -> AppError {
    AppError::not_found(format!("No version of {id} is in force on {date}."))
}

/// La version en vigueur à `date` : celle dont `date_debut..=date_fin` la contient, la plus
/// récente si plusieurs la contiennent. Une version sans `date_debut` ne contient aucune date.
fn in_force(versions: &[ArticleVersion], date: VersionDate) -> Option<&ArticleVersion> {
    let day = date.as_civil();
    versions
        .iter()
        .filter(|version| {
            open_ended(version.date_debut).is_some_and(|start| start <= day)
                && open_ended(version.date_fin).is_none_or(|end| day <= end)
        })
        .max_by_key(|version| version.date_debut)
}

/// La version telle que la réponse la décrit.
fn version_out(version: &ArticleVersion) -> VersionOut {
    VersionOut {
        article_id: version.id.clone(),
        date_debut: version.date_debut,
        date_fin: version.date_fin,
        etat: version.etat.clone(),
    }
}

/// Échéance du diff : [`DIFF_BUDGET`] à partir de maintenant.
fn deadline() -> Option<Instant> {
    Instant::now().checked_add(DIFF_BUDGET)
}

// -------------------------------------------------------------------------------------------
// Diff
// -------------------------------------------------------------------------------------------

/// Un texte découpé en mots (suites de non-blancs), avec la position de chacun.
struct WordIndex<'a> {
    /// Les mots, dans l'ordre.
    words: Vec<&'a str>,
    /// Position du premier caractère de chaque mot, en caractères (points de code).
    starts: Vec<usize>,
    /// Longueur du texte, en caractères.
    chars: usize,
}

impl<'a> WordIndex<'a> {
    fn of(text: &'a str) -> Self {
        let mut words = Vec::new();
        let mut starts = Vec::new();
        // Le mot en cours : son premier octet et son premier caractère.
        let mut open: Option<(usize, usize)> = None;
        let mut chars = 0;
        for (byte, c) in text.char_indices() {
            match (c.is_whitespace(), open) {
                (true, Some((from, start))) => {
                    words.push(&text[from..byte]);
                    starts.push(start);
                    open = None;
                }
                (false, None) => open = Some((byte, chars)),
                _ => {}
            }
            chars = chars.saturating_add(1);
        }
        if let Some((from, start)) = open {
            words.push(&text[from..]);
            starts.push(start);
        }
        Self { words, starts, chars }
    }

    /// Position, en caractères, du mot `index` ; la fin du texte pour l'indice qui suit le dernier.
    fn position(&self, index: usize) -> usize {
        self.starts.get(index).copied().unwrap_or(self.chars)
    }
}

/// Les changements de `old` vers `new`, mot à mot, au plus [`MAX_HUNKS`] + 1 : le surplus sert
/// seulement à savoir qu'il y en avait davantage.
///
/// Un hunk est une suite contiguë de mots retirés et/ou ajoutés ; deux changements séparés par
/// au moins un mot commun sont deux hunks. Les blancs sont ramenés à une espace dans les passages
/// rendus.
#[expect(
    clippy::indexing_slicing,
    reason = "les plages viennent du diff de ces mêmes mots ; `count` est au plus la longueur de `before`"
)]
fn word_hunks(old: &str, new: &str, deadline: Option<Instant>) -> Vec<HunkOut> {
    let before = WordIndex::of(old);
    let after = WordIndex::of(new);
    capture_diff_slices_deadline(Algorithm::Myers, &before.words, &after.words, deadline)
        .iter()
        .map(similar::DiffOp::as_tag_tuple)
        .filter(|(tag, ..)| *tag != DiffTag::Equal)
        .take(MAX_HUNKS + 1)
        .map(|(_, old_range, new_range)| HunkOut {
            at: u32::try_from(before.position(old_range.start)).unwrap_or(u32::MAX),
            context: context_before(&before.words, old_range.start),
            removed: passage(&before.words[old_range]),
            added: passage(&after.words[new_range]),
        })
        .collect()
}

/// Les mots joints par une espace ; `None` s'il n'y en a pas.
fn passage(words: &[&str]) -> Option<String> {
    (!words.is_empty()).then(|| words.join(" "))
}

/// Au plus [`CONTEXT_MAX_CHARS`] caractères des mots qui précèdent `upto`, joints par une espace
/// et coupés à une limite de mot. Précédés de `… ` quand le début manque ; si le dernier mot
/// seul est trop long, ses derniers caractères suivent un `…`.
#[expect(
    clippy::indexing_slicing,
    reason = "les plages viennent du diff de ces mêmes mots ; `count` est au plus la longueur de `before`"
)]
fn context_before(words: &[&str], upto: usize) -> Option<String> {
    let before = &words[..upto];
    let last = before.last()?;
    let mut count = fitting_tail(before, CONTEXT_MAX_CHARS);
    if count < before.len() {
        count = fitting_tail(before, CONTEXT_MAX_CHARS.saturating_sub(ELLIPSIS.chars().count()));
    }
    let tail = &before[before.len().saturating_sub(count)..];
    Some(if tail.is_empty() {
        let kept: Vec<char> = last.chars().rev().take(CONTEXT_MAX_CHARS - 1).collect();
        format!("…{}", kept.into_iter().rev().collect::<String>())
    } else if count < before.len() {
        format!("{ELLIPSIS}{}", tail.join(" "))
    } else {
        tail.join(" ")
    })
}

/// Combien de mots, en partant de la fin, tiennent dans `limit` caractères une fois joints par
/// une espace.
fn fitting_tail(words: &[&str], limit: usize) -> usize {
    let mut used = 0_usize;
    let mut count = 0_usize;
    for word in words.iter().rev() {
        let next = used.saturating_add(chars(word)).saturating_add(usize::from(count > 0));
        if next > limit {
            break;
        }
        used = next;
        count = count.saturating_add(1);
    }
    count
}

// -------------------------------------------------------------------------------------------
// Plafond de réponse
// -------------------------------------------------------------------------------------------

/// Ramène les hunks sous les bornes (05 §5.12, §6.4) : au plus [`MAX_HUNKS`], passages coupés à
/// [`FIELD_CLIP_CHARS`], puis retrait des derniers hunks tant que leur texte dépasse
/// [`HUNK_TEXT_CAP`] ou que l'estimation de la réponse (enveloppe `fixed` comprise) dépasse la
/// cible. Rend les hunks conservés et vrai si quelque chose a été coupé ou retiré.
fn fit(mut hunks: Vec<HunkOut>, fixed: usize) -> (Vec<HunkOut>, bool) {
    let mut truncated = hunks.len() > MAX_HUNKS;
    hunks.truncate(MAX_HUNKS);
    for hunk in &mut hunks {
        truncated |= clip(&mut hunk.removed);
        truncated |= clip(&mut hunk.added);
    }
    let over = |all: &[HunkOut]| {
        fixed.saturating_add(all.iter().map(hunk_cost).fold(0, usize::saturating_add))
            > ResponseBudget::ESTIMATE_TARGET_CHARS
            || all.iter().map(hunk_text_chars).sum::<usize>() > HUNK_TEXT_CAP
    };
    truncated |= shrink_from_end(&mut hunks, over) > 0;
    (hunks, truncated)
}

/// Coupe le passage à [`FIELD_CLIP_CHARS`] caractères, suivis de `…` ; vrai s'il a été coupé.
fn clip(field: &mut Option<String>) -> bool {
    let Some(text) = field else { return false };
    let Some((byte, _)) = text.char_indices().nth(FIELD_CLIP_CHARS) else { return false };
    text.truncate(byte);
    text.push('…');
    true
}

/// Caractères de texte d'un hunk : contexte, passage retiré et passage ajouté.
fn hunk_text_chars(hunk: &HunkOut) -> usize {
    [&hunk.context, &hunk.removed, &hunk.added].into_iter().flatten().map(|text| chars(text)).sum()
}

/// Coût estimé d'un hunk : l'élément, `at`, plus un champ par texte présent.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "estimation de taille : sommes de longueurs de textes et de compteurs déjà en mémoire, donc sous `usize::MAX`"
)]
fn hunk_cost(hunk: &HunkOut) -> usize {
    let present =
        [&hunk.context, &hunk.removed, &hunk.added].iter().filter(|text| text.is_some()).count();
    estimate_chars(2 + present, hunk_text_chars(hunk))
}

/// Longueur en caractères (points de code) : l'unité des plafonds.
fn chars(text: &str) -> usize {
    text.chars().count()
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "tests : valeurs bornées par les données du test"
)]
#[cfg(test)]
#[expect(
    clippy::as_conversions,
    reason = "tests : tailles et indices bornés par les données du test"
)]
mod tests {
    use chrono::{DateTime, TimeZone, Utc};
    use lf_domain::{
        date::CivilDate, entity::ParentText, id::DocumentId, status::LegalStatus, text::LongText,
    };
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        error::{ErrorCode, UpstreamCause},
        testing::{FakeCorpus, FixedClock, PortRequest},
    };

    fn day(year: u16, month: u8, daynum: u8) -> CivilDate {
        CivilDate::new(year, month, daynum).unwrap()
    }

    fn clock() -> FixedClock {
        FixedClock::new(DateTime::UNIX_EPOCH, day(2026, 10, 29))
    }

    fn arti(n: u32) -> LegiArtiId {
        LegiArtiId::parse(&format!("LEGIARTI{n:012}")).unwrap()
    }

    fn id_of(n: u32) -> String {
        arti(n).as_str().to_owned()
    }

    type Ymd = Option<(u16, u8, u8)>;

    /// Version `n` en vigueur de `from` à `to` (fin incluse ; `None` : sans borne).
    fn version(n: u32, from: Ymd, to: Ymd) -> ArticleVersion {
        ArticleVersion {
            id: arti(n),
            num: Some("9".to_owned()),
            etat: Some(LegalStatus::Vigueur),
            date_debut: from.map(|(y, m, d)| day(y, m, d)),
            date_fin: to.map(|(y, m, d)| day(y, m, d)),
        }
    }

    /// Deux versions : la 1 jusqu'au 2023-09-07, la 2 depuis le 2023-09-08.
    fn two_versions() -> Vec<ArticleVersion> {
        vec![
            version(1, Some((2016, 10, 9)), Some((2023, 9, 7))),
            version(2, Some((2023, 9, 8)), None),
        ]
    }

    fn article(n: u32, text: &str) -> ArticleText {
        ArticleText {
            article_id: ArticleId::Legi(arti(n)),
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
            text: LongText::from_plain(text).unwrap(),
            nota: None,
        }
    }

    fn input(id: &str, a: &str, b: Option<&str>) -> DiffArticleInput {
        DiffArticleInput {
            article_id: Some(id.to_owned()),
            date_a: Some(a.to_owned()),
            date_b: b.map(str::to_owned),
        }
    }

    async fn run(
        corpus: &FakeCorpus,
        clock: &FixedClock,
        input: DiffArticleInput,
    ) -> Result<DiffArticleOutput, AppError> {
        DiffArticleUseCase::new(corpus, clock).execute(input).await
    }

    /// Un corpus qui sert `versions`, puis les textes de A et de B.
    fn serving(versions: Vec<ArticleVersion>, a: &str, b: &str) -> FakeCorpus {
        let corpus = FakeCorpus::new();
        corpus.push_article_versions(Ok(versions));
        corpus.push_get_article(Ok(article(1, a)));
        corpus.push_get_article(Ok(article(2, b)));
        corpus
    }

    /// Message d'un `invalid_arg`, après avoir vérifié qu'aucune requête n'est partie.
    async fn rejected(input: DiffArticleInput) -> String {
        let corpus = FakeCorpus::new();
        let error = run(&corpus, &clock(), input).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(corpus.calls(), 0, "un rejet local n'appelle pas le port");
        error.message().to_owned()
    }

    /// Position en caractères de la première occurrence de `needle` dans `text`.
    fn char_at(text: &str, needle: &str) -> u32 {
        let byte = text.find(needle).unwrap();
        u32::try_from(text[..byte].chars().count()).unwrap()
    }

    fn words_of(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    /// Refait B à partir de A et des hunks : l'oracle du diff, indépendant de son calcul.
    fn applied(old: &str, hunks: &[HunkOut]) -> Vec<String> {
        let words = WordIndex::of(old);
        let mut rebuilt: Vec<String> = Vec::new();
        let mut next = 0;
        for hunk in hunks {
            let index = (next..=words.words.len())
                .find(|&index| words.position(index) == hunk.at as usize)
                .unwrap();
            rebuilt.extend(words.words[next..index].iter().map(|word| (*word).to_owned()));
            let removed = hunk.removed.as_deref().map_or(0, |text| text.split(' ').count());
            next = index + removed;
            rebuilt.extend(hunk.added.iter().flat_map(|text| text.split(' ')).map(str::to_owned));
        }
        rebuilt.extend(words.words[next..].iter().map(|word| (*word).to_owned()));
        rebuilt
    }

    const OLD: &str = "Le salarié peut être licencié pour faute grave. La sanction est notifiée \
                       par écrit dans un délai de un mois. Aucun autre motif n'est admis.";
    const NEW: &str = "Le salarié peut être licencié pour faute lourde. La sanction est notifiée \
                       par écrit dans un délai de deux mois. Aucun autre motif n'est admis.";

    #[tokio::test]
    async fn diff_is_compact_hunks() {
        let corpus = serving(two_versions(), OLD, NEW);

        let output = run(&corpus, &clock(), input(&id_of(2), "2020-01-01", Some("2026-01-01")))
            .await
            .unwrap();

        // Deux changements, deux hunks : ni le reste des phrases ni l'article entier.
        assert_eq!(
            output.hunks,
            [
                HunkOut {
                    at: char_at(OLD, "grave."),
                    context: Some("Le salarié peut être licencié pour faute".to_owned()),
                    removed: Some("grave.".to_owned()),
                    added: Some("lourde.".to_owned()),
                },
                HunkOut {
                    at: char_at(OLD, "un mois"),
                    // 60 caractères au plus, `… ` compris, coupés à une limite de mot.
                    context: Some(
                        "… grave. La sanction est notifiée par écrit dans un délai de".to_owned()
                    ),
                    removed: Some("un".to_owned()),
                    added: Some("deux".to_owned()),
                },
            ]
        );
        assert_eq!(output.hunks[0].at, 41);
        assert_eq!(output.hunks[1].context.as_deref().unwrap().chars().count(), 60);
        assert!(!output.identical);
        assert!(!output.truncated);
        assert_eq!(output.article_id, arti(2));
        assert_eq!(output.citation, "Code civil, art. 9");
        assert_eq!(output.version_a.article_id, arti(1));
        assert_eq!(output.version_a.date_debut, Some(day(2016, 10, 9)));
        assert_eq!(output.version_a.date_fin, Some(day(2023, 9, 7)));
        assert_eq!(output.version_a.etat, Some(LegalStatus::Vigueur));
        assert_eq!(output.version_b.article_id, arti(2));
        assert_eq!(output.version_b.date_fin, None);
        // Les versions d'abord, puis les textes de A et de B.
        assert_eq!(
            corpus.seen_requests(),
            [
                PortRequest::ArticleVersions(ArticleId::Legi(arti(2))),
                PortRequest::GetArticle(ArticleId::Legi(arti(1))),
                PortRequest::GetArticle(ArticleId::Legi(arti(2))),
            ]
        );
    }

    #[tokio::test]
    async fn diff_dates_default_today_paris() {
        // 23 h 30 UTC le 28 octobre : il est déjà le 29 à Paris. La version 1 s'est terminée la
        // veille, la 2 commence ce jour-là : seule la date de Paris désigne la 2.
        let versions = vec![
            version(1, Some((2020, 1, 1)), Some((2026, 10, 28))),
            version(2, Some((2026, 10, 29)), None),
        ];
        let utc_evening = Utc.with_ymd_and_hms(2026, 10, 28, 23, 30, 0).unwrap();
        let clock = FixedClock::new(utc_evening, day(2026, 10, 29));
        let corpus = serving(versions, "avant", "après");

        let output = run(&corpus, &clock, input(&id_of(1), "2025-01-01", None)).await.unwrap();

        assert_eq!(output.version_a.article_id, arti(1));
        assert_eq!(output.version_b.article_id, arti(2), "date_b vaut le jour de Paris");
        assert!(!output.identical);
        assert_eq!(
            output.hunks,
            [HunkOut {
                at: 0,
                context: None,
                removed: Some("avant".to_owned()),
                added: Some("après".to_owned())
            }]
        );
    }

    #[tokio::test]
    async fn a_clock_past_the_version_range_is_clamped_to_the_last_valid_day() {
        // Une horloge en 2222 ne rejette pas la requête : la date par défaut est ramenée à la borne.
        let far = FixedClock::new(DateTime::UNIX_EPOCH, day(2222, 5, 1));
        let corpus = FakeCorpus::new();
        corpus.push_article_versions(Ok(vec![version(1, Some((2000, 1, 1)), None)]));
        corpus.push_get_article(Ok(article(1, "texte")));

        let output = run(&corpus, &far, input(&id_of(1), "2020-01-01", None)).await.unwrap();

        assert!(output.identical);
    }

    #[tokio::test]
    async fn dates_in_the_same_version_are_identical_and_read_one_text() {
        let corpus = FakeCorpus::new();
        corpus.push_article_versions(Ok(two_versions()));
        corpus.push_get_article(Ok(article(2, "Le texte en vigueur.")));

        let output = run(&corpus, &clock(), input(&id_of(1), "2024-01-01", Some("2026-01-01")))
            .await
            .unwrap();

        assert!(output.identical);
        assert_eq!(output.hunks, Vec::<HunkOut>::new());
        assert!(!output.truncated);
        assert_eq!(output.version_a, output.version_b);
        assert_eq!(output.citation, "Code civil, art. 9");
        assert_eq!(output.article_id, arti(1), "l'identifiant demandé, quelle que soit sa version");
        assert_eq!(
            corpus.seen_requests(),
            [
                PortRequest::ArticleVersions(ArticleId::Legi(arti(1))),
                PortRequest::GetArticle(ArticleId::Legi(arti(2))),
            ]
        );
    }

    #[tokio::test]
    async fn the_versions_follow_the_dates_even_when_a_is_after_b() {
        let corpus = serving(two_versions(), "ancien texte", "nouveau texte");
        let output = run(&corpus, &clock(), input(&id_of(2), "2026-01-01", Some("2020-01-01")))
            .await
            .unwrap();
        // A est la version de `date_a` : la 2. Son texte est lu en premier.
        assert_eq!(output.version_a.article_id, arti(2));
        assert_eq!(output.version_b.article_id, arti(1));
        assert_eq!(
            corpus.seen_requests()[1..],
            [
                PortRequest::GetArticle(ArticleId::Legi(arti(2))),
                PortRequest::GetArticle(ArticleId::Legi(arti(1))),
            ]
        );
    }

    #[tokio::test]
    async fn a_date_before_the_first_version_is_not_found() {
        for (a, b, wrong) in
            [("2010-05-17", "2026-01-01", "2010-05-17"), ("2026-01-01", "2010-05-17", "2010-05-17")]
        {
            let corpus = FakeCorpus::new();
            corpus.push_article_versions(Ok(two_versions()));
            let error = run(&corpus, &clock(), input(&id_of(1), a, Some(b))).await.unwrap_err();
            assert_eq!(error.code(), Some(ErrorCode::NotFound));
            assert_eq!(
                error.message(),
                format!("No version of {} is in force on {wrong}.", id_of(1))
            );
            assert_eq!(corpus.calls(), 1, "aucun texte n'est lu");
        }
    }

    #[tokio::test]
    async fn an_article_without_versions_is_not_found_at_date_a() {
        let corpus = FakeCorpus::new();
        corpus.push_article_versions(Ok(vec![]));
        let error = run(&corpus, &clock(), input(&id_of(1), "2020-01-01", None)).await.unwrap_err();
        assert_eq!(
            error.message(),
            format!("No version of {} is in force on 2020-01-01.", id_of(1))
        );
    }

    #[tokio::test]
    async fn validation_follows_the_argument_order_and_never_calls_the_port() {
        let mut bad = DiffArticleInput {
            article_id: None,
            date_a: Some("nope".to_owned()),
            date_b: Some("2016-13-45".to_owned()),
        };
        assert_eq!(rejected(bad.clone()).await, "article_id is required.");
        bad.article_id = Some("LEGITEXT000006070721".to_owned());
        let message = rejected(bad.clone()).await;
        assert!(message.starts_with("article_id='LEGITEXT000006070721'"), "{message}");
        assert!(message.contains("fr_get_act"), "{message}");
        bad.article_id = Some(id_of(1));
        bad.date_a = None;
        assert_eq!(rejected(bad.clone()).await, "date_a is required.");
        bad.date_a = Some("nope".to_owned());
        assert_eq!(
            rejected(bad.clone()).await,
            "date_a='nope' is not a valid date (expected YYYY-MM-DD, e.g. 2026-10-15)."
        );
        bad.date_a = Some("2020-01-01".to_owned());
        assert_eq!(
            rejected(bad).await,
            "date_b='2016-13-45' is not a valid date (expected YYYY-MM-DD, e.g. 2026-10-15)."
        );
    }

    #[tokio::test]
    async fn port_failures_are_translated_and_a_missing_article_is_not_found() {
        // Le port ne trouve pas l'article : aucune version n'est en vigueur.
        let corpus = FakeCorpus::new();
        corpus.push_article_versions(Err(CorpusError::NotFound { what: "x".to_owned() }));
        let error = run(&corpus, &clock(), input(&id_of(1), "2020-01-01", None)).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::NotFound));
        assert_eq!(
            error.message(),
            format!("No version of {} is in force on 2020-01-01.", id_of(1))
        );

        // Une panne amont garde son code.
        let corpus = FakeCorpus::new();
        corpus.push_article_versions(Err(CorpusError::Timeout));
        let error = run(&corpus, &clock(), input(&id_of(1), "2020-01-01", None)).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::UpstreamError));
        assert_eq!(error.cause(), Some(UpstreamCause::Timeout));
    }

    #[tokio::test]
    async fn a_version_text_that_cannot_be_read_fails_the_call() {
        let a_missing = FakeCorpus::new();
        a_missing.push_article_versions(Ok(two_versions()));
        a_missing.push_get_article(Err(CorpusError::NotFound { what: "x".to_owned() }));
        a_missing.push_get_article(Ok(article(2, "texte")));
        let error = run(&a_missing, &clock(), input(&id_of(1), "2020-01-01", Some("2026-01-01")))
            .await
            .unwrap_err();
        assert_eq!(
            error.message(),
            format!("No version of {} is in force on 2020-01-01.", id_of(1))
        );

        let b_down = FakeCorpus::new();
        b_down.push_article_versions(Ok(two_versions()));
        b_down.push_get_article(Ok(article(1, "texte")));
        b_down.push_get_article(Err(CorpusError::Timeout));
        let error = run(&b_down, &clock(), input(&id_of(1), "2020-01-01", Some("2026-01-01")))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::UpstreamError));
    }

    // ---------------------------------------------------------------------------------------
    // Version en vigueur
    // ---------------------------------------------------------------------------------------

    #[tokio::test]
    async fn an_identical_pair_whose_text_cannot_be_read_fails_the_call() {
        let corpus = FakeCorpus::new();
        corpus.push_article_versions(Ok(two_versions()));
        corpus.push_get_article(Err(CorpusError::Timeout));
        let error = run(&corpus, &clock(), input(&id_of(1), "2024-01-01", Some("2026-01-01")))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::UpstreamError));
    }

    fn date(raw: &str) -> VersionDate {
        VersionDate::parse(raw).unwrap()
    }

    fn id_on(versions: &[ArticleVersion], raw: &str) -> Option<LegiArtiId> {
        in_force(versions, date(raw)).map(|version| version.id.clone())
    }

    #[test]
    fn a_version_covers_its_first_and_last_day() {
        let versions = two_versions();
        assert_eq!(id_on(&versions, "2016-10-08"), None, "la veille du début");
        assert_eq!(id_on(&versions, "2016-10-09"), Some(arti(1)));
        assert_eq!(id_on(&versions, "2023-09-07"), Some(arti(1)), "la fin est incluse");
        assert_eq!(id_on(&versions, "2023-09-08"), Some(arti(2)));
        assert_eq!(id_on(&versions, "2221-12-31"), Some(arti(2)), "sans date de fin");
    }

    #[test]
    fn a_version_without_a_start_date_is_in_force_at_no_date() {
        // Entrée en vigueur différée (V-101) : la sentinelle de début est devenue `None`.
        let versions =
            vec![version(1, None, None), version(2, Some((2000, 1, 1)), Some((2010, 1, 1)))];
        assert_eq!(id_on(&versions, "2005-01-01"), Some(arti(2)));
        assert_eq!(id_on(&versions, "2020-01-01"), None);
    }

    #[test]
    fn the_latest_start_wins_and_a_sentinel_end_means_no_end() {
        let versions = vec![
            // Fin dans la zone sentinelle : sans date de fin.
            version(1, Some((2000, 1, 1)), Some((2999, 1, 1))),
            version(2, Some((2020, 1, 1)), None),
            version(3, Some((2010, 1, 1)), None),
        ];
        assert_eq!(id_on(&versions, "2015-01-01"), Some(arti(3)));
        assert_eq!(id_on(&versions, "2025-01-01"), Some(arti(2)));
        assert_eq!(id_on(&versions, "2005-01-01"), Some(arti(1)));
    }

    // ---------------------------------------------------------------------------------------
    // Hunks
    // ---------------------------------------------------------------------------------------

    fn hunks(old: &str, new: &str) -> Vec<HunkOut> {
        word_hunks(old, new, None)
    }

    fn hunk(at: u32, context: Option<&str>, removed: Option<&str>, added: Option<&str>) -> HunkOut {
        HunkOut {
            at,
            context: context.map(str::to_owned),
            removed: removed.map(str::to_owned),
            added: added.map(str::to_owned),
        }
    }

    #[test]
    fn equal_texts_have_no_hunk_and_blanks_do_not_count() {
        assert_eq!(hunks("a b c", "a b c"), []);
        assert_eq!(hunks("a  b\n\nc", "a b c"), [], "seuls les mots comptent");
        assert_eq!(hunks("", ""), []);
        assert_eq!(hunks("   ", "\n"), []);
    }

    #[test]
    fn an_insertion_has_no_removed_part_and_a_deletion_no_added_part() {
        assert_eq!(
            hunks("un deux quatre", "un deux trois quatre"),
            [hunk(8, Some("un deux"), None, Some("trois"))]
        );
        assert_eq!(
            hunks("un deux trois quatre", "un quatre"),
            [hunk(3, Some("un"), Some("deux trois"), None)]
        );
    }

    #[test]
    fn a_change_at_the_very_end_sits_at_the_length_of_the_text() {
        assert_eq!(
            hunks("un deux", "un deux trois"),
            [hunk(7, Some("un deux"), None, Some("trois"))]
        );
        // « é » compte pour un caractère, et pour deux octets.
        assert_eq!(hunks("é", "é à"), [hunk(1, Some("é"), None, Some("à"))]);
    }

    #[test]
    fn the_first_or_the_whole_text_can_change() {
        assert_eq!(hunks("", "début"), [hunk(0, None, None, Some("début"))]);
        assert_eq!(hunks("fin", ""), [hunk(0, None, Some("fin"), None)]);
        assert_eq!(hunks("a b", "c d e"), [hunk(0, None, Some("a b"), Some("c d e"))]);
    }

    #[test]
    fn positions_count_characters_not_bytes() {
        // Trois caractères hors BMP (4 octets chacun), puis un accent (2 octets).
        let old = "😀😀😀 é un mot";
        let new = "😀😀😀 é un autre";
        assert_eq!(hunks(old, new), [hunk(9, Some("😀😀😀 é un"), Some("mot"), Some("autre"))]);
    }

    #[test]
    fn a_context_that_fits_is_complete_without_marker() {
        let exactly_sixty = format!("{} {}", "a".repeat(29), "b".repeat(30));
        assert_eq!(exactly_sixty.chars().count(), 60);
        let old = format!("{exactly_sixty} fin");
        let [one] = hunks(&old, &format!("{old}!")).try_into().unwrap();
        // Le mot changé est « fin! » : ce qui précède fait exactement 60 caractères.
        assert_eq!(one.context.as_deref(), Some(exactly_sixty.as_str()));
    }

    #[test]
    fn a_context_cut_at_a_word_starts_with_an_ellipsis_and_stays_within_sixty_characters() {
        // Avec « … » devant, le mot de 29 caractères ne tient plus : il saute entier.
        let exactly_sixty = format!("{} {}", "a".repeat(29), "b".repeat(30));
        let old = format!("début {exactly_sixty} fin");
        let [one] = hunks(&old, &format!("{old}!")).try_into().unwrap();
        let context = one.context.unwrap();
        assert_eq!(context, format!("… {}", "b".repeat(30)));
        assert!(context.chars().count() <= CONTEXT_MAX_CHARS);
        // Un mot de 59 caractères suivi de deux mots courts.
        let long = format!("{} a b", "x".repeat(59));
        let [one] = hunks(&long, &format!("{long} c")).try_into().unwrap();
        assert_eq!(one.context.as_deref(), Some("… a b"));
    }

    #[test]
    fn a_last_word_longer_than_the_context_is_kept_by_its_end() {
        let huge = "z".repeat(100);
        let [one] = hunks(&format!("a {huge}"), &format!("a {huge} b")).try_into().unwrap();
        let context = one.context.unwrap();
        assert_eq!(context, format!("…{}", "z".repeat(59)));
        assert_eq!(context.chars().count(), CONTEXT_MAX_CHARS);
    }

    #[test]
    fn a_long_word_before_the_last_one_is_left_out_of_the_context() {
        let huge = "y".repeat(100);
        let [one] = hunks(&format!("{huge} fin"), &format!("{huge} fin ajout")).try_into().unwrap();
        assert_eq!(one.context.as_deref(), Some("… fin"));
    }

    #[test]
    fn the_hunks_turn_a_into_b_for_a_variety_of_edits() {
        let cases = [
            (OLD, NEW),
            ("", "tout est nouveau"),
            ("tout disparaît", ""),
            ("a b c d e f g", "a x c d y z g h"),
            ("la loi du 7 octobre 2016", "la loi du 7 octobre 2016 modifiée"),
            ("alpha beta gamma delta", "delta gamma beta alpha"),
            ("un\n\ndeux\n\ntrois", "un deux 3"),
        ];
        for (old, new) in cases {
            assert_eq!(applied(old, &hunks(old, new)), words_of(new), "{old:?} -> {new:?}");
        }
    }

    #[test]
    fn an_expired_deadline_still_gives_a_valid_if_rough_diff() {
        let old = (0..400).map(|n| format!("mot{n}")).collect::<Vec<_>>().join(" ");
        let new = (0..400).map(|n| format!("autre{}", n % 7)).collect::<Vec<_>>().join(" ");
        let expired = Instant::now().checked_sub(Duration::from_secs(10));
        let rough = word_hunks(&old, &new, expired);
        assert_ne!(rough, Vec::<HunkOut>::new());
        assert_eq!(applied(&old, &rough), words_of(&new));
    }

    #[test]
    fn at_most_the_maximum_number_of_hunks_plus_one_are_computed() {
        // 1 000 changements isolés (un mot sur deux).
        let old = (0..2_000).map(|n| format!("m{n}")).collect::<Vec<_>>().join(" ");
        let new = (0..2_000)
            .map(|n| if n % 2 == 0 { format!("x{n}") } else { format!("m{n}") })
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(hunks(&old, &new).len(), MAX_HUNKS + 1);
    }

    // ---------------------------------------------------------------------------------------
    // Plafond de réponse
    // ---------------------------------------------------------------------------------------

    fn big(at: u32, size: usize) -> HunkOut {
        HunkOut {
            at,
            context: None,
            removed: Some("r".repeat(size)),
            added: Some("a".repeat(size)),
        }
    }

    #[test]
    fn hunks_under_the_caps_are_left_alone() {
        let small = vec![hunk(0, Some("c"), Some("r"), Some("a")), hunk(9, None, None, Some("b"))];
        assert_eq!(fit(small.clone(), 1_000), (small, false));
    }

    #[test]
    fn a_passage_is_clipped_with_an_ellipsis_and_the_flag_is_raised() {
        let (fitted, truncated) = fit(vec![big(0, 5_000)], 1_000);
        assert!(truncated);
        assert_eq!(fitted.len(), 1, "un seul hunk, même énorme, reste dans la réponse");
        let removed = fitted[0].removed.as_deref().unwrap();
        assert_eq!(removed.chars().count(), FIELD_CLIP_CHARS + 1);
        assert!(removed.ends_with('…'));
        assert!(hunk_text_chars(&fitted[0]) <= HUNK_TEXT_CAP);
        // Un passage de la longueur exacte n'est pas coupé.
        let (exact, truncated) = fit(vec![big(0, FIELD_CLIP_CHARS)], 1_000);
        assert!(!truncated);
        assert_eq!(exact[0].removed.as_deref().unwrap().chars().count(), FIELD_CLIP_CHARS);
    }

    #[test]
    fn clipping_never_splits_a_character() {
        let mut field = Some("é".repeat(FIELD_CLIP_CHARS + 10));
        assert!(clip(&mut field));
        assert_eq!(field.unwrap().chars().count(), FIELD_CLIP_CHARS + 1);
        assert!(!clip(&mut None));
    }

    #[test]
    fn hunks_are_removed_from_the_end_when_their_text_exceeds_eight_thousand_characters() {
        let all: Vec<HunkOut> = (0..5).map(|n| big(n, 1_500)).collect();
        let (fitted, truncated) = fit(all, 1_000);
        // 3 000 caractères par hunk : deux tiennent (6 000), pas trois (9 000).
        assert!(truncated);
        assert_eq!(fitted.iter().map(|h| h.at).collect::<Vec<_>>(), [0, 1]);
        // Pile 8 000 (quatre hunks de 2 000) : rien n'est retiré.
        let (exact, truncated) = fit((0..4).map(|n| big(n, 1_000)).collect(), 1_000);
        assert!(!truncated);
        assert_eq!(exact.len(), 4);
    }

    #[test]
    fn hunks_are_removed_from_the_end_when_the_estimate_exceeds_the_target() {
        // Un petit texte par hunk, mais 64 caractères estimés par champ : moins de 400 tiennent.
        let many: Vec<HunkOut> = (0..400).map(|n| hunk(n, None, Some("a"), Some("b"))).collect();
        let one_cost = hunk_cost(&many[0]);
        let (fitted, truncated) = fit(many, 1_000);
        assert!(truncated);
        let cost: usize = fitted.iter().map(hunk_cost).sum();
        assert!(1_000 + cost <= ResponseBudget::ESTIMATE_TARGET_CHARS);
        assert!(1_000 + cost + one_cost > ResponseBudget::ESTIMATE_TARGET_CHARS);
    }

    #[test]
    fn more_than_the_maximum_number_of_hunks_marks_the_response_truncated() {
        let surplus: Vec<HunkOut> = (0..=u32::try_from(MAX_HUNKS).unwrap())
            .map(|n| hunk(n, None, Some("a"), None))
            .collect();
        assert_eq!(surplus.len(), MAX_HUNKS + 1);
        let (fitted, truncated) = fit(surplus, 0);
        assert!(truncated);
        assert!(fitted.len() <= MAX_HUNKS);
    }

    #[test]
    fn the_cost_of_a_hunk_counts_its_fields_and_its_text() {
        // Élément, `at` et trois textes : cinq champs.
        assert_eq!(hunk_cost(&hunk(1, Some("ab"), Some("cd"), Some("ef"))), estimate_chars(5, 6));
        assert_eq!(hunk_cost(&hunk(1, None, None, Some("ef"))), estimate_chars(3, 2));
    }

    #[tokio::test]
    async fn a_very_different_pair_of_articles_is_truncated_and_stays_under_the_caps() {
        let old = (0..3_000).map(|n| format!("ancien{n}")).collect::<Vec<_>>().join(" ");
        let new = (0..3_000).map(|n| format!("nouveau{n}")).collect::<Vec<_>>().join(" ");
        let corpus = serving(two_versions(), &old, &new);

        let output = run(&corpus, &clock(), input(&id_of(1), "2020-01-01", Some("2026-01-01")))
            .await
            .unwrap();

        assert!(output.truncated);
        assert_eq!(output.hunks.len(), 1);
        let text: usize = output.hunks.iter().map(hunk_text_chars).sum();
        assert!(text <= HUNK_TEXT_CAP, "{text}");
    }
}
