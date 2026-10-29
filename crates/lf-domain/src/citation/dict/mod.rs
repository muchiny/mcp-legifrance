//! Dictionnaires statiques du parseur de citations (annexe B §5 et §4.5 à §4.9).
//!
//! - [`codes`] : les 77 codes, leurs titres, abréviations et sigles ambigus ;
//! - [`courts`] : listes fermées de juridictions (chambres de cassation, formations du Conseil
//!   d'État, sièges des cours et des tribunaux, types du Conseil constitutionnel).

pub mod codes;
pub mod courts;
