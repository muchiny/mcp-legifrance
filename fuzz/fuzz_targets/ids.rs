#![no_main]

use lf_domain::{
    date::CivilDate,
    ecli::Ecli,
    fond::Fond,
    id::{AnyId, ArticleId, DecisionId, LegiTextId},
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
});
