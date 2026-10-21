#![no_main]

use lf_domain::{
    contract::{CITATION_MAX_CHARS, CitationText, Eli},
    date::CivilDate,
    ecli::Ecli,
    fond::Fond,
    id::{AnyId, ArticleId, DecisionId, LegiTextId, SectionId, parse_versioned},
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &str| {
    // La normalisation est idempotente.
    if let Ok(id) = AnyId::parse(data) {
        assert_eq!(AnyId::parse(id.as_str()), Ok(id.clone()));
    }
    let _ = LegiTextId::parse(data);
    let _ = ArticleId::parse(data);
    let _ = DecisionId::parse(data);
    let _ = Ecli::parse(data);
    let _ = Fond::parse(data);
    // Toute date acceptée se relit à l'identique.
    if let Ok(date) = CivilDate::parse(data) {
        assert_eq!(CivilDate::parse(&date.to_string()), Ok(date));
    }

    // Une date de version gardée se reformate en suffixe `_JJ-MM-AAAA` qui se
    // relit. Le second essai impose une tête valide : sans corpus de départ, le
    // fuzzeur atteint alors le suffixe.
    for input in [data, &format!("LEGITEXT000033205014{data}")] {
        if let Ok(versioned) = parse_versioned(input) {
            let reformatted = match versioned.version_start {
                Some(date) => format!(
                    "{}_{:02}-{:02}-{:04}",
                    versioned.id,
                    date.day(),
                    date.month(),
                    date.year()
                ),
                None => versioned.id.to_string(),
            };
            assert_eq!(parse_versioned(&reformatted), Ok(versioned));
        }
    }
    for input in [data, &format!("LEGISCTA000033205040{data}")] {
        if let Ok(section) = SectionId::parse(input) {
            assert_eq!(SectionId::parse(section.as_str()), Ok(section.clone()));
        }
    }
    // La forme absolue d'un ELI se relit à l'identique.
    if let Some(eli) = Eli::parse(data) {
        assert_eq!(Eli::parse(eli.as_str()), Some(eli.clone()));
    }
    // Invariants de CitationText : jamais vide, une ligne rognée, aucun
    // contrôle, au plus CITATION_MAX_CHARS caractères ; le texte rendu se relit
    // tel quel.
    if let Some(citation) = CitationText::new(data) {
        let text = citation.as_str();
        assert!(!text.is_empty() && text.trim() == text);
        assert!(text.chars().count() <= CITATION_MAX_CHARS);
        assert!(text.chars().all(|c| c == ' ' || !(c.is_whitespace() || c.is_control())));
        assert_eq!(CitationText::new(text).as_ref().map(CitationText::as_str), Some(text));
    }
});
