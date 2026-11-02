//! Correctifs 1.1.1 : tests de bout en bout du parseur de citations, regroupés dans un seul binaire.
//! U03 : un numéro d'article suivi d'une lettre minuscule isolée (« art. 757 b ») est refusé en
//! entier (filet N36), jamais tronqué ni lu comme l'article voisin. Les tests de couverture vivent
//! dans `src/citation/`.
use lf_domain::citation::{CitationRef, parse};

/// Clés d'article lues (article de code ou de texte), dans l'ordre.
fn keys(input: &str) -> Vec<String> {
    parse(input)
        .iter()
        .filter_map(|c| match &c.value {
            CitationRef::CodeArticle { article, .. } | CitationRef::TextArticle { article, .. } => {
                Some(article.num.num_key())
            }
            _ => None,
        })
        .collect()
}

/// Textes des citations lues, dans l'ordre.
fn raws(input: &str) -> Vec<&str> {
    parse(input).iter().filter_map(|c| input.get(c.span.start..c.span.end)).collect()
}

/// V1 à V7, V9, V10, V12 et V13 : aucun article n'est lu, ni le voisin ni le numéro tronqué.
#[test]
fn a_lone_lowercase_suffix_is_never_read_as_the_neighbour_article() {
    for input in [
        "article 787 b du CGI",
        "art. 757 b du code général des impôts",
        "CGI, art. 757 b",
        "Voir CGI, art. 757 b.",
        "l'article 757 b du CGI",
        "art. 1240 b du code civil",
        "article L. 1234-5 b du code du travail",
        "article 150-0 b ter du CGI",
        "article 199 undecies b du CGI",
        "articles 757 et 787 b du CGI",
        "art 757 b code general des impots",
        "articles 757 à 760 b du CGI",
        "articles 5 et 5 bis ter du code civil",
        "loi n° 78-17 du 6 janvier 1978, art. 4 b",
        "article 4 b de la loi n° 78-17 du 6 janvier 1978",
        "article 61 b de la Constitution",
    ] {
        assert_eq!(keys(input), Vec::<String>::new(), "{input}");
    }
}

/// V8 : la liste refusée disparaît en entier, la citation voisine reste lue.
#[test]
fn a_refused_list_leaves_the_neighbour_citation_read() {
    let input = "articles 757 b et 787 b du CGI, puis l'article 1240 du code civil.";
    assert_eq!(keys(input), ["1240"]);
    assert_eq!(raws(input), ["article 1240 du code civil"]);
}

/// Une citation refusée ne change le code d'aucune de ses voisines (composition).
#[test]
fn a_refused_citation_does_not_change_the_code_of_its_neighbours_u03() {
    let input = "Le CGI (art. 777 ; art. 757 b ; art. 787 B) prévoit un barème.";
    assert_eq!(keys(input), ["777", "787 B"]);
    let alone = |piece: &str| parse(piece).len();
    let both = parse("art. 777 du CGI ; art. 757 b du CGI ; art. 787 B du CGI").len();
    assert_eq!(both, alone("art. 777 du CGI") + alone("art. 787 B du CGI"));
}

/// Les jumeaux lus : lettre capitale, abréviations, codes en minuscules, « a » et « y ».
#[test]
fn the_neighbours_of_a_lone_lowercase_suffix_stay_read() {
    assert_eq!(keys("article 787 B du CGI"), ["787 B"]);
    assert_eq!(keys("art. 757 B du CGI"), ["757 B"]);
    assert_eq!(keys("art. 1217 s. C. civ."), ["1217"]);
    assert_eq!(keys("art. 1240 s. C. civ."), ["1240"]);
    assert_eq!(keys("art. L1234-9 c. trav."), ["L1234-9"]);
    assert_eq!(keys("art. 1240 c.c."), ["1240"]);
    assert_eq!(keys("art. 9 c. civ."), ["9"]);
    assert_eq!(keys("art. 1103, alinéa 2 du code civil"), ["1103"]);
    assert_eq!(keys("article 1240 du code civil"), ["1240"]);
    assert_eq!(keys("l'article 5 a prévu du code civil"), ["5"]);
    assert_eq!(keys("article 61 de la Constitution"), ["61"]);
    assert_eq!(keys("article 4 de la loi n° 78-17 du 6 janvier 1978"), ["4"]);
    assert_eq!(keys("loi n° 78-17 du 6 janvier 1978, art. 4"), ["4"]);
}
