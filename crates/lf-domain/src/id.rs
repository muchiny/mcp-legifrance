//! Identifiants Légifrance typés (« parse, don't validate »).
//!
//! Forme vérifiée sur les 164 identifiants des fixtures de la référence Python
//! (`tests/fixtures/*.json`, d3b5b13) : préfixe de 8 lettres + 12 chiffres.
//! Le suffixe de version `_JJ-MM-AAAA` (ex. `LEGITEXT000033205014_08-09-2023`)
//! est retiré à la normalisation (`citations.py:55-63`). Contrairement à la
//! référence, le retrait vaut pour tous les préfixes (elle oublie `CONSTEXT`,
//! `citations.py:34-44`).
//!
//! # Suffixe de version
//!
//! Un suffixe est bien formé s'il compte exactement dix octets `JJ-MM-AAAA`
//! (chiffres ASCII) formant une vraie date, années 1 à 9999. Tout autre
//! suffixe (`_` seul, `_foo`, `_31-02-2023`, `_2023-09-08`…) est refusé par
//! [`DomainError::BadSuffix`], quels que soient le préfixe et l'analyseur ; la
//! référence jetait tout suffixe sans contrôle (`citations.py:55-63`).
//! [`parse_versioned`] lit un `LEGITEXT` et garde la date du suffixe (date de
//! version par défaut de `fr_get_act`, `version_start` des résultats de
//! recherche) ; les autres analyseurs ([`Id::parse`] et `FromStr`,
//! [`AnyId::parse`], [`ArticleId::parse`], [`DecisionId::parse`],
//! [`SectionId::parse`]) tolèrent un suffixe bien formé puis le retirent, sur
//! tout préfixe, `LEGITEXT` compris. Signaler un suffixe amont hors `LEGITEXT`
//! (diagnostic, compteur) revient au mapper : le domaine ne journalise pas.
//!
//! Ordre des contrôles : forme de la tête, puis préfixe attendu, puis suffixe.
//! Lu comme `LEGITEXT`, `JORFTEXT000020445850_foo` donne donc `InvalidId` (la
//! famille d'abord, message le plus utile) et `LEGITEXT000033205014_foo`
//! donne `BadSuffix`. Tous les analyseurs tolèrent les espaces autour et la
//! casse.

use std::{
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
    str::FromStr,
};

use crate::{
    date::CivilDate,
    error::{DomainError, echo},
};

/// Nombre de lettres du préfixe.
pub const PREFIX_LEN: usize = 8;
/// Nombre de chiffres après le préfixe.
pub const ID_DIGITS: usize = 12;

/// Normalise une entrée : espaces retirés, majuscules ASCII. Renvoie la tête
/// normalisée et le suffixe brut après le premier `_`, ou `None` si la tête n'a
/// pas la forme `[A-Z]{8}[0-9]{12}`.
///
/// La découpe se fait au premier `_`. Divergences assumées :
/// - `CONSTEXT` est accepté, la référence l'oublie (`citations.py:34-44`) ;
/// - la référence ne découpe au premier `_` que si la tête porte un préfixe
///   connu (`citations.py:59-62`), ici la découpe vaut pour tout préfixe de
///   8 lettres ;
/// - le suffixe est contrôlé (`version_suffix`), la référence le jette
///   (`citations.py:55-63`) ;
/// - `trim` et mise en majuscules ASCII, par tolérance envers les
///   identifiants tapés par un humain ou un modèle (la référence ne le fait pas).
fn normalize(input: &str) -> Option<(String, Option<&str>)> {
    let trimmed = input.trim();
    let (head, suffix) = match trimmed.split_once('_') {
        Some((head, suffix)) => (head, Some(suffix)),
        None => (trimmed, None),
    };
    let upper = head.to_ascii_uppercase();
    let (prefix, digits) = upper.split_at_checked(PREFIX_LEN)?;
    let well_formed = prefix.bytes().all(|b| b.is_ascii_uppercase())
        && digits.len() == ID_DIGITS
        && digits.bytes().all(|b| b.is_ascii_digit());
    well_formed.then_some((upper, suffix))
}

/// Date d'un suffixe `JJ-MM-AAAA` (sans le `_`), `None` si la forme ou la date
/// est invalide.
fn version_suffix(suffix: &str) -> Option<CivilDate> {
    let &[d1, d2, b'-', m1, m2, b'-', y1, y2, y3, y4] = suffix.as_bytes() else {
        return None;
    };
    CivilDate::parse_iso(&[y1, y2, y3, y4, b'-', m1, m2, b'-', d1, d2])
}

/// Lit `input` : tête, puis préfixe (`accepts`), puis suffixe. Tête malformée ou
/// préfixe refusé : `InvalidId` ; suffixe mal formé : `BadSuffix`.
fn read(
    input: &str,
    accepts: impl Fn(&str) -> bool,
    expected: &'static str,
) -> Result<(String, Option<CivilDate>), DomainError> {
    let (raw, suffix) = normalize(input)
        .filter(|(raw, _)| accepts(raw))
        .ok_or_else(|| DomainError::InvalidId { input: echo(input), expected })?;
    match suffix.map(version_suffix) {
        None => Ok((raw, None)),
        Some(Some(date)) => Ok((raw, Some(date))),
        Some(None) => Err(DomainError::BadSuffix { input: echo(input) }),
    }
}

/// Analyseur d'une famille d'identifiants.
type Parser<T> = fn(&str) -> Result<T, DomainError>;

/// Essaie chaque famille dans l'ordre. Seul `InvalidId` fait passer à la
/// suivante, donc `BadSuffix` n'est jamais masqué ; aucune ne convient :
/// `InvalidId { expected }`.
fn parse_one_of<T>(
    input: &str,
    expected: &'static str,
    parsers: &[Parser<T>],
) -> Result<T, DomainError> {
    for parser in parsers {
        match parser(input) {
            Err(DomainError::InvalidId { .. }) => {}
            other => return other,
        }
    }
    Err(DomainError::InvalidId { input: echo(input), expected })
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
    /// Valide et normalise `input` ; un suffixe de version bien formé est retiré.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidId`] si la forme ou le préfixe ne convient pas ;
    /// [`DomainError::BadSuffix`] si le suffixe de version est mal formé.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        Self::parse_with_version(input).map(|(id, _)| id)
    }

    /// Comme [`Id::parse`], en gardant la date du suffixe de version.
    fn parse_with_version(input: &str) -> Result<(Self, Option<CivilDate>), DomainError> {
        read(input, |raw| raw.starts_with(K::PREFIX), K::PREFIX)
            .map(|(raw, date)| (Self { raw, kind: PhantomData }, date))
    }
}

/// Identifiant `LEGITEXT` et date de son suffixe de version.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VersionedId {
    /// Identifiant, suffixe retiré.
    pub id: LegiTextId,
    /// Date du suffixe `_JJ-MM-AAAA`, `None` sans suffixe.
    pub version_start: Option<CivilDate>,
}

/// Lit un `LEGITEXT` et garde la date de son suffixe de version.
///
/// Toute vraie date est gardée (années 1 à 9999) : la plage de `VersionDate`
/// s'applique au cas d'usage, pas ici.
///
/// # Errors
///
/// [`DomainError::InvalidId`] si l'entrée n'est pas un `LEGITEXT` ;
/// [`DomainError::BadSuffix`] si le suffixe est mal formé.
///
/// ```
/// use lf_domain::{date::CivilDate, id::parse_versioned};
///
/// let versioned = parse_versioned("LEGITEXT000033205014_08-09-2023")?;
/// assert_eq!(versioned.id.as_str(), "LEGITEXT000033205014");
/// assert_eq!(versioned.version_start, CivilDate::new(2023, 9, 8));
/// assert_eq!(parse_versioned("LEGITEXT000033205014")?.version_start, None);
/// assert_eq!(
///     parse_versioned("LEGITEXT000033205014_31-02-2023").map_err(|e| e.to_string()),
///     Err("invalid version suffix in \"LEGITEXT000033205014_31-02-2023\": \
///          expected _DD-MM-YYYY with a real date, e.g. _08-09-2023"
///         .to_owned())
/// );
/// # Ok::<(), lf_domain::DomainError>(())
/// ```
pub fn parse_versioned(input: &str) -> Result<VersionedId, DomainError> {
    Id::<LegiText>::parse_with_version(input)
        .map(|(id, version_start)| VersionedId { id, version_start })
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

/// Hache la forme normalisée seule, comme [`PartialEq`] la compare : deux identifiants égaux
/// ont la même empreinte, celle de leur `&str`. Impl manuelle, comme `Clone` et `PartialEq` :
/// `derive(Hash)` exigerait `K: Hash`, que les familles (enums vides) n'implémentent pas.
impl<K> Hash for Id<K> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}

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

    /// [`Id::parse`] : espaces et casse tolérés, suffixe bien formé retiré.
    ///
    /// ```
    /// use lf_domain::id::LegiTextId;
    ///
    /// let id: LegiTextId = " legitext000006070721 ".parse()?;
    /// assert_eq!(id.as_str(), "LEGITEXT000006070721");
    /// let id: LegiTextId = "LEGITEXT000033205014_08-09-2023".parse()?;
    /// assert_eq!(id.as_str(), "LEGITEXT000033205014");
    /// assert_eq!(
    ///     "JORFTEXT000020445850".parse::<LegiTextId>().map_err(|e| e.to_string()),
    ///     Err("invalid Legifrance identifier \"JORFTEXT000020445850\": \
    ///          expected LEGITEXT followed by 12 digits"
    ///         .to_owned())
    /// );
    /// # Ok::<(), lf_domain::DomainError>(())
    /// ```
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AnyId(String);

impl AnyId {
    /// Valide et normalise `input` ; un suffixe de version bien formé est retiré.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidId`] si la forme ne convient pas ;
    /// [`DomainError::BadSuffix`] si le suffixe de version est mal formé.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        read(input, |_| true, "8 letters").map(|(raw, _)| Self(raw))
    }

    /// Forme normalisée.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AnyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AnyId {
    type Err = DomainError;

    /// [`AnyId::parse`] : tout préfixe de 8 lettres.
    ///
    /// ```
    /// use lf_domain::id::AnyId;
    ///
    /// let id: AnyId = "kalitext000005635221_01-01-2020".parse()?;
    /// assert_eq!(id.to_string(), "KALITEXT000005635221");
    /// assert_eq!(
    ///     "LEGITEXT0001".parse::<AnyId>().map_err(|e| e.to_string()),
    ///     Err("invalid Legifrance identifier \"LEGITEXT0001\": \
    ///          expected 8 letters followed by 12 digits"
    ///         .to_owned())
    /// );
    /// # Ok::<(), lf_domain::DomainError>(())
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Article consultable par `/consult/getArticle` (`server.py:284`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    /// [`DomainError::InvalidId`] si aucun préfixe ne convient ;
    /// [`DomainError::BadSuffix`] si le suffixe de version est mal formé.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        parse_one_of(
            input,
            "LEGIARTI or KALIARTI",
            &[|s| LegiArtiId::parse(s).map(Self::Legi), |s| KaliArtiId::parse(s).map(Self::Kali)],
        )
    }

    /// Forme normalisée.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Legi(id) => id.as_str(),
            Self::Kali(id) => id.as_str(),
        }
    }
}

impl fmt::Display for ArticleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ArticleId {
    type Err = DomainError;

    /// [`ArticleId::parse`] : la variante suit le préfixe.
    ///
    /// ```
    /// use lf_domain::id::{ArticleId, KaliArtiId};
    ///
    /// let id: ArticleId = "kaliarti000005805295".parse()?;
    /// assert_eq!(id, ArticleId::Kali("KALIARTI000005805295".parse::<KaliArtiId>()?));
    /// assert_eq!(id.to_string(), "KALIARTI000005805295");
    /// # Ok::<(), lf_domain::DomainError>(())
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Section de table des matières (`LEGISCTA…` ou `KALISCTA…`). Employée seulement par la table
/// des matières (03 §2.2), jamais rendue en sortie.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SectionId(SectionInner);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum SectionInner {
    Legi(Id<LegiScta>),
    Kali(Id<KaliScta>),
}

impl SectionId {
    /// Valide `input` contre les deux préfixes acceptés.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidId`] si aucun préfixe ne convient ;
    /// [`DomainError::BadSuffix`] si le suffixe de version est mal formé.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        parse_one_of(
            input,
            "LEGISCTA or KALISCTA",
            &[
                |s| Id::<LegiScta>::parse(s).map(SectionInner::Legi),
                |s| Id::<KaliScta>::parse(s).map(SectionInner::Kali),
            ],
        )
        .map(Self)
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

    /// [`SectionId::parse`] : `LEGISCTA` ou `KALISCTA`.
    ///
    /// ```
    /// use lf_domain::id::SectionId;
    ///
    /// let section: SectionId = " legiscta000033205040 ".parse()?;
    /// assert_eq!(section.as_str(), "LEGISCTA000033205040");
    /// let section: SectionId = "KALISCTA000049886029".parse()?;
    /// assert_eq!(section.to_string(), "KALISCTA000049886029");
    /// assert_eq!(
    ///     "LEGIARTI000006419288".parse::<SectionId>().map_err(|e| e.to_string()),
    ///     Err("invalid Legifrance identifier \"LEGIARTI000006419288\": \
    ///          expected LEGISCTA or KALISCTA followed by 12 digits"
    ///         .to_owned())
    /// );
    /// # Ok::<(), lf_domain::DomainError>(())
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Décision consultable par `/consult/juri` (`server.py:103`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    /// [`DomainError::InvalidId`] si aucun préfixe ne convient ;
    /// [`DomainError::BadSuffix`] si le suffixe de version est mal formé.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        parse_one_of(
            input,
            "JURITEXT, CONSTEXT or CETATEXT",
            &[
                |s| JuriTextId::parse(s).map(Self::Juri),
                |s| ConsTextId::parse(s).map(Self::Constit),
                |s| CetaTextId::parse(s).map(Self::Cetat),
            ],
        )
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

impl fmt::Display for DecisionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DecisionId {
    type Err = DomainError;

    /// [`DecisionId::parse`] : la variante suit le préfixe.
    ///
    /// ```
    /// use lf_domain::id::{ConsTextId, DecisionId};
    ///
    /// let id: DecisionId = "CONSTEXT000037434572".parse()?;
    /// assert_eq!(id, DecisionId::Constit("CONSTEXT000037434572".parse::<ConsTextId>()?));
    /// assert_eq!(
    ///     "LEGIARTI000006419288".parse::<DecisionId>().map_err(|e| e.to_string()),
    ///     Err("invalid Legifrance identifier \"LEGIARTI000006419288\": \
    ///          expected JURITEXT, CONSTEXT or CETATEXT followed by 12 digits"
    ///         .to_owned())
    /// );
    /// # Ok::<(), lf_domain::DomainError>(())
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Préfixes des dix familles de documents, dans l'ordre de [`DocumentId`].
const DOCUMENT_PREFIXES: [&str; 10] = [
    LegiText::PREFIX,
    LegiArti::PREFIX,
    JorfText::PREFIX,
    KaliText::PREFIX,
    KaliArti::PREFIX,
    JuriText::PREFIX,
    ConsText::PREFIX,
    CetaText::PREFIX,
    CnilText::PREFIX,
    AccoText::PREFIX,
];

/// Préfixe de document connu en tête d'une entrée **déjà normalisée** (espaces retirés,
/// majuscules ASCII), `None` si l'entrée est trop courte, coupe un caractère multi-octet ou
/// porte un autre préfixe.
fn prefix_of(normalized: &str) -> Option<&'static str> {
    let head = normalized.get(..PREFIX_LEN)?;
    DOCUMENT_PREFIXES.into_iter().find(|prefix| *prefix == head)
}

/// Identifiant d'un document de l'un des dix préfixes de résultats de recherche
/// (03 §2.5) : texte, article ou décision. Les sections de table des matières
/// (`LEGISCTA`, `KALISCTA`) n'en font pas partie.
///
/// ```
/// use lf_domain::id::DocumentId;
///
/// let id: DocumentId = " legitext000033205014_08-09-2023 ".parse()?;
/// assert_eq!(id.as_str(), "LEGITEXT000033205014");
/// assert_eq!(id.prefix(), "LEGITEXT");
/// assert_eq!(
///     DocumentId::parse("FOOTEXT000000000001").map_err(|e| e.to_string()),
///     Err("invalid Legifrance identifier \"FOOTEXT000000000001\": expected LEGITEXT, \
///          LEGIARTI, JORFTEXT, KALITEXT, KALIARTI, JURITEXT, CONSTEXT, CETATEXT, CNILTEXT \
///          or ACCOTEXT followed by 12 digits"
///         .to_owned())
/// );
/// # Ok::<(), lf_domain::DomainError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DocumentId {
    /// Texte consolidé `LEGITEXT…`.
    LegiText(LegiTextId),
    /// Article consolidé `LEGIARTI…`.
    LegiArti(LegiArtiId),
    /// Texte du Journal officiel `JORFTEXT…`.
    JorfText(JorfTextId),
    /// Convention collective `KALITEXT…`.
    KaliText(KaliTextId),
    /// Article de convention collective `KALIARTI…`.
    KaliArti(KaliArtiId),
    /// Décision judiciaire `JURITEXT…`.
    JuriText(JuriTextId),
    /// Décision du Conseil constitutionnel `CONSTEXT…`.
    ConsText(ConsTextId),
    /// Décision administrative `CETATEXT…`.
    CetaText(CetaTextId),
    /// Délibération CNIL `CNILTEXT…`.
    CnilText(CnilTextId),
    /// Accord d'entreprise `ACCOTEXT…`.
    AccoText(AccoTextId),
}

impl DocumentId {
    /// Valide `input` contre les dix préfixes : la variante suit les huit premières lettres
    /// (espaces et casse tolérés), puis l'alias correspondant valide le reste, suffixe de
    /// version compris.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidId`] si le préfixe est inconnu ou la tête malformée ;
    /// [`DomainError::BadSuffix`] si le suffixe de version est mal formé.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        let normalized = input.trim().to_ascii_uppercase();
        match prefix_of(&normalized) {
            Some(LegiText::PREFIX) => LegiTextId::parse(input).map(Self::LegiText),
            Some(LegiArti::PREFIX) => LegiArtiId::parse(input).map(Self::LegiArti),
            Some(JorfText::PREFIX) => JorfTextId::parse(input).map(Self::JorfText),
            Some(KaliText::PREFIX) => KaliTextId::parse(input).map(Self::KaliText),
            Some(KaliArti::PREFIX) => KaliArtiId::parse(input).map(Self::KaliArti),
            Some(JuriText::PREFIX) => JuriTextId::parse(input).map(Self::JuriText),
            Some(ConsText::PREFIX) => ConsTextId::parse(input).map(Self::ConsText),
            Some(CetaText::PREFIX) => CetaTextId::parse(input).map(Self::CetaText),
            Some(CnilText::PREFIX) => CnilTextId::parse(input).map(Self::CnilText),
            Some(AccoText::PREFIX) => AccoTextId::parse(input).map(Self::AccoText),
            Some(_) | None => Err(DomainError::InvalidId {
                input: echo(input),
                expected: "LEGITEXT, LEGIARTI, JORFTEXT, KALITEXT, KALIARTI, JURITEXT, \
                           CONSTEXT, CETATEXT, CNILTEXT or ACCOTEXT",
            }),
        }
    }

    /// Forme normalisée.
    pub fn as_str(&self) -> &str {
        match self {
            Self::LegiText(id) => id.as_str(),
            Self::LegiArti(id) => id.as_str(),
            Self::JorfText(id) => id.as_str(),
            Self::KaliText(id) => id.as_str(),
            Self::KaliArti(id) => id.as_str(),
            Self::JuriText(id) => id.as_str(),
            Self::ConsText(id) => id.as_str(),
            Self::CetaText(id) => id.as_str(),
            Self::CnilText(id) => id.as_str(),
            Self::AccoText(id) => id.as_str(),
        }
    }

    /// Préfixe de la famille, ex. `LEGITEXT`.
    pub fn prefix(&self) -> &'static str {
        match self {
            Self::LegiText(_) => LegiText::PREFIX,
            Self::LegiArti(_) => LegiArti::PREFIX,
            Self::JorfText(_) => JorfText::PREFIX,
            Self::KaliText(_) => KaliText::PREFIX,
            Self::KaliArti(_) => KaliArti::PREFIX,
            Self::JuriText(_) => JuriText::PREFIX,
            Self::ConsText(_) => ConsText::PREFIX,
            Self::CetaText(_) => CetaText::PREFIX,
            Self::CnilText(_) => CnilText::PREFIX,
            Self::AccoText(_) => AccoText::PREFIX,
        }
    }
}

impl fmt::Display for DocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DocumentId {
    type Err = DomainError;

    /// [`DocumentId::parse`] : la variante suit le préfixe.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};

    /// Empreinte déterministe (clés fixes, sans graine aléatoire).
    fn hash_of<T: Hash + ?Sized>(value: &T) -> u64 {
        BuildHasherDefault::<DefaultHasher>::default().hash_one(value)
    }

    /// `a` et `b` désignent le même identifiant : valeurs égales, donc
    /// empreintes égales (contrat de `Hash`).
    fn assert_same_hash<T: Hash + PartialEq + fmt::Debug>(parse: Parser<T>, a: &str, b: &str) {
        let (a, b) = (parse(a).unwrap(), parse(b).unwrap());
        assert_eq!(a, b);
        assert_eq!(hash_of(&a), hash_of(&b), "{a:?}");
    }

    #[test]
    fn hash_agrees_with_eq() {
        // `Id<K>` hache la forme normalisée seule, comme `PartialEq` la compare :
        // même empreinte que la chaîne (un corps vidé ne hacherait rien).
        let id = LegiTextId::parse("LEGITEXT000006070721").unwrap();
        assert_eq!(hash_of(&id), hash_of(id.as_str()));
        // Égalité ⇒ même empreinte, quelle que soit la forme lue.
        assert_same_hash(LegiTextId::parse, " legitext000006070721\n", "LEGITEXT000006070721");
        assert_same_hash(AnyId::parse, "jorftext000020445850_01-01-2020", "JORFTEXT000020445850");
        assert_same_hash(ArticleId::parse, " legiarti000006419288", "LEGIARTI000006419288");
        assert_same_hash(ArticleId::parse, " kaliarti000005805295", "KALIARTI000005805295");
        assert_same_hash(DecisionId::parse, "juritext000048465611 ", "JURITEXT000048465611");
        assert_same_hash(DecisionId::parse, "constext000037434572 ", "CONSTEXT000037434572");
        assert_same_hash(DecisionId::parse, "cetatext000051655317 ", "CETATEXT000051655317");
        assert_same_hash(SectionId::parse, "legiscta000033205040", "LEGISCTA000033205040");
        assert_same_hash(SectionId::parse, "kaliscta000049886029", "KALISCTA000049886029");
        assert_same_hash(
            parse_versioned,
            " legitext000033205014_08-09-2023",
            "LEGITEXT000033205014_08-09-2023",
        );
    }

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
        // Vecteurs réels (03 §2.4) : consult_loda.json, search_loda.json,
        // search_code.json ; puis un 29 février d'année bissextile.
        for (input, raw) in [
            ("LEGITEXT000033205014_08-09-2023", "LEGITEXT000033205014"),
            ("LEGITEXT000033696785_28-12-2016", "LEGITEXT000033696785"),
            ("LEGITEXT000006070721_09-04-2026", "LEGITEXT000006070721"),
            ("LEGITEXT000006070721_29-02-2024", "LEGITEXT000006070721"),
        ] {
            assert_eq!(LegiTextId::parse(input).unwrap().as_str(), raw, "{input}");
        }
        // Tout autre suffixe est refusé (décision C4) ; la référence le jetait
        // sans contrôle (citations.py:55-63).
        for suffix in [
            "_",
            "_foo",
            "_31-02-2023",
            "_29-02-2023",
            "_01-13-2023",
            "_00-01-2023",
            "_01-01-0000",
            "_2023-09-08",
            "_8-09-2023",
            "_08-09-23",
            "_08/09/2023",
            "_08-09-2023_x",
        ] {
            let input = format!("LEGITEXT000006070721{suffix}");
            assert_eq!(
                LegiTextId::parse(&input).unwrap_err(),
                DomainError::BadSuffix { input: input.clone() },
                "{input}"
            );
        }
    }

    #[test]
    fn strips_version_suffix_on_constext_too() {
        let id = ConsTextId::parse("CONSTEXT000037434572_01-01-2020").unwrap();
        assert_eq!(id.as_str(), "CONSTEXT000037434572");
    }

    #[test]
    fn versioned_id_splits_suffix() {
        // A-parite:210 (test_strip_date_suffix_and_marks).
        assert_eq!(
            parse_versioned("LEGITEXT000033205014_08-09-2023").unwrap(),
            VersionedId {
                id: LegiTextId::parse("LEGITEXT000033205014").unwrap(),
                version_start: CivilDate::new(2023, 9, 8),
            }
        );
        // Sans suffixe : pas de date ; espaces et casse tolérés.
        assert_eq!(
            parse_versioned(" legitext000006070721\n").unwrap(),
            VersionedId {
                id: LegiTextId::parse("LEGITEXT000006070721").unwrap(),
                version_start: None,
            }
        );
        // Toute vraie date est gardée, même hors de la plage de `VersionDate`.
        for (suffix, (y, m, d)) in [
            ("_01-01-0001", (1, 1, 1)),
            ("_01-01-2999", (2999, 1, 1)),
            ("_31-12-9999", (9999, 12, 31)),
        ] {
            let versioned = parse_versioned(&format!("LEGITEXT000006070721{suffix}")).unwrap();
            assert_eq!(versioned.version_start, CivilDate::new(y, m, d), "{suffix}");
        }
        // Une décision se lit par `Id::parse` ; `parse_versioned` exige LEGITEXT.
        assert_eq!(
            JuriTextId::parse("JURITEXT000051743650").unwrap().as_str(),
            "JURITEXT000051743650"
        );
        assert_eq!(
            parse_versioned("JURITEXT000051743650").unwrap_err(),
            DomainError::InvalidId { input: "JURITEXT000051743650".into(), expected: "LEGITEXT" }
        );
        assert_eq!(
            parse_versioned("LEGITEXT000033205014_31-02-2023").unwrap_err(),
            DomainError::BadSuffix { input: "LEGITEXT000033205014_31-02-2023".into() }
        );
    }

    #[test]
    fn suffix_kept_on_legitext_stripped_elsewhere() {
        // LEGITEXT : `parse_versioned` garde la date, `Id::parse` la retire.
        let input = "LEGITEXT000033205014_08-09-2023";
        assert_eq!(parse_versioned(input).unwrap().version_start, CivilDate::new(2023, 9, 8));
        assert_eq!(LegiTextId::parse(input).unwrap().as_str(), "LEGITEXT000033205014");
        // Ailleurs, chaque analyseur tolère puis retire un suffixe bien formé,
        // et rend `BadSuffix` pour un suffixe mal formé, quelle que soit la
        // famille essayée (première, deuxième ou troisième) : jamais masqué en
        // `InvalidId`.
        let parsers: [(&str, Parser<String>); 9] = [
            ("CONSTEXT000037434572", |s| ConsTextId::parse(s).map(|id| id.as_str().to_owned())),
            ("JORFTEXT000020445850", |s| AnyId::parse(s).map(|id| id.as_str().to_owned())),
            ("LEGIARTI000006419288", |s| ArticleId::parse(s).map(|id| id.as_str().to_owned())),
            ("KALIARTI000005805295", |s| ArticleId::parse(s).map(|id| id.as_str().to_owned())),
            ("JURITEXT000048465611", |s| DecisionId::parse(s).map(|id| id.as_str().to_owned())),
            ("CONSTEXT000037434572", |s| DecisionId::parse(s).map(|id| id.as_str().to_owned())),
            ("CETATEXT000051655317", |s| DecisionId::parse(s).map(|id| id.as_str().to_owned())),
            ("LEGISCTA000033205040", |s| SectionId::parse(s).map(|id| id.as_str().to_owned())),
            ("KALISCTA000049886029", |s| SectionId::parse(s).map(|id| id.as_str().to_owned())),
        ];
        for (raw, parse) in parsers {
            assert_eq!(parse(&format!("{raw}_01-01-2020")), Ok(raw.to_owned()), "{raw}");
            let input = format!("{raw}_31-02-2023");
            assert_eq!(
                parse(&input),
                Err(DomainError::BadSuffix { input: input.clone() }),
                "{raw}"
            );
        }
    }

    #[test]
    fn checks_head_then_prefix_then_suffix() {
        // La famille d'abord : un suffixe mal formé derrière une tête invalide
        // ou un préfixe refusé donne `InvalidId`, le message le plus utile.
        for input in ["JORFTEXT000020445850_foo", "LEGITEXT0001_08-09-2023", "LEGITEXT0001_foo"] {
            let expected = DomainError::InvalidId { input: input.into(), expected: "LEGITEXT" };
            assert_eq!(LegiTextId::parse(input).unwrap_err(), expected, "{input}");
            assert_eq!(parse_versioned(input).unwrap_err(), expected, "{input}");
        }
        assert_eq!(
            AnyId::parse("LEGI_08-09-2023").unwrap_err(),
            DomainError::InvalidId { input: "LEGI_08-09-2023".into(), expected: "8 letters" }
        );
        assert_eq!(
            ArticleId::parse("LEGITEXT000006070721_foo").unwrap_err(),
            DomainError::InvalidId {
                input: "LEGITEXT000006070721_foo".into(),
                expected: "LEGIARTI or KALIARTI",
            }
        );
        assert_eq!(
            DecisionId::parse("CNILTEXT000053352643_foo").unwrap_err(),
            DomainError::InvalidId {
                input: "CNILTEXT000053352643_foo".into(),
                expected: "JURITEXT, CONSTEXT or CETATEXT",
            }
        );
        assert_eq!(
            SectionId::parse("LEGIARTI000033205126_foo").unwrap_err(),
            DomainError::InvalidId {
                input: "LEGIARTI000033205126_foo".into(),
                expected: "LEGISCTA or KALISCTA",
            }
        );
    }

    #[test]
    fn id_parse_is_tolerant_on_whitespace_and_case() {
        for input in ["  legitext000006070721\n", "\tLegiText000006070721_09-04-2026 "] {
            assert_eq!(LegiTextId::parse(input).unwrap().as_str(), "LEGITEXT000006070721");
            let id: LegiTextId = input.parse().unwrap();
            assert_eq!(id.as_str(), "LEGITEXT000006070721", "{input:?}");
        }
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
        assert_eq!(legi.to_string(), "LEGIARTI000006419288");
        assert_eq!(kali.to_string(), "KALIARTI000005805295");
        assert_eq!(legi.to_string().parse::<ArticleId>().unwrap(), legi);
        assert_eq!(kali.to_string().parse::<ArticleId>().unwrap(), kali);
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
        assert_eq!(juri.to_string(), "JURITEXT000048465611");
        assert_eq!(constit.to_string(), "CONSTEXT000037434572");
        assert_eq!(cetat.to_string(), "CETATEXT000051655317");
        assert_eq!(juri.to_string().parse::<DecisionId>().unwrap(), juri);
        assert_eq!(constit.to_string().parse::<DecisionId>().unwrap(), constit);
        assert_eq!(cetat.to_string().parse::<DecisionId>().unwrap(), cetat);
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

    #[test]
    fn document_id_dispatches_on_prefix() {
        let table = [
            (
                DocumentId::LegiText(LegiTextId::parse("LEGITEXT000006070721").unwrap()),
                "LEGITEXT000006070721",
                "LEGITEXT",
            ),
            (
                DocumentId::LegiArti(LegiArtiId::parse("LEGIARTI000006419288").unwrap()),
                "LEGIARTI000006419288",
                "LEGIARTI",
            ),
            (
                DocumentId::JorfText(JorfTextId::parse("JORFTEXT000020445850").unwrap()),
                "JORFTEXT000020445850",
                "JORFTEXT",
            ),
            (
                DocumentId::KaliText(KaliTextId::parse("KALITEXT000005661030").unwrap()),
                "KALITEXT000005661030",
                "KALITEXT",
            ),
            (
                DocumentId::KaliArti(KaliArtiId::parse("KALIARTI000005805295").unwrap()),
                "KALIARTI000005805295",
                "KALIARTI",
            ),
            (
                DocumentId::JuriText(JuriTextId::parse("JURITEXT000048465611").unwrap()),
                "JURITEXT000048465611",
                "JURITEXT",
            ),
            (
                DocumentId::ConsText(ConsTextId::parse("CONSTEXT000037434572").unwrap()),
                "CONSTEXT000037434572",
                "CONSTEXT",
            ),
            (
                DocumentId::CetaText(CetaTextId::parse("CETATEXT000051655317").unwrap()),
                "CETATEXT000051655317",
                "CETATEXT",
            ),
            (
                DocumentId::CnilText(CnilTextId::parse("CNILTEXT000053352643").unwrap()),
                "CNILTEXT000053352643",
                "CNILTEXT",
            ),
            (
                DocumentId::AccoText(AccoTextId::parse("ACCOTEXT000051888026").unwrap()),
                "ACCOTEXT000051888026",
                "ACCOTEXT",
            ),
        ];
        for (expected, raw, prefix) in table {
            // Forme exacte, puis casse et blancs tolérés.
            for input in [raw.to_owned(), format!(" \t{}\n", raw.to_lowercase())] {
                let parsed = DocumentId::parse(&input).unwrap();
                assert_eq!(parsed, expected, "{input:?}");
                assert_eq!(parsed.as_str(), raw);
                assert_eq!(parsed.prefix(), prefix);
                assert_eq!(parsed.to_string(), raw);
                assert_eq!(input.parse::<DocumentId>().unwrap(), expected);
            }
            assert_eq!(hash_of(&expected), hash_of(&DocumentId::parse(raw).unwrap()));
        }
    }

    #[test]
    fn document_id_strips_a_wellformed_version_suffix() {
        let id = DocumentId::parse("legitext000033205014_08-09-2023").unwrap();
        assert_eq!(id.as_str(), "LEGITEXT000033205014");
        assert_eq!(id, DocumentId::LegiText(LegiTextId::parse("LEGITEXT000033205014").unwrap()));
        let id = DocumentId::parse("CONSTEXT000037434572_01-01-2020").unwrap();
        assert_eq!(id.as_str(), "CONSTEXT000037434572");
    }

    #[test]
    fn document_id_rejects_unknown_prefix_and_bad_suffix() {
        let expected = "LEGITEXT, LEGIARTI, JORFTEXT, KALITEXT, KALIARTI, JURITEXT, CONSTEXT, \
                        CETATEXT, CNILTEXT or ACCOTEXT";
        for bad in [
            "FOOTEXT000000000001",
            // Préfixe de section : connu des tables des matières, pas un document.
            "LEGISCTA000033205040",
            "LEGITEX",
            "LEGITEX€000006070721",
            "",
            "   ",
        ] {
            assert_eq!(
                DocumentId::parse(bad).unwrap_err(),
                DomainError::InvalidId { input: bad.into(), expected },
                "{bad:?}"
            );
        }
        let input = "LEGITEXT000033205014_foo";
        assert_eq!(
            DocumentId::parse(input).unwrap_err(),
            DomainError::BadSuffix { input: input.into() }
        );
        // Préfixe reconnu, tête malformée : l'alias répond.
        assert_eq!(
            DocumentId::parse("JURITEXT0001").unwrap_err(),
            DomainError::InvalidId { input: "JURITEXT0001".into(), expected: "JURITEXT" }
        );
    }

    #[test]
    fn prefix_of_reads_the_first_eight_letters() {
        for prefix in [
            "LEGITEXT", "LEGIARTI", "JORFTEXT", "KALITEXT", "KALIARTI", "JURITEXT", "CONSTEXT",
            "CETATEXT", "CNILTEXT", "ACCOTEXT",
        ] {
            assert_eq!(prefix_of(&format!("{prefix}000006070721")), Some(prefix));
            assert_eq!(prefix_of(prefix), Some(prefix));
        }
        for other in ["", "LEGITEX", "FOOTEXT000000000001", "LEGISCTA000033205040", "LEGITEX€1"] {
            assert_eq!(prefix_of(other), None, "{other:?}");
        }
        // Entrée déjà normalisée : une minuscule n'est pas reconnue ici.
        assert_eq!(prefix_of("legitext000006070721"), None);
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
        fn document_id_roundtrips_through_display(
            prefix in proptest::sample::select(vec![
                "LEGITEXT", "LEGIARTI", "JORFTEXT", "KALITEXT", "KALIARTI", "JURITEXT",
                "CONSTEXT", "CETATEXT", "CNILTEXT", "ACCOTEXT",
            ]),
            digits in "[0-9]{12}",
            lower in any::<bool>(),
            pad in "[ \\t]{0,3}",
        ) {
            let raw = format!("{prefix}{digits}");
            let typed = if lower { raw.to_lowercase() } else { raw.clone() };
            let id = DocumentId::parse(&format!("{pad}{typed}{pad}")).unwrap();
            prop_assert_eq!(id.as_str(), raw.as_str());
            prop_assert_eq!(id.prefix(), prefix);
            prop_assert_eq!(id.to_string(), raw.as_str());
            prop_assert_eq!(id.to_string().parse::<DocumentId>(), Ok(id));
        }

        #[test]
        fn any_id_roundtrips(
            prefix in "[a-zA-Z]{8}",
            digits in "[0-9]{12}",
            pad in "[ \\t]{0,3}",
        ) {
            let id: AnyId = format!("{pad}{prefix}{digits}{pad}").parse().unwrap();
            prop_assert_eq!(id.as_str(), format!("{}{digits}", prefix.to_ascii_uppercase()));
            prop_assert_eq!(id.to_string(), id.as_str());
            prop_assert_eq!(id.to_string().parse::<AnyId>(), Ok(id.clone()));
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
            let _ = DocumentId::parse(&s);
            let _ = SectionId::parse(&s);
            let _ = parse_versioned(&s);
        }

        /// Suffixes tirés de vraies dates seulement (jour 1 à 28, tout mois,
        /// années 1 à 9999), ou pas de suffixe.
        #[test]
        fn valid_shape_roundtrips(
            digits in "[0-9]{12}",
            date in prop::option::of((1u16..=9999, 1u8..=12, 1u8..=28)),
        ) {
            let raw = format!("LEGITEXT{digits}");
            let suffix = date.map_or(String::new(), |(y, m, d)| format!("_{d:02}-{m:02}-{y:04}"));
            let input = format!("{raw}{suffix}");
            let id = LegiTextId::parse(&input).unwrap();
            prop_assert_eq!(id.as_str(), raw.as_str());
            prop_assert_eq!(LegiTextId::parse(id.as_str()).unwrap(), id.clone());
            let version_start = date.map(|(y, m, d)| CivilDate::new(y, m, d).unwrap());
            prop_assert_eq!(parse_versioned(&input).unwrap(), VersionedId { id, version_start });
        }

        /// Forme `JJ-MM-AAAA` correcte, mais date impossible (jour 0 ou trop
        /// grand pour le mois, mois 0 ou 13, année 0) : `BadSuffix`. Le filtre
        /// écarte environ 4 tirages sur 5 : il est posé sur la stratégie (rejets
        /// locaux, plafond 65 536) et non par `prop_assume!` (rejets globaux,
        /// plafond 1 024, dépassé par intermittence).
        #[test]
        fn impossible_suffix_date_is_bad_suffix(
            digits in "[0-9]{12}",
            (d, m, y) in (0u8..=32, 0u8..=13, 0u16..=9999)
                .prop_filter("date impossible", |&(d, m, y)| CivilDate::new(y, m, d).is_none()),
        ) {
            let input = format!("LEGITEXT{digits}_{d:02}-{m:02}-{y:04}");
            let expected = DomainError::BadSuffix { input: input.clone() };
            prop_assert_eq!(LegiTextId::parse(&input).unwrap_err(), expected.clone());
            prop_assert_eq!(parse_versioned(&input).unwrap_err(), expected);
        }
    }
}
