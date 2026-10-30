//! Dictionnaire des 77 codes (annexe B §5), synchronisé avec `codes-table.md`.
//!
//! Le domaine ne porte aucun `LEGITEXT` (§5.4) : la correspondance entre un [`CodeKey`] et le
//! texte consolidé vient de `POST /list/code`, lu par l'adapter. Ce module ne tient que les
//! titres officiels, les abréviations et les sigles ambigus.
//!
//! Les formes dont les seules sources sont `[KM]` ou `[JRX]` (AGPL-3.0 : `C. conso.`, `COJ`,
//! `CSI`, `CGFP`, `CJPM`, `C. rur.`) n'entrent pas ici tant qu'une source non AGPL ne les a pas
//! confirmées (08 §9.4 règle 3, [V-159 · abreviations-codes]). Les codes concernés restent
//! reconnus par leur titre.

use crate::citation::types::Confidence;

/// Clé d'un code pour le rendu et l'appariement : un code du dictionnaire, ou un titre libre
/// (`CodeRef::Unknown`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CodeRefKey {
    /// Code du dictionnaire.
    Known(CodeKey),
    /// Code absent du dictionnaire, gardé par son titre.
    Other,
}

/// Entrée statique du dictionnaire (annexe B §5.5). L'entrée dynamique de `list_codes()` est un
/// autre type, côté application.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CodeEntry {
    /// Clé de l'entrée.
    pub key: CodeKey,
    /// Titre officiel (colonne 2 de `codes-table.md`, sans « ¹ »).
    pub title: &'static str,
    /// Abréviations non ambiguës : attestées (`High`) ou d'usage (`Medium`).
    pub abbreviations: &'static [(&'static str, Confidence)],
    /// Sigles ambigus (§5.3), en majuscules, comparés en respectant la casse.
    pub ambiguous_sigles: &'static [&'static str],
    /// Version « ancienne » ou « nouvelle » du même code (Code minier, Code rural).
    pub variant_of: Option<CodeKey>,
}

/// Un code du dictionnaire (77 entrées, `codes-table.md`).
///
/// L'ordre des variantes est celui du tableau : `CODES[key as usize]` est l'entrée de `key`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CodeKey {
    /// 1. Code civil.
    CodeCivil,
    /// 2. Code de commerce.
    CodeDeCommerce,
    /// 3. Code du travail.
    CodeDuTravail,
    /// 4. Code pénal.
    CodePenal,
    /// 5. Code de procédure civile.
    CodeDeProcedureCivile,
    /// 6. Code de procédure pénale.
    CodeDeProcedurePenale,
    /// 7. Code général des impôts.
    CodeGeneralDesImpots,
    /// 8. Code général des impôts, annexe I.
    CodeGeneralDesImpotsAnnexeI,
    /// 9. Code général des impôts, annexe II.
    CodeGeneralDesImpotsAnnexeIi,
    /// 10. Code général des impôts, annexe III.
    CodeGeneralDesImpotsAnnexeIii,
    /// 11. Code général des impôts, annexe IV.
    CodeGeneralDesImpotsAnnexeIv,
    /// 12. Livre des procédures fiscales.
    LivreDesProceduresFiscales,
    /// 13. Code de la santé publique.
    CodeDeLaSantePublique,
    /// 14. Code de la sécurité sociale.
    CodeDeLaSecuriteSociale,
    /// 15. Code de la consommation.
    CodeDeLaConsommation,
    /// 16. Code de justice administrative.
    CodeDeJusticeAdministrative,
    /// 17. Code de l'environnement.
    CodeDeLEnvironnement,
    /// 18. Code général des collectivités territoriales.
    CodeGeneralDesCollectivitesTerritoriales,
    /// 19. Code de l'urbanisme.
    CodeDeLUrbanisme,
    /// 20. Code de la construction et de l'habitation.
    CodeDeLaConstructionEtDeLHabitation,
    /// 21. Code de la propriété intellectuelle.
    CodeDeLaProprieteIntellectuelle,
    /// 22. Code des assurances.
    CodeDesAssurances,
    /// 23. Code monétaire et financier.
    CodeMonetaireEtFinancier,
    /// 24. Code rural et de la pêche maritime.
    CodeRuralEtDeLaPecheMaritime,
    /// 25. Code rural (ancien).
    CodeRuralAncien,
    /// 26. Code des relations entre le public et l'administration.
    CodeDesRelationsEntreLePublicEtLAdministration,
    /// 27. Code de l'entrée et du séjour des étrangers et du droit d'asile.
    CodeDeLEntreeEtDuSejourDesEtrangersEtDuDroitDAsile,
    /// 28. Code de l'action sociale et des familles.
    CodeDeLActionSocialeEtDesFamilles,
    /// 29. Code de l'organisation judiciaire.
    CodeDeLOrganisationJudiciaire,
    /// 30. Code des postes et des communications électroniques.
    CodeDesPostesEtDesCommunicationsElectroniques,
    /// 31. Code de la sécurité intérieure.
    CodeDeLaSecuriteInterieure,
    /// 32. Code général de la fonction publique.
    CodeGeneralDeLaFonctionPublique,
    /// 33. Code de la justice pénale des mineurs.
    CodeDeLaJusticePenaleDesMineurs,
    /// 34. Code général de la propriété des personnes publiques.
    CodeGeneralDeLaProprieteDesPersonnesPubliques,
    /// 35. Code des impositions sur les biens et services.
    CodeDesImpositionsSurLesBiensEtServices,
    /// 36. Code de la commande publique.
    CodeDeLaCommandePublique,
    /// 37. Code de l'éducation.
    CodeDeLEducation,
    /// 38. Code des procédures civiles d'exécution.
    CodeDesProceduresCivilesDExecution,
    /// 39. Code de la route.
    CodeDeLaRoute,
    /// 40. Code électoral.
    CodeElectoral,
    /// 41. Code de la défense.
    CodeDeLaDefense,
    /// 42. Code de l'énergie.
    CodeDeLEnergie,
    /// 43. Code des transports.
    CodeDesTransports,
    /// 44. Code du patrimoine.
    CodeDuPatrimoine,
    /// 45. Code du sport.
    CodeDuSport,
    /// 46. Code du tourisme.
    CodeDuTourisme,
    /// 47. Code de la voirie routière.
    CodeDeLaVoirieRoutiere,
    /// 48. Code de la recherche.
    CodeDeLaRecherche,
    /// 49. Code de la mutualité.
    CodeDeLaMutualite,
    /// 50. Code des juridictions financières.
    CodeDesJuridictionsFinancieres,
    /// 51. Code pénitentiaire.
    CodePenitentiaire,
    /// 52. Code des douanes.
    CodeDesDouanes,
    /// 53. Code des douanes de Mayotte.
    CodeDesDouanesDeMayotte,
    /// 54. Code forestier (nouveau).
    CodeForestierNouveau,
    /// 55. Code minier (nouveau).
    CodeMinierNouveau,
    /// 56. Code minier (ancien).
    CodeMinierAncien,
    /// 57. Code de l'expropriation pour cause d'utilité publique.
    CodeDeLExpropriationPourCauseDUtilitePublique,
    /// 58. Code du cinéma et de l'image animée.
    CodeDuCinemaEtDeLImageAnimee,
    /// 59. Code de justice militaire (nouveau).
    CodeDeJusticeMilitaireNouveau,
    /// 60. Code du service national.
    CodeDuServiceNational,
    /// 61. Code de l'aviation civile.
    CodeDeLAviationCivile,
    /// 62. Code de l'artisanat.
    CodeDeLArtisanat,
    /// 63. Code des pensions civiles et militaires de retraite.
    CodeDesPensionsCivilesEtMilitairesDeRetraite,
    /// 64. Code des pensions militaires d'invalidité et des victimes de guerre.
    CodeDesPensionsMilitairesDInvaliditeEtDesVictimesDeGuerre,
    /// 65. Code des pensions de retraite des marins français du commerce, de pêche ou de plaisance.
    CodeDesPensionsDeRetraiteDesMarinsFrancaisDuCommerceDePecheOuDePlaisance,
    /// 66. Code de la Légion d'honneur, de la Médaille militaire et de l'ordre national du Mérite.
    CodeDeLaLegionDHonneurDeLaMedailleMilitaireEtDeLOrdreNationalDuMerite,
    /// 67. Code du domaine public fluvial et de la navigation intérieure.
    CodeDuDomainePublicFluvialEtDeLaNavigationInterieure,
    /// 68. Code du domaine de l'Etat.
    CodeDuDomaineDeLEtat,
    /// 69. Code du domaine de l'Etat et des collectivités publiques applicable à la collectivité territoriale de Mayotte.
    CodeDuDomaineDeLEtatEtDesCollectivitesPubliquesApplicableALaCollectiviteTerritorialeDeMayotte,
    /// 70. Code des ports maritimes.
    CodeDesPortsMaritimes,
    /// 71. Code du travail maritime.
    CodeDuTravailMaritime,
    /// 72. Code disciplinaire et pénal de la marine marchande.
    CodeDisciplinaireEtPenalDeLaMarineMarchande,
    /// 73. Code des instruments monétaires et des médailles.
    CodeDesInstrumentsMonetairesEtDesMedailles,
    /// 74. Code de déontologie des architectes.
    CodeDeDeontologieDesArchitectes,
    /// 75. Code des communes.
    CodeDesCommunes,
    /// 76. Code des communes de la Nouvelle-Calédonie.
    CodeDesCommunesDeLaNouvelleCaledonie,
    /// 77. Code de la famille et de l'aide sociale.
    CodeDeLaFamilleEtDeLAideSociale,
}

/// Les 77 entrées, dans l'ordre de `codes-table.md` (annexe B §5.5).
pub static CODES: [CodeEntry; 77] = [
    CodeEntry {
        key: CodeKey::CodeCivil,
        title: "Code civil",
        abbreviations: &[("C. civ.", Confidence::High), ("Code Napoléon", Confidence::Medium)],
        ambiguous_sigles: &["CC"],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeCommerce,
        title: "Code de commerce",
        abbreviations: &[("C. com.", Confidence::High), ("C.com", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDuTravail,
        title: "Code du travail",
        abbreviations: &[("C. trav.", Confidence::High)],
        ambiguous_sigles: &["CT"],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodePenal,
        title: "Code pénal",
        abbreviations: &[("C. pén.", Confidence::High)],
        ambiguous_sigles: &["CP"],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeProcedureCivile,
        title: "Code de procédure civile",
        abbreviations: &[("C. pr. civ.", Confidence::High), ("CPC", Confidence::High), ("NCPC", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeProcedurePenale,
        title: "Code de procédure pénale",
        abbreviations: &[("C. pr. pén.", Confidence::High), ("CPP", Confidence::High)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeGeneralDesImpots,
        title: "Code général des impôts",
        abbreviations: &[("CGI", Confidence::High)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeGeneralDesImpotsAnnexeI,
        title: "Code général des impôts, annexe I",
        abbreviations: &[("CGI, ann. I", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeGeneralDesImpotsAnnexeIi,
        title: "Code général des impôts, annexe II",
        abbreviations: &[("CGI, ann. II", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeGeneralDesImpotsAnnexeIii,
        title: "Code général des impôts, annexe III",
        abbreviations: &[("CGI, ann. III", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeGeneralDesImpotsAnnexeIv,
        title: "Code général des impôts, annexe IV",
        abbreviations: &[("CGI, ann. IV", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::LivreDesProceduresFiscales,
        title: "Livre des procédures fiscales",
        abbreviations: &[("LPF", Confidence::High)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLaSantePublique,
        title: "Code de la santé publique",
        abbreviations: &[("CSP", Confidence::High), ("C. santé publ.", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLaSecuriteSociale,
        title: "Code de la sécurité sociale",
        abbreviations: &[("C. séc. soc.", Confidence::High), ("C. sec. soc.", Confidence::High), ("CSS", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLaConsommation,
        title: "Code de la consommation",
        abbreviations: &[("C. consom.", Confidence::High)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeJusticeAdministrative,
        title: "Code de justice administrative",
        abbreviations: &[("CJA", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLEnvironnement,
        title: "Code de l'environnement",
        abbreviations: &[("C. env.", Confidence::High), ("C. envir.", Confidence::High)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeGeneralDesCollectivitesTerritoriales,
        title: "Code général des collectivités territoriales",
        abbreviations: &[("CGCT", Confidence::High)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLUrbanisme,
        title: "Code de l'urbanisme",
        abbreviations: &[("C. urb.", Confidence::High)],
        ambiguous_sigles: &["CU"],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLaConstructionEtDeLHabitation,
        title: "Code de la construction et de l'habitation",
        abbreviations: &[("CCH", Confidence::High)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLaProprieteIntellectuelle,
        title: "Code de la propriété intellectuelle",
        abbreviations: &[],
        ambiguous_sigles: &["CPI"],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDesAssurances,
        title: "Code des assurances",
        abbreviations: &[("C. assur.", Confidence::High)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeMonetaireEtFinancier,
        title: "Code monétaire et financier",
        abbreviations: &[("CMF", Confidence::Medium), ("C. mon. fin.", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeRuralEtDeLaPecheMaritime,
        title: "Code rural et de la pêche maritime",
        abbreviations: &[("CRPM", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: Some(CodeKey::CodeRuralAncien),
    },
    CodeEntry {
        key: CodeKey::CodeRuralAncien,
        title: "Code rural (ancien)",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: Some(CodeKey::CodeRuralEtDeLaPecheMaritime),
    },
    CodeEntry {
        key: CodeKey::CodeDesRelationsEntreLePublicEtLAdministration,
        title: "Code des relations entre le public et l'administration",
        abbreviations: &[("CRPA", Confidence::High)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLEntreeEtDuSejourDesEtrangersEtDuDroitDAsile,
        title: "Code de l'entrée et du séjour des étrangers et du droit d'asile",
        abbreviations: &[("CESEDA", Confidence::High)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLActionSocialeEtDesFamilles,
        title: "Code de l'action sociale et des familles",
        abbreviations: &[("CASF", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLOrganisationJudiciaire,
        title: "Code de l'organisation judiciaire",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDesPostesEtDesCommunicationsElectroniques,
        title: "Code des postes et des communications électroniques",
        abbreviations: &[],
        ambiguous_sigles: &["CPCE"],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLaSecuriteInterieure,
        title: "Code de la sécurité intérieure",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeGeneralDeLaFonctionPublique,
        title: "Code général de la fonction publique",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLaJusticePenaleDesMineurs,
        title: "Code de la justice pénale des mineurs",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeGeneralDeLaProprieteDesPersonnesPubliques,
        title: "Code général de la propriété des personnes publiques",
        abbreviations: &[("CG3P", Confidence::Medium), ("CGPPP", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDesImpositionsSurLesBiensEtServices,
        title: "Code des impositions sur les biens et services",
        abbreviations: &[("CIBS", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLaCommandePublique,
        title: "Code de la commande publique",
        abbreviations: &[],
        ambiguous_sigles: &["CCP"],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLEducation,
        title: "Code de l'éducation",
        abbreviations: &[("C. éduc.", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDesProceduresCivilesDExecution,
        title: "Code des procédures civiles d'exécution",
        abbreviations: &[("CPC exéc.", Confidence::Medium)],
        ambiguous_sigles: &["CPCE"],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLaRoute,
        title: "Code de la route",
        abbreviations: &[("C. route", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeElectoral,
        title: "Code électoral",
        abbreviations: &[("C. élect.", Confidence::Medium)],
        ambiguous_sigles: &["CE"],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLaDefense,
        title: "Code de la défense",
        abbreviations: &[("C. déf.", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLEnergie,
        title: "Code de l'énergie",
        abbreviations: &[("C. énergie", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDesTransports,
        title: "Code des transports",
        abbreviations: &[("C. transp.", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDuPatrimoine,
        title: "Code du patrimoine",
        abbreviations: &[("C. patr.", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDuSport,
        title: "Code du sport",
        abbreviations: &[("C. sport", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDuTourisme,
        title: "Code du tourisme",
        abbreviations: &[("C. tourisme", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLaVoirieRoutiere,
        title: "Code de la voirie routière",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLaRecherche,
        title: "Code de la recherche",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLaMutualite,
        title: "Code de la mutualité",
        abbreviations: &[("C. mut.", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDesJuridictionsFinancieres,
        title: "Code des juridictions financières",
        abbreviations: &[("CJF", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodePenitentiaire,
        title: "Code pénitentiaire",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDesDouanes,
        title: "Code des douanes",
        abbreviations: &[("C. douanes", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDesDouanesDeMayotte,
        title: "Code des douanes de Mayotte",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeForestierNouveau,
        title: "Code forestier (nouveau)",
        abbreviations: &[("C. for.", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeMinierNouveau,
        title: "Code minier (nouveau)",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: Some(CodeKey::CodeMinierAncien),
    },
    CodeEntry {
        key: CodeKey::CodeMinierAncien,
        title: "Code minier (ancien)",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: Some(CodeKey::CodeMinierNouveau),
    },
    CodeEntry {
        key: CodeKey::CodeDeLExpropriationPourCauseDUtilitePublique,
        title: "Code de l'expropriation pour cause d'utilité publique",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDuCinemaEtDeLImageAnimee,
        title: "Code du cinéma et de l'image animée",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeJusticeMilitaireNouveau,
        title: "Code de justice militaire (nouveau)",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDuServiceNational,
        title: "Code du service national",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLAviationCivile,
        title: "Code de l'aviation civile",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLArtisanat,
        title: "Code de l'artisanat",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDesPensionsCivilesEtMilitairesDeRetraite,
        title: "Code des pensions civiles et militaires de retraite",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDesPensionsMilitairesDInvaliditeEtDesVictimesDeGuerre,
        title: "Code des pensions militaires d'invalidité et des victimes de guerre",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDesPensionsDeRetraiteDesMarinsFrancaisDuCommerceDePecheOuDePlaisance,
        title: "Code des pensions de retraite des marins français du commerce, de pêche ou de plaisance",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLaLegionDHonneurDeLaMedailleMilitaireEtDeLOrdreNationalDuMerite,
        title: "Code de la Légion d'honneur, de la Médaille militaire et de l'ordre national du Mérite",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDuDomainePublicFluvialEtDeLaNavigationInterieure,
        title: "Code du domaine public fluvial et de la navigation intérieure",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDuDomaineDeLEtat,
        title: "Code du domaine de l'Etat",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDuDomaineDeLEtatEtDesCollectivitesPubliquesApplicableALaCollectiviteTerritorialeDeMayotte,
        title: "Code du domaine de l'Etat et des collectivités publiques applicable à la collectivité territoriale de Mayotte",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDesPortsMaritimes,
        title: "Code des ports maritimes",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDuTravailMaritime,
        title: "Code du travail maritime",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDisciplinaireEtPenalDeLaMarineMarchande,
        title: "Code disciplinaire et pénal de la marine marchande",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDesInstrumentsMonetairesEtDesMedailles,
        title: "Code des instruments monétaires et des médailles",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeDeontologieDesArchitectes,
        title: "Code de déontologie des architectes",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDesCommunes,
        title: "Code des communes",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDesCommunesDeLaNouvelleCaledonie,
        title: "Code des communes de la Nouvelle-Calédonie",
        abbreviations: &[],
        ambiguous_sigles: &[],
        variant_of: None,
    },
    CodeEntry {
        key: CodeKey::CodeDeLaFamilleEtDeLAideSociale,
        title: "Code de la famille et de l'aide sociale",
        abbreviations: &[("CFAS", Confidence::Medium)],
        ambiguous_sigles: &[],
        variant_of: None,
    },
];

impl CodeKey {
    /// Titre officiel du code.
    pub fn title(self) -> &'static str {
        self.entry().title
    }

    /// Entrée du dictionnaire : `CODES[key as usize]`.
    #[expect(
        clippy::as_conversions,
        reason = "discriminant d'une énumération sans donnée, dans l'ordre de `CODES` (test `keys_follow_the_table_order_and_names_are_unique`)"
    )]
    #[expect(
        clippy::indexing_slicing,
        reason = "le discriminant indexe le tableau `CODES`, ordonné comme l'énumération (test dédié)"
    )]
    pub fn entry(self) -> &'static CodeEntry {
        &CODES[self as usize]
    }
}

#[cfg(test)]
#[expect(
    clippy::as_conversions,
    reason = "tests : tailles et indices bornés par les données du test"
)]
mod tests {
    use std::collections::HashSet;

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::citation::types::Sigle;

    /// Copie de test de `docs/plan/annexes/donnees/codes-table.md` (annexe B §8.1).
    const TABLE: &str = include_str!("../../../tests/data/citations/codes-table.md");

    /// Formes dont les seules sources sont `[KM]` ou `[JRX]` (AGPL-3.0) : absentes de `CODES`
    /// et non exigées tant qu'une source non AGPL ne les a pas confirmées (§5.5).
    const AGPL_EXCEPTIONS: [&str; 6] = ["C. conso.", "COJ", "CSI", "CGFP", "CJPM", "C. rur."];

    /// Cellules des lignes numérotées du tableau ; le titre, la légende, l'en-tête et la ligne
    /// finale sont ignorés.
    fn table_rows() -> Vec<Vec<&'static str>> {
        TABLE
            .lines()
            .filter_map(|line| {
                let cells: Vec<&str> =
                    line.trim().trim_matches('|').split('|').map(str::trim).collect();
                cells
                    .first()
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                    .then_some(cells)
            })
            .collect()
    }

    /// Formes entre apostrophes inversées d'une cellule, hors mentions entre parenthèses.
    fn forms(cell: &str) -> Vec<&str> {
        cell.split('`').skip(1).step_by(2).collect()
    }

    #[test]
    fn dict_matches_codes_table() {
        let rows = table_rows();
        let numbers: Vec<usize> = rows.iter().filter_map(|r| r[0].parse().ok()).collect();
        assert_eq!(numbers, (1..=77).collect::<Vec<_>>());
        assert_eq!(CODES.len(), 77);
        for (row, entry) in rows.iter().zip(&CODES) {
            let title = row[1].replace('\u{b9}', "");
            let number = row[0];
            assert_eq!(title.trim(), entry.title, "titre de la ligne {number}");
            for (column, confidence) in [(3, Confidence::High), (4, Confidence::Medium)] {
                for form in forms(row[column]) {
                    if AGPL_EXCEPTIONS.contains(&form) {
                        continue;
                    }
                    let known =
                        entry.abbreviations.iter().find(|(abbreviation, _)| *abbreviation == form);
                    match known {
                        Some((_, found)) => assert_eq!(*found, confidence, "{form}"),
                        None => assert!(
                            entry.ambiguous_sigles.contains(&form),
                            "forme absente de {} : {form}",
                            entry.title
                        ),
                    }
                }
            }
            // Réciproque : le dictionnaire ne contient rien que le tableau ne porte pas.
            let table_forms: Vec<&str> = [3, 4].into_iter().flat_map(|c| forms(row[c])).collect();
            for form in entry
                .abbreviations
                .iter()
                .map(|(f, _)| *f)
                .chain(entry.ambiguous_sigles.iter().copied())
            {
                assert!(
                    table_forms.contains(&form),
                    "forme inventée pour {} : {form}",
                    entry.title
                );
            }
        }
    }

    #[test]
    fn dict_keeps_agpl_only_forms_out() {
        let all: Vec<&str> = CODES
            .iter()
            .flat_map(|e| {
                e.abbreviations.iter().map(|(f, _)| *f).chain(e.ambiguous_sigles.iter().copied())
            })
            .collect();
        for form in AGPL_EXCEPTIONS {
            assert!(!all.contains(&form), "{form} est une forme AGPL seule");
        }
        // Ces codes restent reconnus par leur titre.
        assert_eq!(CodeKey::CodeDeLaConsommation.entry().abbreviations.len(), 1);
        assert_eq!(CodeKey::CodeDeLOrganisationJudiciaire.entry().abbreviations, []);
    }

    #[test]
    fn keys_follow_the_table_order_and_names_are_unique() {
        for (i, entry) in CODES.iter().enumerate() {
            assert_eq!(entry.key as usize, i);
            assert_eq!(entry.key.entry(), entry);
            assert_eq!(entry.key.title(), entry.title);
        }
        let names: HashSet<String> = CODES.iter().map(|e| format!("{:?}", e.key)).collect();
        assert_eq!(names.len(), 77);
        assert_eq!(CodeKey::CodeCivil.title(), "Code civil");
        assert_eq!(
            CodeKey::CodeDeLaFamilleEtDeLAideSociale.title(),
            "Code de la famille et de l'aide sociale"
        );
        let titles: HashSet<&str> = CODES.iter().map(|e| e.title).collect();
        assert_eq!(titles.len(), 77);
    }

    #[test]
    fn ambiguous_sigles_are_the_eight_of_annex_b_5_3() {
        let mut found: Vec<Sigle> = Vec::new();
        for entry in &CODES {
            for sigle in entry.ambiguous_sigles {
                let parsed = Sigle::parse(sigle).expect("sigle du §5.3");
                if !found.contains(&parsed) {
                    found.push(parsed);
                }
            }
        }
        found.sort_by_key(|s| s.as_str());
        let mut expected = Sigle::ALL.to_vec();
        expected.sort_by_key(|s| s.as_str());
        assert_eq!(found, expected);
        // CPCE désigne deux codes.
        let cpce: Vec<CodeKey> =
            CODES.iter().filter(|e| e.ambiguous_sigles.contains(&"CPCE")).map(|e| e.key).collect();
        assert_eq!(
            cpce,
            [
                CodeKey::CodeDesPostesEtDesCommunicationsElectroniques,
                CodeKey::CodeDesProceduresCivilesDExecution
            ]
        );
    }

    #[test]
    fn variants_point_at_each_other() {
        let with_variant: Vec<&CodeEntry> =
            CODES.iter().filter(|e| e.variant_of.is_some()).collect();
        assert_eq!(with_variant.len(), 4);
        for entry in with_variant {
            let other = entry.variant_of.map(CodeKey::entry);
            assert_eq!(other.and_then(|o| o.variant_of), Some(entry.key), "{}", entry.title);
        }
        assert_eq!(CodeKey::CodeMinierNouveau.entry().variant_of, Some(CodeKey::CodeMinierAncien));
        assert_eq!(
            CodeKey::CodeRuralAncien.entry().variant_of,
            Some(CodeKey::CodeRuralEtDeLaPecheMaritime)
        );
    }

    #[test]
    fn code_ref_key_distinguishes_known_and_other() {
        assert_ne!(CodeRefKey::Known(CodeKey::CodeCivil), CodeRefKey::Other);
        assert_eq!(CodeRefKey::Known(CodeKey::CodeCivil), CodeRefKey::Known(CodeKey::CodeCivil));
    }
}
