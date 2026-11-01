//! Types du parseur de citations (annexe B §6.2 à §6.5, 03 §8.6).
//!
//! Les noms de variantes de [`CitationRef`] reprennent les valeurs `kind` du corpus, qui reste
//! chargeable tel quel. Les chaînes de `code()` sont **exactement** celles du corpus (§4.19) :
//! elles servent au harnais de tests et au rendu des notes.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use thiserror::Error;

use crate::contract::{Eli, Idcc, Nor};
use crate::date::CivilDate;
use crate::ecli::Ecli;
use crate::id::DocumentId;

use super::dict::codes::CodeKey;
use super::dict::courts::{CaSeat, CaaCode, CaaSeat, CassChamber, CcType, CeFormation, TaSeat};

/// Erreur de lecture d'un numéro typé (`FromStr`). Elle ne cite jamais l'entrée : le texte d'un
/// appelant ne doit pas se retrouver dans un message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error("invalid {kind} number")]
pub struct NumberParseError {
    /// Nature du numéro refusé (`pourvoi`, `rg`, `article`…).
    pub kind: &'static str,
}

/// Offsets en OCTETS dans le texte d'ORIGINE, intervalle semi-ouvert, frontières UTF-8
/// garanties.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    /// Premier octet du span.
    pub start: usize,
    /// Premier octet après le span.
    pub end: usize,
}

impl Span {
    /// Même span en points de code Unicode de `input` (vue MCP, 00 §7). Linéaire ; un offset
    /// hors frontière est arrondi au caractère suivant.
    #[must_use]
    pub fn to_chars(self, input: &str) -> Self {
        let (mut start, mut end) = (0_usize, 0_usize);
        for (byte, _) in input.char_indices() {
            start = start.saturating_add(usize::from(byte < self.start));
            end = end.saturating_add(usize::from(byte < self.end));
        }
        Self { start, end }
    }
}

/// Une valeur reconnue, son span, sa confiance, son éventuelle ambiguïté et ses drapeaux.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Spanned<T> {
    /// Valeur reconnue.
    pub value: T,
    /// Position dans le texte d'origine.
    pub span: Span,
    /// Confiance dans la reconnaissance.
    pub confidence: Confidence,
    /// Ambiguïté signalée, une au plus par citation.
    pub ambiguity: Option<Ambiguity>,
    /// Drapeaux de contrôle syntaxique.
    pub flags: Vec<Flag>,
    /// Numéro du groupe d'une énumération (« articles L. 104-1 et L. 104-2 »).
    pub enum_group: Option<u32>,
}

impl<T> Spanned<T> {
    /// Valeur de confiance haute, sans ambiguïté ni drapeau.
    pub fn new(value: T, span: Span) -> Self {
        Self {
            value,
            span,
            confidence: Confidence::High,
            ambiguity: None,
            flags: Vec::new(),
            enum_group: None,
        }
    }
}

/// Confiance dans une reconnaissance (§4.19).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Confidence {
    /// Basse : sigle ambigu, numéro seul, appellation.
    Low,
    /// Moyenne : juridiction inférée de la forme du numéro, code générique.
    Medium,
    /// Haute, par défaut.
    High,
}

impl Confidence {
    /// Valeur du corpus (`low`, `medium`, `high`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// Sigle de code ou de juridiction ambigu (§5.3).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Sigle {
    /// `CE` : Conseil d'État, Code électoral.
    Ce,
    /// `CC` : Code civil, Conseil constitutionnel, Cour de cassation.
    Cc,
    /// `CP` : Code pénal, Code des ports maritimes, conseil de prud'hommes.
    Cp,
    /// `CT` : Code du travail, du tourisme, des transports.
    Ct,
    /// `CU` : Code de l'urbanisme.
    Cu,
    /// `CPI` : Code de la propriété intellectuelle, Cour pénale internationale.
    Cpi,
    /// `CCP` : Code de la commande publique.
    Ccp,
    /// `CPCE` : deux codes.
    Cpce,
}

impl Sigle {
    /// Les huit sigles ambigus.
    pub const ALL: [Self; 8] =
        [Self::Ce, Self::Cc, Self::Cp, Self::Ct, Self::Cu, Self::Cpi, Self::Ccp, Self::Cpce];

    /// Sigle en majuscules.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ce => "CE",
            Self::Cc => "CC",
            Self::Cp => "CP",
            Self::Ct => "CT",
            Self::Cu => "CU",
            Self::Cpi => "CPI",
            Self::Ccp => "CCP",
            Self::Cpce => "CPCE",
        }
    }

    /// Sigle d'un texte en majuscules, sensible à la casse.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == text)
    }
}

/// Ambiguïté d'une citation (§4.19), une au plus.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Ambiguity {
    /// Article sans code ni texte (`code_manquant`).
    CodeManquant,
    /// Code déduit du contexte du document (`code_implicite`).
    CodeImplicite,
    /// Article d'un texte non nommé (`texte_manquant`).
    TexteManquant,
    /// Sigle à plusieurs sens (`sigle_ambigu:CE`).
    SigleAmbigu(Sigle),
    /// « Dudit code » sans antécédent (`anaphore_non_resolue`).
    AnaphoreNonResolue,
    /// Date sans numéro (`numero_manquant`).
    NumeroManquant,
    /// Numéro seul, sans juridiction (`juridiction_manquante`).
    JuridictionManquante,
    /// Ni juridiction ni type (`juridiction_et_type_manquants`).
    JuridictionEtTypeManquants,
    /// Délibération sans émetteur (`emetteur_inconnu`).
    EmetteurInconnu,
    /// Avenant sans convention identifiée (`convention_inconnue`).
    ConventionInconnue,
    /// « La Convention » seule (`convention_non_identifiee`).
    ConventionNonIdentifiee,
    /// Référence de bulletin seule (`reference_de_publication_seule`).
    ReferenceDePublicationSeule,
    /// Appellation d'usage (`appellation`).
    Appellation,
    /// Code à variantes, comme le Code rural (`code_a_variantes`).
    CodeAVariantes,
}

impl Ambiguity {
    /// Valeur du corpus (`sigle_ambigu:CE`…).
    pub fn code(&self) -> Cow<'static, str> {
        match self {
            Self::CodeManquant => "code_manquant".into(),
            Self::CodeImplicite => "code_implicite".into(),
            Self::TexteManquant => "texte_manquant".into(),
            Self::SigleAmbigu(sigle) => format!("sigle_ambigu:{}", sigle.as_str()).into(),
            Self::AnaphoreNonResolue => "anaphore_non_resolue".into(),
            Self::NumeroManquant => "numero_manquant".into(),
            Self::JuridictionManquante => "juridiction_manquante".into(),
            Self::JuridictionEtTypeManquants => "juridiction_et_type_manquants".into(),
            Self::EmetteurInconnu => "emetteur_inconnu".into(),
            Self::ConventionInconnue => "convention_inconnue".into(),
            Self::ConventionNonIdentifiee => "convention_non_identifiee".into(),
            Self::ReferenceDePublicationSeule => "reference_de_publication_seule".into(),
            Self::Appellation => "appellation".into(),
            Self::CodeAVariantes => "code_a_variantes".into(),
        }
    }
}

/// Famille d'ECLI français dont l'ordinal peut être non conforme.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum EcliFamily {
    /// Conseil constitutionnel (`AAAA.N.TYPE`).
    Cc,
    /// Conseil d'État (`requête.AAAAMMJJ`).
    Ce,
    /// Cour de cassation (préfixe de chambre puis cinq chiffres).
    Cass,
}

/// Gravité d'un drapeau.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    /// Information.
    Info,
    /// Citation suspecte, mais possible.
    Suspect,
    /// Citation impossible : à ne pas chercher.
    Impossible,
}

/// Drapeau de contrôle syntaxique (§4.19).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Flag {
    /// Jour ou mois impossible (`date_invalide`).
    DateInvalide,
    /// Année après `today` (`annee_future`).
    AnneeFuture,
    /// Chambre civile numérotée au-delà de 3 (`chambre_inexistante:civ4`).
    ChambreInexistante(String),
    /// Préfixe de chambre de cassation inconnu (`chambre_cass_inconnue:XX`).
    ChambreCassInconnue(String),
    /// Ordinal d'ECLI hors de la forme de la juridiction (`ordinal_cc_non_conforme`…).
    EcliOrdinalNonConforme(EcliFamily),
    /// Conseil constitutionnel : année de l'ECLI avant celle du numéro.
    EcliAnneeAnterieureNumero,
    /// Année du pourvoi après celle de la décision.
    AnneePourvoiPosterieure,
    /// ECLI dont la juridiction est `CE` seule.
    EcliCodeCeSeul,
    /// Texte : année du numéro différente de celle de la date.
    AnneeNumeroDifferenteAnneeDate,
    /// Année hors de 1789 à `today` + 1.
    AnneeImprobable,
    /// Type du Conseil constitutionnel hors liste (`type_cc_inconnu:X`).
    TypeCcInconnu(String),
    /// Code de CAA différent du siège cité.
    CaaCodeSiegeIncoherent,
    /// Date du calendrier républicain, gardée brute.
    DateRepublicaine,
    /// L'étape 1 de la normalisation a modifié le texte.
    MojibakeRepare,
}

impl Flag {
    /// Valeur du corpus.
    pub fn code(&self) -> Cow<'static, str> {
        match self {
            Self::DateInvalide => "date_invalide".into(),
            Self::AnneeFuture => "annee_future".into(),
            Self::ChambreInexistante(s) => format!("chambre_inexistante:{s}").into(),
            Self::ChambreCassInconnue(s) => format!("chambre_cass_inconnue:{s}").into(),
            Self::EcliOrdinalNonConforme(EcliFamily::Cc) => "ordinal_cc_non_conforme".into(),
            Self::EcliOrdinalNonConforme(EcliFamily::Ce) => "ordinal_ce_non_conforme".into(),
            Self::EcliOrdinalNonConforme(EcliFamily::Cass) => "ordinal_cass_non_conforme".into(),
            Self::EcliAnneeAnterieureNumero => "ecli_annee_anterieure_numero".into(),
            Self::AnneePourvoiPosterieure => "annee_pourvoi_posterieure".into(),
            Self::EcliCodeCeSeul => "code_juridiction_CE_seul_jamais_utilise".into(),
            Self::AnneeNumeroDifferenteAnneeDate => "annee_numero_differente_annee_date".into(),
            Self::AnneeImprobable => "annee_improbable".into(),
            Self::TypeCcInconnu(s) => format!("type_cc_inconnu:{s}").into(),
            Self::CaaCodeSiegeIncoherent => "caa_code_siege_incoherent".into(),
            Self::DateRepublicaine => "date_republicaine".into(),
            Self::MojibakeRepare => "mojibake_repare".into(),
        }
    }

    /// Gravité du drapeau (§4.19).
    pub const fn severity(&self) -> Severity {
        match self {
            Self::DateInvalide
            | Self::AnneeFuture
            | Self::ChambreInexistante(_)
            | Self::ChambreCassInconnue(_)
            | Self::EcliOrdinalNonConforme(_)
            | Self::EcliAnneeAnterieureNumero
            | Self::AnneePourvoiPosterieure => Severity::Impossible,
            Self::EcliCodeCeSeul
            | Self::AnneeNumeroDifferenteAnneeDate
            | Self::AnneeImprobable
            | Self::TypeCcInconnu(_)
            | Self::CaaCodeSiegeIncoherent => Severity::Suspect,
            Self::DateRepublicaine | Self::MojibakeRepare => Severity::Info,
        }
    }
}

/// Une citation reconnue (§6.3).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CitationRef {
    /// Article d'un code.
    CodeArticle {
        /// Code cité.
        code: CodeRef,
        /// Article.
        article: ArticleRef,
        /// Qualificatif de version.
        version: Option<VersionQualifier>,
        /// Comment le code a été trouvé, quand il n'est pas écrit.
        code_via: Option<CodeVia>,
    },
    /// Article d'un texte non codifié.
    TextArticle {
        /// Texte cité, `None` si inconnu (« la Convention »).
        text: Option<TextCite>,
        /// Article.
        article: ArticleRef,
        /// Date de version (« au 1er janvier 2018 »).
        at_date: Option<CivilDate>,
    },
    /// Un texte (loi, décret, ordonnance…).
    Text(TextCite),
    /// Une décision de justice.
    Decision(DecisionCite),
    /// Un ECLI français (`ECLI:FR` seulement).
    Ecli(Ecli),
    /// Une délibération de la CNIL.
    CnilDeliberation(CnilCite),
    /// Une convention collective.
    Convention(ConventionCite),
    /// Une référence au Journal officiel.
    Jorf(JorfCite),
    /// Un NOR, en majuscules.
    Nor(Nor),
    /// Un ELI cité.
    Eli(Eli),
    /// Un identifiant Légifrance.
    LegifranceId(LegifranceIdRef),
    /// Hors périmètre : droit de l'UE, CJUE, CEDH, doctrine, ECLI étranger.
    Unsupported(UnsupportedCite),
}

/// Famille d'une citation, sans sa valeur.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CitationKind {
    /// [`CitationRef::CodeArticle`].
    CodeArticle,
    /// [`CitationRef::TextArticle`].
    TextArticle,
    /// [`CitationRef::Text`].
    Text,
    /// [`CitationRef::Decision`].
    Decision,
    /// [`CitationRef::Ecli`].
    Ecli,
    /// [`CitationRef::CnilDeliberation`].
    CnilDeliberation,
    /// [`CitationRef::Convention`].
    Convention,
    /// [`CitationRef::Jorf`].
    Jorf,
    /// [`CitationRef::Nor`].
    Nor,
    /// [`CitationRef::Eli`].
    Eli,
    /// [`CitationRef::LegifranceId`].
    LegifranceId,
    /// [`CitationRef::Unsupported`].
    Unsupported,
}

impl CitationKind {
    /// Les douze familles.
    pub const ALL: [Self; 12] = [
        Self::CodeArticle,
        Self::TextArticle,
        Self::Text,
        Self::Decision,
        Self::Ecli,
        Self::CnilDeliberation,
        Self::Convention,
        Self::Jorf,
        Self::Nor,
        Self::Eli,
        Self::LegifranceId,
        Self::Unsupported,
    ];

    /// Nom en `PascalCase`, valeur `kind` du corpus.
    pub const fn pascal_case(self) -> &'static str {
        match self {
            Self::CodeArticle => "CodeArticle",
            Self::TextArticle => "TextArticle",
            Self::Text => "Text",
            Self::Decision => "Decision",
            Self::Ecli => "Ecli",
            Self::CnilDeliberation => "CnilDeliberation",
            Self::Convention => "Convention",
            Self::Jorf => "Jorf",
            Self::Nor => "Nor",
            Self::Eli => "Eli",
            Self::LegifranceId => "LegifranceId",
            Self::Unsupported => "Unsupported",
        }
    }

    /// Nom en `snake_case`, pour la vue MCP.
    pub const fn snake_case(self) -> &'static str {
        match self {
            Self::CodeArticle => "code_article",
            Self::TextArticle => "text_article",
            Self::Text => "text",
            Self::Decision => "decision",
            Self::Ecli => "ecli",
            Self::CnilDeliberation => "cnil_deliberation",
            Self::Convention => "convention",
            Self::Jorf => "jorf",
            Self::Nor => "nor",
            Self::Eli => "eli",
            Self::LegifranceId => "legifrance_id",
            Self::Unsupported => "unsupported",
        }
    }
}

impl CitationRef {
    /// Famille de la citation.
    pub const fn kind(&self) -> CitationKind {
        match self {
            Self::CodeArticle { .. } => CitationKind::CodeArticle,
            Self::TextArticle { .. } => CitationKind::TextArticle,
            Self::Text(_) => CitationKind::Text,
            Self::Decision(_) => CitationKind::Decision,
            Self::Ecli(_) => CitationKind::Ecli,
            Self::CnilDeliberation(_) => CitationKind::CnilDeliberation,
            Self::Convention(_) => CitationKind::Convention,
            Self::Jorf(_) => CitationKind::Jorf,
            Self::Nor(_) => CitationKind::Nor,
            Self::Eli(_) => CitationKind::Eli,
            Self::LegifranceId(_) => CitationKind::LegifranceId,
            Self::Unsupported(_) => CitationKind::Unsupported,
        }
    }
}

/// Code cité par un article.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CodeRef {
    /// Code du dictionnaire.
    Known(CodeKey),
    /// Code absent du dictionnaire, gardé par son titre (`code de la voirie`).
    Unknown(String),
    /// « Dudit code », « du même code ».
    Anaphora(Anaphora),
    /// Aucun code cité.
    Missing,
}

/// Comment le code d'un article a été trouvé quand il n'est pas écrit à côté.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CodeVia {
    /// « Du même code ».
    AnaphoreMemeCode,
    /// « Dudit code ».
    AnaphoreDudit,
    /// Contexte du document (`ParseOptions.default_text`).
    ContexteDocument,
}

impl CodeVia {
    /// Valeur du corpus.
    pub const fn code(self) -> &'static str {
        match self {
            Self::AnaphoreMemeCode => "anaphore:meme_code",
            Self::AnaphoreDudit => "anaphore:dudit",
            Self::ContexteDocument => "contexte_document",
        }
    }
}

/// Formule qui renvoie au dernier code cité.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Anaphora {
    /// « Du même code ».
    MemeCode,
    /// « Dudit code ».
    Dudit,
    /// « Du présent code ».
    PresentCode,
    /// « De ce code ».
    CeCode,
}

/// Un article et ses subdivisions.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ArticleRef {
    /// Numéro.
    pub num: ArticleNum,
    /// Subdivisions, du plus grand niveau au plus petit.
    pub sub: Vec<Subdivision>,
    /// Fin d'une plage (« articles 1100 à 1100-2 »).
    pub range_to: Option<ArticleNum>,
}

/// Préfixe d'un numéro d'article de code.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ArticlePrefix {
    /// Partie législative.
    L,
    /// Loi organique.
    Lo,
    /// Décret en Conseil d'État.
    R,
    /// Décret en Conseil d'État pris en conseil des ministres.
    RStar,
    /// `R**`, présent dans LEGI.
    RStarStar,
    /// Décret simple.
    D,
    /// `D*`.
    DStar,
    /// `D**`.
    DStarStar,
    /// Arrêté.
    A,
}

impl ArticlePrefix {
    /// Préfixe sans point ni espace (`R*`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::L => "L",
            Self::Lo => "LO",
            Self::R => "R",
            Self::RStar => "R*",
            Self::RStarStar => "R**",
            Self::D => "D",
            Self::DStar => "D*",
            Self::DStarStar => "D**",
            Self::A => "A",
        }
    }

    /// Préfixe sans étoile (`R*` donne `R`).
    #[must_use]
    pub const fn without_stars(self) -> Self {
        match self {
            Self::RStar | Self::RStarStar => Self::R,
            Self::DStar | Self::DStarStar => Self::D,
            other => other,
        }
    }
}

/// Numéro d'article sans chiffre.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SpecialNum {
    /// `1er`.
    Premier,
    /// Article unique.
    Unique,
    /// Article liminaire.
    Liminaire,
    /// Article préliminaire.
    Preliminaire,
    /// Alinéa du Préambule, sans numéro d'article (P085).
    SansNumero,
}

impl SpecialNum {
    /// Forme minuscule attendue par l'API, vide pour `SansNumero`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Premier => "1er",
            Self::Unique => "unique",
            Self::Liminaire => "liminaire",
            Self::Preliminaire => "préliminaire",
            Self::SansNumero => "",
        }
    }

    /// Mot d'un texte, sans tenir compte de la casse (`Unique`, `premier`, `1er`).
    pub fn parse(word: &str) -> Option<Self> {
        match word.to_lowercase().as_str() {
            "1er" | "premier" => Some(Self::Premier),
            "unique" => Some(Self::Unique),
            "liminaire" => Some(Self::Liminaire),
            "préliminaire" | "preliminaire" => Some(Self::Preliminaire),
            _ => None,
        }
    }
}

/// Adverbe latin d'un numéro (`bis`, `undecies`…).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum LatinAdverb {
    /// `bis`.
    Bis,
    /// `ter`.
    Ter,
    /// `quater`.
    Quater,
    /// `quinquies`.
    Quinquies,
    /// `sexies`.
    Sexies,
    /// `septies`.
    Septies,
    /// `octies`.
    Octies,
    /// `nonies`.
    Nonies,
    /// Autre adverbe valide de LEGI (`undecies`, `vicies`, `semel`…), en minuscules.
    Other(String),
}

/// Préfixes d'unités des adverbes composés (`un` dans `undecies`).
const LATIN_UNITS: [&str; 10] =
    ["", "un", "duo", "ter", "quater", "quin", "sex", "sept", "octo", "novo"];

/// Dizaines des adverbes composés (`dec` dans `undecies`).
const LATIN_TENS: [&str; 9] =
    ["dec", "vic", "tric", "quadrag", "quinquag", "sexag", "septuag", "octog", "nonag"];

impl LatinAdverb {
    /// Forme minuscule.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Bis => "bis",
            Self::Ter => "ter",
            Self::Quater => "quater",
            Self::Quinquies => "quinquies",
            Self::Sexies => "sexies",
            Self::Septies => "septies",
            Self::Octies => "octies",
            Self::Nonies => "nonies",
            Self::Other(s) => s,
        }
    }

    /// Adverbe d'un mot (`LATIN_ADV` de l'annexe B §4.1), sans tenir compte de la casse. Les
    /// formes fautives présentes dans LEGI (`quinquedecies`, `sexties`) sont admises.
    pub fn parse(word: &str) -> Option<Self> {
        let lower = word.to_lowercase();
        match lower.as_str() {
            "bis" => return Some(Self::Bis),
            "ter" => return Some(Self::Ter),
            "quater" => return Some(Self::Quater),
            "quinquies" => return Some(Self::Quinquies),
            "sexies" => return Some(Self::Sexies),
            "septies" => return Some(Self::Septies),
            "octies" => return Some(Self::Octies),
            "nonies" => return Some(Self::Nonies),
            _ => {}
        }
        let composed = ["ies", "iès"].iter().any(|end| {
            lower.strip_suffix(end).is_some_and(|stem| {
                LATIN_UNITS.iter().any(|unit| {
                    stem.strip_prefix(unit).is_some_and(|tens| LATIN_TENS.contains(&tens))
                })
            })
        });
        let known = matches!(lower.as_str(), "semel" | "quinto" | "quinquedecies" | "sexties");
        (composed || known).then_some(Self::Other(lower))
    }
}

/// Numéro d'article (annexe B §6.3). `num_key()` est la forme envoyée à l'API.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ArticleNum {
    /// Préfixe (`L`, `R*`…).
    pub prefix: Option<ArticlePrefix>,
    /// Numéro sans chiffre (`1er`, `unique`…).
    pub special: Option<SpecialNum>,
    /// Parties numériques : `[1234, 5]` pour `1234-5`.
    pub base: Vec<u32>,
    /// Adverbe latin placé avant les lettres (`undecies` dans `199 undecies B`).
    pub latin: Option<LatinAdverb>,
    /// Entier collé à l'adverbe par un tiret (`0` dans `199 terdecies-0 A`).
    pub latin_sub: Option<u32>,
    /// Lettres de suffixe (`B` dans `199 undecies B`).
    pub letters: Option<String>,
    /// Adverbe latin placé après les lettres (`ter` dans `150-0 B ter`).
    pub latin_after: Option<LatinAdverb>,
}

/// Suffixes d'un numéro d'article en cours de lecture : `[adverbe [-entier]] [lettres] [adverbe]`.
/// Machine à états commune à la grammaire (`art_num`) et à `ArticleNum::from_str`.
#[derive(Default)]
pub(crate) struct Suffixes {
    /// Adverbe avant les lettres.
    pub(crate) latin: Option<LatinAdverb>,
    /// Entier collé à cet adverbe.
    pub(crate) latin_sub: Option<u32>,
    /// Lettres de suffixe.
    pub(crate) letters: Option<String>,
    /// Adverbe après les lettres.
    pub(crate) latin_after: Option<LatinAdverb>,
}

impl Suffixes {
    /// Ajoute un adverbe : le premier avant toute lettre, le second après les lettres. Refuse
    /// deux adverbes consécutifs (`5 bis ter`) et un troisième.
    pub(crate) fn push_adverb(&mut self, adverb: LatinAdverb) -> bool {
        if self.letters.is_some() && self.latin_after.is_none() {
            self.latin_after = Some(adverb);
            true
        } else if self.letters.is_none() && self.latin.is_none() {
            self.latin = Some(adverb);
            true
        } else {
            false
        }
    }

    /// Ajoute l'entier collé au premier adverbe (`terdecies-0`).
    pub(crate) fn push_sub(&mut self, sub: u32) -> bool {
        let fits = self.latin.is_some() && self.letters.is_none() && self.latin_sub.is_none();
        if fits {
            self.latin_sub = Some(sub);
        }
        fits
    }

    /// Ajoute les lettres : une seule fois.
    pub(crate) fn push_letters(&mut self, letters: String) -> bool {
        let fits = self.letters.is_none();
        if fits {
            self.letters = Some(letters);
        }
        fits
    }

    /// Le dernier suffixe lu est un adverbe : ce qui suit ne peut plus être une lettre (`885-0 V
    /// bis A`) sans que le numéro soit tronqué.
    pub(crate) fn after_adverb(&self) -> bool {
        self.latin_after.is_some() || (self.latin.is_some() && self.letters.is_none())
    }
}

impl ArticleNum {
    /// Clé canonique : préfixe sans point ni espace, numéro, puis suffixes séparés par une
    /// espace (`L761-1`, `R*111-1`, `199 undecies B`, `150-0 B ter`, `199 terdecies-0 A`, `1er`).
    /// Vide pour `SansNumero`.
    pub fn num_key(&self) -> String {
        if let Some(special) = self.special {
            return special.as_str().to_owned();
        }
        let mut key = self.prefix.map_or("", ArticlePrefix::as_str).to_owned();
        let digits: Vec<String> = self.base.iter().map(u32::to_string).collect();
        key.push_str(&digits.join("-"));
        if let Some(latin) = &self.latin {
            key.push(' ');
            key.push_str(latin.as_str());
            if let Some(sub) = self.latin_sub {
                key.push('-');
                key.push_str(&sub.to_string());
            }
        }
        if let Some(letters) = &self.letters {
            key.push(' ');
            key.push_str(letters);
        }
        if let Some(latin) = &self.latin_after {
            key.push(' ');
            key.push_str(latin.as_str());
        }
        key
    }

    /// Formes essayées dans l'ordre par la résolution : `1er` puis `1` ; `R*111-1` puis
    /// `R111-1` ; `unique` puis `Unique`.
    pub fn key_variants(&self) -> Vec<String> {
        let key = self.num_key();
        let mut variants = vec![key.clone()];
        match (self.special, self.prefix) {
            (Some(SpecialNum::Premier), _) => variants.push("1".to_owned()),
            (Some(SpecialNum::Unique | SpecialNum::Liminaire | SpecialNum::Preliminaire), _) => {
                let mut chars = key.chars();
                let first: String = chars.next().into_iter().flat_map(char::to_uppercase).collect();
                variants.push(first + chars.as_str());
            }
            (None, Some(prefix)) if prefix != prefix.without_stars() => {
                let plain = self.prefix.map_or("", |p| p.without_stars().as_str());
                variants.push(key.replacen(prefix.as_str(), plain, 1));
            }
            _ => {}
        }
        variants
    }
}

/// Préfixes d'articles, des plus longs aux plus courts pour une même lettre.
const PREFIXES: [(&str, ArticlePrefix); 9] = [
    ("LO", ArticlePrefix::Lo),
    ("R**", ArticlePrefix::RStarStar),
    ("R*", ArticlePrefix::RStar),
    ("R", ArticlePrefix::R),
    ("D**", ArticlePrefix::DStarStar),
    ("D*", ArticlePrefix::DStar),
    ("D", ArticlePrefix::D),
    ("A", ArticlePrefix::A),
    ("L", ArticlePrefix::L),
];

/// Sépare un préfixe d'article (`L.`, `R*`) de ce qui le suit, qui doit commencer par un chiffre.
fn split_prefix(text: &str) -> (Option<ArticlePrefix>, &str) {
    for (spelled, prefix) in PREFIXES {
        if let Some(rest) = text.strip_prefix(spelled) {
            let rest = rest.strip_prefix('.').unwrap_or(rest).trim_start();
            if rest.starts_with(|c: char| c.is_ascii_digit()) {
                return (Some(prefix), rest);
            }
        }
    }
    (None, text)
}

/// L'unique élément d'une liste d'un seul élément.
fn single(parts: &[u32]) -> Option<u32> {
    match parts {
        &[one] => Some(one),
        _ => None,
    }
}

/// Parties numériques d'un numéro (`1234-5` donne `[1234, 5]`).
fn parse_base(word: &str) -> Option<Vec<u32>> {
    word.split('-')
        .map(|part| {
            let digits = !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
            digits.then(|| part.parse().ok()).flatten()
        })
        .collect()
}

impl FromStr for ArticleNum {
    type Err = NumberParseError;

    /// Même grammaire que `art_num` : `L. 761-1`, `R*111-1`, `1er`, `199 undecies B`. Sert au
    /// gabarit `legifrance://code/{code}/article/{num}` (M7).
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = NumberParseError { kind: "article" };
        let text = text.trim();
        if let Some(special) = SpecialNum::parse(text) {
            return Ok(Self {
                prefix: None,
                special: Some(special),
                base: Vec::new(),
                latin: None,
                latin_sub: None,
                letters: None,
                latin_after: None,
            });
        }
        let (prefix, rest) = split_prefix(text);
        let mut words = rest.split_whitespace();
        let base = words.next().and_then(parse_base).ok_or(invalid)?;
        let mut found = Suffixes::default();
        for word in words {
            let (head, sub) = word.split_once('-').map_or((word, None), |(h, t)| (h, Some(t)));
            let is_letters = word.len() <= 2 && word.bytes().all(|b| b.is_ascii_uppercase());
            let accepted = if let Some(adverb) = LatinAdverb::parse(head) {
                let sub = sub.map(|digits| parse_base(digits).and_then(|parts| single(&parts)));
                found.push_adverb(adverb)
                    && sub.is_none_or(|n| n.is_some_and(|n| found.push_sub(n)))
            } else {
                is_letters && found.push_letters(word.to_owned())
            };
            if !accepted {
                return Err(invalid);
            }
        }
        let Suffixes { latin, latin_sub, letters, latin_after } = found;
        Ok(Self { prefix, special: None, base, latin, latin_sub, letters, latin_after })
    }
}

/// Subdivision d'un article.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Subdivision {
    /// Paragraphe en chiffres romains (`I`, `II`) ou `§ 2`.
    Paragraphe(String),
    /// Alinéa numéroté.
    Alinea(u16),
    /// Dernier alinéa.
    DernierAlinea,
    /// Item (`3°`).
    Item(u16),
    /// Lettre (`a)`).
    Lettre(char),
    /// Phrase numérotée.
    Phrase(u16),
    /// Dernière phrase.
    DernierePhrase,
    /// Section.
    Section(u16),
}

/// Qualificatif de version d'un article.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum VersionQualifier {
    /// « Ancien ».
    Ancien,
    /// « Nouveau ».
    Nouveau,
    /// « Dans sa rédaction applicable au litige ».
    RedactionApplicableAuLitige,
    /// « Dans sa rédaction applicable à la date des faits ».
    RedactionApplicableDateDesFaits,
    /// « Dans sa rédaction en vigueur [à la date]… ».
    RedactionEnVigueur(Option<CivilDate>),
    /// « Dans sa rédaction issue de… ».
    RedactionIssueDe(Box<TextCite>),
    /// « Dans sa rédaction antérieure à celle issue de… ».
    RedactionAnterieureA(Box<TextCite>),
    /// « En vigueur au… ».
    EnVigueurAu(CivilDate),
}

/// Nature d'un texte.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TextNature {
    /// Loi.
    Loi,
    /// Loi organique.
    LoiOrganique,
    /// Loi constitutionnelle.
    LoiConstitutionnelle,
    /// Loi de finances.
    LoiDeFinances,
    /// Loi de financement de la sécurité sociale.
    LoiDeFinancementSecu,
    /// Loi du pays.
    LoiDuPays,
    /// Ordonnance.
    Ordonnance,
    /// Décret.
    Decret,
    /// Décret-loi.
    DecretLoi,
    /// Arrêté.
    Arrete,
    /// Règlement.
    Reglement,
    /// Circulaire.
    Circulaire,
    /// Instruction.
    Instruction,
    /// Délibération.
    Deliberation,
    /// Décision.
    Decision,
    /// Constitution du 4 octobre 1958.
    Constitution,
    /// Déclaration des droits de l'homme et du citoyen.
    Ddhc,
    /// Préambule de la Constitution de 1946.
    Preambule1946,
    /// Charte de l'environnement.
    CharteEnvironnement,
}

impl TextNature {
    /// Les dix-neuf natures.
    pub const ALL: [Self; 19] = [
        Self::Loi,
        Self::LoiOrganique,
        Self::LoiConstitutionnelle,
        Self::LoiDeFinances,
        Self::LoiDeFinancementSecu,
        Self::LoiDuPays,
        Self::Ordonnance,
        Self::Decret,
        Self::DecretLoi,
        Self::Arrete,
        Self::Reglement,
        Self::Circulaire,
        Self::Instruction,
        Self::Deliberation,
        Self::Decision,
        Self::Constitution,
        Self::Ddhc,
        Self::Preambule1946,
        Self::CharteEnvironnement,
    ];

    /// Valeur du corpus (`LOI`, `LOI_ORGANIQUE`, `DECRET_LOI`…).
    pub const fn code(self) -> &'static str {
        match self {
            Self::Loi => "LOI",
            Self::LoiOrganique => "LOI_ORGANIQUE",
            Self::LoiConstitutionnelle => "LOI_CONSTIT",
            Self::LoiDeFinances => "LOI_FINANCES",
            Self::LoiDeFinancementSecu => "LOI_FSS",
            Self::LoiDuPays => "LOI_DU_PAYS",
            Self::Ordonnance => "ORDONNANCE",
            Self::Decret => "DECRET",
            Self::DecretLoi => "DECRET_LOI",
            Self::Arrete => "ARRETE",
            Self::Reglement => "REGLEMENT",
            Self::Circulaire => "CIRCULAIRE",
            Self::Instruction => "INSTRUCTION",
            Self::Deliberation => "DELIBERATION",
            Self::Decision => "DECISION",
            Self::Constitution => "CONSTITUTION",
            Self::Ddhc => "DDHC",
            Self::Preambule1946 => "PREAMBULE_1946",
            Self::CharteEnvironnement => "CHARTE_ENVIRONNEMENT",
        }
    }
}

/// Formule qui renvoie à un texte cité plus haut.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TextAnaphora {
    /// « Susvisée ».
    Susvisee,
    /// « Précitée ».
    Precitee,
    /// « Mentionnée ci-dessus ».
    MentionneeCiDessus,
}

/// Numéro d'un texte (`2016-1321`, `93-1419`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TextNumber {
    /// Année (`1993` pour `93-1419`), `None` pour une numérotation sans année.
    pub year: Option<u16>,
    /// Numéro d'ordre.
    pub seq: u32,
    /// Troisième composant éventuel.
    pub extra: Option<u32>,
    /// Numéro tel qu'écrit.
    pub raw: String,
}

/// Un texte cité.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TextCite {
    /// Nature.
    pub nature: TextNature,
    /// Numéro.
    pub number: Option<TextNumber>,
    /// Date, `None` si absente ou invalide.
    pub date: Option<CivilDate>,
    /// Date telle qu'écrite quand elle n'a pas pu être lue (calendrier républicain).
    pub date_raw: Option<String>,
    /// Début du titre, capturé hors du span.
    pub title_hint: Option<String>,
    /// « Susvisée », « précitée ».
    pub anaphora: Option<TextAnaphora>,
    /// Appellation d'usage (« loi Macron »).
    pub appellation: Option<String>,
    /// Émetteur (« ministériel »).
    pub emitter: Option<String>,
}

/// Une décision de justice citée.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DecisionCite {
    /// Juridiction, `None` si non écrite (N21, N28, P114).
    pub court: Option<Court>,
    /// Comment la juridiction a été déduite, quand elle n'est pas écrite.
    pub court_via: Option<CourtVia>,
    /// Date de la décision.
    pub date: Option<CivilDate>,
    /// Numéros d'affaire.
    pub numbers: Vec<CaseNumber>,
    /// Nature de la décision.
    pub kind: Option<DecisionKind>,
    /// Publication.
    pub publication: Option<Publication>,
    /// Solution entre parenthèses.
    pub solution: Option<String>,
    /// Parties.
    pub parties: Option<String>,
    /// Objet.
    pub subject: Option<String>,
    /// Numéro d'arrêt (« Arrêt n° 399 »).
    pub arret_num: Option<u32>,
    /// Code de formation (`FS-B`).
    pub formation_code: Option<String>,
    /// Lettres de publication (`B`, `R`, `I`…).
    pub publication_codes: Vec<char>,
}

/// Nature d'une décision.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DecisionKind {
    /// Arrêt.
    Arret,
    /// Avis.
    Avis,
    /// Ordonnance.
    Ordonnance,
    /// Jugement.
    Jugement,
}

/// Juridiction d'une décision.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Court {
    /// Cour de cassation.
    CourDeCassation {
        /// Chambre.
        chamber: Option<CassChamber>,
    },
    /// Cour d'appel.
    CourAppel {
        /// Siège.
        seat: Option<CaSeat>,
        /// Chambre, gardée brute (`Pôle 1 - Chambre 12`).
        chamber: Option<String>,
    },
    /// Tribunal judiciaire.
    TribunalJudiciaire {
        /// Siège, en majuscules sans accents.
        seat: Option<String>,
    },
    /// Autre juridiction judiciaire (`T. com.`, `CPH`).
    AutreJudiciaire {
        /// Libellé.
        label: String,
    },
    /// Conseil d'État.
    ConseilEtat {
        /// Formation.
        formation: Option<CeFormation>,
        /// Chambres citées (« 2ème - 7ème chambres réunies » donne `[2, 7]`).
        chambers: Vec<u8>,
    },
    /// Cour administrative d'appel.
    Caa {
        /// Siège.
        seat: Option<CaaSeat>,
        /// Chambre, gardée brute.
        chamber: Option<String>,
    },
    /// Tribunal administratif.
    Ta {
        /// Siège.
        seat: Option<TaSeat>,
        /// Chambre, gardée brute.
        chamber: Option<String>,
    },
    /// Conseil constitutionnel.
    ConseilConstitutionnel,
}

impl Court {
    /// Famille de juridiction, valeur `court` du corpus (`CCASS`, `CA`, `TJ`, `CE`, `CAA`, `TA`,
    /// `CC`). `AUTRE` pour une autre juridiction judiciaire, que le corpus ne code pas.
    pub const fn family_code(&self) -> &'static str {
        match self {
            Self::CourDeCassation { .. } => "CCASS",
            Self::CourAppel { .. } => "CA",
            Self::TribunalJudiciaire { .. } => "TJ",
            Self::AutreJudiciaire { .. } => "AUTRE",
            Self::ConseilEtat { .. } => "CE",
            Self::Caa { .. } => "CAA",
            Self::Ta { .. } => "TA",
            Self::ConseilConstitutionnel => "CC",
        }
    }
}

/// Comment la juridiction d'une décision a été déduite.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CourtVia {
    /// De la forme du numéro (`forme_du_numero`, N29).
    FormeDuNumero,
    /// D'une chambre écrite en toutes lettres seule (`chambre`, P107).
    Chambre,
}

impl CourtVia {
    /// Valeur du corpus.
    pub const fn code(self) -> &'static str {
        match self {
            Self::FormeDuNumero => "forme_du_numero",
            Self::Chambre => "chambre",
        }
    }
}

/// Numéro d'affaire.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CaseNumber {
    /// Pourvoi en cassation.
    Pourvoi(Pourvoi),
    /// Répertoire général d'une juridiction du fond.
    Rg(RgNumber),
    /// Numéro Portalis.
    Portalis(String),
    /// Requête au Conseil d'État.
    CeRequete(u32),
    /// Requête devant une cour administrative d'appel.
    Caa(CaaNumber),
    /// Requête devant un tribunal administratif.
    Ta(TaNumber),
    /// Numéro du Conseil constitutionnel.
    Cc(CcNumber),
    /// Numéro de forme inconnue.
    Unknown(String),
}

/// Publication d'une décision.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Publication {
    /// Bulletin des arrêts de la Cour de cassation.
    pub bulletin: Option<BulletinRef>,
    /// Recueil Lebon.
    pub lebon: Option<LebonRef>,
}

/// Mention au bulletin.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BulletinRef {
    /// « Publié au bulletin ».
    Publie,
    /// « Inédit ».
    Inedit,
    /// Référence (« Bull. civ. I, n° 270 »).
    Reference(String),
}

/// Mention au recueil Lebon.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LebonRef {
    /// Publié au recueil.
    Publie,
    /// Mentionné aux tables.
    Tables,
    /// Inédit.
    Inedit,
    /// Page (`p. 541`).
    Page(String),
}

/// Chiffres d'un texte : `n` chiffres ASCII et rien d'autre.
fn fixed_digits<T: FromStr>(text: &str, n: usize) -> Option<T> {
    (text.len() == n && text.bytes().all(|b| b.is_ascii_digit())).then(|| text.parse().ok())?
}

/// Même chose en `u8`.
fn two_digits(text: &str) -> Option<u8> {
    fixed_digits(text, 2)
}

/// Numéro de pourvoi `AA-NN.NNN` (`N 24-13.470` avec sa lettre-clé).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Pourvoi {
    /// Année, sur deux chiffres.
    pub yy: u8,
    /// Second bloc, sur deux chiffres (sens **NON VÉRIFIÉ** [V-151 · pourvoi-structure]).
    pub mid: u8,
    /// Numéro d'ordre, sur trois chiffres.
    pub seq: u16,
    /// Lettre-clé qui précède le numéro (`N`).
    pub key: Option<char>,
}

impl FromStr for Pourvoi {
    type Err = NumberParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = NumberParseError { kind: "pourvoi" };
        let (key, number) = match text.split_once(' ') {
            Some((letter, rest)) => {
                let mut chars = letter.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) if c.is_ascii_uppercase() => (Some(c), rest.trim_start()),
                    _ => return Err(invalid),
                }
            }
            None => (None, text),
        };
        let (yy, rest) = number.split_once('-').ok_or(invalid)?;
        let (mid, seq) = match rest.split_once('.') {
            Some((mid, seq)) => (mid, seq),
            None => rest.split_at_checked(2).ok_or(invalid)?,
        };
        Ok(Self {
            yy: two_digits(yy).ok_or(invalid)?,
            mid: two_digits(mid).ok_or(invalid)?,
            seq: fixed_digits(seq, 3).ok_or(invalid)?,
            key,
        })
    }
}

impl fmt::Display for Pourvoi {
    /// Forme canonique `21-23.174`, sans lettre-clé.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}-{:02}.{:03}", self.yy, self.mid, self.seq)
    }
}

/// Numéro de répertoire général `AA/NNNN` ou `AA/NNNNN`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct RgNumber {
    /// Année, sur deux chiffres.
    pub yy: u8,
    /// Numéro d'ordre.
    pub seq: u32,
}

impl FromStr for RgNumber {
    type Err = NumberParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = NumberParseError { kind: "rg" };
        let (yy, seq) = text.split_once('/').ok_or(invalid)?;
        if !matches!(seq.len(), 4 | 5) {
            return Err(invalid);
        }
        Ok(Self {
            yy: two_digits(yy).ok_or(invalid)?,
            seq: fixed_digits(seq, seq.len()).ok_or(invalid)?,
        })
    }
}

impl fmt::Display for RgNumber {
    /// Forme `24/15215`, complétée à quatre chiffres au moins. Les zéros de tête d'un numéro
    /// écrit sur cinq chiffres (`14/00554`) ne sont pas gardés : le texte brut les porte.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}/{:04}", self.yy, self.seq)
    }
}

/// Numéro de requête d'une cour administrative d'appel (`24MA02429`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct CaaNumber {
    /// Année, sur deux chiffres.
    pub yy: u8,
    /// Cour.
    pub court: CaaCode,
    /// Numéro d'ordre, sur cinq chiffres.
    pub seq: u32,
}

impl FromStr for CaaNumber {
    type Err = NumberParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = NumberParseError { kind: "caa" };
        let (yy, rest) = text.split_at_checked(2).ok_or(invalid)?;
        let (code, seq) = rest.split_at_checked(2).ok_or(invalid)?;
        Ok(Self {
            yy: two_digits(yy).ok_or(invalid)?,
            court: CaaCode::parse(code).ok_or(invalid)?,
            seq: fixed_digits(seq, 5).ok_or(invalid)?,
        })
    }
}

impl fmt::Display for CaaNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}{}{:05}", self.yy, self.court.as_str(), self.seq)
    }
}

/// Numéro de requête d'un tribunal administratif : sept chiffres (`2005064`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TaNumber(pub u32);

impl FromStr for TaNumber {
    type Err = NumberParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        fixed_digits(text, 7).map(Self).ok_or(NumberParseError { kind: "ta" })
    }
}

impl fmt::Display for TaNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:07}", self.0)
    }
}

/// Première année du Conseil constitutionnel : 1959, début de la forme à deux chiffres.
const CC_FIRST_YEAR: u16 = 1959;

/// Numéro d'une décision du Conseil constitutionnel (`2025-1173 QPC`, `2010-14/22 QPC`,
/// `71-44 DC`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct CcNumber {
    /// Année de la saisine.
    pub year: u16,
    /// Numéro d'ordre dans l'année.
    pub seq: u32,
    /// Numéros joints (`22` dans `2010-14/22`).
    pub joined: Vec<u32>,
    /// Décision rectificative (`2015-491R`).
    pub rectif: bool,
    /// Type de la décision.
    pub kind: CcType,
}

impl FromStr for CcNumber {
    type Err = NumberParseError;

    /// Forme complète, type compris : `2025-1173 QPC`.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = NumberParseError { kind: "cc" };
        let (number, kind) = text.split_once(' ').ok_or(invalid)?;
        let kind = CcType::parse(kind.trim()).ok_or(invalid)?;
        let (number, rectif) = number.strip_suffix('R').map_or((number, false), |n| (n, true));
        let (year, rest) = number.split_once('-').ok_or(invalid)?;
        let mut parts = rest.split('/').map(|p| fixed_digits_any(p).ok_or(invalid));
        let seq = parts.next().unwrap_or(Err(invalid))?;
        Ok(Self {
            year: cc_year(year).ok_or(invalid)?,
            seq,
            joined: parts.collect::<Result<_, _>>()?,
            rectif,
            kind,
        })
    }
}

/// Année d'un numéro du Conseil constitutionnel : quatre chiffres, ou deux chiffres de 59 à 99
/// (`71` pour 1971, forme de `NUM_DEC` des décisions de 1959 à 1999).
fn cc_year(text: &str) -> Option<u16> {
    match two_digits(text) {
        Some(yy) => (CC_FIRST_YEAR..=1999)
            .contains(&1900_u16.saturating_add(u16::from(yy)))
            .then(|| 1900_u16.saturating_add(u16::from(yy))),
        None => fixed_digits(text, 4),
    }
}

/// Un nombre en chiffres ASCII, sans signe.
fn fixed_digits_any(text: &str) -> Option<u32> {
    (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())).then(|| text.parse().ok())?
}

impl CcNumber {
    /// Première forme du numéro, sans numéros joints, `R` ni type : `71-44`, `2010-14`. C'est la
    /// forme écrite dans `NUM_DEC` de Légifrance, que la recherche par numéro attend.
    pub fn first_form(&self) -> String {
        format!("{}-{}", self.year_text(), self.seq)
    }

    /// Année écrite comme Légifrance : deux chiffres de 1959 à 1999, sinon quatre.
    fn year_text(&self) -> String {
        if (CC_FIRST_YEAR..=1999).contains(&self.year) {
            format!("{:02}", self.year.saturating_sub(1900))
        } else {
            format!("{:04}", self.year)
        }
    }

    /// Forme complète `2025-1173 QPC`, que `FromStr` relit.
    pub fn full(&self) -> String {
        format!("{self} {}", self.kind.as_str())
    }
}

impl fmt::Display for CcNumber {
    /// Forme canonique `2025-1173`, avec les numéros joints et le `R` de rectification, sans
    /// le type.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Deux chiffres de 1959 à 1999 (`71-44`), sinon quatre : `FromStr` relit les deux formes.
        let mut out = self.first_form();
        for joined in &self.joined {
            out.push('/');
            out.push_str(&joined.to_string());
        }
        if self.rectif {
            out.push('R');
        }
        f.write_str(&out)
    }
}

/// Numéro d'une délibération de la CNIL (`SAN-2026-002`, `2023-062`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct CnilNumber {
    /// Série (`SAN`, `MED`).
    pub series: Option<String>,
    /// Année.
    pub year: u16,
    /// Numéro d'ordre, sur trois chiffres.
    pub seq: u16,
}

impl FromStr for CnilNumber {
    type Err = NumberParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = NumberParseError { kind: "cnil" };
        let mut parts: Vec<&str> = text.split('-').collect();
        let seq = parts.pop().and_then(|s| fixed_digits(s, 3)).ok_or(invalid)?;
        let year = parts.pop().and_then(|y| fixed_digits(y, 4)).ok_or(invalid)?;
        let series = match parts.as_slice() {
            [] => None,
            [s] if (2..=4).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_uppercase()) => {
                Some((*s).to_owned())
            }
            _ => return Err(invalid),
        };
        Ok(Self { series, year, seq })
    }
}

impl fmt::Display for CnilNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let series = self.series.as_ref().map_or_else(String::new, |s| format!("{s}-"));
        write!(f, "{series}{:04}-{:03}", self.year, self.seq)
    }
}

/// Une délibération de la CNIL citée.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CnilCite {
    /// Série (`SAN`), absente des numéros anciens.
    pub series: Option<String>,
    /// Année.
    pub year: u16,
    /// Numéro d'ordre.
    pub seq: u16,
    /// Date de la délibération.
    pub date: Option<CivilDate>,
    /// Délibération de la formation restreinte.
    pub formation_restreinte: bool,
}

/// Nature d'un accord collectif cité.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ConventionKind {
    /// Convention collective nationale (`CCN`).
    Ccn,
    /// Convention collective.
    Convention,
    /// Accord (national, de branche, d'entreprise).
    Accord,
    /// Accord national interprofessionnel (`ANI`).
    Ani,
    /// Avenant.
    Avenant,
    /// Annexe.
    Annexe,
}

impl ConventionKind {
    /// Valeur `conv_kind` du corpus (`CCN`, `AVENANT`…).
    pub const fn code(self) -> &'static str {
        match self {
            Self::Ccn => "CCN",
            Self::Convention => "CONVENTION",
            Self::Accord => "ACCORD",
            Self::Ani => "ANI",
            Self::Avenant => "AVENANT",
            Self::Annexe => "ANNEXE",
        }
    }
}

/// Une convention collective citée.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConventionCite {
    /// Nature.
    pub kind: Option<ConventionKind>,
    /// Identifiant de convention collective.
    pub idcc: Option<Idcc>,
    /// Numéro de brochure du Journal officiel.
    pub brochure: Option<u16>,
    /// Numéro d'avenant.
    pub avenant: Option<u32>,
    /// Titre, capturé dans le span.
    pub title_hint: Option<String>,
    /// Date.
    pub date: Option<CivilDate>,
}

/// Une référence au Journal officiel (`JORF n°0263 du 8 novembre 2025, texte n° 152`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct JorfCite {
    /// Numéro du Journal officiel.
    pub number: Option<u16>,
    /// Date de parution.
    pub date: Option<CivilDate>,
    /// Numéro du texte.
    pub texte: Option<u32>,
    /// Page.
    pub page: Option<u32>,
}

/// Préfixe d'un identifiant Légifrance : les seize préfixes de l'annexe B §4.13.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum IdPrefix {
    /// `LEGITEXT`.
    LegiText,
    /// `LEGIARTI`.
    LegiArti,
    /// `LEGISCTA`.
    LegiScta,
    /// `JORFTEXT`.
    JorfText,
    /// `JORFARTI`.
    JorfArti,
    /// `JORFCONT`.
    JorfCont,
    /// `JORFDOLE`.
    JorfDole,
    /// `JURITEXT`.
    JuriText,
    /// `CETATEXT`.
    CetaText,
    /// `CONSTEXT`.
    ConsText,
    /// `CNILTEXT`.
    CnilText,
    /// `KALITEXT`.
    KaliText,
    /// `KALIARTI`.
    KaliArti,
    /// `KALICONT`.
    KaliCont,
    /// `KALISCTA`.
    KaliScta,
    /// `ACCOTEXT`.
    AccoText,
}

impl IdPrefix {
    /// Les seize préfixes.
    pub const ALL: [Self; 16] = [
        Self::LegiText,
        Self::LegiArti,
        Self::LegiScta,
        Self::JorfText,
        Self::JorfArti,
        Self::JorfCont,
        Self::JorfDole,
        Self::JuriText,
        Self::CetaText,
        Self::ConsText,
        Self::CnilText,
        Self::KaliText,
        Self::KaliArti,
        Self::KaliCont,
        Self::KaliScta,
        Self::AccoText,
    ];

    /// Préfixe en majuscules, huit lettres.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LegiText => "LEGITEXT",
            Self::LegiArti => "LEGIARTI",
            Self::LegiScta => "LEGISCTA",
            Self::JorfText => "JORFTEXT",
            Self::JorfArti => "JORFARTI",
            Self::JorfCont => "JORFCONT",
            Self::JorfDole => "JORFDOLE",
            Self::JuriText => "JURITEXT",
            Self::CetaText => "CETATEXT",
            Self::ConsText => "CONSTEXT",
            Self::CnilText => "CNILTEXT",
            Self::KaliText => "KALITEXT",
            Self::KaliArti => "KALIARTI",
            Self::KaliCont => "KALICONT",
            Self::KaliScta => "KALISCTA",
            Self::AccoText => "ACCOTEXT",
        }
    }

    /// Préfixe d'un texte en majuscules, sensible à la casse.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == text)
    }
}

/// Un identifiant Légifrance cité, seul ou dans une URL.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LegifranceIdRef {
    /// Préfixe.
    pub prefix: IdPrefix,
    /// Les douze chiffres.
    pub digits: u64,
    /// Date du suffixe de version `_JJ-MM-AAAA`.
    pub version_start: Option<CivilDate>,
    /// Segment d'URL qui précède l'identifiant (`juri`).
    pub url_kind: Option<String>,
}

impl LegifranceIdRef {
    /// Identifiant de document (03 §2), sans suffixe de version. `None` pour les préfixes qui
    /// n'en sont pas (`JORFARTI`, `LEGISCTA`…). Cet identifiant vient du texte cité : il ne
    /// prouve pas que le document existe.
    pub fn to_document_id(&self) -> Option<DocumentId> {
        DocumentId::parse(&format!("{}{:012}", self.prefix.as_str(), self.digits)).ok()
    }
}

/// Une citation hors périmètre.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UnsupportedCite {
    /// Famille.
    pub family: UnsupportedFamily,
    /// Clé de la référence (`85/374/CEE`, `C-740/22`).
    pub key: Option<String>,
}

/// Famille hors périmètre.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum UnsupportedFamily {
    /// Droit de l'Union européenne (directive, règlement).
    Ue,
    /// Cour de justice de l'Union européenne.
    Cjue,
    /// Cour européenne des droits de l'homme.
    Cedh,
    /// Doctrine (Dalloz, JCP, revues).
    Doctrine,
    /// Recueil de jurisprudence (bulletin, Lebon).
    Recueil,
    /// ECLI d'un autre pays que la France.
    ForeignEcli,
}

impl UnsupportedFamily {
    /// Valeur `family` du corpus.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Ue => "UE",
            Self::Cjue => "CJUE",
            Self::Cedh => "CEDH",
            Self::Doctrine => "doctrine",
            Self::Recueil => "recueil",
            Self::ForeignEcli => "ecli_etranger",
        }
    }
}

/// Une citation peut-elle être cherchée dans le corpus ?
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Resolvability {
    /// Oui.
    Resolvable,
    /// Non, pour la raison indiquée.
    Unresolvable(UnresolvableWhy),
}

/// Pourquoi une citation n'est pas résoluble.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum UnresolvableWhy {
    /// Hors du périmètre de Légifrance.
    HorsPerimetre(UnsupportedFamily),
    /// Anaphore sans antécédent.
    AnaphoreNonResolue,
    /// Sigle ambigu sans contexte.
    SigleSansContexte(Sigle),
    /// Article sans code ni texte.
    SansCode,
    /// Référence de publication seule.
    PublicationSeule,
    /// Un drapeau de gravité `Impossible`.
    Impossible(Flag),
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn num(text: &str) -> ArticleNum {
        text.parse().expect("numéro valide")
    }

    #[test]
    fn span_to_chars_counts_code_points() {
        let input = "é à, article L. 1\u{1F600}x";
        let start = input.find("article").expect("présent");
        let end = input.find('x').expect("présent");
        let chars = Span { start, end }.to_chars(input);
        assert_eq!(chars, Span { start: 5, end: 18 });
        assert_eq!(Span { start: 0, end: input.len() }.to_chars(input).end, 19);
        assert_eq!(Span { start: 0, end: 0 }.to_chars(""), Span { start: 0, end: 0 });
        // Un offset hors frontière est arrondi au caractère suivant.
        assert_eq!(Span { start: 1, end: 2 }.to_chars("éa"), Span { start: 1, end: 1 });
    }

    #[test]
    fn spanned_new_has_high_confidence_and_no_extras() {
        let s = Spanned::new(7, Span { start: 1, end: 2 });
        assert_eq!(s.confidence, Confidence::High);
        assert!(s.ambiguity.is_none() && s.flags.is_empty() && s.enum_group.is_none());
        assert!(Confidence::Low < Confidence::Medium && Confidence::Medium < Confidence::High);
        let names = [Confidence::Low, Confidence::Medium, Confidence::High].map(Confidence::as_str);
        assert_eq!(names, ["low", "medium", "high"]);
    }

    #[test]
    fn ambiguity_codes_match_the_corpus() {
        let table = [
            (Ambiguity::CodeManquant, "code_manquant"),
            (Ambiguity::CodeImplicite, "code_implicite"),
            (Ambiguity::TexteManquant, "texte_manquant"),
            (Ambiguity::SigleAmbigu(Sigle::Ce), "sigle_ambigu:CE"),
            (Ambiguity::SigleAmbigu(Sigle::Cpce), "sigle_ambigu:CPCE"),
            (Ambiguity::AnaphoreNonResolue, "anaphore_non_resolue"),
            (Ambiguity::NumeroManquant, "numero_manquant"),
            (Ambiguity::JuridictionManquante, "juridiction_manquante"),
            (Ambiguity::JuridictionEtTypeManquants, "juridiction_et_type_manquants"),
            (Ambiguity::EmetteurInconnu, "emetteur_inconnu"),
            (Ambiguity::ConventionInconnue, "convention_inconnue"),
            (Ambiguity::ConventionNonIdentifiee, "convention_non_identifiee"),
            (Ambiguity::ReferenceDePublicationSeule, "reference_de_publication_seule"),
            (Ambiguity::Appellation, "appellation"),
            (Ambiguity::CodeAVariantes, "code_a_variantes"),
        ];
        for (ambiguity, code) in table {
            assert_eq!(ambiguity.code(), code);
        }
        for sigle in Sigle::ALL {
            assert_eq!(Sigle::parse(sigle.as_str()), Some(sigle));
        }
        assert_eq!(Sigle::parse("ce"), None);
    }

    #[test]
    fn flag_codes_and_severities_match_the_table_of_annex_b_4_19() {
        use Severity::{Impossible, Info, Suspect};
        let table = [
            (Flag::DateInvalide, "date_invalide", Impossible),
            (Flag::AnneeFuture, "annee_future", Impossible),
            (Flag::ChambreInexistante("civ4".into()), "chambre_inexistante:civ4", Impossible),
            (Flag::ChambreCassInconnue("XX".into()), "chambre_cass_inconnue:XX", Impossible),
            (Flag::EcliOrdinalNonConforme(EcliFamily::Cc), "ordinal_cc_non_conforme", Impossible),
            (Flag::EcliOrdinalNonConforme(EcliFamily::Ce), "ordinal_ce_non_conforme", Impossible),
            (
                Flag::EcliOrdinalNonConforme(EcliFamily::Cass),
                "ordinal_cass_non_conforme",
                Impossible,
            ),
            (Flag::EcliAnneeAnterieureNumero, "ecli_annee_anterieure_numero", Impossible),
            (Flag::AnneePourvoiPosterieure, "annee_pourvoi_posterieure", Impossible),
            (Flag::EcliCodeCeSeul, "code_juridiction_CE_seul_jamais_utilise", Suspect),
            (Flag::AnneeNumeroDifferenteAnneeDate, "annee_numero_differente_annee_date", Suspect),
            (Flag::AnneeImprobable, "annee_improbable", Suspect),
            (Flag::TypeCcInconnu("X".into()), "type_cc_inconnu:X", Suspect),
            (Flag::CaaCodeSiegeIncoherent, "caa_code_siege_incoherent", Suspect),
            (Flag::DateRepublicaine, "date_republicaine", Info),
            (Flag::MojibakeRepare, "mojibake_repare", Info),
        ];
        for (flag, code, severity) in table {
            assert_eq!(flag.code(), code);
            assert_eq!(flag.severity(), severity, "{code}");
        }
        assert!(Info < Suspect && Suspect < Impossible);
    }

    #[test]
    fn citation_kinds_have_both_spellings() {
        assert_eq!(CitationKind::ALL.len(), 12);
        let pascal = CitationKind::ALL.map(CitationKind::pascal_case);
        let snake = CitationKind::ALL.map(CitationKind::snake_case);
        assert_eq!(pascal.iter().collect::<std::collections::HashSet<_>>().len(), 12);
        assert_eq!(snake.iter().collect::<std::collections::HashSet<_>>().len(), 12);
        assert_eq!(pascal[0], "CodeArticle");
        assert_eq!(snake[0], "code_article");
        assert_eq!(snake[10], "legifrance_id");
    }

    fn sample_text() -> TextCite {
        TextCite {
            nature: TextNature::Loi,
            number: None,
            date: CivilDate::new(2016, 10, 7),
            date_raw: None,
            title_hint: None,
            anaphora: None,
            appellation: None,
            emitter: None,
        }
    }

    fn sample_article() -> ArticleRef {
        ArticleRef { num: num("L. 761-1"), sub: Vec::new(), range_to: None }
    }

    fn sample_decision() -> DecisionCite {
        DecisionCite {
            court: None,
            court_via: None,
            date: None,
            numbers: Vec::new(),
            kind: None,
            publication: None,
            solution: None,
            parties: None,
            subject: None,
            arret_num: None,
            formation_code: None,
            publication_codes: Vec::new(),
        }
    }

    #[test]
    fn citation_ref_kind_covers_every_variant() {
        let case_id = Ecli::parse("ECLI:FR:CCASS:2025:C100399").expect("ECLI valide");
        let nor = Nor::parse("ECFI1524250L").expect("NOR valide");
        let eli = Eli::parse("/eli/loi/2016/10/7/ECFI1524250L/jo/texte").expect("ELI valide");
        let values = [
            CitationRef::CodeArticle {
                code: CodeRef::Missing,
                article: sample_article(),
                version: None,
                code_via: None,
            },
            CitationRef::TextArticle { text: None, article: sample_article(), at_date: None },
            CitationRef::Text(sample_text()),
            CitationRef::Decision(sample_decision()),
            CitationRef::Ecli(case_id),
            CitationRef::CnilDeliberation(CnilCite {
                series: None,
                year: 2023,
                seq: 62,
                date: None,
                formation_restreinte: false,
            }),
            CitationRef::Convention(ConventionCite {
                kind: None,
                idcc: None,
                brochure: None,
                avenant: None,
                title_hint: None,
                date: None,
            }),
            CitationRef::Jorf(JorfCite { number: None, date: None, texte: None, page: None }),
            CitationRef::Nor(nor),
            CitationRef::Eli(eli),
            CitationRef::LegifranceId(LegifranceIdRef {
                prefix: IdPrefix::LegiText,
                digits: 1,
                version_start: None,
                url_kind: None,
            }),
            CitationRef::Unsupported(UnsupportedCite { family: UnsupportedFamily::Ue, key: None }),
        ];
        let kinds: Vec<CitationKind> = values.iter().map(CitationRef::kind).collect();
        assert_eq!(kinds, CitationKind::ALL);
    }

    #[test]
    fn article_numbers_have_canonical_keys_and_variants() {
        let cases = [
            ("L. 761-1", "L761-1", vec!["L761-1"]),
            ("LO. 141", "LO141", vec!["LO141"]),
            ("R* 111-1", "R*111-1", vec!["R*111-1", "R111-1"]),
            ("R**111-1", "R**111-1", vec!["R**111-1", "R111-1"]),
            ("D*12", "D*12", vec!["D*12", "D12"]),
            ("D** 12", "D**12", vec!["D**12", "D12"]),
            ("39 octies", "39 octies", vec!["39 octies"]),
            ("199 undecies B", "199 undecies B", vec!["199 undecies B"]),
            ("238 bis", "238 bis", vec!["238 bis"]),
            ("600-4-1", "600-4-1", vec!["600-4-1"]),
            ("A 12", "A12", vec!["A12"]),
            ("1er", "1er", vec!["1er", "1"]),
            ("premier", "1er", vec!["1er", "1"]),
            ("unique", "unique", vec!["unique", "Unique"]),
            ("Liminaire", "liminaire", vec!["liminaire", "Liminaire"]),
            ("préliminaire", "préliminaire", vec!["préliminaire", "Préliminaire"]),
        ];
        for (text, key, variants) in cases {
            let parsed = num(text);
            assert_eq!(parsed.num_key(), key, "{text}");
            assert_eq!(parsed.key_variants(), variants, "{text}");
        }
        let blank = ArticleNum {
            prefix: None,
            special: Some(SpecialNum::SansNumero),
            base: Vec::new(),
            latin: None,
            latin_sub: None,
            letters: None,
            latin_after: None,
        };
        assert_eq!(blank.num_key(), "");
        assert_eq!(blank.key_variants(), [""]);
    }

    #[test]
    fn article_number_parsing_rejects_what_the_grammar_rejects() {
        for bad in
            ["", "L.", "L. x", "1234-", "-5", "1234 -5", "12 bis ter", "12 B C", "12 b", "12 foo"]
        {
            assert_eq!(
                bad.parse::<ArticleNum>(),
                Err(NumberParseError { kind: "article" }),
                "{bad:?}"
            );
        }
        let parsed = num("39 octies");
        assert_eq!(parsed.latin, Some(LatinAdverb::Octies));
        assert_eq!(parsed.base, [39]);
        let parsed = num("199 undecies B");
        assert_eq!(parsed.latin, Some(LatinAdverb::Other("undecies".into())));
        assert_eq!(parsed.letters.as_deref(), Some("B"));
        assert_eq!(num("L761-1").prefix, Some(ArticlePrefix::L));
        assert_eq!(num("LO141").prefix, Some(ArticlePrefix::Lo));
        assert_eq!(num("1234-5").prefix, None);
        assert_eq!(num("  12 B ").letters.as_deref(), Some("B"));
    }

    #[test]
    fn article_number_suffixes_round_trip_through_the_key() {
        let cases = [
            ("150-0 B ter", Some("ter")),
            ("199 terdecies-0 A", None),
            ("199 terdecies-0 A bis", Some("bis")),
            ("1649 quater B quinquies", Some("quinquies")),
            ("50 terdecies-0", None),
            ("5 bis-3", None),
            ("5 B bis", Some("bis")),
        ];
        for (text, after) in cases {
            let parsed = num(text);
            assert_eq!(parsed.num_key(), text, "{text}");
            assert_eq!(num(&parsed.num_key()), parsed, "{text}");
            assert_eq!(parsed.latin_after.as_ref().map(LatinAdverb::as_str), after, "{text}");
        }
        let parsed = num("199 terdecies-0 A");
        assert_eq!(parsed.latin, Some(LatinAdverb::Other("terdecies".into())));
        assert_eq!(parsed.latin_sub, Some(0));
        assert_eq!(parsed.letters.as_deref(), Some("A"));
        assert_eq!(parsed.latin_after, None);
        let parsed = num("150-0 B ter");
        assert_eq!(
            (parsed.base.clone(), parsed.latin, parsed.latin_sub),
            (vec![150, 0], None, None)
        );
        assert_eq!(parsed.letters.as_deref(), Some("B"));
        assert_eq!(parsed.latin_after, Some(LatinAdverb::Ter));
        assert_eq!(num("199 terdecies-0 A").key_variants(), ["199 terdecies-0 A"]);
    }

    #[test]
    fn article_number_parsing_never_truncates_a_suffix() {
        for bad in [
            "5 bis ter",
            "5 B bis ter",
            "5 B bis A",
            "885-0 V bis A",
            "199 terdecies-0 ter",
            "5 bis-",
            "5 bis-x",
            "5 bis-3-4",
            "5 bis-+3",
            "5 B bis-3",
            "199 undecies B bis-3",
            "5 B-3",
            "5 -3",
        ] {
            assert_eq!(
                bad.parse::<ArticleNum>(),
                Err(NumberParseError { kind: "article" }),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn latin_adverbs_cover_the_simple_and_composed_forms() {
        let simple = [
            ("bis", LatinAdverb::Bis),
            ("TER", LatinAdverb::Ter),
            ("quater", LatinAdverb::Quater),
            ("quinquies", LatinAdverb::Quinquies),
            ("sexies", LatinAdverb::Sexies),
            ("septies", LatinAdverb::Septies),
            ("octies", LatinAdverb::Octies),
            ("nonies", LatinAdverb::Nonies),
        ];
        for (word, adverb) in simple {
            assert_eq!(LatinAdverb::parse(word), Some(adverb.clone()));
            assert_eq!(adverb.as_str(), word.to_lowercase());
        }
        for word in [
            "decies",
            "undecies",
            "duodecies",
            "terdecies",
            "quaterdecies",
            "quindecies",
            "sexdecies",
            "septdecies",
            "octodecies",
            "novodecies",
            "vicies",
            "unvicies",
            "tricies",
            "quadragies",
            "nonagies",
            "semel",
            "quinto",
            "quinquedecies",
            "sexties",
            "duodeciès",
        ] {
            let parsed = LatinAdverb::parse(word);
            assert_eq!(
                parsed.as_ref().map(LatinAdverb::as_str),
                Some(word.to_lowercase().as_str()),
                "{word}"
            );
        }
        for word in ["", "decie", "foo", "unies", "B", "bi", "uncies"] {
            assert_eq!(LatinAdverb::parse(word), None, "{word}");
        }
    }

    #[test]
    fn special_numbers_and_prefixes_round_trip() {
        for (word, expected) in [
            ("1er", SpecialNum::Premier),
            ("Premier", SpecialNum::Premier),
            ("unique", SpecialNum::Unique),
            ("liminaire", SpecialNum::Liminaire),
            ("Préliminaire", SpecialNum::Preliminaire),
            ("preliminaire", SpecialNum::Preliminaire),
        ] {
            assert_eq!(SpecialNum::parse(word), Some(expected), "{word}");
        }
        assert_eq!(SpecialNum::parse("deuxième"), None);
        assert_eq!(SpecialNum::SansNumero.as_str(), "");
        for prefix in [
            ArticlePrefix::L,
            ArticlePrefix::Lo,
            ArticlePrefix::A,
            ArticlePrefix::D,
            ArticlePrefix::R,
        ] {
            assert_eq!(prefix.without_stars(), prefix);
        }
        assert_eq!(ArticlePrefix::RStar.without_stars(), ArticlePrefix::R);
        assert_eq!(ArticlePrefix::RStarStar.without_stars(), ArticlePrefix::R);
        assert_eq!(ArticlePrefix::DStar.without_stars(), ArticlePrefix::D);
        assert_eq!(ArticlePrefix::DStarStar.without_stars(), ArticlePrefix::D);
    }

    #[test]
    fn text_natures_have_distinct_corpus_codes() {
        let codes: std::collections::HashSet<&str> =
            TextNature::ALL.iter().map(|n| n.code()).collect();
        assert_eq!(codes.len(), 19);
        for code in [
            "LOI",
            "LOI_ORGANIQUE",
            "LOI_CONSTIT",
            "LOI_FINANCES",
            "LOI_FSS",
            "LOI_DU_PAYS",
            "ORDONNANCE",
            "DECRET",
            "DECRET_LOI",
            "ARRETE",
            "REGLEMENT",
            "CIRCULAIRE",
            "INSTRUCTION",
            "DELIBERATION",
            "DECISION",
            "CONSTITUTION",
            "DDHC",
            "PREAMBULE_1946",
            "CHARTE_ENVIRONNEMENT",
        ] {
            assert!(codes.contains(code), "{code}");
        }
    }

    #[test]
    fn small_codes_match_the_corpus() {
        assert_eq!(
            [CodeVia::AnaphoreMemeCode, CodeVia::AnaphoreDudit, CodeVia::ContexteDocument]
                .map(CodeVia::code),
            ["anaphore:meme_code", "anaphore:dudit", "contexte_document"]
        );
        assert_eq!(
            [CourtVia::FormeDuNumero, CourtVia::Chambre].map(CourtVia::code),
            ["forme_du_numero", "chambre"]
        );
        assert_eq!(
            [
                UnsupportedFamily::Ue,
                UnsupportedFamily::Cjue,
                UnsupportedFamily::Cedh,
                UnsupportedFamily::Doctrine,
                UnsupportedFamily::Recueil,
                UnsupportedFamily::ForeignEcli
            ]
            .map(UnsupportedFamily::code),
            ["UE", "CJUE", "CEDH", "doctrine", "recueil", "ecli_etranger"]
        );
        assert_eq!(
            [
                ConventionKind::Ccn,
                ConventionKind::Convention,
                ConventionKind::Accord,
                ConventionKind::Ani,
                ConventionKind::Avenant,
                ConventionKind::Annexe
            ]
            .map(ConventionKind::code),
            ["CCN", "CONVENTION", "ACCORD", "ANI", "AVENANT", "ANNEXE"]
        );
    }

    #[test]
    fn court_family_codes_match_the_corpus() {
        let courts = [
            (Court::CourDeCassation { chamber: None }, "CCASS"),
            (Court::CourAppel { seat: None, chamber: None }, "CA"),
            (Court::TribunalJudiciaire { seat: None }, "TJ"),
            (Court::AutreJudiciaire { label: "T. com.".into() }, "AUTRE"),
            (Court::ConseilEtat { formation: None, chambers: vec![2, 7] }, "CE"),
            (Court::Caa { seat: None, chamber: None }, "CAA"),
            (Court::Ta { seat: None, chamber: None }, "TA"),
            (Court::ConseilConstitutionnel, "CC"),
        ];
        for (court, code) in courts {
            assert_eq!(court.family_code(), code);
        }
    }

    #[test]
    fn legifrance_ids_become_document_ids_only_for_the_ten_families() {
        for prefix in IdPrefix::ALL {
            assert_eq!(IdPrefix::parse(prefix.as_str()), Some(prefix));
            let id =
                LegifranceIdRef { prefix, digits: 6_070_721, version_start: None, url_kind: None };
            let document = id.to_document_id();
            let expected = !matches!(
                prefix,
                IdPrefix::LegiScta
                    | IdPrefix::JorfArti
                    | IdPrefix::JorfCont
                    | IdPrefix::JorfDole
                    | IdPrefix::KaliScta
                    | IdPrefix::KaliCont
            );
            assert_eq!(document.is_some(), expected, "{prefix:?}");
        }
        let id = LegifranceIdRef {
            prefix: IdPrefix::LegiText,
            digits: 6_070_721,
            version_start: None,
            url_kind: None,
        };
        assert_eq!(
            id.to_document_id().map(|d| d.as_str().to_owned()).as_deref(),
            Some("LEGITEXT000006070721")
        );
        assert_eq!(IdPrefix::parse("legitext"), None);
        assert_eq!(IdPrefix::ALL.len(), 16);
    }

    #[test]
    fn pourvoi_numbers_round_trip() {
        let a: Pourvoi = "21-23.174".parse().expect("valide");
        assert_eq!(a, Pourvoi { yy: 21, mid: 23, seq: 174, key: None });
        assert_eq!(a.to_string(), "21-23.174");
        assert_eq!("21-23174".parse::<Pourvoi>(), Ok(a));
        let keyed: Pourvoi = "N 24-13.470".parse().expect("valide");
        assert_eq!(keyed.key, Some('N'));
        assert_eq!(keyed.to_string(), "24-13.470");
        for bad in [
            "",
            "21-23.1745",
            "21-2.174",
            "2123.174",
            "ab-cd.efg",
            "21-23174x",
            "n 24-13.470",
            "NN 24-13.470",
            "21-2",
            "21-23.17",
        ] {
            assert_eq!(
                bad.parse::<Pourvoi>(),
                Err(NumberParseError { kind: "pourvoi" }),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn rg_and_administrative_numbers_round_trip() {
        for text in ["24/15215", "23/1234", "24/0123"] {
            let rg: RgNumber = text.parse().expect("valide");
            assert_eq!(rg.to_string(), text);
        }
        assert_eq!("24/15215".parse::<RgNumber>(), Ok(RgNumber { yy: 24, seq: 15215 }));
        // Le cinquième chiffre d'un numéro à zéros de tête n'est pas gardé.
        assert_eq!("14/00554".parse::<RgNumber>().map(|r| r.to_string()), Ok("14/0554".to_owned()));
        for bad in ["", "24/123", "24/123456", "2/12345", "24-15215", "24/1234a"] {
            assert_eq!(bad.parse::<RgNumber>(), Err(NumberParseError { kind: "rg" }), "{bad:?}");
        }
        let caa: CaaNumber = "24MA02429".parse().expect("valide");
        assert_eq!(caa, CaaNumber { yy: 24, court: CaaCode::Ma, seq: 2429 });
        assert_eq!(caa.to_string(), "24MA02429");
        for bad in ["", "24M", "24XX02429", "24ma02429", "24MA0242", "24MA024290", "4MA02429"] {
            assert_eq!(bad.parse::<CaaNumber>(), Err(NumberParseError { kind: "caa" }), "{bad:?}");
        }
        let ta: TaNumber = "2005064".parse().expect("valide");
        assert_eq!(ta, TaNumber(2_005_064));
        assert_eq!(TaNumber(42).to_string(), "0000042");
        for bad in ["", "200506", "20050645", "200506x"] {
            assert_eq!(bad.parse::<TaNumber>(), Err(NumberParseError { kind: "ta" }), "{bad:?}");
        }
    }

    #[test]
    fn constitutional_council_and_cnil_numbers_round_trip() {
        for (text, canonical) in [
            ("2025-1173 QPC", "2025-1173"),
            ("2010-14/22 QPC", "2010-14/22"),
            ("2015-491R DC", "2015-491R"),
            ("59-1 FNR", "59-1"),
        ] {
            let cc: CcNumber = text.parse().expect("valide");
            assert_eq!(cc.to_string(), canonical);
            assert_eq!(cc.full(), text);
        }
        let cc: CcNumber = "2010-14/22 QPC".parse().expect("valide");
        assert_eq!(
            (cc.year, cc.seq, cc.joined.clone(), cc.rectif, cc.kind),
            (2010, 14, vec![22], false, CcType::Qpc)
        );
        for bad in [
            "",
            "2025-1173",
            "2025-1173 XXX",
            "25-1173 QPC",
            "58-1 DC",
            "100-1 DC",
            "971-44 DC",
            "7-44 DC",
            "2025 QPC",
            "2025-/22 QPC",
            "2025-1173/ QPC",
            "2025-11x QPC",
        ] {
            assert_eq!(bad.parse::<CcNumber>(), Err(NumberParseError { kind: "cc" }), "{bad:?}");
        }
        for text in ["SAN-2026-002", "MED-2018-024", "2023-062"] {
            let cnil: CnilNumber = text.parse().expect("valide");
            assert_eq!(cnil.to_string(), text);
        }
        assert_eq!(
            "SAN-2026-002".parse::<CnilNumber>().map(|n| n.series),
            Ok(Some("SAN".to_owned()))
        );
        for bad in [
            "",
            "SAN-2026",
            "2026-02",
            "S-2026-002",
            "SANTE-2026-002",
            "san-2026-002",
            "A-B-2026-002",
            "SAN-2026-0002",
            "SAN-26-002",
        ] {
            assert_eq!(
                bad.parse::<CnilNumber>(),
                Err(NumberParseError { kind: "cnil" }),
                "{bad:?}"
            );
        }
        assert_eq!(NumberParseError { kind: "rg" }.to_string(), "invalid rg number");
    }

    /// Une année lue sur quatre chiffres avec des zéros de tête se réécrit sur quatre chiffres : la
    /// forme réécrite se relit (cible de fuzzing `citation_parse`).
    #[test]
    fn leading_zero_years_are_written_back_on_four_digits() {
        let cc: CcNumber = "0001-5 DC".parse().expect("valide");
        assert_eq!(
            (cc.year, cc.to_string(), cc.full()),
            (1, "0001-5".to_owned(), "0001-5 DC".to_owned())
        );
        assert_eq!(cc.full().parse::<CcNumber>(), Ok(cc));
        let cnil: CnilNumber = "SAN-0042-007".parse().expect("valide");
        assert_eq!((cnil.year, cnil.to_string()), (42, "SAN-0042-007".to_owned()));
        assert_eq!(cnil.to_string().parse::<CnilNumber>(), Ok(cnil));
    }

    /// L1-04 : une année de 1959 à 1999 s'écrit sur deux chiffres, comme `NUM_DEC` de Légifrance
    /// (`71-44`), et les deux formes se lisent ; `first_form` ignore numéros joints et `R`.
    #[test]
    fn two_digit_cc_years_round_trip_and_first_form() {
        for (text, canonical, year) in [
            ("71-44 DC", "71-44", 1971),
            ("1971-44 DC", "71-44", 1971),
            ("59-1 DC", "59-1", 1959),
            ("1959-1 DC", "59-1", 1959),
            ("99-410 DC", "99-410", 1999),
            ("1999-410 DC", "99-410", 1999),
            ("94-343/344 DC", "94-343/344", 1994),
            // Jumeaux : hors de 1959-1999, quatre chiffres.
            ("2000-427 DC", "2000-427", 2000),
            ("1958-4 DC", "1958-4", 1958),
        ] {
            let cc: CcNumber = text.parse().expect("valide");
            assert_eq!((cc.year, cc.to_string()), (year, canonical.to_owned()), "{text}");
            let again: CcNumber = cc.full().parse().expect("relu");
            assert_eq!(again, cc, "{text}");
        }
        for (text, first) in [
            ("71-44 DC", "71-44"),
            ("94-343/344 DC", "94-343"),
            ("2015-491R QPC", "2015-491"),
            ("2010-14/22 QPC", "2010-14"),
            ("0001-5 DC", "0001-5"),
        ] {
            let cc: CcNumber = text.parse().expect("valide");
            assert_eq!(cc.first_form(), first, "{text}");
        }
    }

    #[test]
    fn value_types_hold_their_fields() {
        let text = sample_text();
        let qualifiers = [
            VersionQualifier::Ancien,
            VersionQualifier::RedactionIssueDe(Box::new(text.clone())),
            VersionQualifier::EnVigueurAu(CivilDate::new(2018, 1, 1).expect("date")),
        ];
        assert_eq!(qualifiers.len(), 3);
        let unresolvable =
            Resolvability::Unresolvable(UnresolvableWhy::HorsPerimetre(UnsupportedFamily::Cedh));
        assert_ne!(unresolvable, Resolvability::Resolvable);
        assert_eq!(text.nature, TextNature::Loi);
    }
}
