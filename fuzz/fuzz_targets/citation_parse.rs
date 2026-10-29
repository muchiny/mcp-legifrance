#![no_main]

use std::str::FromStr;

use lf_domain::{
    citation::{
        ArticleNum, CaaNumber, CcNumber, CnilNumber, ParseOptions, Pourvoi, RgNumber, TaNumber,
        TextContext, format, parse_with,
    },
    contract::{Eli, Nor},
    date::CivilDate,
    ecli::Ecli,
};
use libfuzzer_sys::fuzz_target;

/// Réglages tirés des octets d'entrée : les quatre options de `ParseOptions` (annexe B §6.1), dont
/// la borne `max_citations`, qui doit tronquer sans jamais dépasser.
fn options(bits: u8, max: u8) -> ParseOptions {
    ParseOptions {
        strip_html: bits & 1 != 0,
        repair_mojibake: bits & 2 != 0,
        default_text: (bits & 4 != 0).then(TextContext::default),
        max_citations: usize::from(max) % 40,
        today: if bits & 8 != 0 { CivilDate::new(2026, 10, 15) } else { None },
    }
}

fuzz_target!(|input: (&str, u8, u8)| {
    let (data, bits, max) = input;
    let opts = options(bits, max);
    let found = parse_with(data, &opts);

    // « Aucune panique, spans valides » (03 §8.8) : borne respectée, span non vide qui tombe sur
    // des frontières de caractère, sortie triée par début puis par longueur décroissante.
    assert!(found.len() <= opts.max_citations);
    let chars = data.chars().count();
    for citation in &found {
        let span = citation.span;
        assert!(span.start < span.end);
        assert!(data.get(span.start..span.end).is_some());
        // La vue MCP convertit les spans en points de code Unicode.
        let in_chars = span.to_chars(data);
        assert!(in_chars.start < in_chars.end && in_chars.end <= chars);
        // Forme canonique et résolvabilité se calculent pour toute valeur reconnue.
        let _ = format::canonical(&citation.value);
        let _ = citation.value.resolvability();
        let _ = citation.effective_resolvability();
    }
    let order = |c: &lf_domain::citation::Spanned<lf_domain::citation::CitationRef>| {
        (c.span.start, std::cmp::Reverse(c.span.end))
    };
    assert!(found.windows(2).all(|w| order(&w[0]) <= order(&w[1])));

    // La pile de contexte repart de zéro à chaque appel : un second appel redonne la sortie.
    assert_eq!(found, parse_with(data, &opts));

    // Les numéros typés et les identifiants lisent n'importe quelle chaîne sans paniquer.
    if let Ok(num) = ArticleNum::from_str(data) {
        assert!(!num.key_variants().is_empty());
    }
    let _ = Ecli::parse(data);
    let _ = Nor::parse(data);
    let _ = Eli::parse(data);
    // Un numéro lu se réécrit sous une forme que le même type relit (annexe B §6.4).
    if let Ok(pourvoi) = Pourvoi::from_str(data) {
        assert!(Pourvoi::from_str(&pourvoi.to_string()).is_ok());
    }
    if let Ok(rg) = RgNumber::from_str(data) {
        assert!(RgNumber::from_str(&rg.to_string()).is_ok());
    }
    if let Ok(caa) = CaaNumber::from_str(data) {
        assert_eq!(CaaNumber::from_str(&caa.to_string()), Ok(caa));
    }
    if let Ok(ta) = TaNumber::from_str(data) {
        assert_eq!(TaNumber::from_str(&ta.to_string()), Ok(ta));
    }
    if let Ok(cc) = CcNumber::from_str(data) {
        assert_eq!(CcNumber::from_str(&cc.full()), Ok(cc.clone()));
    }
    if let Ok(cnil) = CnilNumber::from_str(data) {
        assert_eq!(CnilNumber::from_str(&cnil.to_string()), Ok(cnil.clone()));
    }
});
