//! L1-01 : suffixes d'article « 150-0 B ter » et « 199 terdecies-0 A » (E2E resolve-3, gap3-1,
//! gap3-6). Garde de bout en bout ; les tests de couverture vivent dans `src/citation/`.
use lf_domain::citation::{CitationRef, parse};

/// Textes des citations lues, dans l'ordre.
fn raws(input: &str) -> Vec<&str> {
    parse(input).iter().filter_map(|c| input.get(c.span.start..c.span.end)).collect()
}

/// Clés d'article lues, dans l'ordre.
fn keys(input: &str) -> Vec<String> {
    parse(input)
        .iter()
        .filter_map(|c| match &c.value {
            CitationRef::CodeArticle { article, .. } => Some(article.num.num_key()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_e2e_sentence_gives_three_citations_with_complete_numbers() {
    let input = "Le CGI (art. 777 ; art. 150-0 B ter ; art. 199 terdecies-0 A) prévoit un barème.";
    assert_eq!(raws(input), ["art. 777", "art. 150-0 B ter", "art. 199 terdecies-0 A"]);
    assert_eq!(keys(input), ["777", "150-0 B ter", "199 terdecies-0 A"]);
}

#[test]
fn a_code_after_the_number_is_attached_and_never_to_the_neighbour_article() {
    for input in [
        "article 150-0 B ter du code général des impôts",
        "art. 150-0 B ter du CGI",
        "art. 150-0 B ter CGI",
        "CGI, art. 150-0 B ter.",
    ] {
        assert_eq!(keys(input), ["150-0 B ter"], "{input}");
    }
    assert_eq!(keys("art. 199 terdecies-0 A du code général des impôts"), ["199 terdecies-0 A"]);
}

#[test]
fn neighbour_readings_stay_green() {
    assert_eq!(keys("art. 150-0 B du CGI"), ["150-0 B"]);
    assert_eq!(keys("art. 199 undecies B du CGI"), ["199 undecies B"]);
    assert_eq!(keys("art. 121-3 CP"), ["121-3"]);
    assert_eq!(keys("art. 163 quatervicies du CGI"), ["163 quatervicies"]);
}

#[test]
fn a_number_is_never_truncated() {
    // « 885-0 V bis A » est un article réel : jamais lu comme « 885-0 V bis ».
    for input in ["art. 885-0 V bis A du CGI", "art. 5 bis ter du code civil"] {
        assert_eq!(keys(input), Vec::<String>::new(), "{input}");
    }
}
