//! Corpus de citations (annexe B §8, T-M6-03) : 185 positifs et 45 négatifs, lus tels quels dans
//! `tests/data/citations/`, sans aucune table de surcharge (§8.4). Le domaine n'a pas de `serde` :
//! le harnais projette chaque [`Spanned<CitationRef>`] vers un `serde_json::Value` aux clés du
//! corpus (§8.3), puis compare en sous-ensemble.
//!
//! Ce fichier est une cible de test d'intégration : il ne compte pas dans la couverture `--lib`
//! (06 §3). Les tests de couverture du parseur vivent dans les modules de `src/citation/`.
// Harnais de test : un corpus invalide ou une fixture illisible doit faire échouer le test avec un
// message, et la mesure de performance comme l'absence de fixture se signalent sur stderr.
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::print_stderr,
    reason = "harnais de test : échec bruyant sur corpus invalide ou indice hors corpus, diagnostic de mesure et de fixture absente sur stderr"
)]
#![expect(
    clippy::arithmetic_side_effects,
    reason = "tests : valeurs bornées par les données du test"
)]

use std::collections::{BTreeMap, BTreeSet};

use lf_domain::citation::dict::courts::{CaSeat, CassChamber, CeFormation};
use lf_domain::citation::{
    Ambiguity, Anaphora, ArticlePrefix, BulletinRef, CODES, CaseNumber, CitationRef, CodeRef,
    CodeVia, Confidence, ConventionKind, Court, CourtVia, DecisionCite, Flag, LebonRef,
    ParseOptions, Severity, Spanned, Subdivision, TextAnaphora, TextCite, TextContext,
    VersionQualifier, parse_with, unread_article_mentions,
};
use lf_domain::contract::Idcc;
use lf_domain::date::CivilDate;
use serde_json::{Map, Value, json};

const POSITIVES: &str = include_str!("data/citations/citations-corpus-positifs.jsonl");
const NEGATIVES: &str = include_str!("data/citations/citations-corpus-negatifs.jsonl");
const CODES_TABLE: &str = include_str!("data/citations/codes-table.md");

/// Cas construits de mémoire (annexe B §8.9, V-163 « non garantis ») : P065 à P068, P103, P104,
/// P128, P145 et N25. Valables comme tests de syntaxe, jamais comme tests live ni comme
/// référence de contenu juridique.
const FROM_MEMORY: [&str; 9] =
    ["P065", "P066", "P067", "P068", "P103", "P104", "P128", "P145", "N25"];

/// Un cas du corpus (annexe B §8.2).
struct Case {
    id: String,
    src: String,
    input: String,
    expect: Vec<Value>,
    verdict: Option<String>,
}

fn cases(jsonl: &str) -> Vec<Case> {
    jsonl
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let value: Value =
                serde_json::from_str(line).unwrap_or_else(|e| panic!("ligne JSONL invalide : {e}"));
            let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
            Case {
                id: text("id").expect("id"),
                src: text("src").expect("src"),
                input: text("in").expect("in"),
                expect: value.get("expect").and_then(Value::as_array).expect("expect").clone(),
                verdict: text("verdict"),
            }
        })
        .collect()
}

fn all_cases() -> Vec<Case> {
    cases(POSITIVES).into_iter().chain(cases(NEGATIVES)).collect()
}

/// Date de constitution du corpus (§8.3 règle 6).
fn corpus_today() -> Option<CivilDate> {
    CivilDate::new(2026, 10, 15)
}

/// Options d'un cas (§8.3 règle 6) : `repair_mojibake` et `default_text` selon les attentes.
fn options_for(case: &Case) -> ParseOptions {
    let has = |key: &str, wanted: &str| {
        case.expect.iter().any(|e| e.get(key).and_then(Value::as_str) == Some(wanted))
    };
    ParseOptions {
        repair_mojibake: has("requires", "repair_mojibake"),
        default_text: has("code_via", "contexte_document").then(TextContext::default),
        today: corpus_today(),
        ..ParseOptions::default()
    }
}

// ---------------------------------------------------------------------------------------------
// Projection (§8.3) : `Spanned<CitationRef>` vers les clés du corpus.
// ---------------------------------------------------------------------------------------------

fn opt<T: Into<Value>>(value: Option<T>) -> Value {
    value.map_or(Value::Null, Into::into)
}

/// Siège en majuscules sans accents (§8.3 : `seat`), comme la facette de l'API.
fn upper_ascii(name: &str) -> String {
    name.chars()
        .flat_map(char::to_uppercase)
        .map(|c| match c {
            'É' | 'È' | 'Ê' | 'Ë' => 'E',
            'À' | 'Â' | 'Ä' => 'A',
            'Î' | 'Ï' => 'I',
            'Ô' | 'Ö' => 'O',
            'Û' | 'Ü' | 'Ù' => 'U',
            'Ç' => 'C',
            other => other,
        })
        .collect()
}

fn iso(date: Option<CivilDate>) -> Value {
    date.map_or(Value::Null, |d| Value::String(d.to_string()))
}

fn anaphora_code(anaphora: TextAnaphora) -> &'static str {
    match anaphora {
        TextAnaphora::Susvisee => "susvisee",
        TextAnaphora::Precitee => "precitee",
        TextAnaphora::MentionneeCiDessus => "mentionnee_ci_dessus",
    }
}

fn text_summary(text: &TextCite) -> Value {
    let number = text.number.as_ref();
    json!({
        "nature": text.nature.code(),
        "num": opt(number.map(|n| n.raw.clone())),
        "year": opt(number.and_then(|n| n.year)),
        "seq": opt(number.map(|n| n.seq)),
        "date": iso(text.date),
        "anaphora": opt(text.anaphora.map(anaphora_code)),
    })
}

fn text_label(text: &TextCite) -> String {
    let raw = text.number.as_ref().map_or_else(String::new, |n| format!(" {}", n.raw));
    format!("{}{raw}", text.nature.code())
}

fn subdivisions(sub: &[Subdivision]) -> Value {
    Value::Array(
        sub.iter()
            .map(|s| match s {
                Subdivision::Paragraphe(p) => json!({"paragraphe": p}),
                Subdivision::Alinea(n) => json!({"alinea": n}),
                Subdivision::DernierAlinea => json!({"alinea": "dernier"}),
                Subdivision::Item(n) => json!({"item": n}),
                Subdivision::Lettre(c) => json!({"lettre": c.to_string()}),
                Subdivision::Phrase(n) => json!({"phrase": n}),
                Subdivision::DernierePhrase => json!({"phrase": "derniere"}),
                Subdivision::Section(n) => json!({"section": n}),
            })
            .collect(),
    )
}

fn version(qualifier: &VersionQualifier) -> Value {
    match qualifier {
        VersionQualifier::Ancien => json!({"ancien": true}),
        VersionQualifier::Nouveau => json!({"nouveau": true}),
        VersionQualifier::RedactionApplicableAuLitige => {
            json!({"redaction": "applicable_au_litige"})
        }
        VersionQualifier::RedactionApplicableDateDesFaits => {
            json!({"redaction": "applicable_date_des_faits"})
        }
        VersionQualifier::RedactionEnVigueur(date) => {
            json!({"redaction": "en_vigueur", "date": iso(*date)})
        }
        VersionQualifier::RedactionIssueDe(t) => {
            json!({"redaction": "issue_de", "texte": text_label(t)})
        }
        VersionQualifier::RedactionAnterieureA(t) => {
            json!({"redaction": "anterieure_a", "texte": text_label(t)})
        }
        VersionQualifier::EnVigueurAu(date) => json!({"en_vigueur_au": date.to_string()}),
    }
}

fn case_number(number: &CaseNumber) -> Value {
    match number {
        CaseNumber::Pourvoi(p) => json!({
            "type": "pourvoi", "value": p.to_string(), "key_letter": opt(p.key.map(String::from)),
            "yy": p.yy, "seq": p.seq,
        }),
        CaseNumber::Rg(rg) => {
            json!({"type": "rg", "value": rg.to_string(), "yy": rg.yy, "seq": rg.seq})
        }
        CaseNumber::Portalis(p) => json!({"type": "portalis", "value": p}),
        CaseNumber::CeRequete(n) => json!({"type": "ce_requete", "value": n.to_string()}),
        CaseNumber::Caa(c) => json!({
            "type": "caa", "value": c.to_string(), "yy": c.yy,
            "court_code": c.court.as_str(), "seq": c.seq,
        }),
        CaseNumber::Ta(t) => json!({"type": "ta", "value": t.to_string()}),
        CaseNumber::Cc(cc) => {
            let joined: Vec<u32> = if cc.joined.is_empty() {
                Vec::new()
            } else {
                std::iter::once(cc.seq).chain(cc.joined.iter().copied()).collect()
            };
            json!({
                "type": "cc", "value": cc.to_string(), "cc_type": cc.kind.as_str(),
                "joined": (!joined.is_empty()).then_some(joined),
            })
        }
        CaseNumber::Unknown(s) => json!({"type": "inconnu", "value": s}),
    }
}

fn publication(decision: &DecisionCite) -> Value {
    let Some(publication) = &decision.publication else {
        return Value::Null;
    };
    let mut out = Map::new();
    match &publication.bulletin {
        Some(BulletinRef::Publie) => {
            out.insert("bulletin".into(), json!(true));
        }
        Some(BulletinRef::Inedit) => {
            out.insert("bulletin".into(), json!("inedit"));
        }
        Some(BulletinRef::Reference(r)) => {
            out.insert("bulletin".into(), json!(r));
        }
        None => {}
    }
    match &publication.lebon {
        Some(LebonRef::Publie) => {
            out.insert("lebon".into(), json!(true));
        }
        Some(LebonRef::Tables) => {
            out.insert("lebon".into(), json!("tables"));
        }
        Some(LebonRef::Inedit) => {
            out.insert("lebon".into(), json!("inedit"));
        }
        Some(LebonRef::Page(p)) => {
            out.insert("lebon".into(), json!(p));
        }
        None => {}
    }
    Value::Object(out)
}

fn decision(d: &DecisionCite) -> Value {
    let (chamber, formation, chambers, seat) = match &d.court {
        Some(Court::CourDeCassation { chamber }) => {
            (opt(chamber.map(CassChamber::code)), Value::Null, Value::Null, Value::Null)
        }
        Some(Court::CourAppel { seat, chamber }) => {
            (opt(chamber.clone()), Value::Null, Value::Null, opt(seat.map(CaSeat::as_str)))
        }
        Some(Court::TribunalJudiciaire { seat }) => {
            (Value::Null, Value::Null, Value::Null, opt(seat.as_deref().map(upper_ascii)))
        }
        Some(Court::AutreJudiciaire { label }) => {
            (Value::Null, Value::Null, Value::Null, json!(label))
        }
        Some(Court::ConseilEtat { formation, chambers }) => {
            (Value::Null, opt(formation.map(CeFormation::code)), json!(chambers), Value::Null)
        }
        Some(Court::Caa { seat, chamber }) => (
            opt(chamber.clone()),
            Value::Null,
            Value::Null,
            opt(seat.map(|s| upper_ascii(s.as_str()))),
        ),
        Some(Court::Ta { seat, chamber }) => (
            opt(chamber.clone()),
            Value::Null,
            Value::Null,
            opt(seat.map(|s| upper_ascii(s.as_str()))),
        ),
        Some(Court::ConseilConstitutionnel) | None => {
            (Value::Null, Value::Null, Value::Null, Value::Null)
        }
    };
    json!({
        "court": opt(d.court.as_ref().map(Court::family_code)),
        "chamber": chamber,
        "formation": formation,
        "chambers": chambers,
        "seat": seat,
        "date": iso(d.date),
        "numbers": d.numbers.iter().map(case_number).collect::<Vec<_>>(),
        "publication": publication(d),
        "solution": opt(d.solution.clone()),
        "parties": opt(d.parties.clone()),
        "subject": opt(d.subject.clone()),
        "arret_num": opt(d.arret_num),
        "decision_kind": opt(d.kind.map(|k| match k {
            lf_domain::citation::DecisionKind::Arret => "ARRET",
            lf_domain::citation::DecisionKind::Avis => "AVIS",
            lf_domain::citation::DecisionKind::Ordonnance => "ORDONNANCE",
            lf_domain::citation::DecisionKind::Jugement => "JUGEMENT",
        })),
        "formation_code": opt(d.formation_code.clone()),
        "publication_codes": d.publication_codes.iter().map(char::to_string).collect::<Vec<_>>(),
        "court_via": opt(d.court_via.map(CourtVia::code)),
    })
}

/// Interprétations françaises d'un ECLI (annexe B §4.10, `Ecli::fr()`), calculées ici.
fn ecli(e: &lf_domain::ecli::Ecli) -> Value {
    let parts: Vec<&str> = e.as_str().split(':').collect();
    let (court, year, ordinal) = (parts[2], parts[3], parts[4]);
    let fr = if court == "CCASS" {
        let (prefix, digits) = ordinal.split_at(ordinal.len().min(2));
        json!({"cass_chamber": prefix, "arret_seq": digits.trim_start_matches('0').parse::<u32>().ok()})
    } else if let Some(formation) = CeFormation::from_ecli_court(court) {
        let (requete, date) = ordinal.split_once('.').unwrap_or((ordinal, ""));
        let date =
            (date.len() == 8).then(|| format!("{}-{}-{}", &date[..4], &date[4..6], &date[6..]));
        json!({"ce_formation": formation.code(), "ce_requete": requete, "date": date})
    } else if court == "CC" {
        let mut tokens = ordinal.split('.');
        let num = format!("{}-{}", tokens.next().unwrap_or(""), tokens.next().unwrap_or(""));
        json!({"cc_num": num, "cc_type": tokens.next()})
    } else {
        Value::Null
    };
    json!({
        "country": parts[1], "court": court, "year": year.parse::<u32>().ok(),
        "ordinal": ordinal, "fr": fr,
    })
}

/// ELI cité : `/eli/{type}/{AAAA}/{M}/{J}/{nor}/{version}/{partie}` ou `/eli/jo/AAAA/M/J/NNNN`.
fn eli(e: &lf_domain::contract::Eli) -> Value {
    let path = e.as_str().split_once("/eli/").map_or("", |(_, rest)| rest);
    let seg: Vec<&str> = path.split('/').collect();
    let date = (seg.len() >= 4)
        .then(|| {
            let n = |s: &str| s.parse::<u32>().ok();
            Some(format!("{:04}-{:02}-{:02}", n(seg[1])?, n(seg[2])?, n(seg[3])?))
        })
        .flatten();
    json!({
        "eli_type": seg.first(), "date": date,
        "nor": (seg.first() != Some(&"jo")).then(|| seg.get(4)).flatten(),
        "version": (seg.len() > 5).then(|| seg[5]),
        "part": (seg.len() > 6).then(|| seg[6]),
    })
}

fn nor(n: &lf_domain::contract::Nor) -> Value {
    let v = n.as_str();
    json!({
        "value": v, "ministere": &v[..3], "service": &v[3..4],
        "yy": v[4..6].parse::<u32>().ok(), "seq": v[6..11].parse::<u32>().ok(),
        "nature_letter": &v[11..],
    })
}

/// `Spanned<CitationRef>` vers les clés du corpus (§8.3).
fn project(c: &Spanned<CitationRef>) -> Value {
    let mut out = match &c.value {
        CitationRef::CodeArticle { code, article, version: v, code_via } => json!({
            "code": match code {
                CodeRef::Known(k) => json!(k.title()),
                CodeRef::Unknown(s) => json!(s),
                CodeRef::Anaphora(Anaphora::MemeCode | Anaphora::Dudit | Anaphora::PresentCode
                    | Anaphora::CeCode) | CodeRef::Missing => Value::Null,
            },
            "num": article.num.num_key(),
            "num_keys": article.num.key_variants(),
            "prefix": opt(article.num.prefix.map(ArticlePrefix::as_str)),
            "sub": subdivisions(&article.sub),
            "range": article.range_to.as_ref()
                .map(|to| json!([article.num.num_key(), to.num_key()])),
            "version": v.as_ref().map(version),
            "code_via": opt(code_via.map(CodeVia::code)),
        }),
        CitationRef::TextArticle { text, article, at_date } => json!({
            "num": (!article.num.num_key().is_empty()).then(|| article.num.num_key()),
            "sub": subdivisions(&article.sub),
            "range": article.range_to.as_ref()
                .map(|to| json!([article.num.num_key(), to.num_key()])),
            "text": text.as_ref().map(text_summary),
            "at_date": iso(*at_date),
        }),
        CitationRef::Text(t) => {
            let mut v = text_summary(t);
            v["title_hint"] = opt(t.title_hint.clone());
            v["appellation"] = opt(t.appellation.clone());
            v
        }
        CitationRef::Decision(d) => decision(d),
        CitationRef::Ecli(e) => ecli(e),
        CitationRef::CnilDeliberation(c) => json!({
            "series": opt(c.series.clone()), "year": c.year, "seq": c.seq,
            "num": lf_domain::citation::CnilNumber { series: c.series.clone(), year: c.year, seq: c.seq }
                .to_string(),
            "date": iso(c.date),
            "formation": c.formation_restreinte.then_some("formation restreinte"),
        }),
        CitationRef::Convention(c) => json!({
            "conv_kind": opt(c.kind.map(ConventionKind::code)), "idcc": opt(c.idcc.map(Idcc::get)),
            "brochure": opt(c.brochure), "avenant_num": opt(c.avenant),
            "title_hint": opt(c.title_hint.clone()), "date": iso(c.date),
        }),
        CitationRef::Jorf(j) => json!({
            "jo_num": opt(j.number), "date": iso(j.date), "texte_num": opt(j.texte),
            "page": opt(j.page),
        }),
        CitationRef::Nor(n) => nor(n),
        CitationRef::Eli(e) => eli(e),
        CitationRef::LegifranceId(l) => json!({
            "prefix": l.prefix.as_str(), "num": format!("{:012}", l.digits),
            "version_date": iso(l.version_start), "url_kind": opt(l.url_kind.clone()),
        }),
        CitationRef::Unsupported(u) => {
            json!({"family": u.family.code(), "key": opt(u.key.clone())})
        }
    };
    let object = out.as_object_mut().expect("objet");
    object.insert("kind".into(), json!(c.value.kind().pascal_case()));
    object.insert("confidence".into(), json!(c.confidence.as_str()));
    object.insert("ambiguity".into(), opt(c.ambiguity.as_ref().map(|a| a.code().into_owned())));
    object.insert("flags".into(), Value::Array(c.flags.iter().map(|f| json!(f.code())).collect()));
    out
}

// ---------------------------------------------------------------------------------------------
// Comparaison (§8.3).
// ---------------------------------------------------------------------------------------------

/// Clés de l'attente qui ne se comparent pas à une valeur projetée.
const META_KEYS: [&str; 8] =
    ["kind", "match", "enum_group", "resolve_hint", "ecli_expected", "requires", "src", "note"];

/// Comparaison de valeurs en sous-ensemble : seules les clés présentes dans l'attente comptent,
/// une valeur `null` attendue exige l'absence ou `None`.
fn subset(path: &str, expected: &Value, got: &Value) -> Result<(), String> {
    match (expected, got) {
        (Value::Object(want), Value::Object(have)) => {
            for (key, value) in want {
                let found = have.get(key).unwrap_or(&Value::Null);
                subset(&format!("{path}.{key}"), value, found)?;
            }
            Ok(())
        }
        (Value::Array(want), Value::Array(have)) => {
            if want.len() != have.len() {
                return Err(format!(
                    "{path} : {} éléments attendus, {} obtenus ({got})",
                    want.len(),
                    have.len()
                ));
            }
            want.iter()
                .zip(have)
                .enumerate()
                .try_for_each(|(i, (w, h))| subset(&format!("{path}[{i}]"), w, h))
        }
        (want, have) if want == have => Ok(()),
        (want, have) => Err(format!("{path} : attendu {want}, obtenu {have}")),
    }
}

/// Numéros de RG : les zéros de tête du second bloc ne sont pas gardés par le type (`14/00554`).
fn same_rg(expected: &Value, got: &Value) -> bool {
    let parse = |v: &Value| -> Option<(u32, u32)> {
        let (yy, seq) = v.as_str()?.split_once('/')?;
        Some((yy.parse().ok()?, seq.parse().ok()?))
    };
    parse(expected).is_some() && parse(expected) == parse(got)
}

/// Compare la projection `got` à l'attente `expected` (clés présentes seulement).
fn compare_fields(expected: &Value, got: &Value) -> Result<(), String> {
    let want = expected.as_object().expect("attente objet");
    let mut errors = Vec::new();
    for (key, value) in want {
        if META_KEYS.contains(&key.as_str()) {
            continue;
        }
        let have = got.get(key).unwrap_or(&Value::Null);
        let result = match key.as_str() {
            "flags" => {
                let set = |v: &Value| -> BTreeSet<String> {
                    v.as_array().into_iter().flatten().map(ToString::to_string).collect()
                };
                if set(value) == set(have) {
                    Ok(())
                } else {
                    Err(format!("flags : attendu {value}, obtenu {have}"))
                }
            }
            "numbers" => compare_numbers(value, have),
            _ => subset(key, value, have),
        };
        if let Err(e) = result {
            errors.push(e);
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(errors.join(" ; ")) }
}

fn compare_numbers(expected: &Value, got: &Value) -> Result<(), String> {
    let (Some(want), Some(have)) = (expected.as_array(), got.as_array()) else {
        return subset("numbers", expected, got);
    };
    if want.len() != have.len() {
        return Err(format!("numbers : {} attendus, {} obtenus ({got})", want.len(), have.len()));
    }
    for (i, (w, h)) in want.iter().zip(have).enumerate() {
        let is_rg = w.get("type").and_then(Value::as_str) == Some("rg");
        for (key, value) in w.as_object().into_iter().flatten() {
            let found = h.get(key).unwrap_or(&Value::Null);
            let ok = if is_rg && key == "value" { same_rg(value, found) } else { value == found };
            if !ok {
                return Err(format!("numbers[{i}].{key} : attendu {value}, obtenu {found}"));
            }
        }
    }
    Ok(())
}

/// Position `(début, fin)` de chaque attente dans `input`, en octets (§8.3 règle 2) : cherchée
/// depuis 0, ou depuis la fin du membre précédent pour un `enum_group`.
fn expected_spans(input: &str, expect: &[Value]) -> Result<Vec<(usize, usize)>, String> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut previous_group: Option<(u64, usize)> = None;
    for e in expect {
        let needle = e.get("match").and_then(Value::as_str).ok_or("attente sans `match`")?;
        let group = e.get("enum_group").and_then(Value::as_u64);
        let from = match (group, previous_group) {
            (Some(g), Some((pg, end))) if g == pg => end,
            _ => 0,
        };
        let start = input
            .get(from..)
            .and_then(|tail| tail.find(needle))
            .map(|i| i + from)
            .ok_or_else(|| format!("`match` introuvable dans `in` : {needle:?}"))?;
        let end = start + needle.len();
        previous_group = group.map(|g| (g, end));
        spans.push((start, end));
    }
    Ok(spans)
}

/// Règles 1 à 5 du §8.3 sur un cas.
fn check_case(
    input: &str,
    expect: &[Value],
    verdict: Option<&str>,
    got: &[Spanned<CitationRef>],
) -> Result<(), String> {
    let describe = || {
        got.iter()
            .map(|c| format!("{:?}@{}..{}", c.value.kind(), c.span.start, c.span.end))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if got.len() != expect.len() {
        return Err(format!(
            "{} citation(s) attendue(s), {} obtenue(s) : [{}]",
            expect.len(),
            got.len(),
            describe()
        ));
    }
    let spans = expected_spans(input, expect)?;
    let mut groups: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    for (i, ((e, c), (start, end))) in expect.iter().zip(got).zip(&spans).enumerate() {
        let kind = e.get("kind").and_then(Value::as_str).unwrap_or("?");
        let at = format!("attente {i} ({kind})");
        if c.value.kind().pascal_case() != kind {
            return Err(format!("{at} : famille {:?} obtenue", c.value.kind()));
        }
        if (c.span.start, c.span.end) != (*start, *end) {
            return Err(format!(
                "{at} : span attendu {start}..{end} ({:?}), obtenu {}..{} ({:?})",
                &input[*start..*end],
                c.span.start,
                c.span.end,
                input.get(c.span.start..c.span.end)
            ));
        }
        compare_fields(e, &project(c)).map_err(|m| format!("{at} : {m}"))?;
        if let Some(g) = e.get("enum_group").and_then(Value::as_u64) {
            groups.entry(g).or_default().push(i);
        }
        if e.get("flags").is_none()
            && verdict.is_none()
            && let Some(f) = c.flags.iter().find(|f| f.severity() >= Severity::Suspect)
        {
            return Err(format!("{at} : drapeau {} sur un positif", f.code()));
        }
        if e.get("confidence").is_none() && verdict.is_none() && c.confidence < Confidence::Medium {
            return Err(format!("{at} : confiance {} sur un positif", c.confidence.as_str()));
        }
    }
    // Règle 3 : partition en groupes d'énumération.
    let key = |c: &Spanned<CitationRef>| c.enum_group;
    for members in groups.values() {
        let first = key(&got[members[0]]);
        if first.is_none() || members.iter().any(|&i| key(&got[i]) != first) {
            return Err(format!("groupe d'énumération non respecté : attentes {members:?}"));
        }
    }
    let grouped: usize = groups.values().map(Vec::len).sum();
    let got_grouped = got.iter().filter(|c| c.enum_group.is_some()).count();
    if got_grouped != grouped {
        return Err(format!("{got_grouped} citation(s) groupée(s), {grouped} attendue(s)"));
    }
    // Règle 5 : verdicts négatifs.
    match verdict {
        Some("suspect")
            if !got.iter().any(|c| c.flags.iter().any(|f| f.severity() >= Severity::Suspect)) =>
        {
            Err("verdict `suspect` : aucun drapeau Suspect ou Impossible".into())
        }
        // §8.3 règle 5 : une ambiguïté ou une confiance `Low`. N29 (pourvoi seul) est `Medium`
        // sans ambiguïté, la juridiction étant inférée (`court_via`, §4.19) : cette inférence
        // est son ambiguïté.
        Some("ambigu") if !got.iter().any(is_ambiguous) => {
            Err("verdict `ambigu` : ni ambiguïté, ni confiance Low, ni juridiction inférée".into())
        }
        Some("aucune_citation" | "fragment") if !got.is_empty() => {
            Err("verdict sans citation : la sortie devrait être vide".into())
        }
        _ => Ok(()),
    }
}

/// Une citation « ambiguë » au sens du verdict `ambigu` (§8.3 règle 5).
fn is_ambiguous(c: &Spanned<CitationRef>) -> bool {
    let inferred = matches!(&c.value, CitationRef::Decision(d) if d.court_via.is_some());
    c.ambiguity.is_some() || c.confidence == Confidence::Low || inferred
}

fn run(case: &Case) -> Result<(), String> {
    let got = parse_with(&case.input, &options_for(case));
    check_case(&case.input, &case.expect, case.verdict.as_deref(), &got)
}

fn failures<'a>(selected: impl Iterator<Item = &'a Case>) -> Vec<String> {
    selected
        .filter_map(|case| run(case).err().map(|msg| format!("{} ({}) : {msg}", case.id, case.src)))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Tableaux de l'annexe B §4 et §5.3, par le pipeline complet (`parse_with`).
//
// Les grammaires (V2-10 à V2-12) sont éprouvées sur des ancres construites à la main ; ici, chaque
// ligne d'un tableau traverse la normalisation, les ancres, les grammaires et l'arbitrage, avec la
// même comparaison que le corpus (`check_case`).
// ---------------------------------------------------------------------------------------------

/// Une ligne de tableau : un texte, ses attentes et ses réglages.
struct Row {
    input: String,
    expect: Vec<Value>,
    options: ParseOptions,
}

fn base_options() -> ParseOptions {
    ParseOptions { today: corpus_today(), ..ParseOptions::default() }
}

/// Une citation attendue ; sans `match`, c'est tout le texte.
fn row(input: &str, mut expect: Value) -> Row {
    if expect.get("match").is_none() {
        expect["match"] = json!(input);
    }
    Row { input: input.to_owned(), expect: vec![expect], options: base_options() }
}

/// Plusieurs citations attendues, chacune avec son `match`.
fn rows_of(input: &str, expect: Vec<Value>) -> Row {
    Row { input: input.to_owned(), expect, options: base_options() }
}

/// Aucune citation attendue.
fn nothing(input: &str) -> Row {
    Row { input: input.to_owned(), expect: Vec::new(), options: base_options() }
}

impl Row {
    fn with(mut self, adjust: impl FnOnce(&mut ParseOptions)) -> Self {
        adjust(&mut self.options);
        self
    }
}

/// Fait passer toutes les lignes ; rapporte tous les échecs d'un coup.
fn run_table(name: &str, table: &[Row]) {
    let failed: Vec<String> = table
        .iter()
        .filter_map(|r| {
            let got = parse_with(&r.input, &r.options);
            check_case(&r.input, &r.expect, None, &got)
                .err()
                .map(|m| format!("{:?} : {m}", r.input))
        })
        .collect();
    assert!(
        failed.is_empty(),
        "{name} : {} ligne(s) en échec sur {} :\n{}",
        failed.len(),
        table.len(),
        failed.join("\n")
    );
}

/// Décision de la Cour de cassation attendue.
fn cass(chamber: &str, date: &str, pourvoi: &str) -> Value {
    json!({
        "kind": "Decision", "court": "CCASS", "chamber": chamber, "date": date,
        "numbers": [{"type": "pourvoi", "value": pourvoi}],
    })
}

#[test]
fn cass_chamber_variants_of_annex_b_4_5() {
    let (date, pourvoi) = ("2023-05-25", "21-23.174");
    // Formes « courtes » ou précédées de `Cass.` : même hôte, `, 25 mai 2023, n° 21-23.174`.
    let short: &[(&str, &str)] = &[
        ("1re Civ.", "CIV1"),
        ("Civ. 1re", "CIV1"),
        ("Cass. 1re civ.", "CIV1"),
        ("Cass. Civ. 1ère", "CIV1"),
        ("CIV. 1", "CIV1"),
        ("2e civ.", "CIV2"),
        ("Civ. 2e", "CIV2"),
        ("3e civ.", "CIV3"),
        ("Com.", "COM"),
        ("Cass. com.", "COM"),
        ("Soc.", "SOC"),
        ("Cass. soc.", "SOC"),
        ("Crim.", "CRIM"),
        ("Cass. crim.", "CRIM"),
        ("Ass. plén.", "AP"),
        ("Cass. ass. plén.", "AP"),
        ("Ch. mixte", "MIXTE"),
        ("Cass., ch. mixte", "MIXTE"),
    ];
    let mut table: Vec<Row> = short
        .iter()
        .map(|(variant, code)| {
            row(&format!("{variant}, 25 mai 2023, n° {pourvoi}"), cass(code, date, pourvoi))
        })
        .collect();
    // Formes en toutes lettres : `chambre, date, numéro` (P107).
    let long: &[(&str, &str)] = &[
        ("Première chambre civile", "CIV1"),
        ("Deuxième chambre civile", "CIV2"),
        ("Troisième chambre civile", "CIV3"),
        ("Chambre commerciale", "COM"),
        ("Chambre sociale", "SOC"),
        ("Chambre criminelle", "CRIM"),
        ("Assemblée plénière", "AP"),
        ("Chambre mixte", "MIXTE"),
    ];
    table.extend(long.iter().map(|(variant, code)| {
        row(&format!("{variant}, 4 juin 2025, 24-13.470"), cass(code, "2025-06-04", "24-13.470"))
    }));
    // Titres Légifrance : `Chambre civile N` (P091, P101).
    for (n, code) in [(1, "CIV1"), (2, "CIV2"), (3, "CIV3")] {
        table.push(row(
            &format!("Cour de cassation, civile, Chambre civile {n}, 4 juin 2025, 24-13.470"),
            cass(code, "2025-06-04", "24-13.470"),
        ));
    }
    run_table("§4.5 chambres de cassation", &table);
}

#[test]
fn cass_rare_chambers_and_kinds_of_annex_b_4_5() {
    let hosts = [
        // Chambres historiques ou rares du tableau (`Reunies`, `CivUnique`, `Requetes`).
        ("Cass. req., 25 mai 1923, n° 21-23.174", "REQUETES"),
        ("Cass. civ., 25 mai 1923, n° 21-23.174", "CIV_UNIQUE"),
        ("Ch. réunies, 25 mai 1923, n° 21-23.174", "REUNIES"),
    ];
    let table: Vec<Row> = hosts
        .iter()
        .map(|(text, code)| {
            row(text, json!({"kind": "Decision", "court": "CCASS", "chamber": code}))
        })
        .collect();
    run_table("§4.5 chambres rares", &table);
    // `avis` est une nature de décision, pas une chambre (P099).
    run_table(
        "§4.5 avis",
        &[row(
            "Cass. 2e civ., AVIS du 27 juin 2019, n° 19-70.006",
            json!({"kind": "Decision", "court": "CCASS", "chamber": "CIV2",
                   "decision_kind": "AVIS", "date": "2019-06-27"}),
        )],
    );
}

#[test]
fn cass_numbers_forms_of_annex_b_4_5() {
    let table = vec![
        // pourvoi `AA-NN.NNN` et forme sans point `AA-NNNNN` (TIERS justicelibre).
        row(
            "Crim., 28 mars 2018, pourvoi n° 17-82.138",
            json!({"kind": "Decision", "numbers": [{"type": "pourvoi", "value": "17-82.138"}]}),
        ),
        row(
            "Crim., 28 mars 2020, pourvoi n° 19-19311",
            json!({"kind": "Decision", "numbers": [{"type": "pourvoi", "value": "19-19.311"}]}),
        ),
        // lettre-clé qui précède le numéro.
        row(
            "Cass. 1re civ., 4 juin 2025, pourvoi n° N 24-13.470",
            json!({"kind": "Decision", "chamber": "CIV1",
                   "numbers": [{"type": "pourvoi", "value": "24-13.470", "key_letter": "N"}]}),
        ),
        // plusieurs pourvois joints.
        row(
            "Cass. soc., 12 févr. 2025, pourvois n° 23-12.345 et 23-12.346",
            json!({"kind": "Decision", "chamber": "SOC", "date": "2025-02-12", "numbers": [
                {"type": "pourvoi", "value": "23-12.345"},
                {"type": "pourvoi", "value": "23-12.346"}]}),
        ),
        // N39 : `B` refuse `21-23.1745`.
        nothing("pourvoi n° 21-23.1745"),
    ];
    run_table("§4.5 numéros de pourvoi", &table);
}

#[test]
fn ce_formation_variants_of_annex_b_4_7() {
    let ce = |formation: &str, chambers: Value| {
        json!({"kind": "Decision", "court": "CE", "formation": formation, "chambers": chambers,
               "date": "2026-05-29", "numbers": [{"type": "ce_requete", "value": "506507"}]})
    };
    let variants: &[(&str, &str, Value)] = &[
        ("ass.", "ASSEMBLEE", Value::Null),
        ("Ass.", "ASSEMBLEE", Value::Null),
        ("assemblée", "ASSEMBLEE", Value::Null),
        ("sect.", "SECTION", Value::Null),
        ("Sect.", "SECTION", Value::Null),
        ("section", "SECTION", Value::Null),
        ("2ème - 7ème chambres réunies", "CHR", json!([2, 7])),
        ("5e et 6e ch. réunies", "CHR", json!([5, 6])),
        ("CHR", "CHR", Value::Null),
        ("1re ch.", "CHS", json!([1])),
        ("CHS", "CHS", Value::Null),
        ("7 / 5 SSR", "SSR", json!([7, 5])),
        ("SSR", "SSR", Value::Null),
        ("SSJS", "SSJS", Value::Null),
        ("ord.", "ORDONNANCE", Value::Null),
        ("réf.", "ORDONNANCE", Value::Null),
        ("juge des référés", "ORDONNANCE", Value::Null),
    ];
    let mut table: Vec<Row> = variants
        .iter()
        .map(|(variant, code, chambers)| {
            let mut expected = ce(code, chambers.clone());
            if chambers.is_null() {
                expected.as_object_mut().map(|o| o.remove("chambers"));
            }
            row(&format!("CE, {variant}, 29/05/2026, n° 506507"), expected)
        })
        .collect();
    // Lecture de `Section du Contentieux` et de `chambre jugeant seule` en prose.
    table.push(row(
        "CE, Section du Contentieux, 29/05/2026, n° 506507",
        json!({"kind": "Decision", "court": "CE", "formation": "SECTION"}),
    ));
    table.push(row(
        "CE, 1re chambre jugeant seule, 29/05/2026, n° 506507",
        json!({"kind": "Decision", "court": "CE", "formation": "CHS"}),
    ));
    table.push(row(
        "CE, 7e et 5e sous-sections réunies, 28 décembre 2001, 216642",
        json!({"kind": "Decision", "court": "CE", "formation": "SSR"}),
    ));
    table.push(row(
        "CE, 1re sous-section jugeant seule, 28 décembre 2001, 216642",
        json!({"kind": "Decision", "court": "CE", "formation": "SSJS"}),
    ));
    run_table("§4.7 formations du Conseil d'État", &table);
}

#[test]
fn ce_court_and_publication_forms_of_annex_b_4_7() {
    let ce_decision = |extra: Value| {
        let mut base = json!({"kind": "Decision", "court": "CE", "date": "2023-04-14"});
        for (k, v) in extra.as_object().into_iter().flatten() {
            base[k] = v.clone();
        }
        base
    };
    let table = vec![
        row("CE, 14 avril 2023, n° 462797", ce_decision(json!({}))),
        row("C.E., 14 avril 2023, n° 462797", ce_decision(json!({}))),
        row("Conseil d'État, 14 avril 2023, n° 462797", ce_decision(json!({}))),
        row("Conseil d'Etat, 14 avril 2023, n° 462797", ce_decision(json!({}))),
        row("CE, 14 avril 2023, req. n° 462797", ce_decision(json!({}))),
        row("CE, 14 avril 2023, 462797", ce_decision(json!({}))),
        row(
            "CE, 14 avril 2023, n° 462797, mentionné aux tables du Recueil Lebon",
            ce_decision(json!({"publication": {"lebon": "tables"}})),
        ),
        row(
            "CE, Sect., 22 février 2007, req. n° 264541, Leb., p. 541",
            json!({"kind": "Decision", "court": "CE", "formation": "SECTION",
                   "publication": {"lebon": "p. 541"}}),
        ),
    ];
    run_table("§4.7 juridiction et Lebon", &table);
}

#[test]
fn caa_and_ta_forms_of_annex_b_4_8() {
    let mut table = Vec::new();
    // Les neuf codes de CAA, chacun avec son siège.
    let caa: &[(&str, &str, &str)] = &[
        ("PA", "Paris", "PARIS"),
        ("VE", "Versailles", "VERSAILLES"),
        ("MA", "Marseille", "MARSEILLE"),
        ("LY", "Lyon", "LYON"),
        ("BX", "Bordeaux", "BORDEAUX"),
        ("NT", "Nantes", "NANTES"),
        ("NC", "Nancy", "NANCY"),
        ("DA", "Douai", "DOUAI"),
        ("TL", "Toulouse", "TOULOUSE"),
    ];
    for (code, city, seat) in caa {
        table.push(row(
            &format!("CAA de {city}, 22 mai 2025, n° 24{code}02429"),
            json!({"kind": "Decision", "court": "CAA", "seat": seat, "date": "2025-05-22",
                   "numbers": [{"type": "caa", "value": format!("24{code}02429"),
                                "court_code": code, "yy": 24, "seq": 2429}]}),
        ));
    }
    for court in ["CAA de Nancy", "C.A.A. Nancy", "Cour administrative d'appel de Nancy"] {
        table.push(row(
            &format!("{court}, 22 mai 2025, n° 22NC02259"),
            json!({"kind": "Decision", "court": "CAA", "seat": "NANCY"}),
        ));
    }
    for court in ["TA Strasbourg", "T.A. Strasbourg", "Tribunal administratif de Strasbourg"] {
        table.push(row(
            &format!("{court}, 30 juin 2022, n° 2005064"),
            json!({"kind": "Decision", "court": "TA", "seat": "STRASBOURG",
                   "numbers": [{"type": "ta", "value": "2005064"}]}),
        ));
    }
    // Chambre brute gardée telle quelle (P111).
    table.push(row(
        "CAA de Nancy, 1re chambre, 22 mai 2025, n° 22NC02259",
        json!({"kind": "Decision", "court": "CAA", "chamber": "1re chambre"}),
    ));
    // Code incohérent avec le siège cité : drapeau Suspect.
    table.push(row(
        "CAA de Lyon, 22 mai 2025, n° 24MA02429",
        json!({"kind": "Decision", "court": "CAA", "seat": "LYON",
               "flags": ["caa_code_siege_incoherent"]}),
    ));
    run_table("§4.8 CAA et TA", &table);
}

#[test]
fn cc_types_of_annex_b_4_9() {
    let types = [
        "DC", "QPC", "PDR", "AN", "SEN", "D", "I", "OF", "ELEC", "LOM", "LP", "FNR", "REF", "RIP",
        "L", "ORGA", "AR16", "AUTR",
    ];
    let mut table: Vec<Row> = types
        .iter()
        .map(|t| {
            row(
                &format!("Cons. const., décision n° 2025-1173 {t} du 7 novembre 2025"),
                json!({"kind": "Decision", "court": "CC", "date": "2025-11-07",
                       "numbers": [{"type": "cc", "value": "2025-1173", "cc_type": t}]}),
            )
        })
        .collect();
    for court in ["Cons. const.", "CC", "C. const.", "Conseil constitutionnel"] {
        table.push(row(
            &format!("{court}, 10 juin 2009, n° 2009-580 DC"),
            json!({"kind": "Decision", "court": "CC", "date": "2009-06-10"}),
        ));
    }
    // Numéros joints, rectification, forme `type numéro`.
    table.push(row(
        "décision n° 2010-14/22 QPC du 30 juillet 2010",
        json!({"kind": "Decision", "court": "CC", "numbers": [
            {"type": "cc", "value": "2010-14/22", "cc_type": "QPC", "joined": [14, 22]}]}),
    ));
    table.push(row(
        "décision n° 2015-491R QPC du 14 octobre 2015",
        json!({"kind": "Decision", "court": "CC", "numbers": [
            {"type": "cc", "value": "2015-491R", "cc_type": "QPC"}]}),
    ));
    table.push(row(
        "QPC 2025-1173",
        json!({"kind": "Decision", "court": "CC",
               "numbers": [{"type": "cc", "value": "2025-1173", "cc_type": "QPC"}]}),
    ));
    run_table("§4.9 Conseil constitutionnel", &table);
}

#[test]
fn ecli_forms_of_annex_b_4_10() {
    let table = vec![
        row(
            "ECLI:FR:CCASS:2025:C100399",
            json!({"kind": "Ecli", "country": "FR", "court": "CCASS", "year": 2025,
                   "ordinal": "C100399", "fr": {"cass_chamber": "C1", "arret_seq": 399}}),
        ),
        row(
            "ECLI:FR:CECHR:2026:506507.20260529",
            json!({"kind": "Ecli", "fr": {"ce_formation": "CHR", "ce_requete": "506507",
                                          "date": "2026-05-29"}}),
        ),
        row(
            "ECLI:FR:CC:2025:2025.1173.QPC",
            json!({"kind": "Ecli", "fr": {"cc_num": "2025-1173", "cc_type": "QPC"}}),
        ),
        // Casse indifférente en entrée.
        row("ecli:fr:cc:2025:2025.1173.qpc", json!({"kind": "Ecli", "country": "FR"})),
        // Un autre pays est hors périmètre (P159).
        row("ECLI:EU:C:2024:216", json!({"kind": "Unsupported", "family": "ecli_etranger"})),
        // N10 : ECLI tronqué.
        nothing("ECLI:FR:CCASS:2025"),
        // N11 à N13 : drapeaux.
        row(
            "ECLI:FR:CE:2026:506507.20260529",
            json!({"kind": "Ecli", "flags": ["code_juridiction_CE_seul_jamais_utilise"]}),
        ),
        row(
            "ECLI:FR:CC:2025:123.QPC",
            json!({"kind": "Ecli", "flags": ["ordinal_cc_non_conforme"]}),
        ),
        row(
            "ECLI:FR:CECHR:2026:506507",
            json!({"kind": "Ecli", "flags": ["ordinal_ce_non_conforme"]}),
        ),
        row(
            "ECLI:FR:CCASS:2025:C1TOTO",
            json!({"kind": "Ecli", "flags": ["ordinal_cass_non_conforme"]}),
        ),
        row(
            "ECLI:FR:CCASS:2025:XX00399",
            json!({"kind": "Ecli", "flags": ["chambre_cass_inconnue:XX"]}),
        ),
        row("ECLI:FR:CCASS:2099:C100399", json!({"kind": "Ecli", "flags": ["annee_future"]})),
        row(
            "ECLI:FR:CC:2024:2025.1173.QPC",
            json!({"kind": "Ecli", "flags": ["ecli_annee_anterieure_numero"]}),
        ),
    ];
    run_table("§4.10 ECLI", &table);
}

#[test]
fn text_natures_of_annex_b_4_3() {
    let natures: &[(&str, &str)] = &[
        ("loi", "LOI"),
        ("loi organique", "LOI_ORGANIQUE"),
        ("loi constitutionnelle", "LOI_CONSTIT"),
        ("loi de finances", "LOI_FINANCES"),
        ("loi de finances rectificative", "LOI_FINANCES"),
        ("loi de finances pour 2016", "LOI_FINANCES"),
        ("loi de financement de la sécurité sociale", "LOI_FSS"),
        ("loi du pays", "LOI_DU_PAYS"),
        ("ordonnance", "ORDONNANCE"),
        ("ord.", "ORDONNANCE"),
        ("décret-loi", "DECRET_LOI"),
        ("décret", "DECRET"),
        ("décr.", "DECRET"),
        ("arrêté", "ARRETE"),
        ("arr.", "ARRETE"),
        ("règlement", "REGLEMENT"),
        ("circulaire", "CIRCULAIRE"),
        ("instruction", "INSTRUCTION"),
    ];
    let table: Vec<Row> = natures
        .iter()
        .map(|(spelled, code)| {
            row(
                &format!("{spelled} n° 2016-1321 du 7 octobre 2016"),
                json!({"kind": "Text", "nature": code, "num": "2016-1321", "year": 2016,
                       "seq": 1321, "date": "2016-10-07"}),
            )
        })
        .collect();
    run_table("§4.3 natures", &table);
}

#[test]
fn text_forms_of_annex_b_4_3() {
    let table = vec![
        // année sur deux chiffres : 1900 + AA.
        row(
            "loi n° 93-1419 du 31 décembre 1993",
            json!({"kind": "Text", "nature": "LOI", "num": "93-1419", "year": 1993, "seq": 1419}),
        ),
        // nature abrégée `L.` suivie de `n°` (règle 1) ; `D. n°`.
        row("L. n° 2016-1321 du 7 oct. 2016", json!({"kind": "Text", "nature": "LOI"})),
        row("D. n° 2016-1849 du 23 décembre 2016", json!({"kind": "Text", "nature": "DECRET"})),
        // formes à date seule (P058, P059, P060, P071).
        row(
            "loi du 20 novembre 2023",
            json!({"kind": "Text", "nature": "LOI", "num": null, "date": "2023-11-20"}),
        ),
        row(
            "arrêté du 12 février 2024",
            json!({"kind": "Text", "nature": "ARRETE", "num": null, "date": "2024-02-12"}),
        ),
        row(
            "règlement du 4 février 2010",
            json!({"kind": "Text", "nature": "REGLEMENT", "date": "2010-02-04"}),
        ),
        // anaphores de texte.
        row(
            "ordonnance du 7 novembre 1958 susvisée",
            json!({"kind": "Text", "nature": "ORDONNANCE", "anaphora": "susvisee"}),
        ),
        row(
            "loi du 20 novembre 2023 mentionnée ci-dessus",
            json!({"kind": "Text", "nature": "LOI", "anaphora": "mentionnee_ci_dessus"}),
        ),
        // hors span : le titre est capturé dans la valeur (P050).
        rows_of(
            "loi n° 2016-1321 du 7 octobre 2016 pour une République numérique",
            vec![json!({"kind": "Text", "match": "loi n° 2016-1321 du 7 octobre 2016",
                        "title_hint": "pour une République numérique"})],
        ),
        // dates en chiffres, ISO et calendrier républicain.
        row(
            "loi n° 2016-1321 du 07/10/2016",
            json!({"kind": "Text", "nature": "LOI", "date": "2016-10-07"}),
        ),
        row(
            "loi n° 2016-1321 du 2016-10-07",
            json!({"kind": "Text", "nature": "LOI", "date": "2016-10-07"}),
        ),
        // N06 : `AAAA-NNNN` sans nature ; N07 : année du numéro différente de celle de la date.
        nothing("pour la période 2016-1321"),
        row(
            "loi n° 2016-1321 du 7 octobre 2015",
            json!({"kind": "Text", "flags": ["annee_numero_differente_annee_date"]}),
        ),
        // N09 / N37 : date impossible, `date = null` et drapeau.
        row(
            "loi n° 2016-1321 du 31 février 2016",
            json!({"kind": "Text", "date": null, "flags": ["date_invalide"]}),
        ),
    ];
    run_table("§4.3 formes de textes", &table);
}

#[test]
fn constitutional_block_of_annex_b_4_4() {
    let table = vec![
        row(
            "l'article 61-1 de la Constitution",
            json!({"kind": "TextArticle", "match": "article 61-1 de la Constitution",
                   "num": "61-1", "text": {"nature": "CONSTITUTION"}}),
        ),
        row(
            "l'article 6 de la Déclaration de 1789",
            json!({"kind": "TextArticle", "match": "article 6 de la Déclaration de 1789",
                   "num": "6", "text": {"nature": "DDHC"}}),
        ),
        row(
            "article 6 DDHC",
            json!({"kind": "TextArticle", "num": "6", "text": {"nature": "DDHC"}}),
        ),
        row(
            "l'article 1er de la Charte de l'environnement",
            json!({"kind": "TextArticle", "match": "article 1er de la Charte de l'environnement",
                   "num": "1er", "text": {"nature": "CHARTE_ENVIRONNEMENT"}}),
        ),
    ];
    run_table("§4.4 bloc de constitutionnalité", &table);
}

#[test]
fn every_code_title_and_abbreviation_is_recognised() {
    let mut table = Vec::new();
    for entry in &CODES {
        let by_title = format!("article 12 du {}", entry.title);
        table
            .push(row(&by_title, json!({"kind": "CodeArticle", "code": entry.title, "num": "12"})));
        for (abbreviation, _) in entry.abbreviations {
            let text = format!("art. 12 {abbreviation}");
            table
                .push(row(&text, json!({"kind": "CodeArticle", "code": entry.title, "num": "12"})));
        }
    }
    run_table("§5 dictionnaire des codes", &table);
}

#[test]
fn code_article_forms_of_annex_b_4_2() {
    let ca = |code: Value, num: &str| json!({"kind": "CodeArticle", "code": code, "num": num});
    let table = vec![
        // clés canoniques : préfixe sans point ni espace.
        row(
            "article L. 761-1 du code de justice administrative",
            ca(json!("Code de justice administrative"), "L761-1"),
        ),
        row("article LO. 141 du code électoral", ca(json!("Code électoral"), "LO141")),
        row("article R* 111-1 du code civil", ca(json!("Code civil"), "R*111-1")),
        row(
            "article 39 octies du code général des impôts",
            ca(json!("Code général des impôts"), "39 octies"),
        ),
        row(
            "article 199 undecies B du code général des impôts",
            ca(json!("Code général des impôts"), "199 undecies B"),
        ),
        row(
            "article 238 bis du code général des impôts",
            ca(json!("Code général des impôts"), "238 bis"),
        ),
        row(
            "article 1er du code civil",
            json!({"kind": "CodeArticle", "code": "Code civil", "num": "1er",
                                                "num_keys": ["1er", "1"]}),
        ),
        row(
            "article R*111-1 du code civil",
            json!({"kind": "CodeArticle", "num": "R*111-1",
                                                    "num_keys": ["R*111-1", "R111-1"], "prefix": "R*"}),
        ),
        row(
            "article D. 12 du code civil",
            json!({"kind": "CodeArticle", "num": "D12", "prefix": "D"}),
        ),
        row(
            "article A. 12 du code civil",
            json!({"kind": "CodeArticle", "num": "A12", "prefix": "A"}),
        ),
        row(
            "article unique du code civil",
            json!({"kind": "CodeArticle", "num": "unique",
                                                   "num_keys": ["unique", "Unique"]}),
        ),
        row(
            "article liminaire du code de procédure pénale",
            json!({"kind": "CodeArticle", "num": "liminaire"}),
        ),
        // ordre des mots-clés.
        row("art. 9 C. civ.", ca(json!("Code civil"), "9")),
        row("Code civil, art. 9", ca(json!("Code civil"), "9")),
        row("arts 9 du code civil", ca(json!("Code civil"), "9")),
        // numéro préfixé suivi du code, sans mot-clé (P005, P020).
        row(
            "L. 761-1 du code de justice administrative",
            ca(json!("Code de justice administrative"), "L761-1"),
        ),
        // subdivisions rangées du plus grand niveau au plus petit.
        row(
            "première phrase du troisième alinéa de l'article L. 600-4-1 du code de l'urbanisme",
            json!({"kind": "CodeArticle", "num": "L600-4-1", "code": "Code de l'urbanisme",
                   "sub": [{"alinea": 3}, {"phrase": 1}]}),
        ),
        row(
            "3° du I de l'article L. 121-1 du code de l'urbanisme",
            json!({"kind": "CodeArticle", "num": "L121-1", "sub": [{"paragraphe": "I"}, {"item": 3}]}),
        ),
        row(
            "article L. 761-1, alinéa 2 du code de justice administrative",
            json!({"kind": "CodeArticle", "num": "L761-1", "sub": [{"alinea": 2}]}),
        ),
        // code absent du dictionnaire (code_generic) : titre gardé, confiance Medium.
        row(
            "article 12 du code des marchés publics",
            json!({"kind": "CodeArticle", "code": "code des marchés publics", "num": "12",
                   "confidence": "medium"}),
        ),
        // N40 : zonage, N36 : suffixe minuscule collé, tiret final.
        nothing("zone 1AU"),
        nothing("art. L. 1234-5a"),
        nothing("article 2270-"),
        // N01 : nombre nu.
        nothing("1 du code civil."),
    ];
    run_table("§4.2 articles de codes", &table);
}

#[test]
fn code_article_enumerations_ranges_and_versions_of_annex_b_4_2_and_4_15() {
    let table = vec![
        // énumération : une citation par numéro, un `enum_group` commun (P004).
        rows_of(
            "articles L. 104-1 et L. 104-2 du code de l'urbanisme",
            vec![
                json!({"kind": "CodeArticle", "match": "L. 104-1", "num": "L104-1", "enum_group": 1,
                       "code": "Code de l'urbanisme"}),
                json!({"kind": "CodeArticle", "match": "L. 104-2", "num": "L104-2", "enum_group": 1,
                       "code": "Code de l'urbanisme"}),
            ],
        ),
        rows_of(
            "articles L. 122-4, L. 122-14 et L. 122-14-3 du Code du travail",
            vec![
                json!({"kind": "CodeArticle", "match": "L. 122-4", "num": "L122-4", "enum_group": 1}),
                json!({"kind": "CodeArticle", "match": "L. 122-14", "num": "L122-14", "enum_group": 1}),
                json!({"kind": "CodeArticle", "match": "L. 122-14-3", "num": "L122-14-3", "enum_group": 1}),
            ],
        ),
        // plage : une citation avec `range_to` (P035).
        row(
            "articles 1100 à 1100-2 du code civil",
            json!({"kind": "CodeArticle", "num": "1100", "range": ["1100", "1100-2"]}),
        ),
        // qualificatifs de version.
        row(
            "article 2270-1 ancien du code civil",
            json!({"kind": "CodeArticle", "num": "2270-1", "version": {"ancien": true}}),
        ),
        row(
            "article 1240 du code civil dans sa rédaction applicable au litige",
            json!({"kind": "CodeArticle", "num": "1240", "version": {"redaction": "applicable_au_litige"}}),
        ),
        row(
            "article 1240 du code civil, en vigueur au 1er janvier 2018",
            json!({"kind": "CodeArticle", "num": "1240"}),
        ),
        // anaphores de code (P006, P030) : le code de la dernière citation de code.
        rows_of(
            "l'article L. 104-7 du code de l'urbanisme puis l'article R. 104-26 du même code",
            vec![
                json!({"kind": "CodeArticle", "match": "article L. 104-7 du code de l'urbanisme",
                       "code": "Code de l'urbanisme"}),
                json!({"kind": "CodeArticle", "match": "article R. 104-26 du même code",
                       "code": "Code de l'urbanisme", "code_via": "anaphore:meme_code"}),
            ],
        ),
        rows_of(
            "l'article 1240 du code civil et l'article 1241 dudit code",
            vec![
                json!({"kind": "CodeArticle", "match": "article 1240 du code civil"}),
                json!({"kind": "CodeArticle", "match": "article 1241 dudit code",
                       "code": "Code civil", "code_via": "anaphore:dudit"}),
            ],
        ),
        // N03, N26 : un mot-clé sans code ni texte.
        row(
            "conformément à l'article 12",
            json!({"kind": "CodeArticle", "match": "article 12", "code": null,
                   "ambiguity": "texte_manquant"}),
        ),
        // N02 : numéro préfixé sans code, avec tiret.
        row(
            "Le préavis est fixé par L. 1234-5.",
            json!({"kind": "CodeArticle", "match": "L. 1234-5", "num": "L1234-5",
                   "ambiguity": "code_manquant"}),
        ),
    ];
    run_table("§4.2 et §4.15 énumérations, plages, versions, anaphores", &table);
}

#[test]
fn ambiguous_sigles_of_annex_b_5_3() {
    let low = |code: Value, sigle: &str| {
        let mut e = json!({"kind": "CodeArticle", "code": code, "confidence": "low"});
        if !sigle.is_empty() {
            e["ambiguity"] = json!(format!("sigle_ambigu:{sigle}"));
        }
        e
    };
    let table = vec![
        // CE : jamais résolu en code sans contexte (N04).
        row(
            "art. L. 52-8 CE",
            json!({"kind": "CodeArticle", "code": null, "ambiguity": "sigle_ambigu:CE",
                   "confidence": "low"}),
        ),
        // CC : Code civil seulement dans `art. N CC`, confiance Low.
        row("art. 1240 CC", low(json!("Code civil"), "")),
        row("article 121-3 CP", low(json!("Code pénal"), "CP")),
        row("article L. 1221-1 CT", low(json!("Code du travail"), "")),
        row("article L. 600-4-1 CU", low(json!("Code de l'urbanisme"), "")),
        row("article L. 111-1 CPI", low(json!("Code de la propriété intellectuelle"), "")),
        row("article L. 1111-1 CCP", low(json!("Code de la commande publique"), "")),
        // CPCE : deux codes, aucun retenu.
        row(
            "article L. 32 CPCE",
            json!({"kind": "CodeArticle", "code": null, "ambiguity": "sigle_ambigu:CPCE",
                   "confidence": "low"}),
        ),
        // `CE` juridiction : décision du Conseil d'État, pas un code.
        row("CE, 14 avril 2023, n° 462797", json!({"kind": "Decision", "court": "CE"})),
        // `CC` juridiction : Conseil constitutionnel.
        row("CC, déc. n° 2009-580 DC du 10 juin 2009", json!({"kind": "Decision", "court": "CC"})),
    ];
    run_table("§5.3 sigles ambigus", &table);
}

#[test]
fn trial_court_forms_of_annex_b_4_6() {
    let table = vec![
        row(
            "Cour d'appel de Paris, Pôle 1 - Chambre 12, 23 juin 2025, n°24/15215",
            json!({"kind": "Decision", "court": "CA", "seat": "PARIS", "chamber": "Pôle 1 - Chambre 12",
                   "numbers": [{"type": "rg", "value": "24/15215"}]}),
        ),
        row(
            "CA Paris, pôle 5, ch. 9, 4 avr. 2024, n° 23/13690",
            json!({"kind": "Decision", "court": "CA", "chamber": "pôle 5, ch. 9", "date": "2024-04-04"}),
        ),
        row(
            "C.A. Versailles, 4 juin 2020, n° 17/04940",
            json!({"kind": "Decision", "court": "CA", "seat": "VERSAILLES"}),
        ),
        // Siège écrit en toutes lettres : l'alias est ramené à la valeur de la facette.
        row(
            "Cour d'appel d'Aix-en-Provence, 4 juin 2020, n° 17/04940",
            json!({"kind": "Decision", "court": "CA", "seat": "AIX-PROVENCE"}),
        ),
        row(
            "TJ Lille, 2026-04-30, n° 25/00497",
            json!({"kind": "Decision", "court": "TJ", "seat": "LILLE", "date": "2026-04-30",
                   "numbers": [{"type": "rg", "value": "25/00497"}]}),
        ),
        row(
            "Tribunal judiciaire de Lille, 30 avril 2026, n° 25/00497",
            json!({"kind": "Decision", "court": "TJ", "seat": "LILLE"}),
        ),
        // en-tête d'arrêt (P114) : ni juridiction ni date, deux numéros.
        row(
            "N° RG 23/13690 - N° Portalis 35L7-V-B7H-CIDHB",
            json!({"kind": "Decision", "court": null, "numbers": [
                {"type": "rg", "value": "23/13690"},
                {"type": "portalis", "value": "35L7-V-B7H-CIDHB"}]}),
        ),
        // N30 : une mention sans date ni numéro n'est pas une citation.
        nothing("le TA Paris a été saisi"),
    ];
    run_table("§4.6 juridictions du fond", &table);
}

#[test]
fn cnil_forms_of_annex_b_4_11() {
    let cnil = |series: Value, year: u16, seq: u16| json!({"kind": "CnilDeliberation", "series": series, "year": year, "seq": seq});
    let table = vec![
        row("Délibération SAN-2026-002 du 8 janvier 2026", cnil(json!("SAN"), 2026, 2)),
        row("CNIL, délibération n°2023-062 du 13 avril 2023", cnil(json!(null), 2023, 62)),
        row("CNIL, décision n° MED-2018-024", cnil(json!("MED"), 2018, 24)),
        row("CNIL, mise en demeure n° MED-2018-024", cnil(json!("MED"), 2018, 24)),
        row("délib. CNIL n° 2024-012 du 1er févr. 2024", cnil(json!(null), 2024, 12)),
        // tiret demi-cadratin replié (P172).
        row("Délibération n° SAN–2026-003 du 22 janvier 2026", cnil(json!("SAN"), 2026, 3)),
        row(
            "Délibération de la formation restreinte n° SAN-2026-002 du 8 janvier 2026",
            json!({"kind": "CnilDeliberation", "formation": "formation restreinte"}),
        ),
        // N16 : sans CNIL ni série, `Low` et `emetteur_inconnu`.
        row(
            "Délibération n° 2023-062 du 13 avril 2023 du conseil municipal",
            json!({"kind": "CnilDeliberation", "match": "Délibération n° 2023-062 du 13 avril 2023",
                   "confidence": "low", "ambiguity": "emetteur_inconnu"}),
        ),
        // N15 : numéro incomplet.
        nothing("Délibération SAN-2026 du 8 janvier 2026"),
    ];
    run_table("§4.11 délibérations de la CNIL", &table);
}

#[test]
fn convention_forms_of_annex_b_4_12() {
    let table = vec![
        row(
            "Convention collective nationale de l'édition du 14 janvier 2000",
            json!({"kind": "Convention", "conv_kind": "CCN", "date": "2000-01-14"}),
        ),
        row(
            "CCN Syntec (IDCC 1486, brochure n° 3018)",
            json!({"kind": "Convention", "conv_kind": "CCN", "title_hint": "Syntec",
                   "idcc": 1486, "brochure": 3018}),
        ),
        row("IDCC n° 1486", json!({"kind": "Convention", "idcc": 1486})),
        row("IDCC 275", json!({"kind": "Convention", "idcc": 275})),
        row(
            "Avenant n° 18 du 19 juillet 2002",
            json!({"kind": "Convention", "conv_kind": "AVENANT", "avenant_num": 18,
                   "date": "2002-07-19", "ambiguity": "convention_inconnue"}),
        ),
        // N17 : avenant sans convention identifiée.
        row(
            "avenant n° 18",
            json!({"kind": "Convention", "conv_kind": "AVENANT", "avenant_num": 18,
                   "ambiguity": "convention_inconnue"}),
        ),
    ];
    run_table("§4.12 conventions collectives", &table);
}

#[test]
fn official_journal_nor_eli_and_ids_of_annex_b_4_13() {
    let mut table = vec![
        row(
            "JORF n°0263 du 8 novembre 2025, texte n° 152",
            json!({"kind": "Jorf", "jo_num": 263, "date": "2025-11-08", "texte_num": 152}),
        ),
        row(
            "Journal officiel de la République française n° 0235 du 8 octobre 2016",
            json!({"kind": "Jorf", "jo_num": 235, "date": "2016-10-08"}),
        ),
        row("J.O. n° 0235 du 8 octobre 2016", json!({"kind": "Jorf", "jo_num": 235})),
        row(
            "NOR : ECFI1524250L",
            json!({"kind": "Nor", "match": "ECFI1524250L", "value": "ECFI1524250L",
                   "ministere": "ECF", "service": "I", "yy": 15, "seq": 24250,
                   "nature_letter": "L"}),
        ),
        row(
            "NOR: cscx2531180s",
            json!({"kind": "Nor", "match": "cscx2531180s", "value": "CSCX2531180S"}),
        ),
        row("AFSA1629022D", json!({"kind": "Nor", "value": "AFSA1629022D"})),
        // N22 : NOR de onze caractères.
        nothing("NOR ECFI152425L"),
        row(
            "/eli/decret/2021/7/13/PRMD2117108D/jo/article_1",
            json!({"kind": "Eli", "eli_type": "decret", "date": "2021-07-13",
                   "nor": "PRMD2117108D", "version": "jo", "part": "article_1"}),
        ),
        row(
            "https://www.legifrance.gouv.fr/eli/decret/2018/2/13/JUSC1732516D/jo/texte",
            json!({"kind": "Eli", "eli_type": "decret", "part": "texte"}),
        ),
        row(
            "/eli/jo/2019/1/25/0021",
            json!({"kind": "Eli", "eli_type": "jo", "date": "2019-01-25"}),
        ),
        row(
            "https://www.legifrance.gouv.fr/juri/id/JURITEXT000036780011",
            json!({"kind": "LegifranceId", "match": "JURITEXT000036780011", "prefix": "JURITEXT",
                   "num": "000036780011", "url_kind": "juri"}),
        ),
        row(
            "LEGITEXT000033205014_08-09-2023",
            json!({"kind": "LegifranceId", "prefix": "LEGITEXT", "version_date": "2023-09-08"}),
        ),
        // N23 : identifiant de onze chiffres.
        nothing("LEGIARTI00000641928"),
    ];
    // Les seize préfixes d'identifiant.
    for prefix in [
        "LEGITEXT", "LEGIARTI", "LEGISCTA", "JORFTEXT", "JORFARTI", "JORFCONT", "JORFDOLE",
        "JURITEXT", "CETATEXT", "CONSTEXT", "CNILTEXT", "KALITEXT", "KALIARTI", "KALICONT",
        "KALISCTA", "ACCOTEXT",
    ] {
        table.push(row(
            &format!("{prefix}000006419288"),
            json!({"kind": "LegifranceId", "prefix": prefix, "num": "000006419288"}),
        ));
    }
    run_table("§4.13 JORF, NOR, ELI, identifiants", &table);
}

#[test]
fn sinks_of_annex_b_4_14() {
    let unsupported = |family: &str| json!({"kind": "Unsupported", "family": family});
    let table = vec![
        row("directive 85/374/CEE du Conseil du 25 juillet 1985", unsupported("UE")),
        row("Règlement (UE) 2016/679", unsupported("UE")),
        row("CJUE, 4 octobre 2024, C-740/22", unsupported("CJUE")),
        row("CEDH, 4 octobre 2024, req. n° 12345/20", unsupported("CEDH")),
        // N33 : le Recueil Dalloz n'est pas un article.
        row("D. 2016, p. 1234", unsupported("doctrine")),
        row("JCP G 2016, 1234", unsupported("doctrine")),
        row("ECLI:EU:C:2024:216", unsupported("ecli_etranger")),
        // Un article cité d'une directive est englobé dans le puits.
        row(
            "Article 10 de la directive 85/374/CEE du Conseil du 25 juillet 1985",
            unsupported("UE"),
        ),
    ];
    run_table("§4.14 puits", &table);
}

#[test]
fn disambiguation_rules_of_annex_b_4_17() {
    let table = vec![
        // L./D./Ord. + n° = nature de texte (P069 contre P020).
        row("L. n° 2016-1321 du 7 oct. 2016", json!({"kind": "Text", "nature": "LOI"})),
        row(
            "L. 1234-5 du code du travail",
            json!({"kind": "CodeArticle", "code": "Code du travail", "num": "L1234-5"}),
        ),
        // D. + année + p. = Dalloz (N33).
        row(
            "note sous D. 2016, p. 1234",
            json!({"kind": "Unsupported", "match": "D. 2016, p. 1234",
                                                 "family": "doctrine"}),
        ),
        // N01, N36, N40.
        nothing("1 du code civil."),
        nothing("art. L. 1234-5a"),
        nothing("article 2270-"),
        nothing("zone 1AU"),
        // N04, N05.
        row(
            "art. L. 52-8 CE",
            json!({"kind": "CodeArticle", "code": null,
                                      "ambiguity": "sigle_ambigu:CE", "confidence": "low"}),
        ),
        row(
            "article 121-3 CP",
            json!({"kind": "CodeArticle", "code": "Code pénal",
                                       "confidence": "low", "ambiguity": "sigle_ambigu:CP"}),
        ),
        // N18, N19.
        nothing("la Soc. Générale"),
        nothing("Com. de communes"),
        // N08.
        row(
            "Cass. civ. 4e, 5 mai 2021, n° 19-12.345",
            json!({"kind": "Decision", "court": "CCASS", "chamber": null,
                   "flags": ["chambre_inexistante:civ4"]}),
        ),
        // N29, N39, N28, N21.
        row(
            "n° 21-23.174",
            json!({"kind": "Decision", "court": "CCASS", "confidence": "medium",
                                   "court_via": "forme_du_numero"}),
        ),
        nothing("pourvoi n° 21-23.1745"),
        row(
            "sous le n° 506507",
            json!({"kind": "Decision", "match": "n° 506507", "court": null, "confidence": "low",
                   "ambiguity": "juridiction_manquante"}),
        ),
        row(
            "Décision 2025-1173",
            json!({"kind": "Decision", "court": null,
                   "ambiguity": "juridiction_et_type_manquants"}),
        ),
        // N06 et P056 : des nombres qui ne sont pas des numéros.
        nothing("pour la période 2016-1321"),
        // N16, N27, N31, N35.
        row(
            "l'article 6 de la Convention",
            json!({"kind": "TextArticle", "match": "article 6 de la Convention", "text": null,
                   "ambiguity": "convention_non_identifiee"}),
        ),
        row(
            "Bull. civ. I, n° 270",
            json!({"kind": "Decision", "court": "CCASS",
                   "ambiguity": "reference_de_publication_seule", "confidence": "low"}),
        ),
        row(
            "la loi Macron",
            json!({"kind": "Text", "match": "loi Macron", "nature": "LOI", "appellation": "Macron",
                   "ambiguity": "appellation", "confidence": "low"}),
        ),
        // N10, N22, N23.
        nothing("ECLI:FR:CCASS:2025"),
        nothing("NOR ECFI152425L"),
        nothing("LEGIARTI00000641928"),
        // N38, N40, N42, N30.
        nothing("2ème - 7ème chambres réunies"),
        nothing("zone 1AU"),
        nothing("8.4 de la convention)"),
        nothing("le TA Paris a été saisi"),
    ];
    run_table("§4.17 règles de désambiguïsation", &table);
}

#[test]
fn flags_of_annex_b_4_19() {
    let flagged = |kind: &str, flag: &str| json!({"kind": kind, "flags": [flag]});
    let table = vec![
        row(
            "Cass. civ. 1re, 31 février 2023, n° 21-23.174",
            json!({"kind": "Decision", "date": null, "flags": ["date_invalide"]}),
        ),
        row("ECLI:FR:CCASS:2099:C100399", flagged("Ecli", "annee_future")),
        row(
            "Cass. civ. 4e, 5 mai 2021, n° 19-12.345",
            flagged("Decision", "chambre_inexistante:civ4"),
        ),
        row("ECLI:FR:CCASS:2025:XX00399", flagged("Ecli", "chambre_cass_inconnue:XX")),
        row("ECLI:FR:CC:2025:123.QPC", flagged("Ecli", "ordinal_cc_non_conforme")),
        row("ECLI:FR:CECHR:2026:506507", flagged("Ecli", "ordinal_ce_non_conforme")),
        row("ECLI:FR:CCASS:2025:C1TOTO", flagged("Ecli", "ordinal_cass_non_conforme")),
        row("ECLI:FR:CC:2024:2025.1173.QPC", flagged("Ecli", "ecli_annee_anterieure_numero")),
        row(
            "Cass. civ. 1re, 5 mai 2010, n° 21-23.174",
            flagged("Decision", "annee_pourvoi_posterieure"),
        ),
        row(
            "ECLI:FR:CE:2026:506507.20260529",
            flagged("Ecli", "code_juridiction_CE_seul_jamais_utilise"),
        ),
        row(
            "loi n° 2016-1321 du 7 octobre 2015",
            flagged("Text", "annee_numero_differente_annee_date"),
        ),
        row("loi du 7 octobre 1700", flagged("Text", "annee_improbable")),
        row(
            "CAA de Lyon, 22 mai 2025, n° 24MA02429",
            flagged("Decision", "caa_code_siege_incoherent"),
        ),
    ];
    run_table("§4.19 drapeaux", &table);
    // Sévérités annoncées par le tableau.
    let severity = |text: &str| {
        let cites = parse_with(text, &base_options());
        cites.iter().flat_map(|c| c.flags.iter().map(Flag::severity)).max()
    };
    assert_eq!(severity("ECLI:FR:CCASS:2099:C100399"), Some(Severity::Impossible));
    assert_eq!(severity("ECLI:FR:CE:2026:506507.20260529"), Some(Severity::Suspect));
    assert_eq!(severity("loi du 7 octobre 1700"), Some(Severity::Suspect));
}

#[test]
fn republican_calendar_and_mojibake_flags_of_annex_b_4_19() {
    let republican = parse_with("loi du 12 prairial an III", &base_options());
    assert_eq!(republican.len(), 1, "{republican:?}");
    assert!(republican[0].flags.contains(&Flag::DateRepublicaine), "{republican:?}");
    // Mojibake : le drapeau n'existe qu'avec la réparation, et sur toutes les citations (P077).
    let broken = "LOI n\u{c2}\u{b0} 2016-1321 du 7 octobre 2016 et article 1241 du code civil";
    let repaired = parse_with(broken, &ParseOptions { repair_mojibake: true, ..base_options() });
    assert_eq!(repaired.len(), 2, "{repaired:?}");
    assert!(repaired.iter().all(|c| c.flags.contains(&Flag::MojibakeRepare)), "{repaired:?}");
    let plain = parse_with(broken, &base_options());
    assert!(plain.iter().all(|c| !c.flags.contains(&Flag::MojibakeRepare)));
}

#[test]
fn document_context_and_html_options_of_annex_b_6_1() {
    // Contexte de document : `code_implicite` (P009) ; sans contexte, `texte_manquant`.
    let context = rows_of(
        "articles 141-2 et 141-4",
        vec![
            json!({"kind": "CodeArticle", "match": "141-2", "enum_group": 1,
                   "code_via": "contexte_document", "ambiguity": "code_implicite"}),
            json!({"kind": "CodeArticle", "match": "141-4", "enum_group": 1,
                   "code_via": "contexte_document", "ambiguity": "code_implicite"}),
        ],
    )
    .with(|o| o.default_text = Some(TextContext::default()));
    let without = rows_of(
        "articles 141-2 et 141-4",
        vec![
            json!({"kind": "CodeArticle", "match": "141-2", "enum_group": 1, "code": null,
                   "ambiguity": "texte_manquant"}),
            json!({"kind": "CodeArticle", "match": "141-4", "enum_group": 1, "code": null,
                   "ambiguity": "texte_manquant"}),
        ],
    );
    run_table("§6.1 contexte de document", &[context, without]);
    // HTML : les balises disparaissent, les spans restent ceux du texte reçu.
    let html = "<p>vu l'<b>article</b> 1240 du code civil</p>";
    let stripped = parse_with(html, &ParseOptions { strip_html: true, ..base_options() });
    assert_eq!(stripped.len(), 1, "{stripped:?}");
    let span = stripped[0].span;
    assert!(html.get(span.start..span.end).is_some_and(|s| s.contains("1240")), "{span:?}");
    // Le garde-fou `max_citations` tronque la sortie aux premières citations.
    let many = "article 1 du code civil. ".repeat(10);
    let capped = parse_with(&many, &ParseOptions { max_citations: 3, ..base_options() });
    assert_eq!(capped.len(), 3);
    let all = parse_with(&many, &base_options());
    assert_eq!(all.len(), 10);
    assert_eq!(&all[..3], &capped[..]);
}

// ---------------------------------------------------------------------------------------------
// Extractions de référence (annexe B §8.8) : le `texte` de deux décisions réelles de la
// référence, figé citation par citation (type, span en octets, extrait, confiance, ambiguïté,
// drapeaux, groupe). Les fixtures vivent dans `lf-adapter-piste` (06 §2.1) : sans elles, le test
// s'arrête avec un message, comme `golden_copy_matches_plan_data`.
// ---------------------------------------------------------------------------------------------

const CETAT_GOLDEN: &str = include_str!("data/citations/extraction-consult-juri-cetat.jsonl");
const CONSTIT_GOLDEN: &str = include_str!("data/citations/extraction-consult-juri-constit.jsonl");

/// `text.texte` d'une fixture de la référence, ou `None` si le dossier est absent.
fn fixture_text(name: &str) -> Option<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../lf-adapter-piste/tests/fixtures/reference")
        .join(name);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        eprintln!("fixture absente ({}) : extraction ignorée (paquet publié)", path.display());
        return None;
    };
    let json: Value = serde_json::from_str(&raw).expect("fixture JSON");
    Some(json["text"]["texte"].as_str().expect("text.texte").to_owned())
}

/// Une ligne JSON par citation, comme le fichier de référence.
fn extraction(text: &str) -> Vec<String> {
    parse_with(text, &base_options())
        .iter()
        .map(|c| {
            json!({
                "kind": c.value.kind().pascal_case(),
                "start": c.span.start,
                "end": c.span.end,
                "match": &text[c.span.start..c.span.end],
                "confidence": c.confidence.as_str(),
                "ambiguity": c.ambiguity.as_ref().map(|a| a.code().into_owned()),
                "flags": c.flags.iter().map(|f| f.code().into_owned()).collect::<Vec<_>>(),
                "enum_group": c.enum_group,
            })
            .to_string()
        })
        .collect()
}

fn golden_lines(golden: &str) -> Vec<String> {
    golden.lines().map(str::to_owned).collect()
}

#[test]
fn extraction_of_consult_juri_cetat_matches_golden() {
    let Some(text) = fixture_text("consult_juri_cetat.json") else { return };
    let got = extraction(&text);
    assert!(got.len() >= 15, "une quinzaine de citations au moins : {}", got.len());
    pretty_assertions::assert_eq!(got, golden_lines(CETAT_GOLDEN));
}

#[test]
fn extraction_of_consult_juri_constit_matches_golden() {
    let Some(text) = fixture_text("consult_juri_constit.json") else { return };
    let got = extraction(&text);
    pretty_assertions::assert_eq!(got, golden_lines(CONSTIT_GOLDEN));
}

// ---------------------------------------------------------------------------------------------
// Non-régression des pièges (annexe B §8.8).
// ---------------------------------------------------------------------------------------------

#[test]
fn non_regression_pitfalls_of_annex_b_8_8() {
    let table = vec![
        // U+00A0 et U+202F dans un numéro et une date (P042, P078).
        row(
            "article\u{a0}L.\u{202f}761-1 du code de justice administrative",
            json!({"kind": "CodeArticle", "num": "L761-1", "code": "Code de justice administrative"}),
        ),
        row(
            "loi\u{a0}n°\u{202f}2016-1321 du 7\u{a0}octobre\u{202f}2016",
            json!({"kind": "Text", "nature": "LOI", "num": "2016-1321", "date": "2016-10-07"}),
        ),
        // U+2011 et U+2013 dans les numéros (P076, P172).
        row(
            "loi n° 2016\u{2011}1321 du 7 octobre 2016",
            json!({"kind": "Text", "num": "2016-1321"}),
        ),
        row(
            "Délibération n° SAN\u{2013}2026-003 du 22 janvier 2026",
            json!({"kind": "CnilDeliberation", "series": "SAN", "year": 2026, "seq": 3}),
        ),
        // `2018-2022` dans un titre n'est pas un numéro (P056).
        rows_of(
            "loi n° 2019-222 du 23 mars 2019 de programmation 2018-2022 et de réforme pour la justice",
            vec![json!({"kind": "Text", "match": "loi n° 2019-222 du 23 mars 2019",
                        "num": "2019-222",
                        "title_hint": "de programmation 2018-2022 et de réforme pour la justice"})],
        ),
        // `D. 2016, p.` est le Recueil Dalloz (N33), `L. n°` une loi (P069) contre `L. 1234` (P020).
        row(
            "note sous D. 2016, p. 1234",
            json!({"kind": "Unsupported", "match": "D. 2016, p. 1234", "family": "doctrine"}),
        ),
        row("L. n° 2016-1321 du 7 octobre 2016", json!({"kind": "Text", "nature": "LOI"})),
        row(
            "L. 1234-5 du code du travail",
            json!({"kind": "CodeArticle", "code": "Code du travail", "num": "L1234-5"}),
        ),
    ];
    run_table("§8.8 pièges", &table);
}

// ---------------------------------------------------------------------------------------------
// Propriétés (annexe B §8.8) : `parse(format::canonical(c))` redonne `c` aux champs non
// canoniques près.
// ---------------------------------------------------------------------------------------------

mod round_trip {
    use lf_domain::citation::dict::courts::CcType;
    use lf_domain::citation::{
        ArticleNum, ArticlePrefix, ArticleRef, CcNumber, Span, TextNature, TextNumber, format,
    };
    use proptest::prelude::*;

    use super::*;

    fn date() -> impl Strategy<Value = CivilDate> {
        (2000u16..=2025, 1u8..=12, 1u8..=28)
            .prop_filter_map("date valide", |(y, m, d)| CivilDate::new(y, m, d))
    }

    fn code_article() -> impl Strategy<Value = CitationRef> {
        (
            0usize..CODES.len(),
            prop::sample::select(vec![
                None,
                Some(ArticlePrefix::L),
                Some(ArticlePrefix::R),
                Some(ArticlePrefix::D),
            ]),
            prop::collection::vec(1u32..=999, 1..=3),
        )
            .prop_map(|(index, prefix, base)| CitationRef::CodeArticle {
                code: CodeRef::Known(CODES[index].key),
                article: ArticleRef {
                    num: ArticleNum {
                        prefix,
                        special: None,
                        base,
                        latin: None,
                        latin_sub: None,
                        letters: None,
                        latin_after: None,
                    },
                    sub: Vec::new(),
                    range_to: None,
                },
                version: None,
                code_via: None,
            })
    }

    fn text() -> impl Strategy<Value = CitationRef> {
        (
            prop::sample::select(vec![
                TextNature::Loi,
                TextNature::LoiOrganique,
                TextNature::Ordonnance,
                TextNature::Decret,
                TextNature::Arrete,
            ]),
            date(),
            1u32..=4999,
        )
            .prop_map(|(nature, when, seq)| {
                let year = when.year();
                CitationRef::Text(TextCite {
                    nature,
                    number: Some(TextNumber {
                        year: Some(year),
                        seq,
                        extra: None,
                        raw: format!("{year}-{seq}"),
                    }),
                    date: Some(when),
                    date_raw: None,
                    title_hint: None,
                    anaphora: None,
                    appellation: None,
                    emitter: None,
                })
            })
    }

    fn cc_decision() -> impl Strategy<Value = CitationRef> {
        (
            prop::sample::select(vec![CcType::Dc, CcType::Qpc, CcType::An, CcType::Elec]),
            date(),
            1u32..=3000,
        )
            .prop_map(|(kind, when, seq)| {
                CitationRef::Decision(DecisionCite {
                    court: Some(Court::ConseilConstitutionnel),
                    court_via: None,
                    date: Some(when),
                    numbers: vec![CaseNumber::Cc(CcNumber {
                        year: when.year(),
                        seq,
                        joined: Vec::new(),
                        rectif: false,
                        kind,
                    })],
                    kind: None,
                    publication: None,
                    solution: None,
                    parties: None,
                    subject: None,
                    arret_num: None,
                    formation_code: None,
                    publication_codes: Vec::new(),
                })
            })
    }

    fn cnil() -> impl Strategy<Value = CitationRef> {
        (
            prop::sample::select(vec![None, Some("SAN".to_owned()), Some("MED".to_owned())]),
            date(),
            1u16..=999,
        )
            .prop_map(|(series, when, seq)| {
                CitationRef::CnilDeliberation(lf_domain::citation::CnilCite {
                    series,
                    year: when.year(),
                    seq,
                    date: Some(when),
                    formation_restreinte: false,
                })
            })
    }

    /// Clés que la forme canonique porte, par famille.
    fn canonical_keys(kind: &str) -> &'static [&'static str] {
        match kind {
            "CodeArticle" => &["code", "num"],
            "Text" => &["nature", "num", "year", "seq", "date"],
            "Decision" => &["court", "date", "numbers"],
            _ => &["series", "year", "seq", "date"],
        }
    }

    fn assert_round_trip(original: &CitationRef) -> Result<(), TestCaseError> {
        let text = format::canonical(original).expect("forme canonique");
        let parsed = parse_with(&text, &base_options());
        prop_assert_eq!(parsed.len(), 1, "{:?} -> {:?}", text, parsed);
        let before = project(&Spanned::new(original.clone(), Span { start: 0, end: 0 }));
        let after = project(&parsed[0]);
        prop_assert_eq!(&before["kind"], &after["kind"], "{}", text);
        for key in canonical_keys(before["kind"].as_str().unwrap_or_default()) {
            prop_assert_eq!(&before[*key], &after[*key], "{} : {}", text, key);
        }
        prop_assert_eq!(
            (parsed[0].span.start, parsed[0].span.end),
            (0, text.len()),
            "le span couvre la forme canonique"
        );
        Ok(())
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 64,
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn canonical_code_article_round_trips(c in code_article()) {
            assert_round_trip(&c)?;
        }

        #[test]
        fn canonical_text_round_trips(c in text()) {
            assert_round_trip(&c)?;
        }

        #[test]
        fn canonical_cc_decision_round_trips(c in cc_decision()) {
            assert_round_trip(&c)?;
        }

        #[test]
        fn canonical_cnil_round_trips(c in cnil()) {
            assert_round_trip(&c)?;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Performance (annexe B §8.8, 06 §12.1).
// ---------------------------------------------------------------------------------------------

/// Texte d'au moins `chars` caractères, dense en citations de toutes les familles.
fn dense_text(chars: usize) -> String {
    const UNIT: &str = "Vu l'article L. 761-1 du code de justice administrative et les articles \
        1240 et 1241 du code civil, la loi n° 2016-1321 du 7 octobre 2016, Cass. 1re civ., 25 mai \
        2023, n° 21-23.174, CE, 5e et 6e ch. réunies, 10 mars 2025, n° 489701, Cons. const., \
        décision n° 2025-1173 QPC du 7 novembre 2025, ECLI:FR:CCASS:2025:C100399, la délibération \
        SAN-2026-002 du 8 janvier 2026, NOR : ECFI1524250L et LEGIARTI000006419288. ";
    let mut text = String::new();
    while text.chars().count() < chars {
        text.push_str(UNIT);
    }
    text
}

/// Mesure de `perf_parse_20k`. Cible : 50 ms au plus pour 20 000 caractères en release (06
/// §12.1, non bloquante : alerte au-delà, échec au-delà de trois fois la cible). Le profil de
/// test est un profil de débogage, plusieurs fois plus lent : le plafond d'échec y est élargi,
/// pour attraper une dérive super-linéaire sans dépendre de la machine.
#[test]
fn perf_parse_20k() {
    let text = dense_text(20_000);
    assert!(text.chars().count() >= 20_000);
    let options = base_options();
    // Une première passe chauffe les expressions compilées (`LazyLock`), qui ne comptent pas.
    let found = parse_with(&text, &options).len();
    assert!(found > 100, "texte dense en citations : {found} reconnues");
    let best = (0..5)
        .map(|_| {
            let start = std::time::Instant::now();
            let again = parse_with(&text, &options);
            let elapsed = start.elapsed();
            assert_eq!(again.len(), found);
            elapsed
        })
        .min()
        .expect("cinq mesures");
    eprintln!(
        "perf_parse_20k : {best:?} pour {} caractères et {found} citations (cible 50 ms en release)",
        text.chars().count()
    );
    let target = std::time::Duration::from_millis(50);
    let ceiling = if cfg!(debug_assertions) { target * 60 } else { target * 3 };
    assert!(best <= ceiling, "parse_with a mis {best:?} (plafond {ceiling:?}, cible {target:?})");
}

// ---------------------------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------------------------

#[test]
fn corpus_has_the_documented_size() {
    let (pos, neg) = (cases(POSITIVES), cases(NEGATIVES));
    assert_eq!(pos.len(), 185);
    assert_eq!(neg.len(), 45);
    let expectations = |cs: &[Case]| cs.iter().map(|c| c.expect.len()).sum::<usize>();
    assert_eq!(expectations(&pos), 217);
    assert_eq!(expectations(&neg), 32);
    let mut ids: Vec<&str> = pos.iter().chain(&neg).map(|c| c.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 230, "identifiants uniques");
}

#[test]
fn corpus_matches_are_substrings() {
    let mut problems = Vec::new();
    for case in all_cases() {
        if let Err(msg) = expected_spans(&case.input, &case.expect) {
            problems.push(format!("{} ({}) : {msg}", case.id, case.src));
        }
    }
    assert!(problems.is_empty(), "{} cas en échec :\n{}", problems.len(), problems.join("\n"));
}

#[test]
fn corpus_citations() {
    let selected = all_cases();
    let failed = failures(selected.iter().filter(|c| !FROM_MEMORY.contains(&c.id.as_str())));
    assert!(failed.is_empty(), "{} cas en échec :\n{}", failed.len(), failed.join("\n"));
}

#[test]
fn from_memory_cases_are_syntax_only() {
    let selected = all_cases();
    let memory: Vec<&Case> =
        selected.iter().filter(|c| FROM_MEMORY.contains(&c.id.as_str())).collect();
    assert_eq!(memory.len(), FROM_MEMORY.len());
    // V-163 : aucune indice de résolution sur ces cas, qui ne servent à aucun test live.
    for case in &memory {
        assert!(
            case.expect.iter().all(|e| e.get("resolve_hint").is_none()),
            "{} : cas de mémoire avec un resolve_hint",
            case.id
        );
    }
    let failed = failures(memory.into_iter());
    assert!(failed.is_empty(), "{} cas en échec :\n{}", failed.len(), failed.join("\n"));
}

#[test]
fn codes_table_copy_is_the_annex_table() {
    assert!(CODES_TABLE.starts_with("# Table des codes (77 entrées)"));
}

#[test]
fn data_copies_are_identical_to_the_annex() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/plan/annexes/donnees");
    if !dir.is_dir() {
        eprintln!("annexe absente ({}) : comparaison ignorée (paquet publié)", dir.display());
        return;
    }
    for (name, copy) in [
        ("citations-corpus-positifs.jsonl", POSITIVES),
        ("citations-corpus-negatifs.jsonl", NEGATIVES),
        ("codes-table.md", CODES_TABLE),
    ] {
        let original = std::fs::read_to_string(dir.join(name)).expect("lecture de l'annexe");
        assert!(
            original == copy,
            "{name} : la copie de test diffère de l'annexe (octet pour octet)"
        );
    }
}

#[test]
fn flag_and_ambiguity_codes_cover_the_corpus_vocabulary() {
    // Garde du vocabulaire : tout code d'ambiguïté ou de drapeau écrit dans le corpus doit être
    // produit par `code()`, sans quoi le harnais comparerait des chaînes que rien n'émet.
    let known_ambiguities: Vec<String> = [
        Ambiguity::CodeManquant,
        Ambiguity::CodeImplicite,
        Ambiguity::TexteManquant,
        Ambiguity::AnaphoreNonResolue,
        Ambiguity::NumeroManquant,
        Ambiguity::JuridictionManquante,
        Ambiguity::JuridictionEtTypeManquants,
        Ambiguity::EmetteurInconnu,
        Ambiguity::ConventionInconnue,
        Ambiguity::ConventionNonIdentifiee,
        Ambiguity::ReferenceDePublicationSeule,
        Ambiguity::Appellation,
        Ambiguity::CodeAVariantes,
    ]
    .iter()
    .map(|a| a.code().into_owned())
    .collect();
    for case in all_cases() {
        for e in &case.expect {
            if let Some(a) = e.get("ambiguity").and_then(Value::as_str) {
                assert!(
                    known_ambiguities.iter().any(|k| k == a) || a.starts_with("sigle_ambigu:"),
                    "{} : ambiguïté inconnue {a}",
                    case.id
                );
            }
            for f in e.get("flags").and_then(Value::as_array).into_iter().flatten() {
                let f = f.as_str().unwrap_or_default();
                assert!(
                    [
                        Flag::DateInvalide,
                        Flag::AnneeFuture,
                        Flag::EcliCodeCeSeul,
                        Flag::AnneeNumeroDifferenteAnneeDate,
                    ]
                    .iter()
                    .any(|k| k.code() == f)
                        || f.starts_with("chambre_")
                        || f.starts_with("ordinal_"),
                    "{} : drapeau inconnu {f}",
                    case.id
                );
            }
        }
    }
}

/// Revue finale I3 : la note « article non lu » de `fr_verify_citations` ne se déclenche sur aucun
/// des 185 textes du corpus positif (tous ont leurs articles lus par le parseur), et
/// se déclenche sur les deux formes que le filet N36 refuse en entier.
#[test]
fn no_positive_case_has_an_unread_article_mention() {
    let mut flagged = Vec::new();
    for case in cases(POSITIVES) {
        let spans: Vec<_> =
            parse_with(&case.input, &options_for(&case)).iter().map(|cite| cite.span).collect();
        if unread_article_mentions(&case.input, &spans) {
            flagged.push(format!("{} : {}", case.id, case.input));
        }
    }
    assert_eq!(flagged, Vec::<String>::new());
    for refused in ["CGI, art. 885-0 V bis A", "art. 150-0 B ter I du CGI"] {
        let input = format!("article 1240 du code civil ; {refused}");
        let spans: Vec<_> =
            parse_with(&input, &ParseOptions::default()).iter().map(|cite| cite.span).collect();
        assert!(unread_article_mentions(&input, &spans), "{input}");
    }
}
