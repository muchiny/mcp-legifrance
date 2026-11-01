//! L1-04 : numéros et ECLI du Conseil constitutionnel à année sur deux chiffres (E2E
//! decisions-11). Garde de bout en bout ; les tests de couverture vivent dans `src/citation/`.
use lf_domain::citation::{CaseNumber, CitationRef, Court, parse};

/// Numéros du Conseil constitutionnel lus : forme canonique et type.
fn cc_numbers(input: &str) -> Vec<String> {
    parse(input)
        .iter()
        .filter_map(|c| match &c.value {
            CitationRef::Decision(d) if d.court == Some(Court::ConseilConstitutionnel) => {
                d.numbers.iter().find_map(|n| match n {
                    CaseNumber::Cc(cc) => Some(cc.full()),
                    _ => None,
                })
            }
            _ => None,
        })
        .collect()
}

fn eclis(input: &str) -> Vec<(String, Vec<String>)> {
    parse(input)
        .iter()
        .filter_map(|c| match &c.value {
            CitationRef::Ecli(e) => {
                Some((e.as_str().to_owned(), c.flags.iter().map(|f| format!("{f:?}")).collect()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn the_e2e_decision_is_read_as_a_constitutional_council_decision() {
    for input in [
        "Conseil constitutionnel, décision n° 71-44 DC du 16 juillet 1971",
        "Cons. const., décision n° 71-44 DC du 16 juillet 1971",
        "Cons. const., 16 juill. 1971, n° 71-44 DC",
        "DC 71-44",
    ] {
        assert_eq!(cc_numbers(input), ["71-44 DC"], "{input}");
    }
    assert_eq!(cc_numbers("Décision n° 82-132 DC du 16 janvier 1982"), ["82-132 DC"]);
    assert_eq!(
        cc_numbers("Cons. const., décision n° 94-343/344 DC du 27 juillet 1994"),
        ["94-343/344 DC"]
    );
}

#[test]
fn text_numbers_with_two_digit_years_are_not_decisions() {
    for input in [
        "la loi n° 58-1067 du 28 octobre 1958",
        "pourvoi n° 93-12.345",
        "le décret n° 93-1419 L du 29 décembre 1993",
    ] {
        assert_eq!(cc_numbers(input), Vec::<String>::new(), "{input}");
    }
}

#[test]
fn the_native_ecli_of_1971_is_conform() {
    assert_eq!(
        eclis("ECLI:FR:CC:1971:71.44.DC"),
        [("ECLI:FR:CC:1971:71.44.DC".to_owned(), Vec::<String>::new())]
    );
    assert_eq!(eclis("ECLI:FR:CC:2010:2010.14.QPC").len(), 1);
    let bad = eclis("ECLI:FR:CC:1971:05.44.DC");
    assert_eq!(bad.len(), 1);
    assert_eq!(bad[0].1.len(), 1);
}
