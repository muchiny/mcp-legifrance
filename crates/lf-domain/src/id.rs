//! Identifiants Légifrance typés (« parse, don't validate »).
//!
//! Forme vérifiée sur les 164 identifiants des fixtures de la référence Python
//! (`tests/fixtures/*.json`, d3b5b13) : préfixe de 8 lettres + 12 chiffres.
//! Le suffixe de version `_JJ-MM-AAAA` (ex. `LEGITEXT000033205014_08-09-2023`)
//! est retiré à la normalisation (`citations.py:55-63`). Contrairement à la
//! référence, le retrait vaut pour tous les préfixes (elle oublie `CONSTEXT`,
//! `citations.py:34-44`).

use std::{fmt, marker::PhantomData, str::FromStr};

use crate::error::{DomainError, echo};

/// Nombre de lettres du préfixe.
pub const PREFIX_LEN: usize = 8;
/// Nombre de chiffres après le préfixe.
pub const ID_DIGITS: usize = 12;

/// Normalise une entrée : espaces retirés, suffixe `_…` retiré, majuscules ASCII.
/// Renvoie `None` si le reste n'a pas la forme `[A-Z]{8}[0-9]{12}`.
///
/// La découpe se fait au premier `_` : tout ce qui suit est ignoré
/// (`citations.py:55-63`). Divergences assumées :
/// - `CONSTEXT` est accepté, la référence l'oublie (`citations.py:34-44`) ;
/// - la référence ne découpe au premier `_` que si la tête porte un préfixe
///   connu (`citations.py:59-62`), ici la découpe vaut pour tout préfixe de
///   8 lettres ;
/// - `trim` et mise en majuscules ASCII, par tolérance envers les
///   identifiants tapés par un humain ou un modèle (la référence ne le fait pas).
fn normalize(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let head = trimmed.split_once('_').map_or(trimmed, |(head, _)| head);
    let upper = head.to_ascii_uppercase();
    let (prefix, digits) = upper.split_at_checked(PREFIX_LEN)?;
    let well_formed = prefix.bytes().all(|b| b.is_ascii_uppercase())
        && digits.len() == ID_DIGITS
        && digits.bytes().all(|b| b.is_ascii_digit());
    well_formed.then_some(upper)
}

/// Famille d'identifiants : fixe le préfixe attendu.
///
/// Trait scellé (M1-D12, `docs/plan/09-risques-et-questions.md` §3.19) : seules les familles de
/// ce module l'implémentent ; un crate aval ne peut pas en ajouter :
///
/// ```compile_fail
/// enum FakeText {}
///
/// impl lf_domain::id::IdKind for FakeText {
///     const PREFIX: &'static str = "FAKETEXT";
/// }
/// ```
pub trait IdKind: sealed::Sealed {
    /// Préfixe de 8 lettres majuscules, ex. `LEGITEXT`.
    const PREFIX: &'static str;
}

/// Sceau d'`IdKind` : le module est privé, donc `Sealed` ne peut être ni nommé ni implémenté hors
/// du crate.
mod sealed {
    /// Supertrait d'`IdKind`, implémenté par chaque famille de ce crate.
    pub trait Sealed {}
}

/// Identifiant Légifrance validé de la famille `K`.
pub struct Id<K> {
    raw: String,
    kind: PhantomData<fn() -> K>,
}

impl<K: IdKind> Id<K> {
    /// Valide et normalise `input`.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidId`] si la forme ou le préfixe ne convient pas.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        normalize(input)
            .filter(|raw| raw.starts_with(K::PREFIX))
            .map(|raw| Self { raw, kind: PhantomData })
            .ok_or_else(|| DomainError::InvalidId { input: echo(input), expected: K::PREFIX })
    }
}

impl<K> Id<K> {
    /// Forme normalisée, ex. `LEGITEXT000006070721`.
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

impl<K> Clone for Id<K> {
    fn clone(&self) -> Self {
        Self { raw: self.raw.clone(), kind: PhantomData }
    }
}

impl<K> PartialEq for Id<K> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl<K> Eq for Id<K> {}

impl<K> fmt::Debug for Id<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Id").field(&self.raw).finish()
    }
}

impl<K> fmt::Display for Id<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

impl<K: IdKind> FromStr for Id<K> {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Texte consolidé (loi, décret, code) : `LEGITEXT`.
pub enum LegiText {}
impl sealed::Sealed for LegiText {}
impl IdKind for LegiText {
    const PREFIX: &'static str = "LEGITEXT";
}
/// Article consolidé : `LEGIARTI`.
pub enum LegiArti {}
impl sealed::Sealed for LegiArti {}
impl IdKind for LegiArti {
    const PREFIX: &'static str = "LEGIARTI";
}
/// Texte du Journal officiel : `JORFTEXT`.
pub enum JorfText {}
impl sealed::Sealed for JorfText {}
impl IdKind for JorfText {
    const PREFIX: &'static str = "JORFTEXT";
}
/// Convention collective : `KALITEXT`.
pub enum KaliText {}
impl sealed::Sealed for KaliText {}
impl IdKind for KaliText {
    const PREFIX: &'static str = "KALITEXT";
}
/// Article de convention collective : `KALIARTI`.
pub enum KaliArti {}
impl sealed::Sealed for KaliArti {}
impl IdKind for KaliArti {
    const PREFIX: &'static str = "KALIARTI";
}
/// Décision judiciaire : `JURITEXT`.
pub enum JuriText {}
impl sealed::Sealed for JuriText {}
impl IdKind for JuriText {
    const PREFIX: &'static str = "JURITEXT";
}
/// Décision du Conseil constitutionnel : `CONSTEXT`.
pub enum ConsText {}
impl sealed::Sealed for ConsText {}
impl IdKind for ConsText {
    const PREFIX: &'static str = "CONSTEXT";
}
/// Décision administrative : `CETATEXT`.
pub enum CetaText {}
impl sealed::Sealed for CetaText {}
impl IdKind for CetaText {
    const PREFIX: &'static str = "CETATEXT";
}
/// Délibération CNIL : `CNILTEXT`.
pub enum CnilText {}
impl sealed::Sealed for CnilText {}
impl IdKind for CnilText {
    const PREFIX: &'static str = "CNILTEXT";
}
/// Accord d'entreprise : `ACCOTEXT`.
pub enum AccoText {}
impl sealed::Sealed for AccoText {}
impl IdKind for AccoText {
    const PREFIX: &'static str = "ACCOTEXT";
}

/// Section de table des matières consolidée : `LEGISCTA`.
pub enum LegiScta {}
impl sealed::Sealed for LegiScta {}
impl IdKind for LegiScta {
    const PREFIX: &'static str = "LEGISCTA";
}
/// Section de table des matières de convention collective : `KALISCTA`.
pub enum KaliScta {}
impl sealed::Sealed for KaliScta {}
impl IdKind for KaliScta {
    const PREFIX: &'static str = "KALISCTA";
}

/// Identifiant `LEGITEXT…`.
pub type LegiTextId = Id<LegiText>;
/// Identifiant `LEGIARTI…`.
pub type LegiArtiId = Id<LegiArti>;
/// Identifiant `JORFTEXT…`.
pub type JorfTextId = Id<JorfText>;
/// Identifiant `KALITEXT…`.
pub type KaliTextId = Id<KaliText>;
/// Identifiant `KALIARTI…`.
pub type KaliArtiId = Id<KaliArti>;
/// Identifiant `JURITEXT…`.
pub type JuriTextId = Id<JuriText>;
/// Identifiant `CONSTEXT…`.
pub type ConsTextId = Id<ConsText>;
/// Identifiant `CETATEXT…`.
pub type CetaTextId = Id<CetaText>;
/// Identifiant `CNILTEXT…`.
pub type CnilTextId = Id<CnilText>;
/// Identifiant `ACCOTEXT…`.
pub type AccoTextId = Id<AccoText>;

/// Identifiant de forme valide, quel que soit son préfixe (résultats de recherche).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnyId(String);

impl AnyId {
    /// Valide et normalise `input`.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidId`] si la forme ou le préfixe ne convient pas.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        normalize(input)
            .map(Self)
            .ok_or_else(|| DomainError::InvalidId { input: echo(input), expected: "8 letters" })
    }

    /// Forme normalisée.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Article consultable par `/consult/getArticle` (`server.py:284`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArticleId {
    /// Article consolidé `LEGIARTI…`.
    Legi(LegiArtiId),
    /// Article de convention collective `KALIARTI…`.
    Kali(KaliArtiId),
}

impl ArticleId {
    /// Valide `input` contre les deux préfixes acceptés.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidId`] si aucun préfixe ne convient.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        LegiArtiId::parse(input)
            .map(Self::Legi)
            .or_else(|_| KaliArtiId::parse(input).map(Self::Kali))
            .map_err(|_| DomainError::InvalidId {
                input: echo(input),
                expected: "LEGIARTI or KALIARTI",
            })
    }

    /// Forme normalisée.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Legi(id) => id.as_str(),
            Self::Kali(id) => id.as_str(),
        }
    }
}

impl FromStr for ArticleId {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Section de table des matières (`LEGISCTA…` ou `KALISCTA…`). Employée seulement par la table
/// des matières (03 §2.2), jamais rendue en sortie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionId(SectionInner);

#[derive(Debug, Clone, PartialEq, Eq)]
enum SectionInner {
    Legi(Id<LegiScta>),
    Kali(Id<KaliScta>),
}

impl SectionId {
    /// Valide `input` contre les deux préfixes acceptés.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidId`] si aucun préfixe ne convient.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        Id::<LegiScta>::parse(input)
            .map(SectionInner::Legi)
            .or_else(|_| Id::<KaliScta>::parse(input).map(SectionInner::Kali))
            .map(Self)
            .map_err(|_| DomainError::InvalidId {
                input: echo(input),
                expected: "LEGISCTA or KALISCTA",
            })
    }

    /// Forme normalisée.
    pub fn as_str(&self) -> &str {
        match &self.0 {
            SectionInner::Legi(id) => id.as_str(),
            SectionInner::Kali(id) => id.as_str(),
        }
    }
}

impl fmt::Display for SectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SectionId {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Décision consultable par `/consult/juri` (`server.py:103`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionId {
    /// Cour de cassation, cours d'appel : `JURITEXT…`.
    Juri(JuriTextId),
    /// Conseil constitutionnel : `CONSTEXT…`.
    Constit(ConsTextId),
    /// Conseil d'État, CAA, TA : `CETATEXT…`.
    Cetat(CetaTextId),
}

impl DecisionId {
    /// Valide `input` contre les trois préfixes acceptés.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidId`] si aucun préfixe ne convient.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        JuriTextId::parse(input)
            .map(Self::Juri)
            .or_else(|_| ConsTextId::parse(input).map(Self::Constit))
            .or_else(|_| CetaTextId::parse(input).map(Self::Cetat))
            .map_err(|_| DomainError::InvalidId {
                input: echo(input),
                expected: "JURITEXT, CONSTEXT or CETATEXT",
            })
    }

    /// Forme normalisée.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Juri(id) => id.as_str(),
            Self::Constit(id) => id.as_str(),
            Self::Cetat(id) => id.as_str(),
        }
    }
}

impl FromStr for DecisionId {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    #[test]
    fn parses_fixture_ids() {
        // Un identifiant par préfixe, tirés de tests/fixtures/*.json de la référence.
        assert_eq!(
            LegiTextId::parse("LEGITEXT000006070721").unwrap().as_str(),
            "LEGITEXT000006070721"
        );
        assert_eq!(
            LegiArtiId::parse("LEGIARTI000006419288").unwrap().as_str(),
            "LEGIARTI000006419288"
        );
        assert_eq!(
            JorfTextId::parse("JORFTEXT000020445850").unwrap().as_str(),
            "JORFTEXT000020445850"
        );
        assert_eq!(
            KaliTextId::parse("KALITEXT000005661030").unwrap().as_str(),
            "KALITEXT000005661030"
        );
        assert_eq!(
            KaliArtiId::parse("KALIARTI000005805295").unwrap().as_str(),
            "KALIARTI000005805295"
        );
        assert_eq!(
            JuriTextId::parse("JURITEXT000048465611").unwrap().as_str(),
            "JURITEXT000048465611"
        );
        assert_eq!(
            ConsTextId::parse("CONSTEXT000037434572").unwrap().as_str(),
            "CONSTEXT000037434572"
        );
        assert_eq!(
            CetaTextId::parse("CETATEXT000051655317").unwrap().as_str(),
            "CETATEXT000051655317"
        );
        assert_eq!(
            CnilTextId::parse("CNILTEXT000053352643").unwrap().as_str(),
            "CNILTEXT000053352643"
        );
        assert_eq!(
            AccoTextId::parse("ACCOTEXT000051888026").unwrap().as_str(),
            "ACCOTEXT000051888026"
        );
    }

    #[test]
    fn strips_version_suffix() {
        let id = LegiTextId::parse("LEGITEXT000033205014_08-09-2023").unwrap();
        assert_eq!(id.as_str(), "LEGITEXT000033205014");
        // Tout suffixe après le premier « _ » est ignoré (citations.py:55-63).
        let any = LegiTextId::parse("LEGITEXT000006070721_foo").unwrap();
        assert_eq!(any.as_str(), "LEGITEXT000006070721");
        // Suffixe vide : « _ » seul est accepté.
        assert_eq!(
            LegiTextId::parse("LEGITEXT000006070721_").unwrap().as_str(),
            "LEGITEXT000006070721"
        );
    }

    #[test]
    fn strips_version_suffix_on_constext_too() {
        let id = ConsTextId::parse("CONSTEXT000037434572_01-01-2020").unwrap();
        assert_eq!(id.as_str(), "CONSTEXT000037434572");
    }

    #[test]
    fn tolerates_whitespace_and_lowercase() {
        let id = LegiTextId::parse("  legitext000006070721\n").unwrap();
        assert_eq!(id.as_str(), "LEGITEXT000006070721");
    }

    #[test]
    fn rejects_wrong_prefix_with_actionable_error() {
        let err = LegiTextId::parse("JORFTEXT000020445850").unwrap_err();
        assert_eq!(
            err,
            DomainError::InvalidId { input: "JORFTEXT000020445850".into(), expected: "LEGITEXT" }
        );
    }

    #[test]
    fn rejects_malformed_digits() {
        for bad in [
            "",
            "   ",
            "LEGITEXT",
            "LEGITEXT00000607072",   // 11 chiffres
            "LEGITEXT0000060707210", // 13 chiffres
            "LEGITEXT00000607072X",
            "LEGITEX1000006070721",
            "LÉGITEXT000006070721", // préfixe non ASCII
            "_LEGITEXT000006070721",
            "LEGITEXT 000006070721",
            "LEGITEX€000006070721", // multi-octet à la frontière du préfixe (`split_at_checked`)
            // Blanc avant le « _ » : comportement actuel figé (entrée improbable).
            "LEGITEXT000006070721 _x",
        ] {
            assert!(LegiTextId::parse(bad).is_err(), "{bad:?} doit être refusé");
        }
    }

    #[test]
    fn error_echo_is_truncated() {
        let long = "A".repeat(10_000);
        assert_eq!(
            LegiTextId::parse(&long).unwrap_err(),
            DomainError::InvalidId {
                input: format!("{}…", "A".repeat(crate::error::ECHO_MAX_CHARS)),
                expected: "LEGITEXT",
            }
        );
    }

    #[test]
    fn id_traits() {
        let a = LegiTextId::parse("LEGITEXT000006070721").unwrap();
        let b: LegiTextId = "LEGITEXT000006070721".parse().unwrap();
        let c = LegiTextId::parse("LEGITEXT000033205014").unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.clone(), a);
        assert_eq!(a.to_string(), "LEGITEXT000006070721");
        assert_eq!(format!("{a:?}"), "Id(\"LEGITEXT000006070721\")");
        assert!("JORFTEXT000020445850".parse::<LegiTextId>().is_err());
    }

    #[test]
    fn any_id_accepts_every_prefix_shape() {
        assert_eq!(
            AnyId::parse("JORFTEXT000020445850_01-01-2020").unwrap().as_str(),
            "JORFTEXT000020445850"
        );
        assert_eq!(AnyId::parse("KALISCTA000005705795").unwrap().as_str(), "KALISCTA000005705795");
        assert_eq!(
            AnyId::parse("nope").unwrap_err(),
            DomainError::InvalidId { input: "nope".into(), expected: "8 letters" }
        );
    }

    #[test]
    fn article_id_dispatches_on_prefix() {
        let legi = ArticleId::parse("LEGIARTI000006419288").unwrap();
        let kali: ArticleId = "KALIARTI000005805295".parse().unwrap();
        assert_eq!(legi, ArticleId::Legi(LegiArtiId::parse("LEGIARTI000006419288").unwrap()));
        assert_eq!(kali, ArticleId::Kali(KaliArtiId::parse("KALIARTI000005805295").unwrap()));
        assert_eq!(legi.as_str(), "LEGIARTI000006419288");
        assert_eq!(kali.as_str(), "KALIARTI000005805295");
        assert_eq!(
            ArticleId::parse("LEGITEXT000006070721").unwrap_err(),
            DomainError::InvalidId {
                input: "LEGITEXT000006070721".into(),
                expected: "LEGIARTI or KALIARTI",
            }
        );
    }

    #[test]
    fn decision_id_dispatches_on_prefix() {
        let juri = DecisionId::parse("JURITEXT000048465611").unwrap();
        let constit = DecisionId::parse("CONSTEXT000037434572").unwrap();
        let cetat: DecisionId = "CETATEXT000051655317".parse().unwrap();
        assert_eq!(juri, DecisionId::Juri(JuriTextId::parse("JURITEXT000048465611").unwrap()));
        assert_eq!(
            constit,
            DecisionId::Constit(ConsTextId::parse("CONSTEXT000037434572").unwrap())
        );
        assert_eq!(cetat, DecisionId::Cetat(CetaTextId::parse("CETATEXT000051655317").unwrap()));
        assert_eq!(juri.as_str(), "JURITEXT000048465611");
        assert_eq!(constit.as_str(), "CONSTEXT000037434572");
        assert_eq!(cetat.as_str(), "CETATEXT000051655317");
        assert_eq!(
            DecisionId::parse("CNILTEXT000053352643").unwrap_err(),
            DomainError::InvalidId {
                input: "CNILTEXT000053352643".into(),
                expected: "JURITEXT, CONSTEXT or CETATEXT",
            }
        );
    }

    #[test]
    fn section_id_accepts_legi_and_kali_sections() {
        // consult_loda.json et consult_kali.json.
        let legi = SectionId::parse("LEGISCTA000033205040").unwrap();
        let kali: SectionId = " kalisCTA000049886029\n".parse().unwrap();
        assert_eq!(legi.as_str(), "LEGISCTA000033205040");
        assert_eq!(kali.as_str(), "KALISCTA000049886029");
        assert_eq!(legi.to_string(), "LEGISCTA000033205040");
        assert_eq!(kali.to_string(), "KALISCTA000049886029");
        assert_eq!(legi.clone(), legi);
        assert_ne!(legi, kali);
    }

    #[test]
    fn section_id_rejects_other_prefixes() {
        for bad in ["LEGIARTI000033205126", "KALITEXT000005661030", "LEGISCTA00003320504", ""] {
            assert_eq!(
                SectionId::parse(bad).unwrap_err(),
                DomainError::InvalidId { input: bad.into(), expected: "LEGISCTA or KALISCTA" },
                "{bad:?}"
            );
        }
    }

    /// Chaque famille accepte son préfixe et refuse celui des onze autres.
    macro_rules! each_kind_rejects_every_other_prefix {
        ($($kind:ty),+ $(,)?) => {{
            let prefixes = [$(<$kind>::PREFIX),+];
            $(
                let expected = <$kind>::PREFIX;
                for prefix in prefixes {
                    let input = format!("{prefix}000006070721");
                    let parsed = Id::<$kind>::parse(&input);
                    assert_eq!(parsed.is_ok(), prefix == expected, "{input} as {expected}");
                }
            )+
        }};
    }

    #[test]
    fn each_kind_rejects_every_other_prefix() {
        each_kind_rejects_every_other_prefix!(
            LegiText, LegiArti, JorfText, KaliText, KaliArti, JuriText, ConsText, CetaText,
            CnilText, AccoText, LegiScta, KaliScta,
        );
    }

    proptest! {
        #[test]
        fn any_id_roundtrips(
            prefix in "[a-zA-Z]{8}",
            digits in "[0-9]{12}",
            pad in "[ \\t]{0,3}",
        ) {
            let id = AnyId::parse(&format!("{pad}{prefix}{digits}{pad}")).unwrap();
            prop_assert_eq!(id.as_str(), format!("{}{digits}", prefix.to_ascii_uppercase()));
            prop_assert_eq!(AnyId::parse(id.as_str()).unwrap(), id);
        }

        #[test]
        fn section_id_roundtrips(
            prefix in "(LEGISCTA|KALISCTA|legiscta|kalisCTA)",
            digits in "[0-9]{12}",
        ) {
            let id = SectionId::parse(&format!(" {prefix}{digits} ")).unwrap();
            prop_assert_eq!(id.as_str(), format!("{}{digits}", prefix.to_ascii_uppercase()));
            prop_assert_eq!(SectionId::parse(id.as_str()).unwrap(), id);
        }

        #[test]
        fn parse_never_panics(s in "\\PC*") {
            let _ = LegiTextId::parse(&s);
            let _ = AnyId::parse(&s);
            let _ = ArticleId::parse(&s);
            let _ = DecisionId::parse(&s);
            let _ = SectionId::parse(&s);
        }

        #[test]
        fn valid_shape_roundtrips(digits in "[0-9]{12}", suffix in "(_[0-9]{2}-[0-9]{2}-[0-9]{4})?") {
            let raw = format!("LEGITEXT{digits}");
            let id = LegiTextId::parse(&format!("{raw}{suffix}")).unwrap();
            prop_assert_eq!(id.as_str(), raw.as_str());
            prop_assert_eq!(LegiTextId::parse(id.as_str()).unwrap(), id);
        }
    }
}
