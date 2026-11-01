//! L1-07 : articles du TFUE, de la CEDH et du RGPD reconnus comme puits (E2E verify_citations-15).
//! Garde de bout en bout ; les tests de couverture vivent dans `src/citation/grammar/puits.rs`.
use lf_domain::citation::{CitationKind, CitationRef, UnsupportedFamily, parse};

/// Textes et familles des puits lus, dans l'ordre.
fn sinks(input: &str) -> Vec<(String, UnsupportedFamily)> {
    parse(input)
        .iter()
        .filter_map(|c| match &c.value {
            CitationRef::Unsupported(u) => {
                input.get(c.span.start..c.span.end).map(|t| (t.to_owned(), u.family))
            }
            _ => None,
        })
        .collect()
}

/// Nombre de citations d'article de code lues.
fn code_articles(input: &str) -> usize {
    parse(input).iter().filter(|c| c.value.kind() == CitationKind::CodeArticle).count()
}

#[test]
fn the_e2e_sentence_gives_six_sinks_and_no_code_article() {
    let input = "Le règlement (UE) 2016/679 du 27 avril 2016 (RGPD), art. 17. La directive \
                 2000/31/CE. CEDH, 4 mai 2000, Rotaru c. Roumanie, n° 28341/95. L'article 8 de \
                 la Convention européenne des droits de l'homme. Art. 267 TFUE. CJUE, 6 octobre \
                 2015, Schrems, C-362/14.";
    let found = sinks(input);
    assert_eq!(found.len(), 6, "{found:?}");
    assert_eq!(code_articles(input), 0);
    let families: Vec<_> = found.iter().map(|(_, f)| *f).collect();
    assert_eq!(
        families,
        [
            UnsupportedFamily::Ue,
            UnsupportedFamily::Ue,
            UnsupportedFamily::Cedh,
            UnsupportedFamily::Cedh,
            UnsupportedFamily::Ue,
            UnsupportedFamily::Cjue,
        ]
    );
}

#[test]
fn each_article_alone_is_one_sink_and_never_a_code_article() {
    for input in [
        "Art. 267 TFUE",
        "L'article 8 de la Convention européenne des droits de l'homme",
        "article 6 § 1 de la CEDH",
        "article 17 du RGPD",
        "Le règlement (UE) 2016/679 du 27 avril 2016 (RGPD), art. 17",
    ] {
        assert_eq!(sinks(input).len(), 1, "{input}");
        assert_eq!(code_articles(input), 0, "{input}");
    }
}

#[test]
fn french_neighbours_are_still_read_as_code_articles() {
    assert_eq!(code_articles("art. L. 52-8 CE"), 1);
    assert_eq!(code_articles("article 121-3 CP"), 1);
    assert_eq!(code_articles("article 5 du code civil"), 1);
    assert_eq!(sinks("article 5 de la Convention collective nationale"), []);
    assert_eq!(sinks("article 10 de la directive 85/374/CEE").len(), 1);
    // Un article français voisin d'un puits garde sa lecture.
    assert_eq!(code_articles("Art. 267 TFUE ; art. 1240 du code civil"), 1);
}
