//! Ancres de la détection (annexe B §4.16).
//!
//! Une ancre est une plage de `Normalized::text` où une famille a une chance de commencer ; les
//! grammaires la lisent ensuite dans une fenêtre `[ancre - 80, ancre + 400]` caractères. Les
//! classes suivent le tableau du §4.16 :
//!
//! - les **mots-clés** (articles, natures de textes, juridictions, codes, autres familles,
//!   puits) sont repérés par `aho-corasick` (`LeftmostLongest`) sur la vue clé, repliée un pour
//!   un, puis ramenés aux offsets de `text` ;
//! - les **jetons atomiques** (ECLI, NOR, identifiants, pourvoi, CAA, RG, Conseil constitutionnel,
//!   sigles, préfixes d'article) sont repérés par des `regex` sur `text`, parce que la casse y
//!   porte du sens.
//!
//! Un automate par classe de mots-clés : deux classes peuvent ancrer la même plage (`délibération`
//! est à la fois une nature de texte et une autre famille) ou se recouvrir (`civ.` dans
//! `c. civ.`), ce qu'un automate unique, à correspondances sans recouvrement, perdrait. Les
//! automates et les expressions sont construits une fois pour le processus.
//!
//! Les ancres sont volontairement généreuses : une ancre n'est qu'un point de départ, la
//! grammaire refuse ce qui n'est pas une citation (`21-23.1745`, `particle`). Un mot-clé qui
//! commence ou finit par une lettre ou un chiffre doit être isolé de ce côté (`art` n'ancre pas
//! `part`, `loi` n'ancre pas `loisir`).

use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::LazyLock;

use aho_corasick::{AhoCorasick, MatchKind};
use regex::Regex;

use crate::citation::dict::codes::CODES;
use crate::citation::dict::courts::{CaaCode, CcType};
use crate::citation::normalize::{Normalized, fold_char};
use crate::citation::types::IdPrefix;

/// Classe d'une ancre : décide quelles grammaires l'essaient.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum AnchorClass {
    /// Mot-clé d'article ou de subdivision : `article`, `art.`, `alinea`, `phrase`.
    ArticleKeyword,
    /// Nature de texte : `loi`, `ordonnance`, `decret`, `l. n`, `ord.`.
    TextNature,
    /// Juridiction, chambre ou formation : `cass`, `civ.`, `cour d'appel`, `conseil d'etat`.
    Court,
    /// Code : titre replié, abréviation, anaphore (`meme code`, `dudit code`).
    Code,
    /// Autre famille : `cnil`, `idcc`, `avenant`, `jorf`, `nor`, `/eli/`, `constitution`.
    Family,
    /// Puits : `directive`, `cjue`, `jcp`, `bull.`, `leb.`.
    Sink,
    /// `ECLI:`.
    Ecli,
    /// NOR : `[A-Z]{4}\d{7}[A-Z]`.
    Nor,
    /// Identifiant Légifrance : `(LEGITEXT|…)\d{12}`.
    LegifranceId,
    /// Numéro de pourvoi : `\d{2}-\d{2}\.?\d{3}`.
    Pourvoi,
    /// Numéro de CAA : `\d{2}(PA|VE|MA|…)\d{5}`.
    CaaNumber,
    /// Répertoire général : `\d{2}/\d{4,5}`.
    Rg,
    /// Numéro du Conseil constitutionnel : `\d{4}-\d+(/\d+)?\s?(DC|QPC|…)`.
    CcNumber,
    /// Sigle en majuscules : `CE`, `CC`, `CGI`.
    Sigle,
    /// Préfixe d'article suivi d'un chiffre : `(LO|L|R|D|A)\*{0,2}\.?\s?\d`.
    ArticlePrefix,
}

/// Une ancre : sa classe et sa plage, en octets de `Normalized::text`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Anchor {
    /// Classe de l'ancre.
    pub class: AnchorClass,
    /// Plage en octets de `Normalized::text`.
    pub range: Range<usize>,
}

/// Mots-clés d'articles et de subdivisions (§4.2, `art_kw` et `sub`).
const ARTICLE_KEYWORDS: &[&str] =
    &["articles", "article", "arts", "art.", "art", "alineas", "alinea", "al.", "phrase"];

/// Natures de textes (§4.3, `nature`). `l. n` et `d. n` sont les formes abrégées suivies de
/// `n°` : seule `L.` ou `D.` devant un numéro est une nature (§4.2, règle 1).
const TEXT_NATURES: &[&str] = &[
    "loi",
    "ordonnance",
    "ord.",
    "decret-loi",
    "decret",
    "decr.",
    "arrete",
    "arr.",
    "reglement",
    "circulaire",
    "instruction",
    "deliberation",
    "delib.",
    "decision",
    "l. n",
    "d. n",
];

/// Juridictions, chambres et formations (§4.5 à §4.9), puis les indices d'enregistrement qui
/// précèdent un numéro seul (§4.17, N28) : `pourvoi`, `requete`, `enregistre`, `affaire`.
const COURTS: &[&str] = &[
    "cour de cassation",
    "cass.",
    "cass",
    "c. cass.",
    "civ.",
    "crim.",
    "com.",
    "soc.",
    "req.",
    "ass. plen.",
    "ch. mixte",
    "ch. reunies",
    "chambre civile",
    "chambre commerciale",
    "chambre sociale",
    "chambre criminelle",
    "chambre mixte",
    "chambres reunies",
    "assemblee pleniere",
    "premiere chambre civile",
    "deuxieme chambre civile",
    "troisieme chambre civile",
    "cour d'appel",
    "c.a.",
    "tribunal judiciaire",
    "tribunal de commerce",
    "t. com.",
    "conseil de prud'hommes",
    "conseil d'etat",
    "cons. d'etat",
    "c.e.",
    "cour administrative d'appel",
    "c.a.a.",
    "tribunal administratif",
    "t.a.",
    "trib. adm.",
    "jugement",
    "arret",
    "cons. const.",
    "c. const.",
    "conseil constitutionnel",
    "pourvoi",
    "pourvois",
    "requete",
    "enregistre",
    "enregistree",
    "affaire",
    "aff.",
];

/// Anaphores de code (§4.2, `code_anaphora`), ajoutées aux titres et abréviations du
/// dictionnaire.
const CODE_ANAPHORAS: &[&str] =
    &["meme code", "dudit code", "ledit code", "present code", "ce code"];

/// Autres familles : CNIL, conventions collectives, JORF, NOR, ELI, bloc de constitutionnalité
/// (§4.4, §4.11 à §4.13).
const FAMILIES: &[&str] = &[
    "cnil",
    "deliberation",
    "delib.",
    "idcc",
    "convention collective",
    "convention",
    "avenant",
    "brochure",
    "accord",
    "annexe",
    "jorf",
    "journal officiel",
    "j.o.",
    "jo",
    "nor",
    "/eli/",
    "constitution",
    "const.",
    "declaration",
    "preambule",
    "charte de l'environnement",
];

/// Puits anti-faux-positifs : droit de l'Union, CJUE, CEDH, doctrine, recueils (§4.14).
const SINKS: &[&str] = &[
    "directive",
    "reglement (ue)",
    "reglement (ce)",
    "reglement (cee)",
    "reglement ue",
    "cjue",
    "cjce",
    "trib. ue",
    "cedh",
    "cour edh",
    "jcp",
    "rtd civ.",
    "rtd com.",
    "gaz. pal.",
    "ajda",
    "rfda",
    "dr. soc.",
    "rev. crit. dip",
    "bull.",
    "rec.",
    "leb.",
];

/// Sigles de juridictions et de familles, en majuscules (casse significative, §3.3) ; ceux des
/// codes viennent du dictionnaire.
const SIGLES: &[&str] = &[
    "CE", "CC", "CA", "CAA", "TA", "TJ", "TGI", "CPH", "CHR", "CHS", "SSR", "SSJS", "AP", "DDHC",
    "CCN", "ANI",
];

/// Un automate de mots-clés et la classe des ancres qu'il produit.
struct Lexicon {
    class: AnchorClass,
    automaton: AhoCorasick,
}

/// Une expression sur le texte et la classe des ancres qu'elle produit.
struct Token {
    class: AnchorClass,
    regex: Regex,
}

/// Un automate par classe de mots-clés, construits une seule fois.
static LEXICONS: LazyLock<Vec<Lexicon>> = LazyLock::new(|| {
    vec![
        lexicon(AnchorClass::ArticleKeyword, ARTICLE_KEYWORDS),
        lexicon(AnchorClass::TextNature, TEXT_NATURES),
        lexicon(AnchorClass::Court, COURTS),
        lexicon(AnchorClass::Code, &code_patterns()),
        lexicon(AnchorClass::Family, FAMILIES),
        lexicon(AnchorClass::Sink, SINKS),
    ]
});

/// Les expressions des jetons atomiques, compilées une seule fois.
static TOKENS: LazyLock<Vec<Token>> = LazyLock::new(|| {
    let ids = join(IdPrefix::ALL.map(IdPrefix::as_str));
    let caa = join(CaaCode::ALL.map(CaaCode::as_str));
    let cc = join(["SEN/QPC"].into_iter().chain(CcType::ALL.map(CcType::as_str)));
    let sigles = join(sigles());
    vec![
        token(AnchorClass::Ecli, r"(?i)\becli:"),
        token(AnchorClass::Nor, r"\b[A-Z]{4}[0-9]{7}[A-Z]\b"),
        // `_` borne l'identifiant autant qu'une frontière de mot : `_JJ-MM-AAAA` est le suffixe de
        // version (P198), et `\b` ne sépare pas un chiffre d'un `_`.
        token(AnchorClass::LegifranceId, &format!(r"\b(?:{ids})[0-9]{{12}}(?:\b|_)")),
        token(AnchorClass::Pourvoi, r"\b[0-9]{2}-[0-9]{2}\.?[0-9]{3}\b"),
        token(AnchorClass::CaaNumber, &format!(r"\b[0-9]{{2}}(?:{caa})[0-9]{{5}}\b")),
        token(AnchorClass::Rg, r"\b[0-9]{2}/[0-9]{4,5}\b"),
        token(AnchorClass::CcNumber, &format!(r"\b[0-9]{{4}}-[0-9]+(?:/[0-9]+)?R?[ ]?(?:{cc})\b")),
        token(AnchorClass::Sigle, &format!(r"\b(?:{sigles})\b")),
        token(AnchorClass::ArticlePrefix, r"\b(?:LO|L|R|D|A)\*{0,2}\.?[ ]?[0-9]"),
    ]
});

/// Alternation de motifs littéraux (échappés), du plus long au plus court.
fn join<I, S>(items: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut items: Vec<String> = items.into_iter().map(|s| regex::escape(s.as_ref())).collect();
    items.sort_by_key(|s| std::cmp::Reverse(s.len()));
    items.join("|")
}

/// Construit l'automate d'une classe de mots-clés (`LeftmostLongest`).
#[expect(
    clippy::expect_used,
    reason = "motifs constants de l'annexe B §4.16, couverts par les tests"
)]
fn lexicon<S: AsRef<[u8]>>(class: AnchorClass, patterns: &[S]) -> Lexicon {
    let automaton = AhoCorasick::builder()
        .match_kind(MatchKind::LeftmostLongest)
        .build(patterns)
        .expect("les motifs de mots-clés sont valides");
    Lexicon { class, automaton }
}

/// Compile l'expression d'une classe de jetons.
#[expect(
    clippy::expect_used,
    reason = "expressions constantes de l'annexe B §4.16, couvertes par les tests"
)]
fn token(class: AnchorClass, pattern: &str) -> Token {
    Token { class, regex: Regex::new(pattern).expect("les expressions de jetons sont valides") }
}

/// Texte replié un pour un, comme la vue clé.
fn fold_str(text: &str) -> String {
    text.chars().map(fold_char).collect()
}

/// Vrai pour un sigle de code en majuscules (`CGI`, `CPC`) : cherché sur `text`, casse
/// significative, pas sur la vue clé.
fn is_sigle(abbreviation: &str) -> bool {
    abbreviation.len() >= 2 && abbreviation.bytes().all(|b| b.is_ascii_uppercase())
}

/// Sigles en majuscules : juridictions et familles, plus sigles et abréviations en majuscules des
/// codes.
fn sigles() -> BTreeSet<&'static str> {
    let mut sigles: BTreeSet<&str> = SIGLES.iter().copied().collect();
    for entry in &CODES {
        sigles.extend(entry.ambiguous_sigles.iter().copied());
        sigles.extend(entry.abbreviations.iter().map(|&(form, _)| form).filter(|f| is_sigle(f)));
    }
    sigles
}

/// Motifs de la classe `Code` : titres repliés, abréviations non majuscules en trois variantes
/// d'espacement (`c. civ.`, `c.civ.`, `c civ`), anaphores.
fn code_patterns() -> Vec<String> {
    let mut patterns: BTreeSet<String> = CODE_ANAPHORAS.iter().map(|&a| a.to_owned()).collect();
    for entry in &CODES {
        patterns.insert(fold_str(entry.title));
        for &(form, _) in entry.abbreviations.iter().filter(|(form, _)| !is_sigle(form)) {
            let folded = fold_str(form);
            patterns.insert(folded.replace(". ", "."));
            let spaced = folded.replace('.', " ");
            patterns.insert(spaced.split_whitespace().collect::<Vec<_>>().join(" "));
            patterns.insert(folded);
        }
    }
    patterns.into_iter().collect()
}

/// Vrai si la plage `start..end` de `key` est isolée des lettres et des chiffres voisins du côté
/// où elle commence ou finit par une lettre ou un chiffre.
fn isolated(key: &str, start: usize, end: usize) -> bool {
    let word = char::is_alphanumeric;
    let first = key.get(start..).and_then(|s| s.chars().next());
    let last = key.get(..end).and_then(|s| s.chars().next_back());
    let before = key.get(..start).and_then(|s| s.chars().next_back());
    let after = key.get(end..).and_then(|s| s.chars().next());
    let glued = |edge: Option<char>, neighbour: Option<char>| {
        edge.is_some_and(word) && neighbour.is_some_and(word)
    };
    !glued(first, before) && !glued(last, after)
}

/// Ajoute à `anchors` les mots-clés de `lexicon` trouvés dans `n`, en offsets de `n.text`.
fn keyword_anchors(n: &Normalized, lexicon: &Lexicon, anchors: &mut Vec<Anchor>) {
    let to_text = |key_offset: usize| n.key_to_text(key_offset).unwrap_or(n.text.len());
    anchors.extend(
        lexicon
            .automaton
            .find_iter(&n.key)
            .filter(|m| isolated(&n.key, m.start(), m.end()))
            .map(|m| Anchor { class: lexicon.class, range: to_text(m.start())..to_text(m.end()) }),
    );
}

/// Toutes les ancres du texte, triées par début (puis par fin, puis par classe).
pub(crate) fn find_anchors(n: &Normalized) -> Vec<Anchor> {
    let mut anchors = Vec::new();
    for lexicon in LEXICONS.iter() {
        keyword_anchors(n, lexicon, &mut anchors);
    }
    for token in TOKENS.iter() {
        anchors.extend(
            token.regex.find_iter(&n.text).map(|m| Anchor { class: token.class, range: m.range() }),
        );
    }
    #[expect(
        clippy::as_conversions,
        reason = "discriminant d'une énumération sans donnée, clé de tri stable des ancres"
    )]
    let class_key = |anchor: &Anchor| anchor.class as u8;
    anchors.sort_by_key(|a| (a.range.start, a.range.end, class_key(a)));
    anchors
}

#[cfg(test)]
#[expect(
    clippy::as_conversions,
    reason = "tests : tailles et indices bornés par les données du test"
)]
mod tests {
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    use super::*;
    use crate::citation::normalize::{Steps, normalize};

    /// Ancres de `input` : classe et extrait de `text` qu'elles couvrent.
    fn found(input: &str) -> Vec<(AnchorClass, String)> {
        let n = normalize(input, Steps::default());
        find_anchors(&n)
            .into_iter()
            .map(|a| (a.class, n.text.get(a.range).unwrap_or("<hors texte>").to_owned()))
            .collect()
    }

    /// Vrai si `input` porte une ancre de classe `class` qui couvre exactement `slice`.
    fn has(input: &str, class: AnchorClass, slice: &str) -> bool {
        found(input).contains(&(class, slice.to_owned()))
    }

    #[test]
    fn anchors_are_built_by_hand() {
        let n = normalize("art. L. 761-1 CE", Steps::default());
        let hand = Anchor { class: AnchorClass::Sigle, range: 14..16 };
        assert_eq!(n.text.get(hand.range.clone()), Some("CE"));
        assert_eq!(hand.clone(), hand);
        assert_ne!(AnchorClass::Code, AnchorClass::Sink);
        // `find_anchors` retrouve l'ancre construite à la main, et n'en invente pas sans repère.
        assert!(find_anchors(&n).contains(&hand));
        assert_eq!(find_anchors(&normalize("", Steps::default())), Vec::<Anchor>::new());
        assert_eq!(found("bonjour tout le monde"), []);
    }

    /// Une ligne par classe d'ancre du tableau du §4.16 : une entrée, la classe attendue et
    /// l'extrait exact du texte normalisé qu'elle couvre.
    #[test]
    fn anchor_classes_of_annex_b_4_16() {
        use AnchorClass::{
            ArticleKeyword, ArticlePrefix, CaaNumber, CcNumber, Code, Court, Ecli, Family,
            LegifranceId, Nor, Pourvoi, Rg, Sigle, Sink, TextNature,
        };
        let cases: &[(&str, AnchorClass, &str)] = &[
            // Mots-clés d'articles et de subdivisions.
            ("vu les articles L. 104-1", ArticleKeyword, "articles"),
            ("vu l'article 9", ArticleKeyword, "article"),
            ("Art. 9 C. civ.", ArticleKeyword, "Art."),
            ("Art 12", ArticleKeyword, "Art"),
            ("le premier alinéa", ArticleKeyword, "alinéa"),
            ("la première phrase", ArticleKeyword, "phrase"),
            // Natures de textes.
            ("la loi n° 2016-1321", TextNature, "loi"),
            ("l'ordonnance n° 58-1067", TextNature, "ordonnance"),
            ("le Décret n° 2019-1333", TextNature, "Décret"),
            ("un arrêté du 12 février 2024", TextNature, "arrêté"),
            ("le règlement du 4 février 2010", TextNature, "règlement"),
            ("L. n° 2016-1321 du 7 oct. 2016", TextNature, "L. n"),
            ("D. n° 2019-1333 du 11 déc. 2019", TextNature, "D. n"),
            ("Ord. n° 2005-1516", TextNature, "Ord."),
            // Juridictions, chambres, formations.
            ("Cour de cassation, chambre civile", Court, "Cour de cassation"),
            ("Cass. civ. 1re", Court, "Cass."),
            ("1re Civ., 25 mai 2023", Court, "Civ."),
            ("Cass., ch. mixte", Court, "ch. mixte"),
            ("Première chambre civile", Court, "Première chambre civile"),
            ("Cour d'appel de Paris", Court, "Cour d'appel"),
            ("Tribunal judiciaire de Lyon", Court, "Tribunal judiciaire"),
            ("Conseil d\u{2019}État, 29 mai 2026", Court, "Conseil d'État"),
            ("Cour administrative d'appel de Nancy", Court, "Cour administrative d'appel"),
            ("Tribunal administratif de Lille", Court, "Tribunal administratif"),
            ("jugement n° 2005064", Court, "jugement"),
            ("arrêt n° 399 F", Court, "arrêt"),
            ("Cons. const., 7 novembre 2025", Court, "Cons. const."),
            ("Conseil constitutionnel", Court, "Conseil constitutionnel"),
            ("sous le n° 506507, enregistrée", Court, "enregistrée"),
            // Codes : titres, abréviations en trois espacements, anaphores.
            ("du code civil", Code, "code civil"),
            ("Code général des impôts, annexe I", Code, "Code général des impôts, annexe I"),
            ("art. 1240 C. civ.", Code, "C. civ."),
            ("art. 1240 C.civ.", Code, "C.civ."),
            ("art. 1240 c civ", Code, "c civ"),
            ("de l'article 20 du même code", Code, "même code"),
            ("de l'article 20 dudit code", Code, "dudit code"),
            // Autres familles.
            ("CNIL, délibération n° 2023-062", Family, "CNIL"),
            ("IDCC 1486", Family, "IDCC"),
            ("Convention collective nationale", Family, "Convention collective"),
            ("Avenant n° 18", Family, "Avenant"),
            ("brochure 3018", Family, "brochure"),
            ("JORF n°0263 du 8 novembre 2025", Family, "JORF"),
            ("Journal officiel de la République française", Family, "Journal officiel"),
            ("NOR : ECFI1524250L", Family, "NOR"),
            (
                "https://www.legifrance.gouv.fr/eli/loi/2016/10/7/ECFI1524250L/jo/texte",
                Family,
                "/eli/",
            ),
            ("l'article 61-1 de la Constitution", Family, "Constitution"),
            ("la Déclaration des droits de l'homme", Family, "Déclaration"),
            ("le Préambule de 1946", Family, "Préambule"),
            ("la Charte de l'environnement de 2004", Family, "Charte de l'environnement"),
            // Puits.
            ("la directive 85/374/CEE", Sink, "directive"),
            ("Règlement (UE) 2016/679", Sink, "Règlement (UE)"),
            ("CJUE, 4 juin 2024", Sink, "CJUE"),
            ("CEDH, 4 mars 2020", Sink, "CEDH"),
            ("JCP G 2019, 12", Sink, "JCP"),
            ("RTD civ. 2019", Sink, "RTD civ."),
            ("Gaz. Pal. 2019", Sink, "Gaz. Pal."),
            ("AJDA 2019", Sink, "AJDA"),
            ("Bull. civ. I, n° 270", Sink, "Bull."),
            ("Rec. Lebon p. 44", Sink, "Rec."),
            ("Leb. p. 44", Sink, "Leb."),
            // Jetons atomiques, sur le texte.
            ("ECLI:FR:CC:2025:2025.1173.QPC", Ecli, "ECLI:"),
            ("ecli:fr:ccass:2025:c100399", Ecli, "ecli:"),
            ("NOR ECFI1524250L", Nor, "ECFI1524250L"),
            ("LEGIARTI000006419288", LegifranceId, "LEGIARTI000006419288"),
            ("…/juri/id/JURITEXT000036780011", LegifranceId, "JURITEXT000036780011"),
            ("pourvoi n° 21-23.174", Pourvoi, "21-23.174"),
            ("pourvoi n° 19-19311", Pourvoi, "19-19311"),
            ("24MA02429", CaaNumber, "24MA02429"),
            ("N° RG 24/15215", Rg, "24/15215"),
            ("décision n° 2025-1173 QPC", CcNumber, "2025-1173 QPC"),
            ("n° 2010-14/22 QPC", CcNumber, "2010-14/22 QPC"),
            ("n° 2015-491R DC", CcNumber, "2015-491R DC"),
            ("n° 2011-4 SEN/QPC", CcNumber, "2011-4 SEN/QPC"),
            ("art. L. 52-8 CE", Sigle, "CE"),
            ("art. 1 du CGI", Sigle, "CGI"),
            ("art. 700 du CPC", Sigle, "CPC"),
            ("art. 121-3 CP", Sigle, "CP"),
            ("article L. 761-1", ArticlePrefix, "L. 7"),
            ("article LO 141", ArticlePrefix, "LO 1"),
            ("article R*111-1", ArticlePrefix, "R*1"),
            ("note sous D. 2016, p. 1234", ArticlePrefix, "D. 2"),
        ];
        for &(input, class, slice) in cases {
            let seen = found(input);
            assert!(has(input, class, slice), "{input:?} : {class:?} {slice:?} dans {seen:?}");
        }
        // Les quinze classes du tableau sont couvertes.
        let covered: BTreeSet<u8> = cases.iter().map(|&(_, class, _)| class as u8).collect();
        assert_eq!(covered.len(), 15);
    }

    #[test]
    fn keywords_need_isolated_words() {
        // `art` dans `part`, `loi` dans `loisir`, `cass` dans `cassation` : pas d'ancre.
        assert_eq!(found("particle"), []);
        assert_eq!(found("le loisir"), []);
        assert_eq!(found("une cassation"), []);
        assert_eq!(found("une loi2"), []);
        // Collé à une ponctuation ou à un chiffre de l'autre côté, il ancre.
        assert!(has("(article 9)", AnchorClass::ArticleKeyword, "article"));
        assert!(has("art.12", AnchorClass::ArticleKeyword, "art."));
        // Un mot-clé qui finit par un point n'exige rien après lui.
        assert!(has("c. civ.art", AnchorClass::Code, "c. civ."));
        // Début et fin du texte.
        assert!(has("article", AnchorClass::ArticleKeyword, "article"));
        assert!(!isolated("particle", 1, 8));
        assert!(isolated("a article b", 2, 9));
    }

    #[test]
    fn tokens_refuse_malformed_numbers() {
        // N39 : pourvoi mal formé ; N22 : NOR court ; N23 : identifiant court ; N10 : ECLI
        // tronqué reste ancré (la grammaire le refuse), mais aucun jeton n'est inventé.
        let has_class =
            |input: &str, class: AnchorClass| found(input).iter().any(|(c, _)| *c == class);
        assert!(has_class("pourvoi n° 21-23.174", AnchorClass::Pourvoi));
        assert!(has_class("NOR ECFI1524250L", AnchorClass::Nor));
        assert!(has_class("LEGIARTI000006419288", AnchorClass::LegifranceId));
        assert!(!has_class("pourvoi n° 21-23.1745", AnchorClass::Pourvoi));
        assert!(!has_class("NOR ECFI152425L", AnchorClass::Nor));
        assert!(!has_class("LEGIARTI00000641928", AnchorClass::LegifranceId));
        assert!(!has_class("LEGIARTI0000064192888", AnchorClass::LegifranceId));
        // P198 : le suffixe de version `_JJ-MM-AAAA` borne l'identifiant comme une frontière.
        assert!(has_class("LEGITEXT000033205014_08-09-2023", AnchorClass::LegifranceId));
        assert!(!has_class("LEGITEXT0000332050144_08-09-2023", AnchorClass::LegifranceId));
        assert!(!has_class("pour la période 2016-1321", AnchorClass::CcNumber));
        assert!(!has_class("aux 1234 RG", AnchorClass::Rg));
        assert!(has("ECLI:FR:CCASS:2025", AnchorClass::Ecli, "ECLI:"));
        // L'identifiant collé à un autre mot n'est pas isolé.
        assert!(!has_class("xLEGIARTI000006419288", AnchorClass::LegifranceId));
        // Un préfixe d'article exige un chiffre.
        assert!(!has_class("la loi L. n° 5", AnchorClass::ArticlePrefix));
        assert!(!has_class("AR 5", AnchorClass::ArticlePrefix));
    }

    #[test]
    fn overlapping_classes_are_all_kept() {
        // `délibération` est une nature de texte et une autre famille ; `civ.` (juridiction)
        // recouvre la fin de `c. civ.` (code).
        let anchors = found("délibération");
        assert_eq!(
            anchors,
            [
                (AnchorClass::TextNature, "délibération".to_owned()),
                (AnchorClass::Family, "délibération".to_owned()),
            ]
        );
        let anchors = found("art. 9 C. civ.");
        assert!(anchors.contains(&(AnchorClass::Code, "C. civ.".to_owned())));
        assert!(anchors.contains(&(AnchorClass::Court, "civ.".to_owned())));
    }

    #[test]
    fn anchors_are_sorted_by_start_end_and_class() {
        let n = normalize(
            "Cass. civ. 1re, 25 mai 2023, pourvoi n° 21-23.174 ; art. L. 761-1 CPC",
            Steps::default(),
        );
        let anchors = find_anchors(&n);
        assert_ne!(anchors.len(), 0);
        let keys: Vec<_> =
            anchors.iter().map(|a| (a.range.start, a.range.end, a.class as u8)).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted);
    }

    #[test]
    fn anchor_ranges_follow_the_normalized_text() {
        // Les offsets sont ceux de `text`, même avec des caractères multi-octets avant l'ancre,
        // des caractères de largeur nulle et des expansions (`№` donne `n°`).
        for input in [
            "été — l\u{2019}article 9",
            "ar\u{200b}ticle 9",
            "№ 5, article 9, Œuvre",
            "ÀÉÎ l'ARTICLE 9 ÇA",
            "ﬁn article 9",
        ] {
            let n = normalize(input, Steps::default());
            let anchors = find_anchors(&n);
            let seen = found(input);
            assert!(
                anchors.iter().any(|a| a.class == AnchorClass::ArticleKeyword
                    && n.text
                        .get(a.range.clone())
                        .is_some_and(|s| s.eq_ignore_ascii_case("article"))),
                "{input:?} -> {seen:?}"
            );
            for a in &anchors {
                assert!(n.text.get(a.range.clone()).is_some(), "{input:?} {a:?}");
                assert!(a.range.start < a.range.end);
            }
        }
        // Texte HTML : les ancres sont lues sur le texte débarrassé de ses balises.
        let n = normalize(
            "<p>art.&nbsp;9 <mark>C. civ.</mark></p>",
            Steps { strip_html: true, ..Steps::default() },
        );
        assert_eq!(n.text, " art. 9 C. civ. ");
        assert!(find_anchors(&n).contains(&Anchor { class: AnchorClass::Code, range: 8..15 }));
    }

    #[test]
    fn code_patterns_are_folded_and_cover_the_dictionary() {
        let patterns = code_patterns();
        assert!(patterns.iter().all(|p| !p.is_empty() && fold_str(p) == *p), "{patterns:?}");
        for pattern in ["code civil", "c. civ.", "c.civ.", "c civ", "meme code", "dudit code"] {
            assert!(patterns.iter().any(|p| p == pattern), "{pattern}");
        }
        // Les titres des 77 codes y sont tous.
        assert!(CODES.iter().all(|e| patterns.contains(&fold_str(e.title))));
        // Les abréviations en majuscules sont des sigles, pas des motifs de la vue clé.
        assert!(!patterns.iter().any(|p| p == "cgi"));
        assert!(sigles().is_superset(&["CGI", "CPC", "CE", "CP", "CAA"].into_iter().collect()));
    }

    #[test]
    fn keyword_lists_are_folded_and_free_of_duplicates() {
        for list in [ARTICLE_KEYWORDS, TEXT_NATURES, COURTS, CODE_ANAPHORAS, FAMILIES, SINKS] {
            let distinct: BTreeSet<_> = list.iter().collect();
            assert_eq!(distinct.len(), list.len(), "{list:?}");
            assert!(list.iter().all(|p| !p.is_empty() && fold_str(p) == *p), "{list:?}");
        }
        assert!(SIGLES.iter().all(|s| is_sigle(s)));
        assert!(!is_sigle("C"));
        assert!(!is_sigle("Cp"));
        assert!(is_sigle("CGI"));
    }

    proptest! {
        #[test]
        fn anchors_of_any_text_are_valid_sorted_ranges(input in ".{0,200}") {
            let n = normalize(&input, Steps::default());
            let anchors = find_anchors(&n);
            let mut last = (0, 0, 0);
            for a in &anchors {
                prop_assert!(a.range.start < a.range.end);
                prop_assert!(n.text.get(a.range.clone()).is_some(), "{input:?} {a:?}");
                let key = (a.range.start, a.range.end, a.class as u8);
                prop_assert!(last <= key);
                last = key;
            }
        }
    }
}
