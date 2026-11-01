//! Cas d'usage de `fr_get_act` (v0.1) : la lecture d'un texte consolidé et de sa table des matières (`GetActUseCase`).
//!
//! Ordre de validation (05 §5.2, §5.0.4) : `text_id`, `date`, `toc_offset`, `toc_limit`, tous avant
//! l'appel au port. La date servie est celle de `date`, sinon celle du suffixe de `text_id`
//! (conservée par `parse_versioned`), sinon aujourd'hui à Paris (00 §7). La table des matières est
//! aplatie en préordre (`Toc::page`), puis raccourcie en partant de la fin tant que son estimation
//! dépasse la cible du budget (05 §6.4, étape 1) : `next_toc_offset` suit la page réellement
//! rendue. Le découpage de la table est partagé avec `fr_get_convention` (`TocRequest`).
//!
//! Un `NotFound` du port pour une date choisie par l'appelant (`date` ou suffixe de `text_id`)
//! devient « No version of {id} is available on {date}… » : PISTE répond le même 400 pour un id
//! inconnu et pour une date avant la première version, et aucune sonde n'est faite pour les
//! distinguer (L3-11, Q12). Sans date choisie, le message générique de `messages` s'applique.

use std::num::NonZeroU32;

use lf_domain::{
    date::{CivilDate, VersionDate},
    entity::{Act, Toc, TocEntry, TocEntryRef},
    id::{LegiTextId, VersionedId, parse_versioned},
    status::LegalStatus,
};

use crate::{
    budget::{ResponseBudget, bounded, estimate_chars, shrink_from_end},
    error::AppError,
    failure::port_failure,
    input::GetActInput,
    messages,
    output::get_act::{GetActOutput, TocLine},
    ports::{Clock, CorpusError, LegalCorpus},
    tools::ToolName,
};

const TOOL: ToolName = ToolName::FR_GET_ACT;

/// `note` d'un texte de nature `CODE` (05 §5.2, D-47) : une page de table des matières d'un code
/// est la seule lecture possible, et la plus utile est l'article lui-même.
pub(crate) const CODE_NOTE: &str = "This is a whole code: its table of contents can be very long. \
Page it with toc_offset/toc_limit, or find an article with fr_search fond=CODE.";

/// Champs de la réponse hors table des matières, pour l'estimation (`text_id`, dates, `citation`,
/// `source_url`, `toc`, `toc_total`...).
const ACT_FIELDS: usize = 20;

/// Caractères des champs de taille fixe de la réponse : identifiant, URL de la page, dates et état.
const ACT_FIXED_CHARS: usize = 200;

/// Cas d'usage de `fr_get_act` : un texte consolidé et une page de sa table des matières.
pub(crate) struct GetActUseCase<'a, C: LegalCorpus, K: Clock> {
    corpus: &'a C,
    clock: &'a K,
}

impl<'a, C: LegalCorpus, K: Clock> GetActUseCase<'a, C, K> {
    /// Cas d'usage sur `corpus` ; `clock` donne « aujourd'hui » à Paris.
    pub(crate) fn new(corpus: &'a C, clock: &'a K) -> Self {
        Self { corpus, clock }
    }

    /// Valide `input`, lit le texte à la date voulue et en rend une page de table des matières.
    ///
    /// # Errors
    ///
    /// `invalid_arg` pour un argument refusé (sans appel au port), l'erreur du port traduite par
    /// [`port_failure`] sinon, `invalid_arg` pour un `toc_offset` au-delà de la fin de la table.
    pub(crate) async fn execute(&self, input: GetActInput) -> Result<GetActOutput, AppError> {
        let wanted = text_id(input.text_id.as_deref())?;
        let date = self.version_date(&wanted, input.date.as_deref())?;
        let request = TocRequest::parse(input.toc_offset, input.toc_limit)?;
        let dated = input.date.is_some() || wanted.version_start.is_some();
        let mut act = self.corpus.get_act(&wanted.id, date).await.map_err(|error| {
            let failure = port_failure(TOOL, wanted.id.as_str(), &error);
            match error {
                // Le 400 de PISTE est le même pour un id inconnu et pour une date avant la
                // première version : sans sonde (L3-11), le message nomme la date demandée.
                CorpusError::NotFound { .. } if dated => no_version_on(&wanted.id, date),
                _ => failure,
            }
        })?;
        // La table est lue une fois, puis lâchée : la sortie ne porte que la page rendue.
        let toc = std::mem::take(&mut act.body.toc);
        let note = act
            .body
            .nature
            .as_deref()
            .is_some_and(|nature| nature.eq_ignore_ascii_case("CODE"))
            .then_some(CODE_NOTE);
        let window = request.window(TOOL, &toc, metadata_cost(&act, note))?;
        Ok(GetActOutput {
            act,
            date,
            toc: window.lines,
            toc_total: window.total,
            next_toc_offset: window.next_offset,
            note,
        })
    }

    /// `date` si elle est donnée, sinon la date du suffixe de `text_id`, sinon aujourd'hui à Paris.
    fn version_date(
        &self,
        wanted: &VersionedId,
        explicit: Option<&str>,
    ) -> Result<VersionDate, AppError> {
        if let Some(raw) = explicit {
            return VersionDate::parse(raw).map_err(|error| messages::domain(TOOL, &error));
        }
        match wanted.version_start {
            Some(start) => VersionDate::try_from(start).map_err(|_| suffix_out_of_range(start)),
            None => VersionDate::try_from(self.clock.today_paris())
                .map_err(|error| messages::domain(TOOL, &error)),
        }
    }
}

/// `text_id` : absent ou blanc, `required` ; sinon un `LEGITEXT`, suffixe de version gardé.
fn text_id(raw: Option<&str>) -> Result<VersionedId, AppError> {
    let raw =
        raw.filter(|raw| !raw.trim().is_empty()).ok_or_else(|| messages::required("text_id"))?;
    parse_versioned(raw).map_err(|error| messages::domain(TOOL, &error))
}

/// `not_found` d'un texte demandé à une date choisie par l'appelant (date explicite ou suffixe).
fn no_version_on(id: &LegiTextId, date: VersionDate) -> AppError {
    AppError::not_found(format!(
        "No version of {id} is available on {date}. If the id is right, the text may exist only \
         from a later date: try a later date, or omit date."
    ))
}

/// Le suffixe de `text_id` est une vraie date, mais hors de la plage des versions servies.
fn suffix_out_of_range(start: CivilDate) -> AppError {
    messages::invalid_argument(format!(
        "text_id: the version suffix date {start} is outside {}..{}; pass date to choose the \
         version.",
        VersionDate::MIN,
        VersionDate::MAX
    ))
}

/// Estimation des champs de la réponse hors table des matières (05 §6.4).
#[expect(
    clippy::arithmetic_side_effects,
    reason = "estimation de taille : sommes de longueurs de textes et de compteurs déjà en mémoire, donc sous `usize::MAX`"
)]
fn metadata_cost(act: &Act, note: Option<&str>) -> usize {
    let body = &act.body;
    let aliases = body.aliases.iter().map(|alias| Some(alias.as_str()));
    let texts = [body.title.as_deref(), body.nature.as_deref(), body.num_parution.as_deref()]
        .into_iter()
        .chain([body.jo_reference.as_deref(), note])
        .chain(aliases);
    estimate_chars(ACT_FIELDS + body.aliases.len(), ACT_FIXED_CHARS + text_chars(texts))
}

/// Nombre de caractères des textes présents.
pub(crate) fn text_chars<'a>(texts: impl IntoIterator<Item = Option<&'a str>>) -> usize {
    texts.into_iter().flatten().map(|text| text.chars().count()).sum()
}

/// Fenêtre de table des matières demandée : `toc_offset` et `toc_limit` validés.
pub(crate) struct TocRequest {
    offset: u32,
    limit: NonZeroU32,
}

/// Page de table des matières rendue : les entrées gardées après estimation, le total de la
/// table et le début de la page suivante.
pub(crate) struct TocWindow {
    pub(crate) lines: Vec<TocLine>,
    pub(crate) total: u32,
    pub(crate) next_offset: Option<u32>,
}

impl TocRequest {
    /// `toc_offset` (0 par défaut, au plus `u32::MAX`) puis `toc_limit` (100 par défaut, 1 à 500).
    ///
    /// # Errors
    ///
    /// `invalid_arg` (`toc_offset=-1 out of range (0..4294967295).`) pour une valeur hors bornes.
    pub(crate) fn parse(offset: Option<i64>, limit: Option<i64>) -> Result<Self, AppError> {
        let offset = match offset {
            None => 0,
            Some(value) => u32::try_from(value)
                .map_err(|_| messages::out_of_range("toc_offset", value, 0, i64::from(u32::MAX)))?,
        };
        let limit = bounded(
            "toc_limit",
            limit,
            ResponseBudget::TOC_LIMIT_DEFAULT,
            1,
            ResponseBudget::TOC_LIMIT_MAX,
        )?;
        // `bounded` a ramené la valeur dans 1..=500 : la conversion ne retombe jamais sur 1.
        let limit = u32::try_from(limit).ok().and_then(NonZeroU32::new).unwrap_or(NonZeroU32::MIN);
        Ok(Self { offset, limit })
    }

    /// La page demandée de `toc`, raccourcie en partant de la fin tant que son estimation, avec
    /// `fixed` (les champs hors table), dépasse la cible du budget. Une entrée au moins est
    /// gardée : sans cela `next_toc_offset` ne ferait plus avancer la lecture.
    ///
    /// # Errors
    ///
    /// `invalid_arg` si `toc_offset` est au-delà de la fin de la table.
    pub(crate) fn window(
        &self,
        tool: ToolName,
        toc: &Toc,
        fixed: usize,
    ) -> Result<TocWindow, AppError> {
        let page =
            toc.page(self.offset, self.limit).map_err(|error| messages::domain(tool, &error))?;
        let mut lines: Vec<TocLine> = page.entries.iter().map(line_of).collect();
        shrink_from_end(&mut lines, |kept| {
            kept.len() > 1
                && cost(kept).saturating_add(fixed) > ResponseBudget::ESTIMATE_TARGET_CHARS
        });
        let end = self.offset.saturating_add(u32::try_from(lines.len()).unwrap_or(u32::MAX));
        Ok(TocWindow { lines, total: page.total, next_offset: (end < page.total).then_some(end) })
    }
}

/// Entrée rendue d'une entrée aplatie de la table.
fn line_of(entry: &TocEntry<'_>) -> TocLine {
    match entry.node {
        TocEntryRef::Section { title } => {
            TocLine::Section { depth: entry.depth, title: title.to_owned() }
        }
        TocEntryRef::Article(article) => TocLine::Article {
            depth: entry.depth,
            article_id: article.article_id.clone(),
            num: article.num.clone(),
            etat: article.etat.clone(),
        },
    }
}

/// Estimation d'une page : une section rend `depth` et `title` ; un article `depth`,
/// `article_id`, et `num` et `etat` quand ils existent (l'`etat` égal à celui du texte sera omis
/// par la vue, l'estimation le compte quand même).
#[expect(
    clippy::arithmetic_side_effects,
    reason = "estimation de taille : sommes de longueurs de textes et de compteurs déjà en mémoire, donc sous `usize::MAX`"
)]
fn cost(lines: &[TocLine]) -> usize {
    lines.iter().fold(0, |total, line| {
        let (fields, chars) = match line {
            TocLine::Section { title, .. } => (2, title.chars().count()),
            TocLine::Article { article_id, num, etat, .. } => (
                2 + usize::from(num.is_some()) + usize::from(etat.is_some()),
                article_id.as_str().len()
                    + text_chars([num.as_deref(), etat.as_ref().map(LegalStatus::as_api_code)]),
            ),
        };
        total.saturating_add(estimate_chars(fields, chars))
    })
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "tests : valeurs bornées par les données du test"
)]
#[cfg(test)]
pub(crate) mod fixtures {
    //! Textes de test partagés par les cas d'usage `fr_get_act` et `fr_get_convention`, par leurs
    //! façades et par leurs sorties.

    use lf_domain::{
        entity::{Act, ArticleDraft, SectionDraft, TextBody, Toc, TocDraft},
        id::LegiTextId,
        status::LegalStatus,
    };

    /// Article de brouillon : identifiant `LEGIARTI` à 12 chiffres, numéro `n`, abrogé si `n` est
    /// pair et sans état sinon.
    pub(crate) fn article(n: u32) -> ArticleDraft {
        ArticleDraft {
            id: Some(format!("LEGIARTI{n:012}")),
            num: Some(n.to_string()),
            etat: n.is_multiple_of(2).then(|| "ABROGE".to_owned()),
            ..ArticleDraft::default()
        }
    }

    /// Table de `sections` sections titrées `title`, chacune avec `articles` articles : elle
    /// compte `sections * (1 + articles)` entrées.
    pub(crate) fn toc(sections: u32, articles: u32, title: &str) -> Toc {
        let mut next = 0;
        let sections = (0..sections)
            .map(|index| SectionDraft {
                id: Some(format!("LEGISCTA{:012}", index + 1)),
                title: Some(format!("{title} {index}")),
                articles: (0..articles)
                    .map(|_| {
                        next += 1;
                        article(next)
                    })
                    .collect(),
                ..SectionDraft::default()
            })
            .collect();
        Toc::build(TocDraft { sections, articles: Vec::new() }).unwrap()
    }

    /// Loi `LEGITEXT000033205014` de nature `nature`, état en vigueur, avec la table `toc`.
    pub(crate) fn act(nature: &str, toc: Toc) -> Act {
        Act {
            text_id: LegiTextId::parse("LEGITEXT000033205014").unwrap(),
            body: TextBody {
                title: Some(
                    "LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique".into(),
                ),
                nature: Some(nature.into()),
                etat: LegalStatus::parse("VIGUEUR"),
                num_parution: Some("0235".into()),
                jo_reference: Some("JORF n°0235 du 8 octobre 2016".into()),
                aliases: vec!["loi Lemaire".into(), "loi numérique".into()],
                toc,
                ..TextBody::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        budget::{ResponseBudget, estimate_chars},
        error::ErrorCode,
        input::GetActInput,
        testing::{FakeCorpus, FixedClock, PortRequest},
        tools::ToolName,
    };
    use chrono::{DateTime, Utc};
    use fixtures::{act, toc};
    use lf_domain::{
        date::{CivilDate, VersionDate},
        id::LegiTextId,
    };
    use pretty_assertions::assert_eq;

    const TEXT_ID: &str = "LEGITEXT000033205014";

    fn utc(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339).unwrap().to_utc()
    }

    fn day(year: u16, month: u8, day: u8) -> CivilDate {
        CivilDate::new(year, month, day).unwrap()
    }

    fn version(text: &str) -> VersionDate {
        VersionDate::parse(text).unwrap()
    }

    fn id(text: &str) -> LegiTextId {
        LegiTextId::parse(text).unwrap()
    }

    /// Aujourd'hui à Paris : le 29 octobre 2026.
    fn clock() -> FixedClock {
        FixedClock::new(DateTime::UNIX_EPOCH, day(2026, 10, 29))
    }

    fn input(text_id: &str) -> GetActInput {
        GetActInput { text_id: Some(text_id.to_owned()), ..GetActInput::default() }
    }

    /// Corpus qui répond `act` à son premier appel.
    fn corpus_with(act: lf_domain::entity::Act) -> FakeCorpus {
        let corpus = FakeCorpus::new();
        corpus.push_get_act(Ok(act));
        corpus
    }

    async fn run(
        corpus: &FakeCorpus,
        clock: &FixedClock,
        input: GetActInput,
    ) -> Result<crate::output::get_act::GetActOutput, crate::error::AppError> {
        GetActUseCase::new(corpus, clock).execute(input).await
    }

    /// Code et message d'un échec, pour les comparer d'un bloc.
    fn failure(error: &crate::error::AppError) -> (Option<ErrorCode>, &str) {
        (error.code(), error.message())
    }

    #[tokio::test]
    async fn get_act_default_date_is_today_paris() {
        // 22:30 UTC le 14 juillet est 00:30 le 15 à Paris (heure d'été) : la version servie est
        // celle du 15, jamais celle de la veille (D-19).
        let clock = FixedClock::new(utc("2026-07-14T22:30:00Z"), day(2026, 7, 15));
        let corpus = corpus_with(act("LOI", toc(1, 2, "Titre")));

        let output = run(&corpus, &clock, input(TEXT_ID)).await.unwrap();

        assert_eq!(
            corpus.seen_requests(),
            [PortRequest::GetAct(id(TEXT_ID), version("2026-07-15"))]
        );
        assert_eq!(output.date, version("2026-07-15"));
    }

    #[tokio::test]
    async fn get_act_suffix_date_is_default_date() {
        let corpus = corpus_with(act("LOI", toc(1, 2, "Titre")));

        let output =
            run(&corpus, &clock(), input("LEGITEXT000033205014_15-07-2020")).await.unwrap();

        // L'identifiant nu part au port, avec la date du suffixe.
        assert_eq!(
            corpus.seen_requests(),
            [PortRequest::GetAct(id(TEXT_ID), version("2020-07-15"))]
        );
        assert_eq!(output.date, version("2020-07-15"));
    }

    #[tokio::test]
    async fn get_act_explicit_date_beats_the_suffix() {
        let corpus = corpus_with(act("LOI", toc(1, 2, "Titre")));
        let wanted = GetActInput {
            date: Some("2016-10-07".to_owned()),
            ..input("LEGITEXT000033205014_15-07-2020")
        };

        let output = run(&corpus, &clock(), wanted).await.unwrap();

        assert_eq!(
            corpus.seen_requests(),
            [PortRequest::GetAct(id(TEXT_ID), version("2016-10-07"))]
        );
        assert_eq!(output.date, version("2016-10-07"));
    }

    #[tokio::test]
    async fn get_act_trims_and_uppercases_the_identifier() {
        let corpus = corpus_with(act("LOI", toc(1, 1, "Titre")));

        run(&corpus, &clock(), input(" legitext000033205014\n")).await.unwrap();

        assert_eq!(
            corpus.seen_requests(),
            [PortRequest::GetAct(id(TEXT_ID), version("2026-10-29"))]
        );
    }

    #[tokio::test]
    async fn get_act_rejects_bad_arguments_in_order_without_network() {
        let corpus = FakeCorpus::new();
        let cases: [(GetActInput, &str); 8] = [
            (GetActInput::default(), "text_id is required."),
            (input("  "), "text_id is required."),
            (
                input("JURITEXT000037999394"),
                "text_id='JURITEXT000037999394' must be a Legifrance LEGITEXT... identifier. \
                 JURITEXT... identifies a court decision: use fr_get_decision.",
            ),
            (
                input("LEGITEXT000033205014_31-02-2023"),
                "text_id='LEGITEXT000033205014_31-02-2023': invalid version suffix (expected \
                 _DD-MM-YYYY, e.g. _08-09-2023).",
            ),
            (
                GetActInput { date: Some("2016-13-45".to_owned()), ..input(TEXT_ID) },
                "date='2016-13-45' is not a valid date (expected YYYY-MM-DD, e.g. 2026-10-15).",
            ),
            (
                GetActInput { toc_offset: Some(-1), ..input(TEXT_ID) },
                "toc_offset=-1 out of range (0..4294967295).",
            ),
            (
                GetActInput { toc_limit: Some(0), ..input(TEXT_ID) },
                "toc_limit=0 out of range (1..500).",
            ),
            (
                GetActInput { toc_limit: Some(501), ..input(TEXT_ID) },
                "toc_limit=501 out of range (1..500).",
            ),
        ];
        for (wanted, message) in cases {
            let error = run(&corpus, &clock(), wanted.clone()).await.unwrap_err();
            assert_eq!(failure(&error), (Some(ErrorCode::InvalidArg), message), "{wanted:?}");
        }
        // Le premier argument fautif est celui qui parle : l'identifiant avant la date.
        let both = GetActInput {
            date: Some("2016-13-45".to_owned()),
            toc_limit: Some(0),
            ..input("JURITEXT000037999394")
        };
        let error = run(&corpus, &clock(), both).await.unwrap_err();
        assert_eq!(error.message().split('=').next(), Some("text_id"));
        let both = GetActInput {
            date: Some("2016-13-45".to_owned()),
            toc_limit: Some(0),
            ..input(TEXT_ID)
        };
        let error = run(&corpus, &clock(), both).await.unwrap_err();
        assert_eq!(error.message().split('=').next(), Some("date"));
        assert_eq!(corpus.calls(), 0, "aucun appel au port pour un rejet local");
    }

    #[tokio::test]
    async fn get_act_toc_offset_beyond_u32_is_out_of_range() {
        let corpus = FakeCorpus::new();
        let wanted = GetActInput { toc_offset: Some(5_000_000_000), ..input(TEXT_ID) };

        let error = run(&corpus, &clock(), wanted).await.unwrap_err();

        assert_eq!(
            failure(&error),
            (Some(ErrorCode::InvalidArg), "toc_offset=5000000000 out of range (0..4294967295).")
        );
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn get_act_suffix_outside_the_supported_range_asks_for_a_date() {
        let corpus = FakeCorpus::new();

        let error =
            run(&corpus, &clock(), input("LEGITEXT000033205014_01-01-1400")).await.unwrap_err();

        assert_eq!(
            failure(&error),
            (
                Some(ErrorCode::InvalidArg),
                "text_id: the version suffix date 1400-01-01 is outside 1500-01-01..2221-12-31; \
                 pass date to choose the version."
            )
        );
        assert_eq!(corpus.calls(), 0);
        // Une date explicite évite le suffixe : la requête part.
        let corpus = corpus_with(act("LOI", toc(1, 1, "Titre")));
        let wanted = GetActInput {
            date: Some("2016-10-07".to_owned()),
            ..input("LEGITEXT000033205014_01-01-1400")
        };
        run(&corpus, &clock(), wanted).await.unwrap();
        assert_eq!(corpus.calls(), 1);
    }

    #[tokio::test]
    async fn get_act_a_broken_clock_is_an_invalid_date() {
        // Une horloge hors de la plage des versions ne part jamais au port.
        let broken = FixedClock::new(DateTime::UNIX_EPOCH, day(2999, 1, 1));
        let corpus = FakeCorpus::new();

        let error = run(&corpus, &broken, input(TEXT_ID)).await.unwrap_err();

        assert_eq!(
            failure(&error),
            (
                Some(ErrorCode::InvalidArg),
                "date='2999-01-01' is outside the supported range (1500-01-01..2221-12-31)."
            )
        );
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn toc_page_bounds() {
        // 4 sections de 21 articles : 88 entrées (la fixture de 05 §5.2).
        let all = || corpus_with(act("LOI", toc(4, 21, "Titre")));
        let window = |offset: Option<i64>, limit: Option<i64>| GetActInput {
            toc_offset: offset,
            toc_limit: limit,
            ..input(TEXT_ID)
        };

        // Par défaut : tout tient dans une page de 100.
        let output = run(&all(), &clock(), window(None, None)).await.unwrap();
        assert_eq!((output.toc.len(), output.toc_total, output.next_toc_offset), (88, 88, None));

        // Une fenêtre au milieu : `next_toc_offset` = offset + longueur.
        let output = run(&all(), &clock(), window(Some(80), Some(5))).await.unwrap();
        assert_eq!((output.toc.len(), output.toc_total, output.next_toc_offset), (5, 88, Some(85)));

        // La dernière fenêtre exacte n'a pas de suite.
        let output = run(&all(), &clock(), window(Some(83), Some(5))).await.unwrap();
        assert_eq!((output.toc.len(), output.toc_total, output.next_toc_offset), (5, 88, None));

        // `toc_limit` = 500 est admis ; 1 aussi.
        let output = run(&all(), &clock(), window(None, Some(500))).await.unwrap();
        assert_eq!(output.toc.len(), 88);
        let output = run(&all(), &clock(), window(None, Some(1))).await.unwrap();
        assert_eq!((output.toc.len(), output.next_toc_offset), (1, Some(1)));

        // Au-delà de la fin : `invalid_arg`, avec le total.
        let error = run(&all(), &clock(), window(Some(88), None)).await.unwrap_err();
        assert_eq!(
            failure(&error),
            (
                Some(ErrorCode::InvalidArg),
                "toc_offset=88 is past the end of the table of contents (toc_total=88); restart \
                 from toc_offset=0."
            )
        );

        // Une table vide se lit à 0 ; à 1 elle est déjà trop loin.
        let empty = || corpus_with(act("LOI", toc(0, 0, "Titre")));
        let output = run(&empty(), &clock(), window(None, None)).await.unwrap();
        assert_eq!((output.toc.len(), output.toc_total, output.next_toc_offset), (0, 0, None));
        let error = run(&empty(), &clock(), window(Some(1), None)).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
    }

    #[tokio::test]
    async fn toc_lines_carry_depth_titles_and_article_ids() {
        use crate::output::get_act::TocLine;
        let corpus = corpus_with(act("LOI", toc(1, 2, "Chapitre")));

        let output = run(&corpus, &clock(), input(TEXT_ID)).await.unwrap();

        let shape: Vec<String> = output
            .toc
            .iter()
            .map(|line| match line {
                TocLine::Section { depth, title } => format!("S{depth} {title}"),
                TocLine::Article { depth, article_id, num, etat } => {
                    format!("A{depth} {} {num:?} {etat:?}", article_id.as_str())
                }
            })
            .collect();
        assert_eq!(
            shape,
            [
                "S1 Chapitre 0",
                r#"A2 LEGIARTI000000000001 Some("1") None"#,
                r#"A2 LEGIARTI000000000002 Some("2") Some(Abroge)"#,
            ]
        );
    }

    #[tokio::test]
    async fn toc_shrunk_under_estimate_with_next_offset() {
        // 500 sections de 100 caractères de titre : environ 164 caractères estimés par entrée,
        // soit 82 000 pour la page : elle ne tient pas sous la cible de 42 000.
        let title = "T".repeat(100);
        let corpus = corpus_with(act("LOI", toc(500, 0, &title)));
        let wanted = GetActInput { toc_limit: Some(500), ..input(TEXT_ID) };

        let output = run(&corpus, &clock(), wanted).await.unwrap();

        let kept = output.toc.len();
        assert!(0 < kept && kept < 500, "{kept}");
        assert_eq!(output.toc_total, 500);
        assert_eq!(output.next_toc_offset, Some(u32::try_from(kept).unwrap()));
        // L'estimation des entrées gardées tient dans la cible, et une de plus n'y tiendrait pas.
        let per_entry = estimate_chars(2, 100 + 4);
        assert!(kept * per_entry <= ResponseBudget::ESTIMATE_TARGET_CHARS, "{kept}");
        assert!((kept + 1) * per_entry > ResponseBudget::ESTIMATE_TARGET_CHARS - 2_000, "{kept}");
    }

    #[tokio::test]
    async fn toc_shrunk_page_continues_from_its_own_offset() {
        let title = "T".repeat(100);
        let corpus = corpus_with(act("LOI", toc(500, 0, &title)));
        let wanted = GetActInput { toc_offset: Some(100), toc_limit: Some(400), ..input(TEXT_ID) };

        let output = run(&corpus, &clock(), wanted).await.unwrap();

        let kept = u32::try_from(output.toc.len()).unwrap();
        assert!(0 < kept && kept < 400, "{kept}");
        assert_eq!(output.next_toc_offset, Some(100 + kept));
    }

    #[tokio::test]
    async fn toc_shrinking_always_keeps_one_entry() {
        // Une seule entrée démesurée : la page ne se vide jamais, sinon `next_toc_offset` ne
        // ferait plus avancer la lecture.
        let corpus = corpus_with(act("LOI", toc(3, 0, &"T".repeat(50_000))));

        let output = run(&corpus, &clock(), input(TEXT_ID)).await.unwrap();

        assert_eq!(output.toc.len(), 1);
        assert_eq!(output.next_toc_offset, Some(1));
    }

    #[tokio::test]
    async fn get_act_on_code_adds_note() {
        for nature in ["CODE", "code"] {
            let corpus = corpus_with(act(nature, toc(1, 1, "Titre")));
            let output = run(&corpus, &clock(), input(TEXT_ID)).await.unwrap();
            assert_eq!(
                output.note,
                Some(
                    "This is a whole code: its table of contents can be very long. Page it with \
                     toc_offset/toc_limit, or find an article with fr_search fond=CODE."
                ),
                "{nature}"
            );
        }
        for nature in ["LOI", "DECRET"] {
            let corpus = corpus_with(act(nature, toc(1, 1, "Titre")));
            let output = run(&corpus, &clock(), input(TEXT_ID)).await.unwrap();
            assert_eq!(output.note, None, "{nature}");
        }
        // Sans nature : un texte LODA, donc sans note.
        let mut unnamed = act("LOI", toc(1, 1, "Titre"));
        unnamed.body.nature = None;
        let output = run(&corpus_with(unnamed), &clock(), input(TEXT_ID)).await.unwrap();
        assert_eq!(output.note, None);
    }

    #[tokio::test]
    async fn output_metric_counts_the_returned_toc_entries() {
        use crate::audited::AuditMetric;
        let corpus = corpus_with(act("LOI", toc(2, 3, "Titre")));
        let wanted = GetActInput { toc_limit: Some(5), ..input(TEXT_ID) };

        let output = run(&corpus, &clock(), wanted).await.unwrap();

        assert_eq!(output.output_count_or_size(), 5);
        // Le total est celui de la table entière, pas de la page.
        assert_eq!(output.toc_total, 8);
    }

    #[tokio::test]
    async fn port_failures_become_the_get_act_messages() {
        let cases = [
            (
                CorpusError::NotFound { what: "LEGITEXT000033205014".to_owned() },
                ErrorCode::NotFound,
                "No text found for LEGITEXT000033205014. Check the id, or search with fr_search \
                 fond=LODA or CODE.",
            ),
            (
                CorpusError::TooLarge { detail: "32 MiB".to_owned() },
                ErrorCode::UpstreamError,
                "this text is too large for fr_get_act; read its articles with fr_get_text, or \
                 find them with fr_search fond=CODE.",
            ),
        ];
        for (port_error, code, message) in cases {
            let corpus = FakeCorpus::new();
            corpus.push_get_act(Err(port_error));
            let error = run(&corpus, &clock(), input(TEXT_ID)).await.unwrap_err();
            assert_eq!(failure(&error), (Some(code), message));
        }
        // Le nom d'outil des messages est celui de l'audit.
        assert_eq!(ToolName::FR_GET_ACT.as_str(), "fr_get_act");
    }

    #[tokio::test]
    async fn not_found_cites_the_bare_identifier_not_the_suffix() {
        let corpus = FakeCorpus::new();
        corpus.push_get_act(Err(CorpusError::NotFound { what: "x".to_owned() }));

        let error =
            run(&corpus, &clock(), input("LEGITEXT000033205014_15-07-2020")).await.unwrap_err();

        assert!(error.message().starts_with("No version of LEGITEXT000033205014 is available on "));
    }

    #[tokio::test]
    async fn a_date_without_a_version_says_so_without_a_second_call() {
        let corpus = FakeCorpus::new();
        corpus.push_get_act(Err(CorpusError::NotFound { what: "x".to_owned() }));
        let input = GetActInput { date: Some("1700-01-01".to_owned()), ..input(TEXT_ID) };

        let error = run(&corpus, &clock(), input).await.unwrap_err();

        assert_eq!(
            failure(&error),
            (
                Some(ErrorCode::NotFound),
                "No version of LEGITEXT000033205014 is available on 1700-01-01. If the id is \
                 right, the text may exist only from a later date: try a later date, or omit \
                 date."
            )
        );
        assert_eq!(corpus.calls(), 1);
    }

    #[tokio::test]
    async fn a_suffix_date_without_a_version_says_so_with_the_bare_identifier() {
        let corpus = FakeCorpus::new();
        corpus.push_get_act(Err(CorpusError::NotFound { what: "x".to_owned() }));

        let error =
            run(&corpus, &clock(), input("LEGITEXT000033205014_06-01-1978")).await.unwrap_err();

        assert_eq!(
            failure(&error),
            (
                Some(ErrorCode::NotFound),
                "No version of LEGITEXT000033205014 is available on 1978-01-06. If the id is \
                 right, the text may exist only from a later date: try a later date, or omit \
                 date."
            )
        );
    }

    #[tokio::test]
    async fn an_unknown_text_without_a_date_keeps_the_generic_message() {
        let corpus = FakeCorpus::new();
        corpus.push_get_act(Err(CorpusError::NotFound { what: "x".to_owned() }));

        let error = run(&corpus, &clock(), input(TEXT_ID)).await.unwrap_err();

        assert_eq!(
            failure(&error),
            (
                Some(ErrorCode::NotFound),
                "No text found for LEGITEXT000033205014. Check the id, or search with fr_search \
                 fond=LODA or CODE."
            )
        );
    }

    #[tokio::test]
    async fn a_dated_call_keeps_the_other_port_failures_unmasked() {
        let corpus = FakeCorpus::new();
        corpus.push_get_act(Err(CorpusError::TooLarge { detail: "32 MiB".to_owned() }));
        let input = GetActInput { date: Some("2020-01-01".to_owned()), ..input(TEXT_ID) };

        let error = run(&corpus, &clock(), input).await.unwrap_err();

        assert_eq!(error.code(), Some(ErrorCode::UpstreamError));
        assert!(error.message().starts_with("this text is too large for fr_get_act"));
    }
}
