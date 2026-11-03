//! Correctifs 1.1.1 : tests de bout en bout du parseur de citations, regroupés dans un seul binaire.
//! U03 : un numéro d'article suivi d'une lettre minuscule isolée (« art. 757 b ») est refusé en
//! entier (filet N36), jamais tronqué ni lu comme l'article voisin. Les tests de couverture vivent
//! dans `src/citation/`.
//! U07 : les décisions et mises en demeure de la CNIL à numéro à série (« décision n°
//! MED-2024-150 ») sont lues sans « CNIL » ni « délibération » devant, et les numéros d'ordre à
//! quatre chiffres (`DF-2017-1448`) sont lus quand une série est écrite.
//! U08 : deux décisions du Conseil d'État citées à la suite ne sont jamais lues comme une seule
//! (la date de l'une, le numéro de l'autre).
//! U11 : une décision du Conseil d'État à numéros joints (tiret ou barre) suivie d'une autre citation
//! ne fusionne pas avec elle.
use lf_domain::citation::{
    CaseNumber, CitationRef, CnilNumber, Confidence, Court, ParseOptions, parse, parse_with,
};
use proptest::prelude::*;

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

/// Décisions du Conseil d'État lues : texte du span, date (`aaaa-mm-jj`) et numéros de requête.
fn ce_decisions(input: &str, strip_html: bool) -> Vec<(String, String, Vec<u32>)> {
    let options = ParseOptions { strip_html, ..ParseOptions::default() };
    parse_with(input, &options)
        .iter()
        .filter_map(|c| match &c.value {
            CitationRef::Decision(d) if matches!(d.court, Some(Court::ConseilEtat { .. })) => {
                let numbers = d
                    .numbers
                    .iter()
                    .filter_map(|n| match n {
                        CaseNumber::CeRequete(n) => Some(*n),
                        _ => None,
                    })
                    .collect();
                Some((
                    input.get(c.span.start..c.span.end).unwrap_or_default().to_owned(),
                    d.date.map(|date| date.to_string()).unwrap_or_default(),
                    numbers,
                ))
            }
            _ => None,
        })
        .collect()
}

/// Un seul triplet attendu : la seconde référence, seule, avec sa date et son numéro.
fn second_only(input: &str, strip_html: bool, text: &str) {
    assert_eq!(
        ce_decisions(input, strip_html),
        [(text.to_owned(), "2010-07-23".to_owned(), vec![328_757])],
        "{input:?}"
    );
}

/// U08, V0 à V4, V6, V7 et V9 : la date de la première référence n'est jamais prêtée au numéro de
/// la seconde. Chaînes reconstituées d'après le défaut related_diff-5 (non relevées).
#[test]
fn v1_to_v7_and_neighbours() {
    // V1 : séparateur « ; », parties et page du Lebon.
    second_only(
        "CE, Ass., 30 mars 1966, Compagnie générale d'énergie radio-électrique, p. 257 ; CE, 23 juillet 2010, Soc. X, n° 328757.",
        false,
        "CE, 23 juillet 2010, Soc. X, n° 328757",
    );
    // V2 : sans parties.
    second_only(
        "CE, Ass., 30 mars 1966, p. 257 ; CE, 23 juillet 2010, n° 328757.",
        false,
        "CE, 23 juillet 2010, n° 328757",
    );
    // V3 : « , et ».
    second_only(
        "CE, Ass., 30 mars 1966, Cie X, p. 257, et CE, 23 juillet 2010, n° 328757.",
        false,
        "CE, 23 juillet 2010, n° 328757",
    );
    // V4 : « . » après le Lebon ; la mention « Leb. » de la seconde est reprise.
    second_only(
        "CE, Ass., 30 mars 1966, Compagnie générale, Leb. p. 257. CE, 23 juillet 2010, Société Y, n° 328757, Leb.",
        false,
        "CE, 23 juillet 2010, Société Y, n° 328757, Leb.",
    );
    // V6 : forme Légifrance en toutes lettres.
    second_only(
        "Conseil d'État, Assemblée, 30 mars 1966, Cie X ; Conseil d'État, 23 juillet 2010, Y, n° 328757",
        false,
        "Conseil d'État, 23 juillet 2010, Y, n° 328757",
    );
    // V7 : retour à la ligne, `<br/>` (citationJp) et `<br>`.
    second_only(
        "CE, Ass., 30 mars 1966, Cie X, p. 257\nCE, 23 juillet 2010, Y, n° 328757",
        false,
        "CE, 23 juillet 2010, Y, n° 328757",
    );
    second_only(
        "CE, Ass., 30 mars 1966, Cie X, p. 257<br/>CE, 23 juillet 2010, Y, n° 328757",
        true,
        "CE, 23 juillet 2010, Y, n° 328757",
    );
    // V9 : « Assemblée » en toutes lettres et liste numérotée.
    second_only(
        "1. CE, Assemblée, 30 mars 1966, Cie X, p. 257<br/>2. CE, 23 juillet 2010, Y, n° 328757",
        true,
        "CE, 23 juillet 2010, Y, n° 328757",
    );
    // Trois références : la première (sans numéro) disparaît, les deux autres restent entières.
    assert_eq!(
        ce_decisions(
            "CE, Ass., 30 mars 1966, Cie X, p. 257 ; CE, 23 juillet 2010, n° 328757 ; CE, 5 mai 2011, n° 400000",
            false
        ),
        [
            ("CE, 23 juillet 2010, n° 328757".to_owned(), "2010-07-23".to_owned(), vec![328_757]),
            ("CE, 5 mai 2011, n° 400000".to_owned(), "2011-05-05".to_owned(), vec![400_000]),
        ]
    );
}

/// U08, V5 : une seconde référence SANS ancre « CE » (forme du Lebon, date ISO) ne prête pas son
/// numéro à la première, et n'est pas lue seule.
#[test]
fn v5_a_second_reference_without_an_anchor_is_not_read() {
    for input in [
        "CE, Ass., 30 mars 1966, Cie X, p. 257 ; Section, 2010-07-23, Y, n° 328757.",
        "CE, Ass., 30 mars 1966, Cie X ; Section, 23 juillet 2010, Y, n° 328757.",
        "CE, Ass., 30 mars 1966, Cie X ; 23 juillet 2010, Y, n° 328757.",
    ] {
        assert_eq!(ce_decisions(input, false), [], "{input:?}");
    }
}

/// U08, V8 : une seconde référence d'une autre juridiction, avec une date « du », n'est pas avalée.
#[test]
fn v8_a_foreign_second_reference_is_not_swallowed() {
    for input in [
        "CE, Ass., 30 mars 1966, Cie X, p. 257, ainsi que TA Lyon du 4 mai 2000, Y, n° 123456",
        "CE, Ass., 30 mars 1966, Cie X, TA Lyon du 4 mai 2000, Y, n° 123456",
        "CE, Ass., 30 mars 1966, Cie X, CAA Paris du 4 mai 2000, Y, n° 123456",
        "CE, Ass., 30 mars 1966, Cie X, Cass. civ. 1re du 4 mai 2000, Y, n° 123456",
    ] {
        assert!(
            ce_decisions(input, false).iter().all(|d| d.1 != "1966-03-30"),
            "{input:?} : {:?}",
            ce_decisions(input, false)
        );
    }
}

/// U08, garde : les voisins d'une référence du Conseil d'État qui ne sont pas du Conseil d'État
/// restent lus seuls (le Conseil constitutionnel et la CEDH ne fusionnent pas avec le CE).
#[test]
fn neighbours_of_other_courts_stay_read_alone() {
    let kinds = |input: &str| -> Vec<(String, String)> { shape(input) };
    for (input, other) in [
        (
            "CE, Ass., 30 mars 1966, Cie X, p. 257 ; Cons. const., 22 juillet 2010, n° 2010-14/22 QPC",
            ("Decision", "Cons. const., 22 juillet 2010, n° 2010-14/22 QPC"),
        ),
        (
            "CE, Ass., 30 mars 1966, Cie X, p. 257 ; CEDH, 4 mai 2000, Rotaru c. Roumanie, n° 28341/95",
            ("Unsupported", "CEDH, 4 mai 2000, Rotaru c. Roumanie, n° 28341/95"),
        ),
    ] {
        let read = kinds(input);
        assert_eq!(read, [(other.0.to_owned(), other.1.to_owned())], "{input:?}");
    }
}

/// U08, garde : des parties qui portent une date « du », un « ; » entre deux noms ou une date de
/// procédure après une virgule restent des parties (comme en 1.1.0).
#[test]
fn legit_parties_stay_one_decision() {
    for (input, date, number) in [
        (
            "CE, 3 mars 2020, Commune de Lyon (arrêté du 5 mai 2018), n° 123456 ; CE, Ass., 30 mars 1966, Compagnie Générale, n° 50515",
            "2020-03-03",
            123_456,
        ),
        ("CE, 3 mars 2020, Nicolo ; Boisdet, n° 108243", "2020-03-03", 108_243),
        ("CE, 5 mars 1999, Rouquette, 12 mai 2018, n° 194658", "1999-03-05", 194_658),
        ("CE, ass., 3 févr. 1989, Compagnie Alitalia, n° 74052, Rec. p. 44", "1989-02-03", 74_052),
    ] {
        let read = ce_decisions(input, false);
        assert_eq!((read[0].1.as_str(), read[0].2.clone()), (date, vec![number]), "{input:?}");
    }
    // La seconde référence du premier cas est lue avec sa propre date : la vraie décision de 1966.
    let both = ce_decisions(
        "CE, 3 mars 2020, Commune de Lyon (arrêté du 5 mai 2018), n° 123456 ; CE, Ass., 30 mars 1966, Compagnie Générale, n° 50515",
        false,
    );
    assert_eq!(both.len(), 2);
    assert_eq!((both[1].1.as_str(), both[1].2.clone()), ("1966-03-30", vec![50_515]));
}

/// U08, garde : les formes à référence unique du corpus restent lues entières, à l'identique.
#[test]
fn every_single_reference_ce_form_of_the_corpus_is_unchanged() {
    for (input, date, numbers) in [
        (
            "Conseil d'État, 2ème - 7ème chambres réunies, 29/05/2026, 506507",
            "2026-05-29",
            vec![506_507],
        ),
        (
            "Conseil d'Etat, 7 / 5 SSR, du 28 décembre 2001, 216642, mentionné aux tables du recueil Lebon",
            "2001-12-28",
            vec![216_642],
        ),
        (
            "CE, 14 avril 2023, n° 462797, mentionné aux tables du Recueil Lebon",
            "2023-04-14",
            vec![462_797],
        ),
        ("CE, Sect., 22 février 2007, req. n° 264541, Leb., p. 541", "2007-02-22", vec![264_541]),
        ("CE, 5e et 6e ch. réunies, 10 mars 2025, n° 489701", "2025-03-10", vec![489_701]),
        ("CE, ord., 27 juillet 2021, n° 454852", "2021-07-27", vec![454_852]),
        ("CE, 3 mars 2020, Nicolo ; Boisdet, n° 108243", "2020-03-03", vec![108_243]),
    ] {
        assert_eq!(
            ce_decisions(input, false),
            [(input.to_owned(), date.to_owned(), numbers)],
            "{input:?}"
        );
    }
}

/// U08, garde : une longue liste de références est lue en temps borné.
#[test]
fn a_long_chain_of_ce_references_is_read_in_bounded_time() {
    let input = "CE, 1 janvier 2000, X ; ".repeat(2_000);
    let started = std::time::Instant::now();
    let read = parse(&input);
    assert!(read.len() <= 1_000);
    assert!(started.elapsed() < std::time::Duration::from_secs(20), "{:?}", started.elapsed());
}

/// Une référence du Conseil d'État tirée au hasard.
#[derive(Clone, Debug)]
struct Reference {
    formation: &'static str,
    parties: &'static str,
    page: bool,
    number: Option<u32>,
    day: u32,
    month: (&'static str, u32),
    year: u32,
}

const MONTHS: [(&str, u32); 6] =
    [("mars", 3), ("juillet", 7), ("mai", 5), ("octobre", 10), ("janvier", 1), ("décembre", 12)];

impl Reference {
    fn date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month.1, self.day)
    }

    fn text(&self) -> String {
        let head = format!(
            "CE, {}{} {} {}, {}",
            self.formation, self.day, self.month.0, self.year, self.parties
        );
        match self.number {
            Some(n) => format!("{head}n° {n}"),
            None if self.page => format!("{head}p. 257"),
            None => head.trim_end_matches([',', ' ']).to_owned(),
        }
    }
}

fn reference(years: std::ops::Range<u32>, numbered: bool) -> impl Strategy<Value = Reference> {
    (
        prop::sample::select(vec!["", "Ass., ", "Sect., "]),
        prop::sample::select(vec!["", "Cie X, ", "Société Y, "]),
        any::<bool>(),
        10_000u32..999_999,
        1u32..=28,
        prop::sample::select(MONTHS.to_vec()),
        years,
    )
        .prop_map(move |(formation, parties, page, number, day, month, year)| Reference {
            formation,
            parties,
            page,
            number: numbered.then_some(number),
            day,
            month,
            year,
        })
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::default() })]

    /// U08 : deux références à la suite, quel que soit le séparateur, ne se mélangent jamais.
    /// Volet 1 : chaque décision lue a la date et le numéro d'une seule des deux références.
    /// Volet 2 : la seconde référence (ancre CE, date, numéro) est toujours lue entière.
    #[test]
    fn no_cross_pairing(
        first in reference(1960..2000, false),
        second in reference(2000..2025, true),
        separator in prop::sample::select(vec![" ; ", ", ", ", et ", ". ", "\n", " puis "]),
    ) {
        let (a, b) = (first.text(), second.text());
        let input = format!("{a}{separator}{b}.");
        let read = ce_decisions(&input, false);
        for (_, date, numbers) in &read {
            if let Some(n) = numbers.first() {
                prop_assert!(
                    *date == second.date() && Some(*n) == second.number,
                    "{input:?} : {read:?}"
                );
            }
        }
        prop_assert!(
            read.iter().any(|(_, date, numbers)| *date == second.date()
                && numbers.as_slice() == [second.number.unwrap_or_default()]),
            "{input:?} : {read:?}"
        );
    }

    /// U08 : la même propriété quand la première référence porte aussi un numéro : les deux sont
    /// lues, chacune avec sa date et son numéro.
    #[test]
    fn two_numbered_references_are_both_read(
        first in reference(1960..2000, true),
        second in reference(2000..2025, true),
        separator in prop::sample::select(vec![" ; ", ", ", ", et ", ". ", "\n", " puis "]),
    ) {
        let input = format!("{}{separator}{}.", first.text(), second.text());
        let read = ce_decisions(&input, false);
        let pairs: Vec<(String, u32)> =
            read.iter().filter_map(|d| Some((d.1.clone(), *d.2.first()?))).collect();
        prop_assert!(pairs.contains(&(first.date(), first.number.unwrap_or_default())), "{input:?} : {read:?}");
        prop_assert!(pairs.contains(&(second.date(), second.number.unwrap_or_default())), "{input:?} : {read:?}");
        prop_assert_eq!(pairs.len(), 2, "{:?} : {:?}", input, read);
    }
}

// U12 : un pourvoi de la Cour de cassation cité avec une date à tirets, un sigle collé, un
// numéro entre parenthèses ou une chambre sans date est lu en entier ; la tête qui ne l'est pas
// est signalée par un drapeau.
mod u12 {
    use lf_domain::citation::dict::courts::CassChamber;
    use lf_domain::citation::{Ambiguity, CitationRef, Court, Flag, Spanned, parse};
    use lf_domain::date::CivilDate;
    use proptest::prelude::*;

    /// Une décision lue : texte du span, date, chambre, drapeau de tête non lue, ambiguïté.
    type Read = (String, Option<CivilDate>, Option<CassChamber>, bool, Option<Ambiguity>);

    fn reads(input: &str) -> Vec<Read> {
        parse(input)
            .iter()
            .filter_map(|c| {
                let CitationRef::Decision(d) = &c.value else { return None };
                let Some(Court::CourDeCassation { chamber }) = d.court else { return None };
                Some((
                    input.get(c.span.start..c.span.end)?.to_owned(),
                    d.date,
                    chamber,
                    c.flags.contains(&Flag::TeteDecisionNonLue),
                    c.ambiguity.clone(),
                ))
            })
            .collect()
    }

    fn day(y: u16, m: u8, d: u8) -> Option<CivilDate> {
        CivilDate::new(y, m, d)
    }

    const POURVOI: &str = "n° 14-24.444";

    /// V1 à V8 : les dates à tirets et le sigle collé sont lus en entier, chambre et date comprises.
    #[test]
    fn the_reported_drafts_and_variants_v1_to_v17() {
        let whole = |input: &str, date, chamber| {
            assert_eq!(reads(input), [(input.to_owned(), date, chamber, false, None)], "{input}");
        };
        // V1 à V3 : les trois entrées du rapport.
        whole("Cass. crim. 01-01-2000, n° 14-24.444", day(2000, 1, 1), Some(CassChamber::Crim));
        whole("Cass. soc. 25-11-2015, n° 14-24.444", day(2015, 11, 25), Some(CassChamber::Soc));
        whole("Cass. crim. 25-11-2015, n° 14-24.444", day(2015, 11, 25), Some(CassChamber::Crim));
        // V4.
        // Le sigle collé à « Cass. » : le span commence au sigle (comme « Cass.crim. »).
        whole("soc. 25-11-2015 n° 14-24.444", day(2015, 11, 25), Some(CassChamber::Soc));
        let got = reads("Cass.soc. 25-11-2015 n° 14-24.444");
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].0, "soc. 25-11-2015 n° 14-24.444");
        whole("Cass. soc., le 25-11-2015, n° 14-24.444", day(2015, 11, 25), Some(CassChamber::Soc));
        whole(
            "Cour de cassation, chambre criminelle, 25-11-2015, n° 14-24.444",
            day(2015, 11, 25),
            Some(CassChamber::Crim),
        );
        // « ch. soc. » : la lecture commence au sigle, comme en 1.1.0.
        whole("soc. 25/11/2015 n° 14-24.444", day(2015, 11, 25), Some(CassChamber::Soc));
        // V5 : sans « n° », la date à tirets est lue.
        whole("Cass. soc. 25-11-2015, 14-24.444", day(2015, 11, 25), Some(CassChamber::Soc));
        // V6 à V8 : sigle collé.
        whole("civ.1re, 4 juin 2025, n° 24-13.470", day(2025, 6, 4), Some(CassChamber::Civ1));
        let got = reads("Cass.civ.1re, 4 juin 2025, n° 24-13.470");
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].0, "civ.1re, 4 juin 2025, n° 24-13.470");
        let got = reads("Cass.civ.2e, 4 juin 2025, n° 24-13.470");
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].2, Some(CassChamber::Civ2));
        let got = reads("Cass.civ.1re, 4 juin 2024, n° 24-13.470");
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].1, day(2024, 6, 4));
        // V9 à V12 : une seule citation, le numéro est dans le span (V12 : le tiret ne lie pas).
        whole("Cass. com., 12 mars 1999 (n° 14-24.444)", day(1999, 3, 12), Some(CassChamber::Com));
        for input in [
            "Cass. crim. 25 novembre 2015 (pourvoi n° 14-24.444)",
            "Cass. soc. du 25 novembre 2015 (pourvoi n° 14-24.444)",
            "Cass. soc., arrêt du 25 novembre 2015 (n° 14-24.444)",
            "Cass. soc., 25 novembre 2015, [n° 14-24.444]",
            "Cass. soc., 25 novembre 2015 [n° 14-24.444]",
        ] {
            let got = reads(input);
            assert_eq!(got.len(), 1, "{input} : {got:?}");
            assert_eq!(got[0].0, input, "{input}");
            assert_eq!(got[0].1, day(2015, 11, 25), "{input}");
            assert!(!got[0].3, "{input}");
        }
        let got = reads("Cass. crim., 25 novembre 2015 - n° 14-24.444");
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(got[0].4, Some(Ambiguity::NumeroManquant));
        assert!(got[1].3, "V12 : le pourvoi lu seul porte le drapeau : {got:?}");
        // V13 à V16 : tête non lisible, signalée.
        for input in [
            "Cass. crim., 25 novembre 2015, req. n° 14-24.444",
            "Cass. crim., 25/11/15, n° 14-24.444",
            "Cass. soc., 25.11.15, n° 14-24.444",
            "Cass. crim., nov. 2015, n° 14-24.444",
        ] {
            let got = reads(input);
            let flagged = got.iter().filter(|r| r.3).count();
            assert_eq!(flagged, 1, "{input} : {got:?}");
        }
        // V16 : chambre sans date, lue (AM-84), comparée par l'application.
        let got = reads("Cass. soc., pourvoi n° 24-13.470");
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].0, "Cass. soc., pourvoi n° 24-13.470");
        assert_eq!(got[0].2, Some(CassChamber::Soc));
        // V17 : sans « n° » derrière la parenthèse, le pourvoi n'est plus perdu.
        let got = reads("Cass. soc. 25 novembre 2015 (14-24.444)");
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].0, "Cass. soc. 25 novembre 2015 (14-24.444)");
    }

    /// Garde : les formes déjà complètes et le pourvoi nu gardent leur lecture (contrôles OK du rapport).
    #[test]
    fn the_complete_forms_and_the_bare_number_are_read_as_in_1_1_0() {
        for (input, date, chamber) in [
            (
                "Cass. soc., 25 novembre 2015, n° 14-24.444",
                day(2015, 11, 25),
                Some(CassChamber::Soc),
            ),
            (
                "Cass. com., 25 novembre 2015, n° 14-24.444",
                day(2015, 11, 25),
                Some(CassChamber::Com),
            ),
            ("Cass. crim. 25/11/2015 n° 14-24.444", day(2015, 11, 25), Some(CassChamber::Crim)),
            ("Cass. crim. 25.11.2015 n° 14-24.444", day(2015, 11, 25), Some(CassChamber::Crim)),
            (
                "Cass. civ. 1ère, 4 juin 2025, n° 24-13.470",
                day(2025, 6, 4),
                Some(CassChamber::Civ1),
            ),
            ("Cass. 1re civ., 4 juin 2025, n° 24-13.470", day(2025, 6, 4), Some(CassChamber::Civ1)),
            ("Civ. 1re, 4 juin 2025, 24-13.470", day(2025, 6, 4), Some(CassChamber::Civ1)),
        ] {
            let got = reads(input);
            assert_eq!(got.len(), 1, "{input} : {got:?}");
            assert_eq!(got[0].0, input, "{input}");
            assert_eq!((got[0].1, got[0].2, got[0].3), (date, chamber, false), "{input}");
        }
        assert_eq!(reads(POURVOI), [(POURVOI.to_owned(), None, None, false, None)]);
    }

    /// Les trois entrées du rapport : le nombre de citations lues.
    #[test]
    fn the_reported_sentence_reads_three_decisions() {
        let input = "Cass. crim. 01-01-2000, n° 14-24.444 ; Cass. com., 12 mars 1999 (n° 14-24.444) ; Cass. crim. 1/1/2000 n° 14-24.444";
        let got = reads(input);
        assert_eq!(got.len(), 3, "{got:?}");
        assert_eq!(got[0].0, "Cass. crim. 01-01-2000, n° 14-24.444");
        assert_eq!(got[1].0, "Cass. com., 12 mars 1999 (n° 14-24.444)");
        assert_eq!(got[2].0, "Cass. crim. 1/1/2000 n° 14-24.444");
    }

    /// Les familles voisines lisent aussi les dates à tirets, comme les dates à barres.
    #[test]
    fn neighbour_families_keep_their_slash_reading_and_gain_dashes() {
        for (dashed, slashed) in [
            ("CE, 12-03-2020, n° 123456", "CE, 12/03/2020, n° 123456"),
            ("CAA Paris, 12-03-2020, n° 18PA01234", "CAA Paris, 12/03/2020, n° 18PA01234"),
            (
                "Cons. const., décision n° 2020-123 QPC du 12-03-2020",
                "Cons. const., décision n° 2020-123 QPC du 12/03/2020",
            ),
            ("TA Lyon, 12-03-2020, n° 1801234", "TA Lyon, 12/03/2020, n° 1801234"),
        ] {
            let slash = parse(slashed);
            assert!(!slash.is_empty(), "{slashed}");
            let dash = parse(dashed);
            let values =
                |v: &[Spanned<CitationRef>]| v.iter().map(|c| c.value.clone()).collect::<Vec<_>>();
            assert_eq!(values(&dash), values(&slash), "{dashed}");
            assert_eq!(dash.len(), slash.len(), "{dashed}");
        }
    }

    /// Garde (AM-82) : la famille « fond » (cours d'appel, tribunaux judiciaires) lit aussi les
    /// dates à tirets, de bout en bout : même valeur et même étendue que la forme à barres, avec la
    /// date écrite lue.
    #[test]
    fn the_fond_family_reads_dashed_dates() {
        for (dashed, slashed, date) in [
            (
                "Cour d'appel de Limoges, 21-05-2015, 14/00554",
                "Cour d'appel de Limoges, 21/05/2015, 14/00554",
                day(2015, 5, 21),
            ),
            (
                "CA Paris, pôle 5, ch. 9, 4-04-2024, n° 23/13690",
                "CA Paris, pôle 5, ch. 9, 4/04/2024, n° 23/13690",
                day(2024, 4, 4),
            ),
            (
                "Cour d'appel de Paris, Pôle 1 - Chambre 12, 23-06-2025, n°24/15215",
                "Cour d'appel de Paris, Pôle 1 - Chambre 12, 23/06/2025, n°24/15215",
                day(2025, 6, 23),
            ),
        ] {
            let slash = parse(slashed);
            assert_eq!(slash.len(), 1, "{slashed}");
            let dash = parse(dashed);
            assert_eq!(dash.len(), 1, "{dashed}");
            assert_eq!(dash[0].value, slash[0].value, "{dashed}");
            assert_eq!((dash[0].span.start, dash[0].span.end), (0, dashed.len()), "{dashed}");
            assert!(
                matches!(&dash[0].value, CitationRef::Decision(read)
                    if matches!(read.court, Some(Court::CourAppel { .. })) && read.date == date),
                "{dashed} : {:?}",
                dash[0].value
            );
        }
    }

    /// Garde : les numéros d'affaire à tirets ne sont jamais lus comme des dates, ni les lois à tirets.
    #[test]
    fn numbered_cases_with_dashes_are_never_dates() {
        let value = |input: &str| parse(input).into_iter().map(|c| c.value).collect::<Vec<_>>();
        for input in [
            "Cons. const., décision n° 2020-123 QPC",
            "Cons. const., décision n° 71-44 DC",
            "Cass. soc., n° 10-12.345",
            "CE, n° 12-12-345",
        ] {
            let found = value(input);
            for v in &found {
                if let CitationRef::Decision(d) = v {
                    assert_eq!(d.date, None, "{input} : {d:?}");
                }
            }
        }
        // Le lecteur des lois ne change pas (garantie 14 n° 8).
        let found = parse("loi n° 89-462 du 6-07-1989");
        let again = parse("loi n° 89-462 du 6-07-1989");
        assert_eq!(found, again);
    }

    proptest! {
        /// Une date valide à tirets, à barres ou à points donne la même lecture ; le numéro entre
        /// parenthèses ou après la virgule aussi, pour toutes les chambres.
        #[test]
        fn dash_slash_and_parenthesis_forms_parse_alike(
            d in 1_u8..=28, m in 1_u8..=12, y in 1950_u16..=2025,
            chamber in prop::sample::select(vec!["soc.", "crim.", "com.", "civ. 1re", "civ. 2e", "civ. 3e", "ass. plén."]),
        ) {
            let canon = format!("Cass. {chamber}, {d}/{m}/{y}, n° 14-24.444");
            let want = parse(&canon);
            prop_assert_eq!(want.len(), 1);
            for variant in [
                format!("Cass. {chamber}, {d}-{m}-{y}, n° 14-24.444"),
                format!("Cass. {chamber}, {d}.{m}.{y}, n° 14-24.444"),
                format!("Cass. {chamber}, {d}-{m}-{y} (n° 14-24.444)"),
                format!("Cass. {chamber}, {d}/{m}/{y} [n° 14-24.444]"),
            ] {
                let got = parse(&variant);
                prop_assert_eq!(got.len(), 1, "{}", variant);
                prop_assert_eq!(&got[0].value, &want[0].value, "{}", variant);
                prop_assert_eq!(got[0].span.end.saturating_sub(got[0].span.start), variant.len(), "{}", variant);
            }
        }
    }
}

/// U11 (AM-83), composition : une décision du Conseil d'État à numéros joints par un tiret ou une
/// barre, suivie d'une autre décision, donne deux citations aux spans disjoints, la première
/// avec ses deux numéros, la seconde avec le sien, quel que soit le séparateur et ce qui suit.
#[test]
fn a_joined_ce_citation_followed_by_another_never_merges_with_it() {
    let first = |link: &str| format!("CE, Ass., 5 mars 1999, n° 194658{link}196116");
    let second = "CE, 5 mai 2011, n° 400000";
    let numbers = |c: &lf_domain::citation::Spanned<CitationRef>| match &c.value {
        CitationRef::Decision(d) => d.numbers.clone(),
        _ => Vec::new(),
    };

    for link in ["-", "/"] {
        for sep in [" ; ", ". ", ", ", " et "] {
            for after in ["", ",", " et", ", publié au recueil Lebon", " ; art. 9 C. civ."] {
                let head = first(link);
                let input = format!("{head}{sep}{second}{after}");
                let got = parse(&input);
                let decisions: Vec<_> =
                    got.iter().filter(|c| matches!(c.value, CitationRef::Decision(_))).collect();
                assert_eq!(decisions.len(), 2, "{input}");
                assert_eq!(
                    numbers(decisions[0]),
                    [CaseNumber::CeRequete(194_658), CaseNumber::CeRequete(196_116)],
                    "{input}"
                );
                assert_eq!(numbers(decisions[1]), [CaseNumber::CeRequete(400_000)], "{input}");
                assert_eq!(
                    input.get(decisions[0].span.start..decisions[0].span.end),
                    Some(head.as_str()),
                    "{input}"
                );
                assert!(decisions[0].span.end <= decisions[1].span.start, "{input}");
                assert!(
                    input[decisions[1].span.start..decisions[1].span.end].starts_with(second),
                    "{input}"
                );
            }
        }
    }
}
