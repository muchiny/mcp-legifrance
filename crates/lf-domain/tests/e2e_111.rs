//! Correctifs 1.1.1 : tests de bout en bout du parseur de citations, regroupés dans un seul binaire.
//! U03 : un numéro d'article suivi d'une lettre minuscule isolée (« art. 757 b ») est refusé en
//! entier (filet N36), jamais tronqué ni lu comme l'article voisin. Les tests de couverture vivent
//! dans `src/citation/`.
//! U07 : les décisions et mises en demeure de la CNIL à numéro à série (« décision n°
//! MED-2024-150 ») sont lues sans « CNIL » ni « délibération » devant, et les numéros d'ordre à
//! quatre chiffres (`DF-2017-1448`) sont lus quand une série est écrite.
use lf_domain::citation::{CitationRef, CnilNumber, Confidence, parse};

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

/// Citations CNIL lues : texte du span, série, année, numéro d'ordre, confiance.
fn cnil(input: &str) -> Vec<(String, Option<String>, u16, u16, Confidence)> {
    parse(input)
        .iter()
        .filter_map(|c| match &c.value {
            CitationRef::CnilDeliberation(n) => Some((
                input.get(c.span.start..c.span.end).unwrap_or_default().to_owned(),
                n.series.clone(),
                n.year,
                n.seq,
                c.confidence,
            )),
            _ => None,
        })
        .collect()
}

/// Genre (nom de variante) et texte de chaque citation lue.
fn shape(input: &str) -> Vec<(String, String)> {
    parse(input)
        .iter()
        .map(|c| {
            let debug = format!("{:?}", c.value);
            let kind = debug.split(['(', ' ', '{']).next().unwrap_or_default().to_owned();
            (kind, input.get(c.span.start..c.span.end).unwrap_or_default().to_owned())
        })
        .collect()
}

/// U07, cnil_kali_acco-7 : la phrase du test E2E donne trois citations de la CNIL, aux spans exacts.
#[test]
fn the_e2e_sentence_reads_two_cnil_decisions() {
    let input = "La décision n° MED-2024-150 du 15 novembre 2024 met en demeure le ministère de \
                 l'intérieur ; voir la décision DF-2017-1448 et la délibération n° 2025-131 du \
                 18 décembre 2025.";
    let read = cnil(input);
    let texts: Vec<&str> = read.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(
        texts,
        [
            "décision n° MED-2024-150 du 15 novembre 2024",
            "décision DF-2017-1448",
            "délibération n° 2025-131 du 18 décembre 2025",
        ],
        "{read:?}"
    );
    assert_eq!(
        read.iter().map(|r| (r.1.as_deref(), r.2, r.3)).collect::<Vec<_>>(),
        [(Some("MED"), 2024, 150), (Some("DF"), 2017, 1448), (None, 2025, 131)]
    );
    // Les deux séries sont de confiance haute.
    assert_eq!((read[0].4, read[1].4), (Confidence::High, Confidence::High));
    // Les trois citations, et rien d'autre.
    assert_eq!(parse(input).len(), 3);
}

/// U07 : une décision lue ne dépend pas de ses voisines (« délibération » à moins de 80 caractères
/// ne doit plus décider seule de la lecture).
#[test]
fn decision_without_cnil_is_read_whatever_the_neighbours() {
    let alone = cnil("La décision DR-2024-196 du 26 juillet 2024.");
    assert_eq!(alone.len(), 1, "{alone:?}");
    assert_eq!(alone[0].0, "décision DR-2024-196 du 26 juillet 2024");
    assert_eq!((alone[0].1.as_deref(), alone[0].2, alone[0].3), (Some("DR"), 2024, 196));

    let crowded = "La mise en demeure MED-2023-018 du 3 avril 2023, puis la délibération n° \
                   MEDP-2023-001 du 13 avril 2023 ; décision DR-2024-196 du 26 juillet 2024.";
    let read = cnil(crowded);
    assert_eq!(
        read.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
        [
            "mise en demeure MED-2023-018 du 3 avril 2023",
            "délibération n° MEDP-2023-001 du 13 avril 2023",
            "décision DR-2024-196 du 26 juillet 2024",
        ],
        "{read:?}"
    );
    // La même citation, quelles que soient les voisines : celle de la phrase seule.
    assert_eq!(read[2].1, alone[0].1);
    assert_eq!((read[2].2, read[2].3, read[2].4), (alone[0].2, alone[0].3, alone[0].4));
    // Sans « délibération » voisine, les deux premières restent lues.
    assert_eq!(
        cnil("La mise en demeure MED-2023-018 du 3 avril 2023, puis la décision MEDP-2023-001.")
            .len(),
        2
    );
}

/// U07 : chaque décision CNIL ancrée par « CNIL » ET par « décision » ne donne qu'une citation.
#[test]
fn one_citation_per_cnil_decision_through_parse() {
    for (input, wanted) in [
        ("CNIL, décision n° MED-2018-024", "CNIL, décision n° MED-2018-024"),
        ("CNIL, mise en demeure MED-2018-024", "CNIL, mise en demeure MED-2018-024"),
        (
            "Délibération SAN-2026-002 du 8 janvier 2026",
            "Délibération SAN-2026-002 du 8 janvier 2026",
        ),
    ] {
        let read = cnil(input);
        assert_eq!(read.len(), 1, "{input:?} : {read:?}");
        assert_eq!(read[0].0, wanted);
        assert_eq!(parse(input).len(), 1, "{input:?}");
    }
    let two =
        cnil("CNIL, décision DR-2024-196 du 26 juillet 2024 ; CNIL, mise en demeure MED-2022-034");
    assert_eq!(two.len(), 2, "{two:?}");
}

/// U07, garde : les décisions du Conseil constitutionnel, du Conseil d'État ou des communes ne
/// deviennent jamais des citations CNIL ; leur lecture est celle de la 1.1.0.
#[test]
fn non_cnil_decisions_are_untouched() {
    for input in [
        "décision n° 2020-092",
        "décision n° 2019-123",
        "Décision n° 2023-062",
        "décision n° 2025-1173",
    ] {
        assert_eq!(cnil(input), [], "{input:?}");
        assert_eq!(shape(input), [("Decision".to_owned(), input.to_owned())], "{input:?}");
    }
    assert_eq!(
        shape("décision n° 2020-092 QPC"),
        [("Decision".to_owned(), "décision n° 2020-092 QPC".to_owned())]
    );
    for input in ["mise en demeure n° 2023-062", "décision PC-2023-045", "décision AG-2024-001"]
    {
        assert_eq!(shape(input), [], "{input:?}");
    }
}

/// U07, garde : les décisions du Conseil constitutionnel (toutes formes) sont lues à l'identique.
#[test]
fn cc_ce_decisions_are_byte_identical_before_after() {
    for input in [
        "décision n° 60-233 AN",
        "décision n° 59-219/222 SEN",
        "décision n° 59-2 ORGA",
        "décision n° 2010-14/22 QPC",
        "décision n° 2026-1226 QPC",
        "décision n° 97-393 DC",
    ] {
        assert_eq!(cnil(input), [], "{input:?}");
        assert_eq!(shape(input), [("Decision".to_owned(), input.to_owned())], "{input:?}");
    }
}

/// U07, garde : une série inconnue reste ouverte derrière « CNIL » ou « délibération » (comme en
/// 1.1.0), mais silencieuse derrière « décision » seul.
#[test]
fn open_series_stays_open_when_cnil_or_deliberation_is_written() {
    let read = cnil("CNIL, décision MLD-2016-253");
    assert_eq!(read.len(), 1, "{read:?}");
    assert_eq!(read[0].4, Confidence::High);
    assert_eq!(cnil("délibération PC-2023-045").len(), 1);
    assert_eq!(cnil("décision MLD-2016-253"), []);
}

/// U07 : la forme canonique d'un numéro à quatre chiffres se relit.
#[test]
fn a_four_digit_cnil_order_is_read_back_from_its_canonical_form() {
    let number = "DF-2017-1448".parse::<CnilNumber>();
    assert_eq!(number.as_ref().map(ToString::to_string).ok().as_deref(), Some("DF-2017-1448"));
    let read = cnil("CNIL, décision DF-2017-1448");
    assert_eq!(read.len(), 1, "{read:?}");
    assert_eq!((read[0].1.as_deref(), read[0].2, read[0].3), (Some("DF"), 2017, 1448));
}
