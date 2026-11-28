//! Intitulés des codes de Judilibre (ADR-0035, décisions 6 et 11).
//!
//! Les mentions exigées par les CGU (juridiction et siège, chambre ou formation) se rendent en
//! clair. Ces tables reprennent la taxonomie de la Cour de cassation
//! (`Cour-de-cassation/judilibre-search` @ `f773a6b`, `src/taxons/chamber.js`, `formation.js`,
//! `publication.js`, `solution.js` et `ca/location.js`) : la Cour de cassation et les 36 cours
//! d'appel. Un code absent des tables se rend tel quel, jamais inventé.

use super::Court;

/// Chambres de la Cour de cassation (`chamber`).
const CASSATION_CHAMBERS: [(&str, &str); 12] = [
    ("pl", "Assemblée plénière"),
    ("mi", "Chambre mixte"),
    ("civ1", "Première chambre civile"),
    ("civ2", "Deuxième chambre civile"),
    ("civ3", "Troisième chambre civile"),
    ("comm", "Chambre commerciale financière et économique"),
    ("soc", "Chambre sociale"),
    ("cr", "Chambre criminelle"),
    ("creun", "Chambres réunies"),
    ("ordo", "Première présidence (Ordonnance)"),
    ("allciv", "Toutes les chambres civiles"),
    ("other", "Autre"),
];

/// Formations de la Cour de cassation (`formation`).
const CASSATION_FORMATIONS: [(&str, &str); 6] = [
    ("fp", "Formation plénière de chambre"),
    ("fm", "Formation mixte"),
    ("fs", "Formation de section"),
    ("f", "Formation restreinte"),
    ("frh", "Formation restreinte hors RNSM/NA"),
    ("frr", "Formation restreinte RNSM/NA"),
];

/// Niveaux de publication (`publication`).
const PUBLICATIONS: [(&str, &str); 5] = [
    ("b", "Publié au Bulletin"),
    ("r", "Publié au Rapport"),
    ("l", "Publié aux Lettres de chambre"),
    ("c", "Communiqué"),
    ("n", "Non publié"),
];

/// Solutions de la Cour de cassation (`solution`).
const SOLUTIONS: [(&str, &str); 14] = [
    ("cassation", "Cassation"),
    ("rejet", "Rejet"),
    ("annulation", "Annulation"),
    ("avis", "Avis"),
    ("decheance", "Déchéance"),
    ("designation", "Désignation de juridiction"),
    ("irrecevabilite", "Irrecevabilité"),
    ("nonlieu", "Non-lieu à statuer"),
    ("qpc", "QPC renvoi"),
    ("qpcother", "QPC autres"),
    ("rabat", "Rabat"),
    ("reglement", "Règlement des juges"),
    ("renvoi", "Renvoi"),
    ("other", "Autre"),
];

/// Les 36 cours d'appel (`location` d'une décision `ca`).
const APPEAL_COURTS: [(&str, &str); 36] = [
    ("ca_agen", "Cour d'appel d'Agen"),
    ("ca_aix_provence", "Cour d'appel d'Aix-en-Provence"),
    ("ca_amiens", "Cour d'appel d'Amiens"),
    ("ca_angers", "Cour d'appel d'Angers"),
    ("ca_basse_terre", "Cour d'appel de Basse-Terre"),
    ("ca_bastia", "Cour d'appel de Bastia"),
    ("ca_besancon", "Cour d'appel de Besançon"),
    ("ca_bordeaux", "Cour d'appel de Bordeaux"),
    ("ca_bourges", "Cour d'appel de Bourges"),
    ("ca_caen", "Cour d'appel de Caen"),
    ("ca_cayenne", "Cour d'appel de Cayenne"),
    ("ca_chambery", "Cour d'appel de Chambéry"),
    ("ca_colmar", "Cour d'appel de Colmar"),
    ("ca_dijon", "Cour d'appel de Dijon"),
    ("ca_douai", "Cour d'appel de Douai"),
    ("ca_fort_de_france", "Cour d'appel de Fort-de-France"),
    ("ca_grenoble", "Cour d'appel de Grenoble"),
    ("ca_limoges", "Cour d'appel de Limoges"),
    ("ca_lyon", "Cour d'appel de Lyon"),
    ("ca_metz", "Cour d'appel de Metz"),
    ("ca_montpellier", "Cour d'appel de Montpellier"),
    ("ca_nancy", "Cour d'appel de Nancy"),
    ("ca_nimes", "Cour d'appel de Nîmes"),
    ("ca_noumea", "Cour d'appel de Nouméa"),
    ("ca_orleans", "Cour d'appel d'Orléans"),
    ("ca_papeete", "Cour d'appel de Papeete"),
    ("ca_paris", "Cour d'appel de Paris"),
    ("ca_pau", "Cour d'appel de Pau"),
    ("ca_poitiers", "Cour d'appel de Poitiers"),
    ("ca_reims", "Cour d'appel de Reims"),
    ("ca_rennes", "Cour d'appel de Rennes"),
    ("ca_riom", "Cour d'appel de Riom"),
    ("ca_rouen", "Cour d'appel de Rouen"),
    ("ca_st_denis_reunion", "Cour d'appel de Saint-Denis de la Réunion"),
    ("ca_toulouse", "Cour d'appel de Toulouse"),
    ("ca_versailles", "Cour d'appel de Versailles"),
];

/// Intitulé du code `code` dans `table`, ou le code tel quel.
fn label_in(table: &[(&str, &'static str)], code: &str) -> Option<&'static str> {
    table.iter().find(|(key, _)| *key == code).map(|(_, label)| *label)
}

/// Intitulé de la juridiction et de son siège : « Cour de cassation », « Cour d'appel de
/// Paris ». Le siège d'une cour d'appel vient de la table ; celui d'une autre juridiction n'a
/// pas d'intitulé connu, et son code est ajouté entre parenthèses (« Tribunal judiciaire
/// (tj75056) »). Sans siège, l'intitulé générique de la juridiction.
#[must_use]
pub fn court_label(court: Court, location: Option<&str>) -> String {
    let location = location.map(str::trim).filter(|code| !code.is_empty());
    match (court, location) {
        (Court::Cassation, _) | (_, None) => court.generic_label().to_owned(),
        (Court::Appeal, Some(code)) => {
            label_in(&APPEAL_COURTS, code).map_or_else(|| with_code(court, code), str::to_owned)
        }
        (_, Some(code)) => with_code(court, code),
    }
}

/// Intitulé générique suivi du code du siège entre parenthèses.
fn with_code(court: Court, code: &str) -> String {
    format!("{} ({code})", court.generic_label())
}

/// Intitulé d'une chambre : la table pour la Cour de cassation (`civ1` → « Première chambre
/// civile ») ; ailleurs, la chambre telle que publiée (« Pôle 3 - Chambre 1 »).
#[must_use]
pub fn chamber_label(court: Court, chamber: &str) -> String {
    let found = (court == Court::Cassation).then(|| label_in(&CASSATION_CHAMBERS, chamber));
    found.flatten().map_or_else(|| chamber.to_owned(), str::to_owned)
}

/// Intitulé d'une formation de la Cour de cassation (`fs` → « Formation de section »), ou le
/// code tel quel.
#[must_use]
pub fn formation_label(formation: &str) -> String {
    label_in(&CASSATION_FORMATIONS, formation).map_or_else(|| formation.to_owned(), str::to_owned)
}

/// Intitulé d'un niveau de publication (`b` → « Publié au Bulletin »), ou le code tel quel.
#[must_use]
pub fn publication_label(code: &str) -> String {
    label_in(&PUBLICATIONS, code).map_or_else(|| code.to_owned(), str::to_owned)
}

/// Intitulé d'une solution (`cassation` → « Cassation »), ou le code tel quel. `other`
/// (« Autre ») ne dit rien : l'appelant lui préfère la solution en clair (`solution_alt`) quand
/// elle existe.
#[must_use]
pub fn solution_label(code: &str) -> String {
    label_in(&SOLUTIONS, code).map_or_else(|| code.to_owned(), str::to_owned)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn courts_are_labelled_with_their_seat() {
        assert_eq!(court_label(Court::Cassation, None), "Cour de cassation");
        assert_eq!(court_label(Court::Cassation, Some("ca_paris")), "Cour de cassation");
        assert_eq!(court_label(Court::Appeal, Some("ca_paris")), "Cour d'appel de Paris");
        assert_eq!(court_label(Court::Appeal, Some(" ca_nimes ")), "Cour d'appel de Nîmes");
        assert_eq!(court_label(Court::Appeal, Some("ca_inconnue")), "Cour d'appel (ca_inconnue)");
        assert_eq!(court_label(Court::Appeal, None), "Cour d'appel");
        assert_eq!(court_label(Court::Appeal, Some("  ")), "Cour d'appel");
        assert_eq!(court_label(Court::Judicial, Some("tj75056")), "Tribunal judiciaire (tj75056)");
        assert_eq!(court_label(Court::Labour, None), "Conseil de prud'hommes");
    }

    #[test]
    fn the_36_courts_of_appeal_are_all_labelled() {
        assert_eq!(APPEAL_COURTS.len(), 36);
        for (code, label) in APPEAL_COURTS {
            assert!(code.starts_with("ca_"));
            assert!(label.starts_with("Cour d'appel d"));
            assert_eq!(court_label(Court::Appeal, Some(code)), label);
        }
    }

    #[test]
    fn chambers_formations_publications_and_solutions() {
        assert_eq!(chamber_label(Court::Cassation, "civ1"), "Première chambre civile");
        assert_eq!(chamber_label(Court::Cassation, "Pôle 3"), "Pôle 3");
        assert_eq!(chamber_label(Court::Appeal, "soc"), "soc");
        assert_eq!(chamber_label(Court::Appeal, "Pôle 3 - Chambre 1"), "Pôle 3 - Chambre 1");
        assert_eq!(formation_label("fs"), "Formation de section");
        assert_eq!(formation_label("f"), "Formation restreinte");
        assert_eq!(formation_label("zz"), "zz");
        assert_eq!(publication_label("b"), "Publié au Bulletin");
        assert_eq!(publication_label("n"), "Non publié");
        assert_eq!(publication_label("x"), "x");
        assert_eq!(solution_label("cassation"), "Cassation");
        assert_eq!(solution_label("other"), "Autre");
        assert_eq!(solution_label("inconnue"), "inconnue");
    }
}
