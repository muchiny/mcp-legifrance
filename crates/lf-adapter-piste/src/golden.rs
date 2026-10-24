//! Harnais du golden de parité (annexe A §4, §5.1 à §5.4 ; 06 §2.3 `parity_golden`) :
//! les divergences volontaires déclarées en données et les valeurs explicites (surcharges). Seule
//! la liste des dix-neuf clés de divergence est rapprochée de l'annexe A par machine
//! (`golden_divergences_match_annex_a`) ; le texte de l'annexe, lui, ne l'est pas. Ce module
//! n'existe que sous `cfg(test)`.
//!
//! Le golden (`tests/fixtures/reference/parite-golden.json`) décrit la **référence** Python sur ses
//! seize fixtures ; il est copié sans modification depuis `docs/plan/annexes/donnees/`, et la copie
//! est comparée à l'octet au fichier du plan (`golden_copy_matches_plan_data`, jamais ignoré).

use std::collections::BTreeSet;

use lf_domain::text::clean_field;
use lf_domain::text::encoding::repair_latin1_mojibake;
use serde_json::{Value, json};

use crate::integrity::{known_issues, sidecar};
use crate::map::Diagnostics;
use crate::map::fixtures::{Fixture, Kind, decoded, fixture, golden, map_body};
use crate::view::view;

/// Une variante par clé du §4 de l'annexe A dont la colonne « Golden » vaut « oui », dans l'ordre
/// de l'annexe. Le test `golden_divergences_match_annex_a` garde l'égalité des deux ensembles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum GoldenDivergence {
    /// `human_readable_citation` devient `citation`.
    CitationRenamed,
    /// `eli_uri` supprimé.
    EliUriDropped,
    /// `eli_note` et `dataset_note` absentes.
    NotesDropped,
    /// Champs `null` et listes vides par défaut omis.
    NullsOmitted,
    /// `fond` et `query` non renvoyés par les recherches.
    SearchEchoDropped,
    /// Champs renommés (`date_texte`, `text_id`, `date_parution`, `title_long`, `known_gaps`).
    FieldsRenamed,
    /// Champs que la sortie ne rend plus.
    FieldsNotRendered,
    /// `title` omis quand la citation le contient.
    TitleOmittedWhenInCitation,
    /// Champs ajoutés par la sortie (liste blanche).
    FieldsAdded,
    /// Typographie des citations (accents, suffixe d'objet).
    CitationTypography,
    /// Dates sentinelles (à partir de 2222-01-01) omises.
    SentinelDatesNone,
    /// Mojibake réparé avant tout traitement.
    MojibakeRepaired,
    /// Textes courts nettoyés (`text::clean`).
    ShortTextCleaned,
    /// Textes longs structurés en paragraphes (comparaison aux blancs près).
    LongTextStructured,
    /// `byte_size` remplacé par `text_length`.
    ByteSizeToTextLength,
    /// Table des matières aplatie en préordre (projection).
    TocTree,
    /// `etat` de repli sur `jurisState`, en code API.
    EtatJurisStateFallback,
    /// `numero_affaire` d'une décision du Conseil d'État renseigné.
    NumeroAffaireCetat,
    /// Identifiant renvoyé par l'API conservé, non l'identifiant demandé.
    ReturnedIdKept,
}

impl GoldenDivergence {
    /// Les dix-neuf divergences, dans l'ordre de l'annexe A §4.
    pub(crate) const ALL: [Self; 19] = [
        Self::CitationRenamed,
        Self::EliUriDropped,
        Self::NotesDropped,
        Self::NullsOmitted,
        Self::SearchEchoDropped,
        Self::FieldsRenamed,
        Self::FieldsNotRendered,
        Self::TitleOmittedWhenInCitation,
        Self::FieldsAdded,
        Self::CitationTypography,
        Self::SentinelDatesNone,
        Self::MojibakeRepaired,
        Self::ShortTextCleaned,
        Self::LongTextStructured,
        Self::ByteSizeToTextLength,
        Self::TocTree,
        Self::EtatJurisStateFallback,
        Self::NumeroAffaireCetat,
        Self::ReturnedIdKept,
    ];

    /// Les huit divergences qui ne changent que la sortie d'outil : le niveau 1 (entités) ne
    /// peut pas les exercer, le niveau 2 les couvre (M5).
    pub(crate) const TOOL_OUTPUT_ONLY: [Self; 8] = [
        Self::NotesDropped,
        Self::NullsOmitted,
        Self::SearchEchoDropped,
        Self::FieldsRenamed,
        Self::FieldsNotRendered,
        Self::TitleOmittedWhenInCitation,
        Self::FieldsAdded,
        Self::ByteSizeToTextLength,
    ];

    /// Clé de l'annexe A (`snake_case`).
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::CitationRenamed => "citation_renamed",
            Self::EliUriDropped => "eli_uri_dropped",
            Self::NotesDropped => "notes_dropped",
            Self::NullsOmitted => "nulls_omitted",
            Self::SearchEchoDropped => "search_echo_dropped",
            Self::FieldsRenamed => "fields_renamed",
            Self::FieldsNotRendered => "fields_not_rendered",
            Self::TitleOmittedWhenInCitation => "title_omitted_when_in_citation",
            Self::FieldsAdded => "fields_added",
            Self::CitationTypography => "citation_typography",
            Self::SentinelDatesNone => "sentinel_dates_none",
            Self::MojibakeRepaired => "mojibake_repaired",
            Self::ShortTextCleaned => "short_text_cleaned",
            Self::LongTextStructured => "long_text_structured",
            Self::ByteSizeToTextLength => "byte_size_to_text_length",
            Self::TocTree => "toc_tree",
            Self::EtatJurisStateFallback => "etat_juris_state_fallback",
            Self::NumeroAffaireCetat => "numero_affaire_cetat",
            Self::ReturnedIdKept => "returned_id_kept",
        }
    }
}

/// Valeur explicite d'une entrée du golden, quand l'attendu n'est pas une transformation
/// mécanique de la valeur de la référence (annexe A §5.4, comparateur `Override`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Override {
    /// Clé de l'entrée dans `normalizer` (par exemple `consult_loda.json`).
    pub(crate) entry: &'static str,
    /// Pointeur JSON du champ dans l'entrée.
    pub(crate) pointer: &'static str,
    /// Valeur attendue côté Rust, en JSON.
    pub(crate) expected: &'static str,
    /// Divergence qui justifie la valeur.
    pub(crate) because: GoldenDivergence,
}

/// Entrée de la variante de `test_parse.py:226` : l'identifiant demandé est une `KALIARTI`.
const RETURNED_ID_ENTRY: &str = "consult_article.json[id=KALIARTI000049886059]";

/// Les six lignes de l'annexe A §5.4 (« Valeurs explicites »).
pub(crate) const OVERRIDES: [Override; 6] = [
    Override {
        entry: "consult_juri_constit.json",
        pointer: "/human_readable_citation",
        expected: r#""Cons. const., décision n° 2025-1173 QPC du 7 novembre 2025""#,
        because: GoldenDivergence::CitationTypography,
    },
    Override {
        entry: "consult_cnil.json",
        pointer: "/human_readable_citation",
        expected: r#""CNIL, délibération n° SAN-2026-002 du 8 janvier 2026""#,
        because: GoldenDivergence::CitationTypography,
    },
    Override {
        entry: "consult_loda.json",
        pointer: "/etat",
        expected: r#""VIGUEUR""#,
        because: GoldenDivergence::EtatJurisStateFallback,
    },
    Override {
        entry: "consult_juri_cetat.json",
        pointer: "/numero_affaire",
        expected: r#"["506507"]"#,
        because: GoldenDivergence::NumeroAffaireCetat,
    },
    Override {
        entry: RETURNED_ID_ENTRY,
        pointer: "/article_id",
        expected: r#""LEGIARTI000006419288""#,
        because: GoldenDivergence::ReturnedIdKept,
    },
    Override {
        entry: RETURNED_ID_ENTRY,
        pointer: "/source_url",
        expected: r#""https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288""#,
        because: GoldenDivergence::ReturnedIdKept,
    },
];

/// Valeur explicite et divergence qui la justifie pour le champ `pointer` de l'entrée `entry`,
/// `None` si le champ n'a pas de surcharge.
///
/// # Panics
///
/// Si une valeur de [`OVERRIDES`] n'est pas du JSON (le test des surcharges l'exclut).
pub(crate) fn overridden(entry: &str, pointer: &str) -> Option<(Value, GoldenDivergence)> {
    OVERRIDES
        .iter()
        .find(|over| over.entry == entry && over.pointer == pointer)
        .map(|over| (serde_json::from_str(over.expected).expect("override is JSON"), over.because))
}

/// Règle de comparaison d'une clé du golden au niveau 1 (annexe A §5.4), avec la clé de la
/// projection `view` de l'entité Rust.
#[derive(Debug, Clone, Copy)]
enum Rule {
    /// Égalité stricte.
    Eq(&'static str),
    /// `clean_field(repair(golden))` égale la valeur Rust.
    Clean(&'static str),
    /// Date `AAAA-MM-JJ` ; une date ≥ 2222-01-01 exige l'absence côté Rust.
    Date(&'static str),
    /// Texte long : blancs repliés des deux côtés, ECLI final retiré du golden s'il le faut.
    LongText,
    /// Table des matières : `article_id` des articles aplatis, dans l'ordre ; `num` et `etat`.
    Toc,
    /// Articles cités par un résultat de recherche CODE ou KALI.
    Articles,
    /// `human_readable_citation` ⇒ `contract.citation`.
    Citation,
    /// `source_url` ⇒ `contract.source_url`.
    SourceUrl,
    /// `ecli` ⇒ `contract.ecli`.
    Ecli,
    /// `kind` (genre d'URL de la référence) ⇒ nom du fond en minuscules.
    Kind,
    /// Clé sans équivalent dans l'entité.
    Dropped,
}

/// Clés d'un résultat de recherche.
const HIT: [(&str, Rule); 12] = [
    ("kind", Rule::Kind),
    ("id", Rule::Eq("id")),
    ("cid", Rule::Eq("cid")),
    ("title", Rule::Clean("title")),
    ("nature", Rule::Clean("nature")),
    ("etat", Rule::Eq("etat")),
    ("idcc", Rule::Eq("idcc")),
    ("raison_sociale", Rule::Clean("raison_sociale")),
    ("articles", Rule::Articles),
    ("human_readable_citation", Rule::Citation),
    ("source_url", Rule::SourceUrl),
    ("eli_uri", Rule::Dropped),
];

/// Clés d'un texte consolidé (`Act`) ou d'une convention collective.
const TEXT: [(&str, Rule); 14] = [
    ("text_id", Rule::Eq("text_id")),
    ("cid", Rule::Eq("cid")),
    ("nor", Rule::Eq("nor")),
    ("title", Rule::Clean("title")),
    ("nature", Rule::Clean("nature")),
    ("etat", Rule::Eq("etat")),
    ("date_parution", Rule::Date("date_parution")),
    ("num_parution", Rule::Eq("num_parution")),
    ("date_debut_version", Rule::Date("date_debut_version")),
    ("date_fin_version", Rule::Date("date_fin_version")),
    ("articles", Rule::Toc),
    ("human_readable_citation", Rule::Citation),
    ("source_url", Rule::SourceUrl),
    ("eli_uri", Rule::Dropped),
];

/// Clés d'un article.
const ARTICLE: [(&str, Rule); 11] = [
    ("article_id", Rule::Eq("article_id")),
    ("num", Rule::Clean("num")),
    ("etat", Rule::Eq("etat")),
    ("code_title", Rule::Clean("code_title")),
    ("full_sections_titre", Rule::Clean("full_sections_titre")),
    ("date_debut", Rule::Date("date_debut")),
    ("date_fin", Rule::Date("date_fin")),
    ("text", Rule::LongText),
    ("human_readable_citation", Rule::Citation),
    ("source_url", Rule::SourceUrl),
    ("eli_uri", Rule::Dropped),
];

/// Clés d'une décision.
const DECISION: [(&str, Rule); 13] = [
    ("decision_id", Rule::Eq("decision_id")),
    ("ecli", Rule::Ecli),
    ("juridiction", Rule::Clean("juridiction")),
    ("formation", Rule::Clean("formation")),
    ("solution", Rule::Clean("solution")),
    ("nature", Rule::Clean("nature")),
    ("title", Rule::Clean("title")),
    ("numero_affaire", Rule::Eq("numero_affaire")),
    ("date_texte", Rule::Date("date_texte")),
    ("text", Rule::LongText),
    ("human_readable_citation", Rule::Citation),
    ("source_url", Rule::SourceUrl),
    ("eli_uri", Rule::Dropped),
];

/// Clés d'une délibération.
const DELIBERATION: [(&str, Rule); 13] = [
    ("deliberation_id", Rule::Eq("deliberation_id")),
    ("num", Rule::Clean("num")),
    ("nature", Rule::Clean("nature")),
    ("nature_delib", Rule::Clean("nature_delib")),
    ("title", Rule::Clean("title")),
    ("title_long", Rule::Clean("title_long")),
    ("etat", Rule::Eq("etat")),
    ("date_texte", Rule::Date("date_texte")),
    ("date_publication", Rule::Date("date_publication")),
    ("text", Rule::LongText),
    ("human_readable_citation", Rule::Citation),
    ("source_url", Rule::SourceUrl),
    ("eli_uri", Rule::Dropped),
];

/// Clés d'un accord d'entreprise ; `attachment_note` est un texte de l'outil (05), pas de
/// l'entité.
const AGREEMENT: [(&str, Rule); 17] = [
    ("agreement_id", Rule::Eq("agreement_id")),
    ("nature", Rule::Clean("nature")),
    ("title", Rule::Clean("title")),
    ("raison_sociale", Rule::Clean("raison_sociale")),
    ("siret", Rule::Eq("siret")),
    ("idcc", Rule::Eq("idcc")),
    ("code_ape", Rule::Eq("code_ape")),
    ("secteur", Rule::Clean("secteur")),
    ("themes", Rule::Eq("themes")),
    ("syndicats", Rule::Eq("syndicats")),
    ("date_texte", Rule::Date("date_texte")),
    ("date_effet", Rule::Date("date_effet")),
    ("date_depot", Rule::Date("date_depot")),
    ("attachment_note", Rule::Dropped),
    ("human_readable_citation", Rule::Citation),
    ("source_url", Rule::SourceUrl),
    ("eli_uri", Rule::Dropped),
];

/// Comparaison d'une entrée du golden à l'entité Rust correspondante : divergences exercées et
/// écarts constatés.
struct Level1<'a> {
    /// Clé de l'entrée dans `normalizer`.
    entry: &'a str,
    /// Le sidecar de la fixture déclare du mojibake : le golden est réparé avant comparaison.
    mojibake: bool,
    /// Le `texteHtml` de la fixture porte `<RECH_ECLI>` : l'ECLI final du golden est retiré.
    rech_ecli: bool,
    /// Divergences qu'une comparaison a effectivement appliquées.
    exercised: BTreeSet<GoldenDivergence>,
    /// Écarts constatés, un par clé.
    mismatches: Vec<String>,
}

impl Level1<'_> {
    /// Valeur `value` du golden au pointeur `pointer`, remplacée par sa valeur explicite s'il y en a une.
    fn golden_at(&mut self, pointer: &str, value: &Value) -> Value {
        match overridden(self.entry, pointer) {
            Some((expected, because)) => {
                self.exercised.insert(because);
                expected
            }
            None => value.clone(),
        }
    }

    fn mismatch(&mut self, pointer: &str, golden: &Value, rust: &Value) {
        self.mismatches.push(format!("{} {pointer}: golden {golden} != rust {rust}", self.entry));
    }

    /// Texte du golden réparé du mojibake si la fixture en porte (transformation 1).
    fn repaired(&mut self, text: &str) -> String {
        if !self.mojibake {
            return text.to_owned();
        }
        let repaired = repair_latin1_mojibake(text);
        if repaired != text {
            self.exercised.insert(GoldenDivergence::MojibakeRepaired);
        }
        repaired.into_owned()
    }

    /// `clean_field(repair(golden))` (transformation 2).
    fn cleaned(&mut self, golden: &Value) -> Value {
        let Some(text) = golden.as_str() else { return golden.clone() };
        let repaired = self.repaired(text);
        let cleaned = clean_field(Some(&repaired));
        if cleaned.as_deref() != Some(repaired.as_str()) {
            self.exercised.insert(GoldenDivergence::ShortTextCleaned);
        }
        cleaned.map_or(Value::Null, Value::String)
    }

    /// Date du golden, `null` pour une sentinelle (transformation 4).
    fn dated(&mut self, golden: &Value) -> Value {
        match golden.as_str() {
            Some(date) if date >= "2222-01-01" => {
                self.exercised.insert(GoldenDivergence::SentinelDatesNone);
                Value::Null
            }
            _ => golden.clone(),
        }
    }

    /// Compare la clé au pointeur `pointer` de l'entrée selon `rule`.
    fn compare(&mut self, pointer: &str, rule: Rule, golden: &Value, rust: &Value) {
        let golden = self.golden_at(pointer, golden);
        let (expected, actual) = match rule {
            Rule::Eq(field) => (golden, rust[field].clone()),
            Rule::Clean(field) => (self.cleaned(&golden), rust[field].clone()),
            Rule::Date(field) => (self.dated(&golden), rust[field].clone()),
            Rule::LongText => self.long_text(&golden, rust),
            Rule::Toc => {
                let expected = self.toc_golden(&golden);
                if expected != golden {
                    self.exercised.insert(GoldenDivergence::TocTree);
                }
                (expected, toc_projection(&rust["toc"]))
            }
            Rule::Articles => {
                (self.articles_golden(&golden), articles_projection(&rust["articles"]))
            }
            Rule::Citation => {
                let expected = self.cleaned(&golden);
                if expected != golden {
                    self.exercised.insert(GoldenDivergence::CitationRenamed);
                }
                (expected, rust["contract"]["citation"].clone())
            }
            Rule::SourceUrl => (golden, rust["contract"]["source_url"].clone()),
            Rule::Ecli => (golden, rust["contract"]["ecli"].clone()),
            Rule::Kind => {
                let fond = rust["fond"].as_str().map(str::to_lowercase);
                (golden, fond.map_or(Value::Null, Value::String))
            }
            Rule::Dropped => {
                if pointer.ends_with("/eli_uri") {
                    self.exercised.insert(GoldenDivergence::EliUriDropped);
                }
                return;
            }
        };
        if expected != actual {
            self.mismatch(pointer, &expected, &actual);
        }
    }

    /// Texte long : `fold_ws` des deux côtés ; l'ECLI final retiré du golden quand la fixture a
    /// un `<RECH_ECLI>` ; préfixe commun jusqu'au premier ` [...]` pour un golden tronqué.
    fn long_text(&mut self, golden: &Value, rust: &Value) -> (Value, Value) {
        let rust_text = fold_ws(rust["text"]["text"].as_str().unwrap_or_default());
        let mut expected = fold_ws(&self.repaired(golden.as_str().unwrap_or_default()));
        if self.rech_ecli {
            let ecli = rust["contract"]["ecli"].as_str().expect("native ECLI");
            expected = expected
                .strip_suffix(&format!(" {ecli}"))
                .expect("golden ends with the ECLI")
                .to_owned();
        }
        // Fixture tronquée (CNIL : `texte` et `texteHtml` coupés à deux endroits, marqués
        // ` [...]`) : seul le préfixe commun, jusqu'à la première marque d'un des deux côtés, est
        // comparé (annexe A §5.4, `long_text_structured`).
        let cut = [&expected, &rust_text]
            .map(|text| text.find(" [...]").map_or(usize::MAX, |at| text[..at].chars().count()));
        let length = cut[0].min(cut[1]);
        let expected: String = expected.chars().take(length).collect();
        let actual: String = rust_text.chars().take(length).collect();
        let (expected, actual) = (Value::String(expected), Value::String(actual));
        if expected != *golden {
            self.exercised.insert(GoldenDivergence::LongTextStructured);
        }
        (expected, actual)
    }

    /// Articles du golden d'un texte : `[article_id, clean(num), etat]`.
    fn toc_golden(&mut self, golden: &Value) -> Value {
        let articles = golden.as_array().map(Vec::as_slice).unwrap_or_default();
        Value::Array(
            articles
                .iter()
                .map(|article| {
                    json!([article["article_id"], self.cleaned(&article["num"]), article["etat"]])
                })
                .collect(),
        )
    }

    /// Articles du golden d'un résultat : `[article_id, clean(num), clean(snippet)]`.
    fn articles_golden(&mut self, golden: &Value) -> Value {
        let articles = golden.as_array().map(Vec::as_slice).unwrap_or_default();
        Value::Array(
            articles
                .iter()
                .map(|article| {
                    json!([
                        article["article_id"],
                        self.cleaned(&article["num"]),
                        self.cleaned(&article["snippet"])
                    ])
                })
                .collect(),
        )
    }

    /// Compare l'entrée `expected` à la projection `rust` de l'entité de genre `kind`.
    fn entity(&mut self, kind: Kind, expected: &Value, rust: &Value) {
        match kind {
            Kind::Search(_) => {
                self.compare("/total", Rule::Eq("total"), &expected["total"], rust);
                let hits = expected["hits"].as_array().map(Vec::as_slice).unwrap_or_default();
                let actual = rust["hits"].as_array().map(Vec::as_slice).unwrap_or_default();
                if hits.len() != actual.len() {
                    self.mismatch("/hits", &Value::from(hits.len()), &Value::from(actual.len()));
                }
                for (index, (hit, rust_hit)) in hits.iter().zip(actual).enumerate() {
                    self.object(&format!("/hits/{index}"), &HIT, hit, rust_hit);
                }
            }
            Kind::LawDecree | Kind::KaliText => self.object("", &TEXT, expected, rust),
            Kind::GetArticle => self.object("", &ARTICLE, expected, rust),
            Kind::Juri => self.object("", &DECISION, expected, rust),
            Kind::Cnil => self.object("", &DELIBERATION, expected, rust),
            Kind::Acco => self.object("", &AGREEMENT, expected, rust),
        }
    }

    /// Compare l'objet `golden` à `rust`, clé par clé, selon `table` ; une clé du golden absente de
    /// la table est un écart (aucune clé n'est ignorée en silence).
    fn object(&mut self, prefix: &str, table: &[(&str, Rule)], golden: &Value, rust: &Value) {
        for (key, value) in golden.as_object().expect("golden entry is an object") {
            let pointer = format!("{prefix}/{key}");
            match table.iter().find(|(known, _)| known == key) {
                Some((_, rule)) => self.compare(&pointer, *rule, value, rust),
                None => self
                    .mismatches
                    .push(format!("{} {pointer}: unclassified golden key", self.entry)),
            }
        }
    }
}

/// Toute suite de blancs Unicode (NBSP compris) remplacée par une espace, bords rognés.
fn fold_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Articles de la TOC de la projection Rust, aplatis en préordre : `[article_id, num, etat]`.
fn toc_projection(toc: &Value) -> Value {
    fn walk(nodes: &Value, out: &mut Vec<Value>) {
        for node in nodes.as_array().map(Vec::as_slice).unwrap_or_default() {
            if let Some(article) = node.get("article") {
                out.push(json!([article["article_id"], article["num"], article["etat"]]));
            }
            if let Some(section) = node.get("section") {
                walk(&section["children"], out);
            }
        }
    }
    let mut out = Vec::new();
    walk(toc, &mut out);
    Value::Array(out)
}

/// Articles d'un résultat de la projection Rust : `[article_id, num, snippet]`.
fn articles_projection(articles: &Value) -> Value {
    let articles = articles.as_array().map(Vec::as_slice).unwrap_or_default();
    Value::Array(
        articles
            .iter()
            .map(|article| json!([article["article_id"], article["num"], article["snippet"]]))
            .collect(),
    )
}

/// Fixture et identifiant demandé d'une clé du golden (`consult_article.json[id=X]` ⇒ `X`).
fn entry_fixture(entry: &str) -> (&'static Fixture, &str) {
    let (file, requested) = match entry.split_once("[id=") {
        Some((file, rest)) => (file, rest.strip_suffix(']').expect("closing bracket")),
        None => (entry, ""),
    };
    let fixture = fixture(file.strip_suffix(".json").expect("json entry"));
    (fixture, if requested.is_empty() { fixture.requested } else { requested })
}

/// Compare chacune des dix-sept entrées du golden à l'entité que produit le chemin de
/// production ; rend les divergences exercées et les écarts.
fn level1() -> (BTreeSet<GoldenDivergence>, Vec<String>) {
    let golden = golden();
    let mut exercised = BTreeSet::new();
    let mut mismatches = Vec::new();
    for (entry, expected) in golden["normalizer"].as_object().expect("normalizer") {
        let (fixture, requested) = entry_fixture(entry);
        let body = decoded(fixture, true).text;
        let mapped = map_body(fixture.kind, &body, requested, &mut Diagnostics::default())
            .expect("fixture maps");
        let rust = view(&mapped);
        let mut level = Level1 {
            entry,
            mojibake: known_issues(&sidecar(fixture.name)).contains(&"mojibake"),
            rech_ecli: body.contains("<RECH_ECLI>"),
            exercised: BTreeSet::new(),
            mismatches: Vec::new(),
        };
        level.entity(fixture.kind, expected, &rust);
        exercised.extend(level.exercised);
        mismatches.extend(level.mismatches);
    }
    (exercised, mismatches)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::{Value, json};

    use lf_domain::fond::Fond;

    use super::{GoldenDivergence, Level1, OVERRIDES, level1, overridden};
    use crate::integrity::{hex, sha256, sidecar, text_at};
    use crate::map::fixtures::{Kind, golden};

    /// Copie du golden dans le crate, à l'octet.
    const COPY: &[u8] = include_bytes!("../tests/fixtures/reference/parite-golden.json");
    /// Fichier du plan dont la copie est tirée, à l'octet.
    const PLAN: &[u8] = include_bytes!("../../../docs/plan/annexes/donnees/parite-golden.json");
    /// Annexe A de parité.
    const ANNEX_A: &str = include_str!("../../../docs/plan/annexes/A-parite.md");

    /// Clés du §4 de l'annexe dont la colonne « Golden » vaut « oui », lues ligne à ligne.
    fn annex_keys(annex: &str) -> BTreeSet<String> {
        let start = annex.find("## 4. Divergences volontaires consolidées").expect("§4 starts");
        let end = annex.find("## 5. Données golden").expect("§5 starts");
        annex[start..end]
            .lines()
            .filter(|line| line.starts_with("| `"))
            .filter(|line| {
                line.trim_end()
                    .trim_end_matches('|')
                    .rsplit('|')
                    .next()
                    .is_some_and(|last| last.trim().starts_with("oui"))
            })
            .map(|line| line.split('|').nth(1).expect("first cell").trim().trim_matches('`').into())
            .collect()
    }

    /// Clés de `ALL`, sans doublon.
    fn code_keys() -> BTreeSet<String> {
        GoldenDivergence::ALL.iter().map(|d| d.key().to_owned()).collect()
    }

    #[test]
    fn golden_divergences_match_annex_a() {
        let from_code = code_keys();
        assert_eq!(from_code.len(), 19);
        assert_eq!(annex_keys(ANNEX_A), from_code);
        assert_eq!(annex_keys(&ANNEX_A.replace('\n', "\r\n")), from_code);
    }

    #[test]
    fn tool_output_only_divergences_are_known_variants() {
        assert_eq!(GoldenDivergence::TOOL_OUTPUT_ONLY.len(), 8);
        let all: BTreeSet<_> = GoldenDivergence::ALL.into_iter().collect();
        let only: BTreeSet<_> = GoldenDivergence::TOOL_OUTPUT_ONLY.into_iter().collect();
        assert_eq!(only.len(), 8);
        assert!(only.is_subset(&all));
    }

    #[test]
    fn overrides_point_to_existing_golden_values() {
        let golden = golden();
        for over in OVERRIDES {
            let entry = &golden["normalizer"][over.entry];
            let current = entry.pointer(over.pointer).expect("pointer exists in the golden");
            let expected: Value = serde_json::from_str(over.expected).expect("expected is JSON");
            assert_ne!(current, &expected, "{} {}", over.entry, over.pointer);
            assert_eq!(
                overridden(over.entry, over.pointer),
                Some((expected, over.because)),
                "{} {}",
                over.entry,
                over.pointer
            );
            assert!(GoldenDivergence::ALL.contains(&over.because));
            assert!(!GoldenDivergence::TOOL_OUTPUT_ONLY.contains(&over.because));
        }
        assert_eq!(overridden("consult_loda.json", "/title"), None);
    }

    #[test]
    fn golden_meta_is_expected() {
        let golden = golden();
        assert_eq!(
            golden["_meta"],
            json!({
                "generated_by": "scratchpad/scripts/golden.py (re-implementation independante, n'importe pas la reference)",
                "reference": "matematicsolutions/fr-eli-mcp@d3b5b13 tests/fixtures/*.json",
                "normalizer": "sortie des fonctions citations.normalize_* (dict brut, avant modele pydantic)",
                "tool_output": "structuredContent attendu de l'outil (ordre des champs des modeles models.py, defauts appliques, byte_size, eli_note/dataset_note)",
            })
        );
        assert_eq!(golden["normalizer"].as_object().expect("object").len(), 17);
        assert_eq!(golden["tool_output"].as_object().expect("object").len(), 16);
        let meta = sidecar("parite-golden");
        let keys: BTreeSet<&str> = meta.keys().map(String::as_str).collect();
        assert_eq!(keys, BTreeSet::from(["license", "reference", "sha256", "source"]));
        assert_eq!(text_at(&meta, "source"), Some("docs/plan/annexes/donnees/parite-golden.json"));
        assert_eq!(text_at(&meta, "reference"), Some("matematicsolutions/fr-eli-mcp@d3b5b13"));
        assert_eq!(text_at(&meta, "sha256"), Some(hex(&sha256(COPY)).as_str()));
    }

    #[test]
    fn golden_copy_matches_plan_data() {
        assert_eq!(COPY.len(), 118_107);
        assert!(COPY == PLAN, "the crate copy of the golden differs from the plan data");
    }

    #[test]
    fn mapping_matches_golden() {
        let (_, mismatches) = level1();
        assert_eq!(mismatches, Vec::<String>::new());
    }

    #[test]
    fn every_golden_divergence_is_exercised() {
        // Niveau 1 : les onze divergences qu'une entité porte ; les huit autres sont celles de la
        // sortie d'outil, exercées au niveau 2 (M5).
        let (exercised, _) = level1();
        let tool: BTreeSet<_> = GoldenDivergence::TOOL_OUTPUT_ONLY.into_iter().collect();
        assert_eq!(exercised.intersection(&tool).count(), 0);
        let all: BTreeSet<_> = GoldenDivergence::ALL.into_iter().collect();
        assert_eq!(exercised.union(&tool).copied().collect::<BTreeSet<_>>(), all);
        assert_eq!(exercised.len(), 11);
    }

    #[test]
    fn the_harness_reports_mismatches_and_unknown_keys() {
        // Témoin négatif : sans lui, un comparateur toujours vrai passerait inaperçu.
        let mut level = Level1 {
            entry: "synthetic.json",
            mojibake: false,
            rech_ecli: false,
            exercised: BTreeSet::new(),
            mismatches: Vec::new(),
        };
        let golden = json!({"total": 2, "hits": [{"id": "A", "secret_key": 1}, {"id": "B"}]});
        let rust = json!({"total": 3, "hits": [{"id": "Z"}]});
        level.entity(Kind::Search(Fond::Juri), &golden, &rust);
        assert_eq!(
            level.mismatches,
            [
                "synthetic.json /total: golden 2 != rust 3",
                "synthetic.json /hits: golden 2 != rust 1",
                "synthetic.json /hits/0/id: golden \"A\" != rust \"Z\"",
                "synthetic.json /hits/0/secret_key: unclassified golden key",
            ]
        );
    }
}
