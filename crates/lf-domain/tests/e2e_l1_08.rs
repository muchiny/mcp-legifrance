//! L1-08 : article cité après le texte, « loi n° X du …, art. 1er, I, 2° » (E2E gap3-5).
//! Garde de bout en bout ; les tests de couverture vivent dans `src/citation/grammar/textes.rs`.
use lf_domain::citation::{CitationRef, CodeRef, parse};

/// Une lecture : (kind, texte du span, clé d'article, nature et numéro du texte ou code).
type Read = (&'static str, String, String, String);

fn reads(input: &str) -> Vec<Read> {
    parse(input)
        .iter()
        .filter_map(|c| {
            let raw = input.get(c.span.start..c.span.end)?.to_owned();
            Some(match &c.value {
                CitationRef::TextArticle { text: Some(t), article, .. } => (
                    "text_article",
                    raw,
                    article.num.num_key(),
                    format!("{:?} {}", t.nature, t.number.as_ref().map_or("-", |n| n.raw.as_str())),
                ),
                CitationRef::Text(t) => (
                    "text",
                    raw,
                    String::new(),
                    format!("{:?} {}", t.nature, t.number.as_ref().map_or("-", |n| n.raw.as_str())),
                ),
                CitationRef::CodeArticle { code, article, .. } => (
                    "code_article",
                    raw,
                    article.num.num_key(),
                    match code {
                        CodeRef::Known(key) => format!("{key:?}"),
                        CodeRef::Missing => "Missing".to_owned(),
                        other => format!("{other:?}"),
                    },
                ),
                _ => return None,
            })
        })
        .collect()
}

fn kinds(input: &str) -> Vec<&'static str> {
    reads(input).into_iter().map(|r| r.0).collect()
}

#[test]
fn the_e2e_sentence_attaches_the_article_to_the_law() {
    let input = "Voir loi n° 2016-1321 du 7 octobre 2016, art. 1er, I, 2°, ainsi que le décret \
                 n° 2019-1500 du 27 décembre 2019.";
    let found = reads(input);
    assert_eq!(
        found.iter().map(|r| r.0).collect::<Vec<_>>(),
        ["text_article", "text"],
        "{found:?}"
    );
    assert_eq!(found[0].1, "loi n° 2016-1321 du 7 octobre 2016, art. 1er, I, 2°");
    assert_eq!(found[0].2, "1er");
    assert_eq!(found[0].3, "Loi 2016-1321");
    assert_eq!(found[1].3, "Decret 2019-1500");
}

#[test]
fn an_enumeration_after_a_decree_is_one_group() {
    let found = reads("décret n° 2019-1500 du 27 décembre 2019, article 3 et 4");
    assert_eq!(
        found.iter().map(|r| (r.0, r.2.as_str())).collect::<Vec<_>>(),
        [("text_article", "3"), ("text_article", "4")]
    );
}

#[test]
fn the_repros_of_the_defect_are_text_articles() {
    assert_eq!(kinds("art. 1 de la loi n° 2018-493 du 20 juin 2018"), ["text_article"]);
    assert_eq!(kinds("loi n° 2018-493 du 20 juin 2018, art. 1er"), ["text_article"]);
    assert_eq!(kinds("loi 89-462, art. 22"), ["text_article"]);
    assert_eq!(kinds("loi 78-17"), ["text"]);
    assert_eq!(kinds("la loi 78-17, article 4"), ["text_article"]);
}

#[test]
fn an_article_of_a_code_is_never_attached_to_the_law() {
    for input in [
        "loi n° 2016-1321 du 7 octobre 2016 ; art. 9 C. civ.",
        "loi n° 2016-1321 du 7 octobre 2016, art. 1240 du code civil",
        "loi n° 2016-1321 du 7 octobre 2016, art. 9 C. civ.",
        "loi n° 2016-1321 du 7 octobre 2016, art. 9 CPC",
        "loi n° 2016-1321 du 7 octobre 2016, art. L. 1234-9 C. trav.",
        "la loi susvisée, art. 4",
    ] {
        assert!(!kinds(input).contains(&"text_article"), "{input} : {:?}", reads(input));
    }
    assert_eq!(
        reads("loi n° 2016-1321 du 7 octobre 2016, art. 9 C. civ.")
            .iter()
            .map(|r| (r.0, r.3.as_str()))
            .collect::<Vec<_>>(),
        [("text", "Loi 2016-1321"), ("code_article", "CodeCivil")]
    );
}

#[test]
fn a_decision_number_followed_by_a_type_letter_is_not_a_decree() {
    assert!(!kinds("décret 93-1419 L").contains(&"text"), "{:?}", reads("décret 93-1419 L"));
    assert_eq!(kinds("décret 93-1419"), ["text"]);
    assert_eq!(kinds("décret 93-1419 du 31 décembre 1993"), ["text"]);
}
