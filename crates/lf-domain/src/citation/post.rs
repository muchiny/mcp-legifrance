//! Arbitrage des chevauchements et post-traitement (annexe B §4.15, §4.16, §4.18, §4.19).
//!
//! [`arbitrate`] reçoit les candidats de toutes les familles (plages en offsets du texte
//! d'origine) et rend les citations retenues, triées par début de span puis par longueur
//! décroissante :
//!
//! 1. **Unités.** Un candidat seul est une unité ; les membres d'une énumération (même
//!    `enum_group` et même `group_extent`) forment une seule unité dont l'étendue va du premier
//!    numéro à la fin de la référence de code ou de texte (§4.16, règle 4). Les chevauchements
//!    s'évaluent sur l'étendue ; les spans émis restent ceux des numéros (§4.18).
//! 2. **Arbitrage.** Les unités sont triées par étendue décroissante, puis par priorité de
//!    famille, puis par position, et acceptées gloutonnement quand elles ne chevauchent aucune
//!    unité déjà acceptée (règles 1 et 2). Un puits (`Unsupported`) de même plage qu'un autre
//!    candidat l'emporte sur lui (règle 3) ; un candidat contenu dans un puits plus long en est
//!    écarté par le chevauchement.
//! 3. **Exception.** Le texte cité dans un qualificatif de version (« dans leur rédaction
//!    antérieure à celle issue de l'ordonnance n° 2016-131… ») est émis en plus de l'article qui
//!    le contient (règle 5, §4.15).
//! 4. **Pile de contexte**, remise à zéro à chaque appel (§4.15) : le dernier code cité résout
//!    « du même code », « dudit code », « du présent code » et « de ce code » ; le contexte du
//!    document (`ParseOptions::default_text`) comble un code absent ; un texte cité par une
//!    anaphore (« susvisée », « mentionnée ci-dessus ») prend le numéro du dernier texte de même
//!    nature et de même date cité plus haut ; un article de texte sans texte (« son article 16 »)
//!    prend le dernier texte cité.
//! 5. **Drapeaux calculés ici**, car seul ce module connaît `ParseOptions::today` : `annee_future`
//!    (une date cite une année postérieure à celle d'aujourd'hui), `annee_improbable` (une date
//!    cite une année avant 1789) et `annee_numero_differente_annee_date` (§4.19). La borne haute
//!    d'`annee_improbable` (aujourd'hui + 1) est absorbée par `annee_future`, comme dans le cas
//!    N13 (`ECLI:FR:CCASS:2099:…` ne porte que `annee_future`). Un drapeau déjà posé par la
//!    grammaire n'est pas répété. La confiance et l'ambiguïté viennent des grammaires ; ce module
//!    ne les modifie que pour les résolutions ci-dessus.
//! 6. **Garde-fou.** La sortie est tronquée aux `ParseOptions::max_citations` premières citations
//!    (§6.1) ; une sortie de cette longueur peut donc avoir été tronquée.
//!
//! Les numéros de groupe d'énumération sont renumérotés à partir de 1 dans l'ordre de la sortie :
//! deux grammaires peuvent numéroter leurs groupes chacune de leur côté.

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use crate::citation::grammar::Candidate;
use crate::citation::types::{
    Ambiguity, Anaphora, CitationKind, CitationRef, CodeRef, CodeVia, Confidence, Flag, Span,
    Spanned, TextCite, TextNature, TextNumber, VersionQualifier,
};
use crate::citation::{ParseOptions, TextContext};
use crate::date::CivilDate;

/// Année avant laquelle une date citée est improbable (la Déclaration de 1789 est la plus
/// ancienne citation courante).
const FIRST_PLAUSIBLE_YEAR: u16 = 1789;

/// Un candidat, ou les candidats d'une énumération : ce qu'arbitre l'acceptation gloutonne.
struct Unit {
    /// Rang de l'unité parmi toutes les unités.
    id: usize,
    /// Les candidats de l'unité (un seul hors énumération).
    members: Vec<Candidate>,
    /// Plage qui sert aux chevauchements.
    extent: Range<usize>,
    /// Plus forte priorité des membres.
    priority: u8,
}

impl Unit {
    fn of(id: usize, candidate: Candidate) -> Self {
        let extent = hull(&candidate.range, candidate.flags.group_extent.as_ref());
        Self { id, extent, priority: candidate.priority, members: vec![candidate] }
    }

    fn absorb(&mut self, candidate: Candidate) {
        self.extent = hull(&self.extent, Some(&candidate.range));
        self.priority = self.priority.max(candidate.priority);
        self.members.push(candidate);
    }

    /// Vrai si tous les membres sont hors périmètre (un puits).
    fn is_sink(&self) -> bool {
        self.members.iter().all(|c| c.value.kind() == CitationKind::Unsupported)
    }
}

/// Plus petite plage qui contient `a` et `b`.
fn hull(a: &Range<usize>, b: Option<&Range<usize>>) -> Range<usize> {
    b.map_or_else(|| a.clone(), |b| a.start.min(b.start)..a.end.max(b.end))
}

/// Regroupe les candidats en unités. Les plages vides ou inversées, qu'aucune grammaire ne doit
/// produire, sont écartées.
#[expect(
    clippy::indexing_slicing,
    reason = "`Unit::id` est le rang de l'unité dans `units` ; `selected` en a autant"
)]
fn units_of(candidates: Vec<Candidate>) -> Vec<Unit> {
    let mut units: Vec<Unit> = Vec::new();
    let mut groups: HashMap<(u32, Option<Range<usize>>), usize> = HashMap::new();
    for candidate in candidates.into_iter().filter(|c| c.range.start < c.range.end) {
        let key = candidate.flags.enum_group.map(|g| (g, candidate.flags.group_extent.clone()));
        let Some(key) = key else {
            units.push(Unit::of(units.len(), candidate));
            continue;
        };
        if let Some(&index) = groups.get(&key) {
            units[index].absorb(candidate);
        } else {
            groups.insert(key, units.len());
            units.push(Unit::of(units.len(), candidate));
        }
    }
    units
}

/// Unités dans l'ordre d'acceptation : étendue décroissante, puis priorité de famille, puis
/// position. Un puits de même étendue qu'une unité d'une autre famille passe avant elle
/// (règle 3) ; à étendues qui se recouvrent sans coïncider, la priorité de famille décide.
fn arbitration_order(units: &[Unit]) -> Vec<&Unit> {
    let plain: HashSet<&Range<usize>> =
        units.iter().filter(|u| !u.is_sink()).map(|u| &u.extent).collect();
    let mut order: Vec<&Unit> = units.iter().collect();
    order.sort_by_cached_key(|u| {
        (
            Reverse(u.extent.len()),
            Reverse(u.is_sink() && plain.contains(&u.extent)),
            Reverse(u.priority),
            u.extent.start,
            u.id,
        )
    });
    order
}

/// Acceptation gloutonne : une unité est retenue si son étendue ne chevauche aucune unité déjà
/// retenue. Les étendues retenues sont disjointes : la plus proche à gauche de la fin de la
/// nouvelle étendue est la seule à tester.
#[expect(
    clippy::indexing_slicing,
    reason = "`Unit::id` est le rang de l'unité dans `units` ; `selected` en a autant"
)]
fn accept(order: &[&Unit], total: usize) -> Vec<bool> {
    let mut taken: BTreeMap<usize, usize> = BTreeMap::new();
    let mut selected = vec![false; total];
    for unit in order {
        let free = taken
            .range(..unit.extent.end)
            .next_back()
            .is_none_or(|(_, &end)| end <= unit.extent.start);
        if free {
            taken.insert(unit.extent.start, unit.extent.end);
            selected[unit.id] = true;
        }
    }
    selected
}

/// Texte cité dans le qualificatif de version d'un article de code.
fn version_text(value: &CitationRef) -> Option<&TextCite> {
    match value {
        CitationRef::CodeArticle {
            version:
                Some(
                    VersionQualifier::RedactionIssueDe(text)
                    | VersionQualifier::RedactionAnterieureA(text),
                ),
            ..
        } => Some(text),
        _ => None,
    }
}

/// Texte d'une unité faite d'un seul candidat `Text`.
fn lone_text(unit: &Unit) -> Option<&TextCite> {
    match unit.members.as_slice() {
        [Candidate { value: CitationRef::Text(text), .. }] => Some(text),
        _ => None,
    }
}

/// Vrai si `a` et `b` désignent le même texte (nature, numéro et date).
fn same_text(a: &TextCite, b: &TextCite) -> bool {
    a.nature == b.nature && a.number == b.number && a.date == b.date
}

/// Règle 5 : le texte cité dans un qualificatif de version, écarté parce qu'il tient dans
/// l'article retenu, est retenu aussi : une seule mention (la plus longue) par texte et par
/// article retenu, même si plusieurs articles de l'énumération portent le même qualificatif.
///
/// Les textes sont triés par début de span : chaque article retenu ne parcourt que ceux de son
/// étendue, et les étendues retenues sont disjointes, d'où un coût total linéaire en leur nombre.
#[expect(
    clippy::indexing_slicing,
    reason = "`Unit::id` est le rang de l'unité dans `units` ; `selected` en a autant"
)]
fn rescue_version_texts(order: &[&Unit], selected: &mut [bool]) {
    let mut texts: Vec<(&Unit, &TextCite)> =
        order.iter().filter_map(|u| lone_text(u).map(|text| (*u, text))).collect();
    texts.sort_by_key(|(unit, _)| unit.extent.start);
    let outers: Vec<&Unit> = order.iter().copied().filter(|u| selected[u.id]).collect();
    for outer in outers {
        let from = texts.partition_point(|(unit, _)| unit.extent.start < outer.extent.start);
        let inside: Vec<&(&Unit, &TextCite)> = texts[from..]
            .iter()
            .take_while(|(unit, _)| outer.extent.contains(&unit.extent.start))
            .filter(|(unit, _)| unit.extent.end <= outer.extent.end)
            .collect();
        let mut wanted: Vec<&TextCite> = Vec::new();
        for text in outer.members.iter().filter_map(|m| version_text(&m.value)) {
            if !wanted.contains(&text) {
                wanted.push(text);
            }
        }
        for text in wanted {
            let matching: Vec<&&(&Unit, &TextCite)> =
                inside.iter().filter(|(_, found)| same_text(found, text)).collect();
            if !matching.iter().any(|(unit, _)| selected[unit.id])
                && let Some((longest, _)) =
                    matching.iter().min_by_key(|(unit, _)| Reverse(unit.extent.len()))
            {
                selected[longest.id] = true;
            }
        }
    }
}

/// Ajoute `flag` s'il n'y est pas déjà.
fn push_flag(flags: &mut Vec<Flag>, flag: Flag) {
    if !flags.contains(&flag) {
        flags.push(flag);
    }
}

/// Citation proposée par une grammaire, sans doublon de drapeau.
fn spanned_of(candidate: Candidate) -> Spanned<CitationRef> {
    let mut flags = Vec::new();
    for flag in candidate.flags.list {
        push_flag(&mut flags, flag);
    }
    Spanned {
        value: candidate.value,
        span: Span { start: candidate.range.start, end: candidate.range.end },
        confidence: candidate.confidence,
        ambiguity: candidate.flags.ambiguity,
        flags,
        enum_group: candidate.flags.enum_group,
    }
}

/// Façon dont une anaphore de code désigne son antécédent.
const fn via_of(anaphora: Anaphora) -> CodeVia {
    match anaphora {
        Anaphora::MemeCode | Anaphora::PresentCode => CodeVia::AnaphoreMemeCode,
        Anaphora::Dudit | Anaphora::CeCode => CodeVia::AnaphoreDudit,
    }
}

/// Pile de contexte d'un appel : dernier code, dernier numéro de chaque texte daté, dernier texte
/// (§4.15).
struct Context<'a> {
    document: Option<&'a TextContext>,
    last_code: Option<CodeRef>,
    cited: HashMap<(TextNature, CivilDate), TextNumber>,
    last_text: Option<TextCite>,
}

impl<'a> Context<'a> {
    fn new(opts: &'a ParseOptions) -> Self {
        Self {
            document: opts.default_text.as_ref(),
            last_code: None,
            cited: HashMap::new(),
            last_text: None,
        }
    }

    /// Résout ce que `item` laisse à son contexte, puis le retient comme contexte des suivantes.
    fn resolve(&mut self, item: &mut Spanned<CitationRef>) {
        let Spanned { value, ambiguity, confidence, .. } = item;
        match value {
            CitationRef::CodeArticle { code, code_via, .. } => {
                self.code_article(code, code_via, ambiguity);
            }
            CitationRef::Text(text) | CitationRef::TextArticle { text: Some(text), .. } => {
                self.text(text);
            }
            CitationRef::TextArticle { text, .. } => {
                if *ambiguity == Some(Ambiguity::TexteManquant)
                    && let Some(last) = &self.last_text
                {
                    *text = Some(last.clone());
                    *ambiguity = None;
                    *confidence = (*confidence).min(Confidence::Medium);
                }
            }
            _ => {}
        }
    }

    fn code_article(
        &mut self,
        code: &mut CodeRef,
        via: &mut Option<CodeVia>,
        ambiguity: &mut Option<Ambiguity>,
    ) {
        match code {
            CodeRef::Known(_) | CodeRef::Unknown(_) => self.last_code = Some(code.clone()),
            CodeRef::Anaphora(anaphora) => {
                let how = via_of(*anaphora);
                if let Some(last) = &self.last_code {
                    *code = last.clone();
                    *via = Some(how);
                    if *ambiguity == Some(Ambiguity::AnaphoreNonResolue) {
                        *ambiguity = None;
                    }
                } else {
                    *ambiguity = Some(Ambiguity::AnaphoreNonResolue);
                }
            }
            CodeRef::Missing => {
                // Un code absent seulement : un sigle ambigu (« L. 52-8 CE ») reste ce qu'il est.
                let absent = matches!(
                    ambiguity,
                    None | Some(Ambiguity::CodeManquant | Ambiguity::TexteManquant)
                );
                if let Some(document) = self.document
                    && absent
                {
                    if let Some(key) = document.code {
                        *code = CodeRef::Known(key);
                    }
                    *via = Some(CodeVia::ContexteDocument);
                    *ambiguity = Some(Ambiguity::CodeImplicite);
                }
            }
        }
    }

    fn text(&mut self, text: &mut TextCite) {
        if text.number.is_none()
            && text.anaphora.is_some()
            && let Some(date) = text.date
        {
            text.number = self.cited.get(&(text.nature, date)).cloned();
        }
        if let Some(number) = &text.number
            && let Some(date) = text.date
        {
            self.cited.insert((text.nature, date), number.clone());
        }
        self.last_text = Some(text.clone());
    }
}

/// Texte cité par une valeur, s'il y en a un.
fn text_of(value: &CitationRef) -> Option<&TextCite> {
    match value {
        CitationRef::Text(text) | CitationRef::TextArticle { text: Some(text), .. } => Some(text),
        _ => None,
    }
}

/// Années des dates que cite une valeur.
fn years_of(value: &CitationRef) -> Vec<u16> {
    let year = |date: Option<CivilDate>| date.map(CivilDate::year);
    let years = match value {
        CitationRef::CodeArticle { version: Some(version), .. } => match version {
            VersionQualifier::RedactionEnVigueur(date) => vec![year(*date)],
            VersionQualifier::EnVigueurAu(date) => vec![year(Some(*date))],
            VersionQualifier::RedactionIssueDe(text)
            | VersionQualifier::RedactionAnterieureA(text) => {
                vec![year(text.date)]
            }
            VersionQualifier::Ancien
            | VersionQualifier::Nouveau
            | VersionQualifier::RedactionApplicableAuLitige
            | VersionQualifier::RedactionApplicableDateDesFaits => vec![],
        },
        CitationRef::TextArticle { text, at_date, .. } => {
            vec![year(*at_date), year(text.as_ref().and_then(|t| t.date))]
        }
        CitationRef::Text(text) => vec![year(text.date)],
        CitationRef::Decision(decision) => vec![year(decision.date)],
        CitationRef::Ecli(ecli) => {
            vec![ecli.as_str().split(':').nth(3).and_then(|y| y.parse().ok())]
        }
        CitationRef::CnilDeliberation(cnil) => vec![Some(cnil.year), year(cnil.date)],
        CitationRef::Convention(convention) => vec![year(convention.date)],
        CitationRef::Jorf(jorf) => vec![year(jorf.date)],
        CitationRef::LegifranceId(id) => vec![year(id.version_start)],
        CitationRef::CodeArticle { version: None, .. }
        | CitationRef::Nor(_)
        | CitationRef::Eli(_)
        | CitationRef::Unsupported(_) => vec![],
    };
    years.into_iter().flatten().collect()
}

/// Drapeaux qui dépendent de la date du jour et des dates citées (§4.19).
fn add_date_flags(item: &mut Spanned<CitationRef>, today: Option<CivilDate>) {
    let years = years_of(&item.value);
    if today.is_some_and(|today| years.iter().any(|&y| y > today.year())) {
        push_flag(&mut item.flags, Flag::AnneeFuture);
    }
    if years.iter().any(|&y| y < FIRST_PLAUSIBLE_YEAR) {
        push_flag(&mut item.flags, Flag::AnneeImprobable);
    }
    let number_year = text_of(&item.value).and_then(|text| {
        text.number.as_ref().and_then(|n| n.year).zip(text.date.map(CivilDate::year))
    });
    if number_year.is_some_and(|(number, date)| number != date) {
        push_flag(&mut item.flags, Flag::AnneeNumeroDifferenteAnneeDate);
    }
}

/// Numéro de groupe affiché pour le `n`-ième groupe de la sortie (à partir de 1).
fn group_number(n: usize) -> u32 {
    u32::try_from(n + 1).unwrap_or(u32::MAX)
}

/// Citations retenues, triées par début de span puis par longueur décroissante.
#[expect(
    clippy::indexing_slicing,
    reason = "`Unit::id` est le rang de l'unité dans `units` ; `selected` en a autant"
)]
pub(crate) fn arbitrate(
    candidates: Vec<Candidate>,
    opts: &ParseOptions,
) -> Vec<Spanned<CitationRef>> {
    let units = units_of(candidates);
    let order = arbitration_order(&units);
    let mut selected = accept(&order, units.len());
    rescue_version_texts(&order, &mut selected);

    let mut emitted: Vec<(usize, Spanned<CitationRef>)> = units
        .into_iter()
        .filter(|unit| selected[unit.id])
        .flat_map(|unit| {
            let id = unit.id;
            unit.members.into_iter().map(move |member| (id, spanned_of(member)))
        })
        .collect();
    emitted.sort_by_key(|(_, s)| (s.span.start, Reverse(s.span.end)));
    emitted.truncate(opts.max_citations);

    let mut context = Context::new(opts);
    let mut numbering: HashMap<usize, u32> = HashMap::new();
    for (unit, item) in &mut emitted {
        context.resolve(item);
        add_date_flags(item, opts.today);
        if item.enum_group.is_some() {
            let next = group_number(numbering.len());
            item.enum_group = Some(*numbering.entry(*unit).or_insert(next));
        }
    }
    emitted.into_iter().map(|(_, item)| item).collect()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    use super::*;
    use crate::citation::dict::codes::CodeKey;
    use crate::citation::grammar::{Flags, priority_of};
    use crate::citation::types::{
        ArticleNum, ArticleRef, CnilCite, ConventionCite, DecisionCite, IdPrefix, JorfCite,
        LegifranceIdRef, TextAnaphora, UnsupportedCite, UnsupportedFamily,
    };
    use crate::contract::{Eli, Nor};
    use crate::ecli::Ecli;

    fn date(year: u16, month: u8, day: u8) -> CivilDate {
        CivilDate::new(year, month, day).expect("date valide")
    }

    fn article(n: u32) -> ArticleRef {
        ArticleRef {
            num: ArticleNum {
                prefix: None,
                special: None,
                base: vec![n],
                latin: None,
                letters: None,
            },
            sub: Vec::new(),
            range_to: None,
        }
    }

    fn code_value(code: CodeRef, n: u32) -> CitationRef {
        CitationRef::CodeArticle { code, article: article(n), version: None, code_via: None }
    }

    fn civil(n: u32) -> CitationRef {
        code_value(CodeRef::Known(CodeKey::CodeCivil), n)
    }

    fn text_cite(nature: TextNature, number: Option<&str>, when: Option<CivilDate>) -> TextCite {
        TextCite {
            nature,
            number: number.map(|raw| {
                let (year, seq) = raw.split_once('-').expect("AAAA-N");
                TextNumber {
                    year: year.parse().ok(),
                    seq: seq.parse().expect("séquence"),
                    extra: None,
                    raw: raw.to_owned(),
                }
            }),
            date: when,
            date_raw: None,
            title_hint: None,
            anaphora: None,
            appellation: None,
            emitter: None,
        }
    }

    fn text_article(text: Option<TextCite>, n: u32) -> CitationRef {
        CitationRef::TextArticle { text, article: article(n), at_date: None }
    }

    /// Code et origine du code d'un article de code.
    fn code_of(item: &Spanned<CitationRef>) -> Option<(CodeRef, Option<CodeVia>)> {
        match &item.value {
            CitationRef::CodeArticle { code, code_via, .. } => Some((code.clone(), *code_via)),
            _ => None,
        }
    }

    fn sink_value() -> CitationRef {
        CitationRef::Unsupported(UnsupportedCite { family: UnsupportedFamily::Doctrine, key: None })
    }

    fn candidate(range: Range<usize>, value: CitationRef) -> Candidate {
        Candidate::new(range, value)
    }

    fn members(
        range: Range<usize>,
        value: CitationRef,
        group: u32,
        extent: Range<usize>,
    ) -> Candidate {
        let mut c = candidate(range, value);
        c.flags = Flags { enum_group: Some(group), group_extent: Some(extent), ..Flags::default() };
        c
    }

    fn run(candidates: Vec<Candidate>) -> Vec<Spanned<CitationRef>> {
        arbitrate(candidates, &ParseOptions::default())
    }

    fn spans(out: &[Spanned<CitationRef>]) -> Vec<(usize, usize)> {
        out.iter().map(|s| (s.span.start, s.span.end)).collect()
    }

    fn kinds(out: &[Spanned<CitationRef>]) -> Vec<CitationKind> {
        out.iter().map(|s| s.value.kind()).collect()
    }

    #[test]
    fn overlaps_resolved_by_annex_rules() {
        // Règle 1 et 2 : le plus long l'emporte, quelle que soit sa famille.
        let out = run(vec![
            candidate(0..10, civil(1)),
            candidate(5..20, CitationRef::Text(text_cite(TextNature::Loi, None, None))),
        ]);
        assert_eq!((spans(&out), kinds(&out)), (vec![(5, 20)], vec![CitationKind::Text]));

        // À longueur égale, la priorité de famille décide : TextArticle > CodeArticle.
        let out = run(vec![candidate(5..15, civil(1)), candidate(0..10, text_article(None, 2))]);
        assert_eq!((spans(&out), kinds(&out)), (vec![(0, 10)], vec![CitationKind::TextArticle]));
        // Ecli > Nor, avec le même recouvrement.
        let nor = CitationRef::Nor(Nor::parse("ECFI1524250L").expect("NOR"));
        let ecli = CitationRef::Ecli(Ecli::parse("ECLI:FR:CC:2025:2025.1173.QPC").expect("ECLI"));
        let out = run(vec![candidate(0..10, nor), candidate(4..14, ecli)]);
        assert_eq!(kinds(&out), [CitationKind::Ecli]);

        // Même plage, même priorité : le premier candidat reçu l'emporte.
        let out = run(vec![candidate(0..9, civil(1)), candidate(0..9, civil(2))]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].value, civil(1));

        // Un candidat contenu dans un plus long de même sorte est écarté.
        let out = run(vec![candidate(2..6, civil(1)), candidate(0..30, civil(2))]);
        assert_eq!(spans(&out), [(0, 30)]);

        // Des spans qui se touchent ne se chevauchent pas ; la sortie est triée par début, quel
        // que soit l'ordre des candidats.
        let out = run(vec![
            candidate(5..9, civil(2)),
            candidate(0..5, civil(1)),
            candidate(20..25, civil(3)),
        ]);
        assert_eq!(spans(&out), [(0, 5), (5, 9), (20, 25)]);

        // Plages vides ou inversées : écartées.
        #[allow(clippy::reversed_empty_ranges, reason = "plage inversée volontaire")]
        let inverted = 9..3;
        let out = run(vec![
            candidate(4..4, civil(1)),
            candidate(inverted, civil(2)),
            candidate(0..2, civil(3)),
        ]);
        assert_eq!(spans(&out), [(0, 2)]);
        assert_eq!(run(Vec::new()), []);
    }

    #[test]
    fn candidate_fields_reach_the_citation() {
        let mut c = candidate(3..12, civil(9));
        c.confidence = Confidence::Low;
        c.flags.ambiguity = Some(Ambiguity::CodeManquant);
        c.flags.list = vec![Flag::DateInvalide, Flag::DateRepublicaine, Flag::DateInvalide];
        let out = run(vec![c]);
        assert_eq!(out.len(), 1);
        let item = &out[0];
        assert_eq!(item.span, Span { start: 3, end: 12 });
        assert_eq!(item.confidence, Confidence::Low);
        assert_eq!(item.ambiguity, Some(Ambiguity::CodeManquant));
        // Le doublon de drapeau est retiré, l'ordre est gardé.
        assert_eq!(item.flags, [Flag::DateInvalide, Flag::DateRepublicaine]);
        assert_eq!(item.enum_group, None);
    }

    #[test]
    fn sink_drops_contained_candidates() {
        // « D. 2016, p. 1234 » : le puits plus long écarte le candidat qu'il contient.
        let out = run(vec![candidate(3..8, civil(1)), candidate(0..20, sink_value())]);
        assert_eq!((spans(&out), kinds(&out)), (vec![(0, 20)], vec![CitationKind::Unsupported]));

        // Même plage : le puits l'emporte, bien que l'autre famille soit prioritaire (règle 3).
        let out =
            run(vec![candidate(0..16, text_article(None, 1)), candidate(0..16, sink_value())]);
        assert_eq!(kinds(&out), [CitationKind::Unsupported]);

        // Recouvrement sans inclusion à longueur égale : la priorité de famille décide, le
        // puits perd.
        let out = run(vec![candidate(0..10, sink_value()), candidate(5..15, civil(1))]);
        assert_eq!(kinds(&out), [CitationKind::CodeArticle]);

        // Un puits écarté par un candidat plus long n'écarte pas ce qu'il contient hors de ce
        // candidat : seul un puits accepté le fait.
        let out = run(vec![
            candidate(0..12, CitationRef::Text(text_cite(TextNature::Loi, None, None))),
            candidate(5..15, sink_value()),
            candidate(12..15, civil(1)),
        ]);
        assert_eq!(spans(&out), [(0, 12), (12, 15)]);
        assert_eq!(kinds(&out), [CitationKind::Text, CitationKind::CodeArticle]);

        // Un puits et un candidat disjoints restent tous deux.
        let out = run(vec![candidate(0..10, sink_value()), candidate(12..20, civil(1))]);
        assert_eq!(spans(&out), [(0, 10), (12, 20)]);
    }

    /// « articles L. 104-1 et L. 104-2 du code de l'urbanisme » : les spans émis sont ceux des
    /// numéros, le chevauchement s'évalue sur l'énumération entière.
    #[test]
    fn enumeration_spans_are_numbers() {
        let input = "les articles L. 104-1 et L. 104-2 du code de l'urbanisme, et 12";
        let at = |needle: &str| {
            let start = input.find(needle).expect("présent");
            start..start + needle.len()
        };
        let urbanisme = CodeRef::Known(CodeKey::CodeDeLUrbanisme);
        let extent = at("L. 104-1").start..at("code de l'urbanisme").end;
        let group = |first: u32, second: u32, id: u32| {
            vec![
                members(at("L. 104-1"), code_value(urbanisme.clone(), first), id, extent.clone()),
                members(at("L. 104-2"), code_value(urbanisme.clone(), second), id, extent.clone()),
            ]
        };

        let out = run(group(1, 2, 7));
        assert_eq!(
            spans(&out),
            [
                (at("L. 104-1").start, at("L. 104-1").end),
                (at("L. 104-2").start, at("L. 104-2").end)
            ]
        );
        assert_eq!(out.iter().map(|s| s.enum_group).collect::<Vec<_>>(), [Some(1), Some(1)]);

        // Un candidat plus court qui chevauche l'énumération, même entre deux numéros, est
        // écarté : la comparaison se fait sur l'étendue du groupe, pas sur les numéros.
        let mut candidates = group(1, 2, 7);
        candidates.push(candidate(at(" et "), civil(99)));
        candidates.push(candidate(at("du code"), civil(98)));
        let out = run(candidates);
        assert_eq!(out.len(), 2);

        // Un candidat plus long qui chevauche l'étendue écarte tous les membres.
        let mut candidates = group(1, 2, 7);
        candidates.push(candidate(0..input.len(), civil(99)));
        let out = run(candidates);
        assert_eq!(spans(&out), [(0, input.len())]);

        // Deux groupes de même numéro d'origine mais d'étendues différentes sont deux groupes ;
        // la sortie les renumérote à partir de 1, dans l'ordre du texte, et un candidat hors
        // énumération garde `None`.
        let other = at("12");
        let far = 200..204;
        let mut candidates = group(1, 2, 7);
        candidates.push(members(other.clone(), civil(12), 7, other.clone()));
        candidates.push(members(far.clone(), civil(13), 7, far));
        candidates.push(candidate(0..3, civil(50)));
        let out = run(candidates);
        assert_eq!(
            out.iter().map(|s| s.enum_group).collect::<Vec<_>>(),
            [None, Some(1), Some(1), Some(2), Some(3)]
        );

        // Sans étendue déclarée, l'étendue est l'enveloppe des membres.
        let mut loose = vec![candidate(0..4, civil(1)), candidate(10..14, civil(2))];
        for c in &mut loose {
            c.flags.enum_group = Some(5);
        }
        loose.push(candidate(6..8, civil(3)));
        let out = run(loose);
        assert_eq!(spans(&out), [(0, 4), (10, 14)]);
        assert_eq!(out[0].enum_group, out[1].enum_group);
    }

    #[test]
    fn context_stack_resets_per_call() {
        let anaphora = |a: Anaphora, range: Range<usize>| {
            let mut c = candidate(range, code_value(CodeRef::Anaphora(a), 5));
            c.flags.ambiguity = Some(Ambiguity::AnaphoreNonResolue);
            c
        };
        // Premier appel : le code cité résout l'anaphore qui suit.
        let first = run(vec![candidate(0..10, civil(1)), anaphora(Anaphora::MemeCode, 20..30)]);
        assert_eq!(first.len(), 2);
        assert_eq!(
            code_of(&first[1]),
            Some((CodeRef::Known(CodeKey::CodeCivil), Some(CodeVia::AnaphoreMemeCode)))
        );
        assert_eq!(first[1].ambiguity, None);

        // Second appel, sans autre citation : rien ne subsiste du premier.
        let second = run(vec![anaphora(Anaphora::MemeCode, 0..10)]);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].value, code_value(CodeRef::Anaphora(Anaphora::MemeCode), 5));
        assert_eq!(second[0].ambiguity, Some(Ambiguity::AnaphoreNonResolue));
        // Même sans ambiguïté posée par la grammaire, l'anaphore sans antécédent la reçoit.
        let mut bare = candidate(0..10, code_value(CodeRef::Anaphora(Anaphora::Dudit), 5));
        bare.flags.ambiguity = None;
        assert_eq!(run(vec![bare])[0].ambiguity, Some(Ambiguity::AnaphoreNonResolue));
    }

    #[test]
    fn code_anaphoras_resolve_to_the_last_cited_code() {
        let via = |a: Anaphora| {
            let out = run(vec![
                candidate(0..5, code_value(CodeRef::Known(CodeKey::CodePenal), 1)),
                candidate(10..15, code_value(CodeRef::Unknown("code de la voirie".into()), 2)),
                candidate(20..25, code_value(CodeRef::Anaphora(a), 3)),
            ]);
            code_of(&out[2])
        };
        let unknown = CodeRef::Unknown("code de la voirie".into());
        // Le dernier code cité, connu ou non, est l'antécédent.
        let resolved = |via| Some((unknown.clone(), Some(via)));
        assert_eq!(via(Anaphora::MemeCode), resolved(CodeVia::AnaphoreMemeCode));
        assert_eq!(via(Anaphora::PresentCode), resolved(CodeVia::AnaphoreMemeCode));
        assert_eq!(via(Anaphora::Dudit), resolved(CodeVia::AnaphoreDudit));
        assert_eq!(via(Anaphora::CeCode), resolved(CodeVia::AnaphoreDudit));
        // Une citation qui n'est pas un article de code n'a ni code ni origine de code.
        assert_eq!(code_of(&run(vec![candidate(0..5, text_article(None, 1))])[0]), None);
    }

    #[test]
    fn document_context_fills_a_missing_code() {
        let with_code = ParseOptions {
            default_text: Some(TextContext { code: Some(CodeKey::CodeDeLUrbanisme), text: None }),
            ..ParseOptions::default()
        };
        let no_code =
            ParseOptions { default_text: Some(TextContext::default()), ..ParseOptions::default() };
        let missing = |ambiguity: Option<Ambiguity>| {
            let mut c = candidate(0..10, code_value(CodeRef::Missing, 4));
            c.flags.ambiguity = ambiguity;
            c
        };
        let fill =
            |opts: &ParseOptions, ambiguity| arbitrate(vec![missing(ambiguity)], opts).remove(0);

        for ambiguity in [None, Some(Ambiguity::CodeManquant), Some(Ambiguity::TexteManquant)] {
            let item = fill(&with_code, ambiguity);
            assert_eq!(
                code_of(&item),
                Some((CodeRef::Known(CodeKey::CodeDeLUrbanisme), Some(CodeVia::ContexteDocument)))
            );
            assert_eq!(item.ambiguity, Some(Ambiguity::CodeImplicite));
        }
        // Contexte sans code : le code reste absent, mais la source est dite.
        let item = fill(&no_code, Some(Ambiguity::CodeManquant));
        assert_eq!(code_of(&item), Some((CodeRef::Missing, Some(CodeVia::ContexteDocument))));
        assert_eq!(item.ambiguity, Some(Ambiguity::CodeImplicite));
        // Un sigle ambigu n'est pas un code absent.
        let item =
            fill(&with_code, Some(Ambiguity::SigleAmbigu(crate::citation::types::Sigle::Ce)));
        assert_eq!(item.value, code_value(CodeRef::Missing, 4));
        assert_eq!(item.ambiguity, Some(Ambiguity::SigleAmbigu(crate::citation::types::Sigle::Ce)));
        // Sans contexte de document, rien ne change.
        let item = fill(&ParseOptions::default(), Some(Ambiguity::CodeManquant));
        assert_eq!(item.value, code_value(CodeRef::Missing, 4));
        assert_eq!(item.ambiguity, Some(Ambiguity::CodeManquant));
        // Un code cité par le texte l'emporte sur le contexte.
        let out = arbitrate(vec![candidate(0..10, civil(1))], &with_code);
        assert_eq!(out[0].value, civil(1));
    }

    #[test]
    fn text_anaphora_takes_the_number_of_the_antecedent() {
        let day = date(2023, 11, 20);
        let full = text_cite(TextNature::Loi, Some("2023-1059"), Some(day));
        let mut anaphoric = text_cite(TextNature::Loi, None, Some(day));
        anaphoric.anaphora = Some(TextAnaphora::MentionneeCiDessus);
        let number = |out: &[Spanned<CitationRef>], index: usize| {
            text_of(&out[index].value).expect("texte cité").number.clone()
        };

        // « loi n° 2023-1059 du 20 novembre 2023 … la loi du 20 novembre 2023 mentionnée
        // ci-dessus » : le numéro est repris.
        let out = run(vec![
            candidate(0..10, CitationRef::Text(full.clone())),
            candidate(20..40, CitationRef::Text(anaphoric.clone())),
        ]);
        assert_eq!(number(&out, 1), full.number);
        assert_eq!(
            text_of(&out[1].value).map(|t| t.anaphora),
            Some(Some(TextAnaphora::MentionneeCiDessus))
        );

        // Sans antécédent, d'une autre nature ou d'une autre date : numéro absent (P058).
        for before in [
            None,
            Some(text_cite(TextNature::Decret, Some("2023-1059"), Some(day))),
            Some(text_cite(TextNature::Loi, Some("2023-1059"), Some(date(2023, 11, 21)))),
            Some(text_cite(TextNature::Loi, None, Some(day))),
        ] {
            let mut candidates = vec![candidate(20..40, CitationRef::Text(anaphoric.clone()))];
            candidates.extend(before.map(|t| candidate(0..10, CitationRef::Text(t))));
            let out = run(candidates);
            assert_eq!(number(&out, out.len() - 1), None);
        }

        // Le numéro vient aussi d'un article de texte antérieur, et le texte d'un article peut
        // être l'anaphore.
        let out = run(vec![
            candidate(0..12, text_article(Some(full.clone()), 5)),
            candidate(20..45, text_article(Some(anaphoric.clone()), 7)),
        ]);
        assert_eq!(text_of(&out[1].value).map(|t| t.number.clone()), Some(full.number.clone()));

        // Une date citée sans anaphore ne cherche pas d'antécédent.
        let plain = text_cite(TextNature::Loi, None, Some(day));
        let out = run(vec![
            candidate(0..10, CitationRef::Text(full)),
            candidate(20..40, CitationRef::Text(plain)),
        ]);
        assert_eq!(number(&out, 1), None);
    }

    #[test]
    fn son_article_takes_the_last_cited_text() {
        let constitution = text_cite(TextNature::Constitution, None, Some(date(1958, 10, 4)));
        let missing = |range: Range<usize>, ambiguity| {
            let mut c = candidate(range, text_article(None, 16));
            c.flags.ambiguity = ambiguity;
            c
        };
        let out = run(vec![
            candidate(0..10, text_article(Some(constitution.clone()), 61)),
            missing(20..30, Some(Ambiguity::TexteManquant)),
        ]);
        assert_eq!(out[1].value, text_article(Some(constitution), 16));
        assert_eq!(out[1].ambiguity, None);
        assert_eq!(out[1].confidence, Confidence::Medium);

        // Pas de texte cité avant : la lacune est gardée.
        let out = run(vec![missing(20..30, Some(Ambiguity::TexteManquant))]);
        assert_eq!(out[0].value, text_article(None, 16));
        assert_eq!(out[0].ambiguity, Some(Ambiguity::TexteManquant));
        // « La Convention » seule n'est pas « son article » : ambiguïté différente, texte gardé
        // absent même s'il y a un texte plus haut.
        let out = run(vec![
            candidate(
                0..10,
                CitationRef::Text(text_cite(TextNature::Loi, Some("2016-1321"), None)),
            ),
            missing(20..30, Some(Ambiguity::ConventionNonIdentifiee)),
        ]);
        assert_eq!(out[1].value, text_article(None, 16));
        // Une confiance déjà basse n'est pas relevée.
        let mut low = missing(20..30, Some(Ambiguity::TexteManquant));
        low.confidence = Confidence::Low;
        let out = run(vec![
            candidate(
                0..10,
                CitationRef::Text(text_cite(TextNature::Loi, Some("2016-1321"), None)),
            ),
            low,
        ]);
        assert_eq!(out[1].confidence, Confidence::Low);
    }

    /// Bornes de la règle 5 : un texte qui commence exactement au début de l'étendue de l'article
    /// retenu est sauvé (le début de l'étendue appartient à l'étendue).
    #[test]
    fn version_text_starting_at_the_article_start_is_emitted() {
        let ordinance =
            text_cite(TextNature::Ordonnance, Some("2016-131"), Some(date(2016, 2, 10)));
        let qualified = CitationRef::CodeArticle {
            code: CodeRef::Known(CodeKey::CodeCivil),
            article: article(1244),
            version: Some(VersionQualifier::RedactionAnterieureA(Box::new(ordinance.clone()))),
            code_via: None,
        };
        let out =
            run(vec![candidate(0..60, qualified), candidate(0..40, CitationRef::Text(ordinance))]);
        assert_eq!(spans(&out), [(0, 60), (0, 40)]);
        assert_eq!(kinds(&out), [CitationKind::CodeArticle, CitationKind::Text]);
    }

    #[test]
    fn version_qualified_text_is_also_emitted() {
        // « articles 1244 et 1253 du code civil, dans leur rédaction antérieure à celle issue de
        // l'ordonnance n° 2016-131 du 10 février 2016 » (P045).
        let ordinance =
            text_cite(TextNature::Ordonnance, Some("2016-131"), Some(date(2016, 2, 10)));
        let qualified = |n: u32| CitationRef::CodeArticle {
            code: CodeRef::Known(CodeKey::CodeCivil),
            article: article(n),
            version: Some(VersionQualifier::RedactionAnterieureA(Box::new(ordinance.clone()))),
            code_via: None,
        };
        let extent = 0..120;
        let candidates = vec![
            members(11..15, qualified(1244), 3, extent.clone()),
            members(19..23, qualified(1253), 3, extent.clone()),
            candidate(70..110, CitationRef::Text(ordinance.clone())),
        ];
        let out = run(candidates.clone());
        assert_eq!(spans(&out), [(11, 15), (19, 23), (70, 110)]);
        assert_eq!(
            kinds(&out),
            [CitationKind::CodeArticle, CitationKind::CodeArticle, CitationKind::Text]
        );
        assert_eq!(out.iter().map(|s| s.enum_group).collect::<Vec<_>>(), [Some(1), Some(1), None]);

        // Un texte différent du qualificatif reste écarté.
        let other = text_cite(TextNature::Ordonnance, Some("2016-132"), Some(date(2016, 2, 10)));
        let mut different = candidates.clone();
        different[2] = candidate(70..110, CitationRef::Text(other));
        assert_eq!(kinds(&run(different)), [CitationKind::CodeArticle, CitationKind::CodeArticle]);
        // Un texte qui sort de l'étendue de l'article retenu n'est pas sauvé.
        let mut outside = candidates.clone();
        outside[2] = candidate(70..140, CitationRef::Text(ordinance.clone()));
        assert_eq!(kinds(&run(outside)), [CitationKind::CodeArticle, CitationKind::CodeArticle]);
        // Un article sans qualificatif de ce genre ne sauve rien.
        let plain = vec![
            members(11..15, civil(1244), 3, extent.clone()),
            candidate(70..110, CitationRef::Text(ordinance.clone())),
        ];
        assert_eq!(kinds(&run(plain)), [CitationKind::CodeArticle]);
        // Le texte sauvé n'est pas répété s'il est reçu deux fois : seule la mention la plus
        // longue est gardée, quel que soit l'ordre de réception.
        let mut twice = candidates.clone();
        twice.push(candidate(72..100, CitationRef::Text(ordinance.clone())));
        assert_eq!(spans(&run(twice)), [(11, 15), (19, 23), (70, 110)]);
        let mut reversed = candidates.clone();
        reversed.insert(0, candidate(72..100, CitationRef::Text(ordinance.clone())));
        assert_eq!(spans(&run(reversed)), [(11, 15), (19, 23), (70, 110)]);
        // Deux qualificatifs qui désignent le même texte sous des libellés différents ne sauvent
        // qu'une mention.
        let mut hinted = ordinance.clone();
        hinted.title_hint = Some("relative à la réforme".to_owned());
        let other_wording = CitationRef::CodeArticle {
            code: CodeRef::Known(CodeKey::CodeCivil),
            article: article(1253),
            version: Some(VersionQualifier::RedactionAnterieureA(Box::new(hinted))),
            code_via: None,
        };
        let mixed = vec![
            members(11..15, qualified(1244), 3, extent.clone()),
            members(19..23, other_wording, 3, extent),
            candidate(70..110, CitationRef::Text(ordinance)),
        ];
        assert_eq!(spans(&run(mixed)), [(11, 15), (19, 23), (70, 110)]);
    }

    /// Beaucoup de candidats : l'arbitrage et la règle 5 restent quasi linéaires (pas de balayage
    /// de tous les candidats pour chaque article retenu).
    #[test]
    fn many_version_qualified_articles_stay_fast() {
        let ordinance =
            text_cite(TextNature::Ordonnance, Some("2016-131"), Some(date(2016, 2, 10)));
        let pairs = 20_000;
        let mut candidates = Vec::with_capacity(pairs * 2);
        for i in 0..pairs {
            let base = i * 100;
            let value = CitationRef::CodeArticle {
                code: CodeRef::Known(CodeKey::CodeCivil),
                article: article(1),
                version: Some(VersionQualifier::RedactionAnterieureA(Box::new(ordinance.clone()))),
                code_via: None,
            };
            candidates.push(members(
                base..base + 5,
                value,
                1 + u32::try_from(i).unwrap_or(0),
                base..base + 90,
            ));
            candidates.push(candidate(base + 50..base + 80, CitationRef::Text(ordinance.clone())));
        }
        let opts = ParseOptions { max_citations: usize::MAX, ..ParseOptions::default() };
        let out = arbitrate(candidates, &opts);
        assert_eq!(out.len(), pairs * 2);
    }

    #[test]
    fn max_citations_guard() {
        let many = |n: usize| -> Vec<Candidate> {
            (0..n).rev().map(|i| candidate(i * 10..i * 10 + 5, civil(1))).collect()
        };
        let capped = |max: usize, n: usize| {
            arbitrate(many(n), &ParseOptions { max_citations: max, ..ParseOptions::default() })
        };
        // On garde les premières citations du texte, pas les premières reçues.
        assert_eq!(spans(&capped(3, 5)), [(0, 5), (10, 15), (20, 25)]);
        // Zéro : rien ; au-dessus du nombre de citations : tout.
        assert_eq!(capped(0, 5), []);
        assert_eq!(capped(9, 5).len(), 5);
        assert_eq!(capped(5, 5).len(), 5);
        // Garde-fou par défaut du domaine : 1 000.
        assert_eq!(run(many(1_500)).len(), 1_000);
        assert_eq!(run(many(1_000)).len(), 1_000);
        assert_eq!(run(many(999)).len(), 999);
        // La coupure au milieu d'une énumération renumérote les groupes restants.
        let candidates = vec![
            members(0..2, civil(1), 9, 0..30),
            members(10..12, civil(2), 9, 0..30),
            members(40..42, civil(3), 4, 40..60),
        ];
        let out =
            arbitrate(candidates, &ParseOptions { max_citations: 2, ..ParseOptions::default() });
        assert_eq!(out.iter().map(|s| s.enum_group).collect::<Vec<_>>(), [Some(1), Some(1)]);
    }

    #[test]
    fn year_flags_use_today_and_the_lower_bound() {
        let today = Some(date(2026, 10, 15));
        let opts = ParseOptions { today, ..ParseOptions::default() };
        let flags = |value: CitationRef, opts: &ParseOptions| {
            arbitrate(vec![candidate(0..10, value)], opts).remove(0).flags
        };
        let ecli = |year: u16| {
            CitationRef::Ecli(Ecli::parse(&format!("ECLI:FR:CCASS:{year}:C100399")).expect("ECLI"))
        };
        // N13 : une année future ne porte que `annee_future`, derrière les drapeaux de la grammaire.
        let mut c = candidate(0..10, ecli(2099));
        c.flags.list = vec![Flag::ChambreCassInconnue("XX".into())];
        assert_eq!(
            arbitrate(vec![c], &opts).remove(0).flags,
            [Flag::ChambreCassInconnue("XX".into()), Flag::AnneeFuture]
        );
        // L'année courante n'est pas future ; sans `today`, aucune année ne l'est.
        assert_eq!(flags(ecli(2026), &opts), []);
        assert_eq!(flags(ecli(2027), &opts), [Flag::AnneeFuture]);
        assert_eq!(flags(ecli(2099), &ParseOptions::default()), []);
        // Borne basse : avant 1789, improbable, avec ou sans `today`.
        assert_eq!(flags(ecli(1788), &opts), [Flag::AnneeImprobable]);
        assert_eq!(flags(ecli(1788), &ParseOptions::default()), [Flag::AnneeImprobable]);
        assert_eq!(flags(ecli(1789), &opts), []);
        // Un drapeau déjà posé par la grammaire n'est pas répété.
        let mut c = candidate(0..10, ecli(2099));
        c.flags.list = vec![Flag::AnneeFuture];
        assert_eq!(arbitrate(vec![c], &opts).remove(0).flags, [Flag::AnneeFuture]);
        // Une date future et une date improbable dans la même citation : les deux drapeaux.
        let both = text_cite(TextNature::Loi, None, Some(date(2099, 1, 1)));
        let value = CitationRef::TextArticle {
            text: Some(both),
            article: article(1),
            at_date: Some(date(1700, 1, 1)),
        };
        assert_eq!(flags(value, &opts), [Flag::AnneeFuture, Flag::AnneeImprobable]);
    }

    #[test]
    fn number_year_differs_from_date_year() {
        let flags = |value: CitationRef| {
            arbitrate(vec![candidate(0..10, value)], &ParseOptions::default()).remove(0).flags
        };
        // N07 : numéro de 2016, date de 2015.
        let suspicious = text_cite(TextNature::Loi, Some("2016-1321"), Some(date(2015, 10, 7)));
        assert_eq!(
            flags(CitationRef::Text(suspicious.clone())),
            [Flag::AnneeNumeroDifferenteAnneeDate]
        );
        assert_eq!(
            flags(text_article(Some(suspicious), 3)),
            [Flag::AnneeNumeroDifferenteAnneeDate]
        );
        // Années identiques, date absente, numéro absent ou sans année : rien.
        let same = text_cite(TextNature::Loi, Some("2016-1321"), Some(date(2016, 10, 7)));
        assert_eq!(flags(CitationRef::Text(same)), []);
        assert_eq!(
            flags(CitationRef::Text(text_cite(TextNature::Loi, Some("2016-1321"), None))),
            []
        );
        assert_eq!(
            flags(CitationRef::Text(text_cite(TextNature::Loi, None, Some(date(2016, 1, 1))))),
            []
        );
        let mut no_year = text_cite(TextNature::Loi, Some("2016-1"), Some(date(2015, 1, 1)));
        no_year.number = no_year.number.map(|number| TextNumber { year: None, ..number });
        assert_eq!(flags(CitationRef::Text(no_year)), []);
        // Un article de code n'a pas de texte.
        assert_eq!(flags(civil(1)), []);
    }

    /// Une valeur de chaque sorte qui cite une date de 1700 : `annee_improbable` pour chacune.
    #[test]
    fn dated_values_of_every_kind_are_checked() {
        let old = Some(date(1700, 5, 1));
        let decision = DecisionCite {
            court: None,
            court_via: None,
            date: old,
            numbers: Vec::new(),
            kind: None,
            publication: None,
            solution: None,
            parties: None,
            subject: None,
            arret_num: None,
            formation_code: None,
            publication_codes: Vec::new(),
        };
        let qualified = |version| CitationRef::CodeArticle {
            code: CodeRef::Missing,
            article: article(1),
            version: Some(version),
            code_via: None,
        };
        let old_text = || Box::new(text_cite(TextNature::Loi, None, old));
        let improbable: Vec<CitationRef> = vec![
            qualified(VersionQualifier::RedactionEnVigueur(old)),
            qualified(VersionQualifier::EnVigueurAu(date(1700, 5, 1))),
            qualified(VersionQualifier::RedactionIssueDe(old_text())),
            qualified(VersionQualifier::RedactionAnterieureA(old_text())),
            CitationRef::TextArticle { text: None, article: article(1), at_date: old },
            CitationRef::Text(text_cite(TextNature::Loi, None, old)),
            CitationRef::Decision(decision),
            CitationRef::Ecli(Ecli::parse("ECLI:FR:CE:1700:123").expect("ECLI")),
            CitationRef::CnilDeliberation(CnilCite {
                series: None,
                year: 1700,
                seq: 1,
                date: None,
                formation_restreinte: false,
            }),
            CitationRef::CnilDeliberation(CnilCite {
                series: None,
                year: 2020,
                seq: 1,
                date: old,
                formation_restreinte: false,
            }),
            CitationRef::Convention(ConventionCite {
                kind: None,
                idcc: None,
                brochure: None,
                avenant: None,
                title_hint: None,
                date: old,
            }),
            CitationRef::Jorf(JorfCite { number: None, date: old, texte: None, page: None }),
            CitationRef::LegifranceId(LegifranceIdRef {
                prefix: IdPrefix::LegiArti,
                digits: 1,
                version_start: old,
                url_kind: None,
            }),
        ];
        for value in improbable {
            let out = run(vec![candidate(0..10, value.clone())]);
            assert_eq!(out[0].flags, [Flag::AnneeImprobable], "{value:?}");
        }
        // Valeurs sans date, ou dont la date est plausible ou absente : aucun drapeau.
        let quiet: Vec<CitationRef> = vec![
            qualified(VersionQualifier::Ancien),
            qualified(VersionQualifier::Nouveau),
            qualified(VersionQualifier::RedactionApplicableAuLitige),
            qualified(VersionQualifier::RedactionApplicableDateDesFaits),
            qualified(VersionQualifier::RedactionEnVigueur(None)),
            civil(1),
            CitationRef::Nor(Nor::parse("ECFI1524250L").expect("NOR")),
            CitationRef::Eli(Eli::parse("/eli/loi/2016/10/7/ECFI1524250L/jo/texte").expect("ELI")),
            sink_value(),
        ];
        for value in quiet {
            assert_eq!(run(vec![candidate(0..10, value.clone())])[0].flags, [], "{value:?}");
        }
    }

    fn arbitrary_candidates() -> impl Strategy<Value = Vec<Candidate>> {
        let value = prop_oneof![
            Just(civil(1)),
            Just(text_article(None, 2)),
            Just(sink_value()),
            Just(CitationRef::Text(text_cite(TextNature::Loi, None, None))),
        ];
        let one = (0usize..80, 1usize..30, value, any::<bool>(), 0u32..3).prop_map(
            |(start, len, value, grouped, group)| {
                let mut c = Candidate::new(start..start + len, value);
                if grouped {
                    c.flags.enum_group = Some(group);
                }
                c
            },
        );
        prop::collection::vec(one, 0..24)
    }

    proptest! {
        #[test]
        fn output_is_sorted_bounded_and_deterministic(
            candidates in arbitrary_candidates(),
            max in 0usize..30,
        ) {
            let opts = ParseOptions { max_citations: max, ..ParseOptions::default() };
            let out = arbitrate(candidates.clone(), &opts);
            prop_assert!(out.len() <= max);
            prop_assert!(out.windows(2).all(|w| (w[0].span.start, Reverse(w[0].span.end)) <= (w[1].span.start, Reverse(w[1].span.end))));
            prop_assert!(out.iter().all(|s| s.span.start < s.span.end));
            prop_assert_eq!(&out, &arbitrate(candidates.clone(), &opts));
            // Hors énumération, deux citations retenues ne se chevauchent jamais.
            let plain: Vec<_> = out.iter().filter(|s| s.enum_group.is_none()).collect();
            for (i, a) in plain.iter().enumerate() {
                for b in &plain[i + 1..] {
                    prop_assert!(a.span.end <= b.span.start || b.span.end <= a.span.start, "{a:?} {b:?}");
                }
            }
            // Les numéros de groupe sont denses à partir de 1.
            let mut seen: Vec<u32> = out.iter().filter_map(|s| s.enum_group).collect();
            seen.dedup();
            seen.sort_unstable();
            seen.dedup();
            prop_assert!(seen.iter().enumerate().all(|(i, &g)| usize::try_from(g) == Ok(i + 1)));
        }
    }

    #[test]
    fn priorities_given_by_candidates_decide_ties() {
        // La priorité lue est celle du candidat, pas une priorité recalculée par famille.
        let mut high = candidate(0..10, civil(1));
        high.priority = priority_of(CitationKind::Ecli);
        let low = candidate(5..15, text_article(None, 2));
        assert_eq!(kinds(&run(vec![low, high])), [CitationKind::CodeArticle]);
    }
}
