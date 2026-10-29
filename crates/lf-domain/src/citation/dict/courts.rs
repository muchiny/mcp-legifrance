//! Dictionnaires de juridictions : valeurs fermées des facettes et correspondances avec les
//! codes ECLI (annexe B §2.4, §4.5 à §4.9).
//!
//! Les listes de sièges et de formations sont celles des facettes réelles de l'API
//! (`APPEL_SIEGE_APPEL`, `CASSATION_FORMATION`, `JURIDICTION_NATURE`, relevées sur les
//! fixtures de la référence) ; les correspondances avec les chambres, les formations et les
//! types du Conseil constitutionnel viennent du §4.5 au §4.9. Aucune entrée n'est inventée :
//! ce qui n'est pas attesté est marqué comme tel.

use crate::citation::normalize::fold_char;

/// Valeurs de la facette `CASSATION_FORMATION` (JURI), dans l'ordre de l'annexe B §2.4.
pub const CASSATION_FORMATION: [&str; 16] = [
    "CHAMBRE_CIVILE_1",
    "CHAMBRE_CIVILE_2",
    "CHAMBRE_CIVILE_3",
    "CHAMBRE_COMMERCIALE",
    "CHAMBRE_SOCIALE",
    "CHAMBRE_CRIMINELLE",
    "CHAMBRE_MIXTE",
    "ASSEMBLEE_PLENIERE",
    "CHAMBRES_REUNIES",
    "CHAMBRE_CIVILE",
    "ORDONNANCE_PREMIER_PRESIDENT",
    "AVIS",
    "COUR_REVISION",
    "COMMISSION_REVISION",
    "COMMISSION_REPARATION_DETENTION",
    "TRIBUNAL_CONFLIT",
];

/// Valeurs de la facette `APPEL_SIEGE_APPEL` (JURI) : les 35 sièges de cour d'appel, en
/// majuscules sans accents.
pub const CA_SEATS: [&str; 35] = [
    "GRENOBLE",
    "TOULOUSE",
    "PAU",
    "AMIENS",
    "BASSE-TERRE",
    "ROUEN",
    "MONTPELLIER",
    "NIMES",
    "LYON",
    "COLMAR",
    "AGEN",
    "AIX-PROVENCE",
    "ST-DENIS-REUNION",
    "BESANCON",
    "CAEN",
    "NANCY",
    "ANGERS",
    "BORDEAUX",
    "BASTIA",
    "ORLEANS",
    "DIJON",
    "PARIS",
    "CHAMBERY",
    "NOUMEA",
    "VERSAILLES",
    "DOUAI",
    "RIOM",
    "BOURGES",
    "PAPEETE",
    "REIMS",
    "POITIERS",
    "METZ",
    "LIMOGES",
    "FORT-DE-FRANCE",
    "RENNES",
];

/// Formes courantes d'un siège de cour d'appel qui diffèrent de la facette une fois repliées :
/// `(forme repliée, siège de la facette)`.
const CA_ALIASES: [(&str, &str); 3] = [
    ("aix-en-provence", "AIX-PROVENCE"),
    ("saint-denis de la reunion", "ST-DENIS-REUNION"),
    ("saint-denis", "ST-DENIS-REUNION"),
];

/// Sièges des 42 tribunaux administratifs (facette `JURIDICTION_NATURE`, sous-valeurs de
/// `TRIBUNAL_ADMINISTATIF`), avec la graphie de l'API.
pub const TA_SEATS: [&str; 42] = [
    "Papeete",
    "Nîmes",
    "Bordeaux",
    "Cergy-Pontoise",
    "Pau",
    "Marseille",
    "Lille",
    "Nancy",
    "Saint-Barthélemy",
    "Limoges",
    "Châlons-en-Champagne",
    "Orléans",
    "Grenoble",
    "Poitiers",
    "Rouen",
    "Nice",
    "Saint-Martin",
    "Saint-Pierre et Miquelon",
    "Melun",
    "Mamoudzou",
    "Nantes",
    "Besançon",
    "Lyon",
    "Fort-de-France",
    "Toulouse",
    "Dijon",
    "Basse-Terre",
    "Montpellier",
    "Montreuil",
    "Strasbourg",
    "Amiens",
    "Rennes",
    "Bastia",
    "Saint-Denis de la Réunion",
    "Caen",
    "Versailles",
    "Clermont-Ferrand",
    "Cayenne",
    "Toulon",
    "Nouméa",
    "Paris",
    "Mata-Utu",
];

/// Siège d'un texte replié par `fold_char` : égalité de la forme repliée de `name` avec `key`.
fn folded_eq(name: &str, key: &str) -> bool {
    name.chars().map(fold_char).eq(key.chars())
}

/// Chambre de la Cour de cassation (annexe B §4.5).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CassChamber {
    /// Première chambre civile.
    Civ1,
    /// Deuxième chambre civile.
    Civ2,
    /// Troisième chambre civile.
    Civ3,
    /// Chambre commerciale.
    Com,
    /// Chambre sociale.
    Soc,
    /// Chambre criminelle.
    Crim,
    /// Assemblée plénière.
    AssPlen,
    /// Chambre mixte.
    Mixte,
    /// Chambres réunies (historique).
    Reunies,
    /// Chambre civile sans numéro (historique).
    CivUnique,
    /// Chambre des requêtes (historique).
    Requetes,
    /// Ordonnance du premier président.
    PremierPresident,
}

impl CassChamber {
    /// Les douze chambres.
    pub const ALL: [Self; 12] = [
        Self::Civ1,
        Self::Civ2,
        Self::Civ3,
        Self::Com,
        Self::Soc,
        Self::Crim,
        Self::AssPlen,
        Self::Mixte,
        Self::Reunies,
        Self::CivUnique,
        Self::Requetes,
        Self::PremierPresident,
    ];

    /// Code du corpus (`CIV1`, `COM`, `AP`, `MIXTE`…). Les quatre chambres que le tableau du
    /// §4.5 ne chiffre pas (`Reunies`, `CivUnique`, `Requetes`, `PremierPresident`) reçoivent
    /// un code en majuscules de même forme, non attesté par le corpus.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Civ1 => "CIV1",
            Self::Civ2 => "CIV2",
            Self::Civ3 => "CIV3",
            Self::Com => "COM",
            Self::Soc => "SOC",
            Self::Crim => "CRIM",
            Self::AssPlen => "AP",
            Self::Mixte => "MIXTE",
            Self::Reunies => "REUNIES",
            Self::CivUnique => "CIV_UNIQUE",
            Self::Requetes => "REQUETES",
            Self::PremierPresident => "PREMIER_PRESIDENT",
        }
    }

    /// Valeur de la facette `CASSATION_FORMATION`. `None` pour la chambre des requêtes : valeur
    /// **NON VÉRIFIÉ** [V-147 · ecli-codes-cass-rares].
    pub const fn facet(self) -> Option<&'static str> {
        match self {
            Self::Civ1 => Some("CHAMBRE_CIVILE_1"),
            Self::Civ2 => Some("CHAMBRE_CIVILE_2"),
            Self::Civ3 => Some("CHAMBRE_CIVILE_3"),
            Self::Com => Some("CHAMBRE_COMMERCIALE"),
            Self::Soc => Some("CHAMBRE_SOCIALE"),
            Self::Crim => Some("CHAMBRE_CRIMINELLE"),
            Self::AssPlen => Some("ASSEMBLEE_PLENIERE"),
            Self::Mixte => Some("CHAMBRE_MIXTE"),
            Self::Reunies => Some("CHAMBRES_REUNIES"),
            Self::CivUnique => Some("CHAMBRE_CIVILE"),
            Self::Requetes => None,
            Self::PremierPresident => Some("ORDONNANCE_PREMIER_PRESIDENT"),
        }
    }

    /// Préfixe de l'ordinal d'un ECLI `ECLI:FR:CCASS` (`C1`, `CO`, `AP`…). `None` pour les
    /// chambres dont le préfixe est **NON VÉRIFIÉ** [V-147 · ecli-codes-cass-rares].
    pub const fn ecli_prefix(self) -> Option<&'static str> {
        match self {
            Self::Civ1 => Some("C1"),
            Self::Civ2 => Some("C2"),
            Self::Civ3 => Some("C3"),
            Self::Com => Some("CO"),
            Self::Soc => Some("SO"),
            Self::Crim => Some("CR"),
            Self::AssPlen => Some("AP"),
            Self::PremierPresident => Some("OR"),
            Self::Mixte | Self::Reunies | Self::CivUnique | Self::Requetes => None,
        }
    }

    /// Chambre d'un préfixe d'ordinal ECLI (`C1` donne `Civ1`).
    pub fn from_ecli_prefix(prefix: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.ecli_prefix() == Some(prefix))
    }

    /// Chambre d'une valeur de la facette `CASSATION_FORMATION`.
    pub fn from_facet(facet: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.facet() == Some(facet))
    }
}

/// Formation du Conseil d'État (annexe B §4.7).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CeFormation {
    /// Assemblée du contentieux.
    Assemblee,
    /// Section du contentieux.
    Section,
    /// Chambres réunies.
    ChambresReunies,
    /// Chambre jugeant seule.
    ChambreSeule,
    /// Sous-sections réunies (avant 2016).
    SousSectionsReunies,
    /// Sous-section jugeant seule (avant 2016).
    SousSectionSeule,
    /// Ordonnance (juge des référés).
    Ordonnance,
    /// Avis contentieux.
    Avis,
}

impl CeFormation {
    /// Les huit formations.
    pub const ALL: [Self; 8] = [
        Self::Assemblee,
        Self::Section,
        Self::ChambresReunies,
        Self::ChambreSeule,
        Self::SousSectionsReunies,
        Self::SousSectionSeule,
        Self::Ordonnance,
        Self::Avis,
    ];

    /// Code du corpus (`ASSEMBLEE`, `CHR`, `SSR`…).
    pub const fn code(self) -> &'static str {
        match self {
            Self::Assemblee => "ASSEMBLEE",
            Self::Section => "SECTION",
            Self::ChambresReunies => "CHR",
            Self::ChambreSeule => "CHS",
            Self::SousSectionsReunies => "SSR",
            Self::SousSectionSeule => "SSJS",
            Self::Ordonnance => "ORDONNANCE",
            Self::Avis => "AVIS",
        }
    }

    /// Code de juridiction de l'ECLI (`CEASS`, `CECHR`…). `None` pour l'avis : **NON VÉRIFIÉ**
    /// [V-148 · ecli-juridictions-fond].
    pub const fn ecli_court(self) -> Option<&'static str> {
        match self {
            Self::Assemblee => Some("CEASS"),
            Self::Section => Some("CESEC"),
            Self::ChambresReunies => Some("CECHR"),
            Self::ChambreSeule => Some("CECHS"),
            Self::SousSectionsReunies => Some("CESSR"),
            Self::SousSectionSeule => Some("CESJS"),
            Self::Ordonnance => Some("CEORD"),
            Self::Avis => None,
        }
    }

    /// Formation d'un code de juridiction d'ECLI (`CECHR` donne `ChambresReunies`).
    pub fn from_ecli_court(court: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.ecli_court() == Some(court))
    }
}

/// Siège d'une cour d'appel judiciaire : une valeur fermée de la facette `APPEL_SIEGE_APPEL`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct CaSeat(&'static str);

impl CaSeat {
    /// Siège d'une valeur exacte de la facette (`AIX-PROVENCE`).
    pub fn from_facet(facet: &str) -> Option<Self> {
        CA_SEATS.iter().find(|s| **s == facet).map(|&s| Self(s))
    }

    /// Siège d'un nom replié par `fold_char` (`aix-en-provence`, `saint-denis de la reunion`).
    pub fn parse_folded(key: &str) -> Option<Self> {
        let by_facet = CA_SEATS.iter().find(|s| s.eq_ignore_ascii_case(key));
        let by_alias = CA_ALIASES.iter().find(|(alias, _)| *alias == key).map(|(_, facet)| facet);
        by_facet.or(by_alias).map(|&s| Self(s))
    }

    /// Valeur de la facette, en majuscules sans accents (`PARIS`).
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// Code d'une cour administrative d'appel dans un numéro de requête (`24MA02429`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CaaCode {
    /// Paris.
    Pa,
    /// Versailles (jamais observé : **NON VÉRIFIÉ** [V-149 · caa-codes-liste]).
    Ve,
    /// Marseille.
    Ma,
    /// Lyon.
    Ly,
    /// Bordeaux.
    Bx,
    /// Nantes.
    Nt,
    /// Nancy.
    Nc,
    /// Douai.
    Da,
    /// Toulouse.
    Tl,
}

impl CaaCode {
    /// Les neuf codes.
    pub const ALL: [Self; 9] =
        [Self::Pa, Self::Ve, Self::Ma, Self::Ly, Self::Bx, Self::Nt, Self::Nc, Self::Da, Self::Tl];

    /// Code en majuscules (`MA`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pa => "PA",
            Self::Ve => "VE",
            Self::Ma => "MA",
            Self::Ly => "LY",
            Self::Bx => "BX",
            Self::Nt => "NT",
            Self::Nc => "NC",
            Self::Da => "DA",
            Self::Tl => "TL",
        }
    }

    /// Code d'un texte en majuscules (`MA`), sensible à la casse.
    pub fn parse(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == code)
    }

    /// Siège de la cour qui porte ce code.
    pub const fn seat(self) -> CaaSeat {
        CaaSeat(match self {
            Self::Pa => "Paris",
            Self::Ve => "Versailles",
            Self::Ma => "Marseille",
            Self::Ly => "Lyon",
            Self::Bx => "Bordeaux",
            Self::Nt => "Nantes",
            Self::Nc => "Nancy",
            Self::Da => "Douai",
            Self::Tl => "Toulouse",
        })
    }
}

/// Siège d'une cour administrative d'appel : l'un des neuf de la facette `JURIDICTION_NATURE`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct CaaSeat(&'static str);

impl CaaSeat {
    /// Siège d'un nom replié par `fold_char` (`marseille`).
    pub fn parse_folded(key: &str) -> Option<Self> {
        CaaCode::ALL.into_iter().map(CaaCode::seat).find(|s| folded_eq(s.0, key))
    }

    /// Code du numéro de requête de cette cour.
    pub fn code(self) -> Option<CaaCode> {
        CaaCode::ALL.into_iter().find(|c| c.seat() == self)
    }

    /// Nom du siège, avec la graphie de l'API (`Marseille`).
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// Siège d'un tribunal administratif : l'un des 42 de la facette `JURIDICTION_NATURE`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TaSeat(&'static str);

impl TaSeat {
    /// Siège d'un nom replié par `fold_char` (`nimes`, `chalons-en-champagne`).
    pub fn parse_folded(key: &str) -> Option<Self> {
        TA_SEATS.iter().find(|s| folded_eq(s, key)).map(|&s| Self(s))
    }

    /// Nom du siège, avec la graphie de l'API (`Nîmes`).
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// Type d'une décision du Conseil constitutionnel (annexe B §4.9) : les dix-huit types connus.
/// Sens de `OF`, `ORGA`, `AR16`, `ELEC`, `REF`, `AUTR` : **NON VÉRIFIÉ** [V-114 · natures-constit].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CcType {
    /// Contrôle de constitutionnalité des lois.
    Dc,
    /// Question prioritaire de constitutionnalité.
    Qpc,
    /// Partage des compétences, délégalisation (`PDR`).
    Pdr,
    /// Élections à l'Assemblée nationale.
    An,
    /// Élections au Sénat.
    Sen,
    /// Déchéance.
    D,
    /// Incompatibilité.
    I,
    /// Organisation financière (`OF`).
    Of,
    /// Élections.
    Elec,
    /// Loi organique de Mayotte ou de Nouvelle-Calédonie (`LOM`).
    Lom,
    /// Loi du pays (`LP`).
    Lp,
    /// Fin de non-recevoir (`FNR`).
    Fnr,
    /// Référendum (`REF`).
    Ref,
    /// Référendum d'initiative partagée (`RIP`).
    Rip,
    /// Délégalisation (`L`).
    L,
    /// Loi organique (`ORGA`).
    Orga,
    /// Article 16 de la Constitution (`AR16`).
    Ar16,
    /// Autres décisions (`AUTR`).
    Autr,
}

impl CcType {
    /// Les dix-huit types.
    pub const ALL: [Self; 18] = [
        Self::Dc,
        Self::Qpc,
        Self::Pdr,
        Self::An,
        Self::Sen,
        Self::D,
        Self::I,
        Self::Of,
        Self::Elec,
        Self::Lom,
        Self::Lp,
        Self::Fnr,
        Self::Ref,
        Self::Rip,
        Self::L,
        Self::Orga,
        Self::Ar16,
        Self::Autr,
    ];

    /// Sigle en majuscules (`QPC`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dc => "DC",
            Self::Qpc => "QPC",
            Self::Pdr => "PDR",
            Self::An => "AN",
            Self::Sen => "SEN",
            Self::D => "D",
            Self::I => "I",
            Self::Of => "OF",
            Self::Elec => "ELEC",
            Self::Lom => "LOM",
            Self::Lp => "LP",
            Self::Fnr => "FNR",
            Self::Ref => "REF",
            Self::Rip => "RIP",
            Self::L => "L",
            Self::Orga => "ORGA",
            Self::Ar16 => "AR16",
            Self::Autr => "AUTR",
        }
    }

    /// Type d'un sigle en majuscules, sensible à la casse.
    pub fn parse(sigle: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == sigle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn distinct<T: std::hash::Hash + Eq>(items: impl IntoIterator<Item = T>) -> usize {
        items.into_iter().collect::<HashSet<_>>().len()
    }

    #[test]
    fn juridictions_closed_values() {
        // Effectifs relevés sur les facettes réelles (annexe B §2.4), sans doublon.
        assert_eq!(CASSATION_FORMATION.len(), 16);
        assert_eq!(distinct(CASSATION_FORMATION), 16);
        assert_eq!(CA_SEATS.len(), 35);
        assert_eq!(distinct(CA_SEATS), 35);
        assert!(CA_SEATS.iter().all(|s| s.bytes().all(|b| b.is_ascii_uppercase() || b == b'-')));
        assert_eq!(TA_SEATS.len(), 42);
        assert_eq!(distinct(TA_SEATS), 42);
        assert_eq!(CaaCode::ALL.len(), 9);
        assert_eq!(distinct(CaaCode::ALL.map(CaaCode::as_str)), 9);
        assert_eq!(CcType::ALL.len(), 18);
        assert_eq!(distinct(CcType::ALL.map(CcType::as_str)), 18);

        // Chaque chambre pointe une valeur de la facette ; les cinq autres valeurs n'ont pas de
        // chambre (avis, révision, réparation des détentions, tribunal des conflits).
        let facets: HashSet<&str> = CassChamber::ALL.iter().filter_map(|c| c.facet()).collect();
        assert!(facets.iter().all(|f| CASSATION_FORMATION.contains(f)));
        let without_chamber: Vec<&str> =
            CASSATION_FORMATION.into_iter().filter(|f| !facets.contains(f)).collect();
        assert_eq!(
            without_chamber,
            [
                "AVIS",
                "COUR_REVISION",
                "COMMISSION_REVISION",
                "COMMISSION_REPARATION_DETENTION",
                "TRIBUNAL_CONFLIT"
            ]
        );
        assert_eq!(distinct(CassChamber::ALL.map(CassChamber::code)), 12);
    }

    #[test]
    fn cassation_chambers_map_to_ecli_prefixes_and_facets() {
        let prefixes = [
            ("C1", CassChamber::Civ1),
            ("C2", CassChamber::Civ2),
            ("C3", CassChamber::Civ3),
            ("CO", CassChamber::Com),
            ("SO", CassChamber::Soc),
            ("CR", CassChamber::Crim),
            ("AP", CassChamber::AssPlen),
            ("OR", CassChamber::PremierPresident),
        ];
        for (prefix, chamber) in prefixes {
            assert_eq!(CassChamber::from_ecli_prefix(prefix), Some(chamber));
            assert_eq!(chamber.ecli_prefix(), Some(prefix));
        }
        assert_eq!(CassChamber::from_ecli_prefix("XX"), None);
        assert_eq!(CassChamber::Mixte.ecli_prefix(), None);
        assert_eq!(CassChamber::from_facet("CHAMBRE_CIVILE_1"), Some(CassChamber::Civ1));
        assert_eq!(CassChamber::from_facet("CHAMBRE_MIXTE"), Some(CassChamber::Mixte));
        assert_eq!(CassChamber::from_facet("AVIS"), None);
        assert_eq!(CassChamber::Requetes.facet(), None);
        // Codes du corpus.
        assert_eq!(CassChamber::Civ1.code(), "CIV1");
        assert_eq!(CassChamber::AssPlen.code(), "AP");
        assert_eq!(CassChamber::Mixte.code(), "MIXTE");
    }

    #[test]
    fn conseil_etat_formations_map_to_ecli_courts() {
        let suffixes = [
            ("CEASS", CeFormation::Assemblee, "ASSEMBLEE"),
            ("CESEC", CeFormation::Section, "SECTION"),
            ("CECHR", CeFormation::ChambresReunies, "CHR"),
            ("CECHS", CeFormation::ChambreSeule, "CHS"),
            ("CESSR", CeFormation::SousSectionsReunies, "SSR"),
            ("CESJS", CeFormation::SousSectionSeule, "SSJS"),
            ("CEORD", CeFormation::Ordonnance, "ORDONNANCE"),
        ];
        for (court, formation, code) in suffixes {
            assert_eq!(CeFormation::from_ecli_court(court), Some(formation));
            assert_eq!(formation.ecli_court(), Some(court));
            assert_eq!(formation.code(), code);
        }
        assert_eq!(CeFormation::from_ecli_court("CE"), None);
        assert_eq!(CeFormation::Avis.ecli_court(), None);
        assert_eq!(CeFormation::Avis.code(), "AVIS");
        assert_eq!(CeFormation::ALL.len(), 8);
    }

    #[test]
    fn appeal_court_seats_parse_from_facet_and_natural_forms() {
        assert_eq!(CaSeat::from_facet("PARIS").map(CaSeat::as_str), Some("PARIS"));
        assert_eq!(CaSeat::from_facet("Paris"), None);
        for key in ["paris", "nimes", "fort-de-france", "basse-terre"] {
            assert_eq!(CaSeat::parse_folded(key).map(CaSeat::as_str), Some(&*key.to_uppercase()));
        }
        for (key, facet) in [
            ("aix-en-provence", "AIX-PROVENCE"),
            ("saint-denis de la reunion", "ST-DENIS-REUNION"),
            ("saint-denis", "ST-DENIS-REUNION"),
        ] {
            assert_eq!(CaSeat::parse_folded(key).map(CaSeat::as_str), Some(facet));
        }
        assert_eq!(CaSeat::parse_folded("atlantis"), None);
    }

    #[test]
    fn administrative_seats_and_codes_agree() {
        for code in CaaCode::ALL {
            assert_eq!(CaaCode::parse(code.as_str()), Some(code));
            assert_eq!(code.seat().code(), Some(code));
        }
        assert_eq!(CaaCode::parse("ma"), None);
        assert_eq!(CaaCode::parse("XX"), None);
        assert_eq!(CaaCode::Ma.seat().as_str(), "Marseille");
        assert_eq!(CaaSeat::parse_folded("marseille"), Some(CaaCode::Ma.seat()));
        assert_eq!(CaaSeat::parse_folded("brest"), None);
        assert_eq!(CaaSeat("Brest").code(), None);
        assert_eq!(TaSeat::parse_folded("nimes").map(TaSeat::as_str), Some("Nîmes"));
        assert_eq!(
            TaSeat::parse_folded("chalons-en-champagne").map(TaSeat::as_str),
            Some("Châlons-en-Champagne")
        );
        assert_eq!(
            TaSeat::parse_folded("saint-denis de la reunion").map(TaSeat::as_str),
            Some("Saint-Denis de la Réunion")
        );
        assert_eq!(TaSeat::parse_folded("brest"), None);
        // Les neuf sièges de cour d'appel administrative sont aussi des sièges de TA ou non :
        // seule la facette fait foi, ici Paris et Lyon figurent dans les deux listes.
        assert!(TA_SEATS.contains(&"Paris") && TA_SEATS.contains(&"Lyon"));
    }

    #[test]
    fn constitutional_council_types_round_trip() {
        for t in CcType::ALL {
            assert_eq!(CcType::parse(t.as_str()), Some(t));
        }
        assert_eq!(CcType::parse("qpc"), None);
        assert_eq!(CcType::parse("SEN/QPC"), None);
        assert_eq!(CcType::Ar16.as_str(), "AR16");
    }
}
