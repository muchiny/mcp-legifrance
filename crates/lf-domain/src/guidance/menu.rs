//! Menu d'un public de Service-Public (`menu.xml`) et chemin d'une page dans le site
//! (ADR-0033 décision 3, Q41).
//!
//! Le menu range les pages en trois niveaux : thèmes, sous-thèmes, dossiers. L'adapter le lit dans
//! l'ordre du document et donne chaque élément avec sa profondeur, son numéro et son intitulé
//! bruts ([`MenuEntry`]) ; le domaine en tire le chemin de chaque page ([`GuidanceMenu::path`]) :
//! les intitulés qui mènent à elle, du plus large au plus étroit.
//!
//! Mesure du 2026-11-24 sur les archives du jour : 304 éléments chez Particuliers et 100 chez
//! Entreprendre (deux dossiers y sont placés deux fois), trois niveaux, intitulés de 76 caractères
//! au plus. Pour chacune des 2 972 fiches de Particuliers et des 762 d'Entreprendre qui nomment
//! leur dossier (`DossierPere`), la place de ce dossier dans le menu concorde avec le thème et le
//! sous-thème que la fiche écrit ; un chemin fait 147 caractères au plus.

use std::collections::HashMap;

use super::{id::SheetId, line::clean_line};

/// Profondeur du menu : thème (1), sous-thème (2), dossier (3).
pub const MENU_DEPTH_MAX: usize = 3;

/// Nombre d'éléments du menu lus ; les suivants sont ignorés (304 mesurés au plus).
pub const MENU_ENTRIES_MAX: usize = 5_000;

/// Longueur d'un intitulé du chemin, en caractères, `…` compris (76 mesurés au plus).
pub const MENU_TITLE_MAX_CHARS: usize = 120;

/// Un élément du menu tel que l'adapter le lit (`ItemMenu`) : rien n'y est encore validé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuEntry {
    /// Profondeur dans le document : 1 pour un élément de premier niveau.
    pub depth: usize,
    /// Numéro (attribut `ID`), brut.
    pub id: String,
    /// Intitulé (`Titre`), brut.
    pub title: String,
}

/// Place d'une page dans le menu.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Place {
    /// Intitulés non vides des éléments qui la contiennent, du plus large au plus étroit.
    ancestors: Vec<String>,
    /// Son intitulé, vide s'il manque.
    title: String,
}

/// Le menu d'un public : la place de chaque page qu'il range.
///
/// ```
/// use lf_domain::guidance::{GuidanceMenu, MenuEntry, SheetId};
///
/// let entry = |depth, id: &str, title: &str| MenuEntry {
///     depth,
///     id: id.to_owned(),
///     title: title.to_owned(),
/// };
/// let menu = GuidanceMenu::build([
///     entry(1, "N19807", "Justice"),
///     entry(2, "N20385", "Réparation du préjudice"),
///     entry(3, "N277", "Saisies et recouvrements"),
/// ]);
/// let (sheet, folder) = (SheetId::parse("F1437")?, SheetId::parse("N277")?);
/// assert_eq!(
///     menu.path(&sheet, Some(&folder)),
///     ["Justice", "Réparation du préjudice", "Saisies et recouvrements"]
/// );
/// assert_eq!(menu.path(&folder, None), ["Justice", "Réparation du préjudice"]);
/// # Ok::<(), lf_domain::error::DomainError>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GuidanceMenu {
    places: HashMap<SheetId, Place>,
}

impl GuidanceMenu {
    /// Menu tiré des éléments lus, dans l'ordre du document ; [`MENU_ENTRIES_MAX`] éléments au
    /// plus.
    ///
    /// Un élément trop profond, ou qui saute un niveau, est ignoré avec tout ce qu'il contient ;
    /// un élément au numéro invalide garde sa place, pour que ses enfants restent bien rangés, sans
    /// être cherchable ; un numéro placé deux fois garde sa première place ; un intitulé est mis
    /// sur une ligne et borné à [`MENU_TITLE_MAX_CHARS`] caractères, et un intitulé vide ne paraît
    /// dans aucun chemin.
    #[must_use]
    pub fn build(entries: impl IntoIterator<Item = MenuEntry>) -> Self {
        let mut places = HashMap::new();
        // Intitulés des éléments ouverts, un par niveau.
        let mut open: Vec<String> = Vec::new();
        for entry in entries.into_iter().take(MENU_ENTRIES_MAX) {
            let depth = entry.depth;
            if depth == 0 || depth > MENU_DEPTH_MAX || depth > open.len().saturating_add(1) {
                continue;
            }
            open.truncate(depth.saturating_sub(1));
            let title = bounded_title(&entry.title);
            if let Ok(id) = SheetId::parse(&entry.id) {
                places.entry(id).or_insert_with(|| Place {
                    ancestors: open.iter().filter(|title| !title.is_empty()).cloned().collect(),
                    title: title.clone(),
                });
            }
            open.push(title);
        }
        Self { places }
    }

    /// Chemin d'une page dans le site. Pour une page de navigation (`N…`) : les intitulés des
    /// éléments qui la contiennent. Pour une fiche : ceux de la place de son dossier `parent`
    /// (`DossierPere`), plus l'intitulé de ce dossier. Vide quand la page, ou son dossier, n'est
    /// pas dans le menu.
    #[must_use]
    pub fn path(&self, id: &SheetId, parent: Option<&SheetId>) -> Vec<String> {
        if id.is_navigation() {
            return self.places.get(id).map(|place| place.ancestors.clone()).unwrap_or_default();
        }
        parent
            .and_then(|parent| self.places.get(parent))
            .map(|place| {
                let mut path = place.ancestors.clone();
                if !place.title.is_empty() {
                    path.push(place.title.clone());
                }
                path
            })
            .unwrap_or_default()
    }

    /// Nombre de pages rangées.
    #[must_use]
    pub fn len(&self) -> usize {
        self.places.len()
    }

    /// Vrai si le menu ne range aucune page.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.places.is_empty()
    }
}

/// Intitulé sur une ligne, sans invisible, [`MENU_TITLE_MAX_CHARS`] caractères au plus, `…`
/// compris.
fn bounded_title(raw: &str) -> String {
    let line = clean_line(raw);
    if line.chars().count() <= MENU_TITLE_MAX_CHARS {
        return line;
    }
    let mut kept: String = line.chars().take(MENU_TITLE_MAX_CHARS.saturating_sub(1)).collect();
    kept.push('…');
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use regex::Regex;

    fn entry(depth: usize, id: &str, title: &str) -> MenuEntry {
        MenuEntry { depth, id: id.to_owned(), title: title.to_owned() }
    }

    fn id(raw: &str) -> SheetId {
        SheetId::parse(raw).unwrap()
    }

    /// Thème, sous-thème, dossier ; et, comme chez Entreprendre, un dossier rangé directement sous
    /// son thème.
    fn menu() -> GuidanceMenu {
        GuidanceMenu::build([
            entry(1, "N1", "Argent"),
            entry(2, "N2", "Banque"),
            entry(3, "N3", "Compte bancaire"),
            entry(3, "N4", "Crédit"),
            entry(1, "N5", "Travail"),
            entry(2, "N6", "Salaire"),
        ])
    }

    #[test]
    fn a_menu_gives_each_page_its_path() {
        let menu = menu();
        assert_eq!(menu.len(), 6);
        assert_eq!(menu.path(&id("N3"), None), ["Argent", "Banque"]);
        assert_eq!(menu.path(&id("F1"), Some(&id("N3"))), ["Argent", "Banque", "Compte bancaire"]);
        assert_eq!(menu.path(&id("F2"), Some(&id("N4"))), ["Argent", "Banque", "Crédit"]);
        assert_eq!(menu.path(&id("F3"), Some(&id("N6"))), ["Travail", "Salaire"]);
        // Un thème est à la racine ; une page de navigation ne prend pas le chemin d'un parent.
        assert_eq!(menu.path(&id("N1"), None), Vec::<String>::new());
        assert_eq!(menu.path(&id("N6"), Some(&id("N3"))), ["Travail"]);
        // Une fiche sans dossier, ou dont le dossier n'est pas rangé, n'a pas de chemin.
        assert_eq!(menu.path(&id("F1"), None), Vec::<String>::new());
        assert_eq!(menu.path(&id("F1"), Some(&id("N99"))), Vec::<String>::new());
        assert_eq!(menu.path(&id("N99"), None), Vec::<String>::new());
    }

    #[test]
    fn a_folder_placed_twice_keeps_its_first_place() {
        let menu = GuidanceMenu::build([
            entry(1, "N1", "Argent"),
            entry(2, "N3", "Compte bancaire"),
            entry(1, "N5", "Travail"),
            entry(2, "N3", "Compte bancaire"),
        ]);
        assert_eq!(menu.len(), 3);
        assert_eq!(menu.path(&id("F1"), Some(&id("N3"))), ["Argent", "Compte bancaire"]);
    }

    #[test]
    fn malformed_entries_are_ignored_with_what_they_contain() {
        let menu = GuidanceMenu::build([
            // Profondeur nulle, ou un niveau sauté sans parent.
            entry(0, "N10", "Nul"),
            entry(2, "N11", "Orphelin"),
            entry(1, "N1", "Argent"),
            // Numéro invalide : gardé pour ranger ses enfants, sans être cherchable.
            entry(2, "X2", "Banque"),
            entry(3, "N3", "Compte bancaire"),
            // Trop profond, avec son enfant.
            entry(4, "N12", "Trop profond"),
            entry(5, "N13", "Plus profond"),
            // Intitulé vide : la page est rangée, mais ne paraît pas dans les chemins.
            entry(2, "N4", " \u{200b} "),
            entry(3, "N7", "Prêt"),
            // Un niveau sauté sous un thème.
            entry(1, "N5", "Travail"),
            entry(3, "N14", "Sauté"),
        ]);
        let ignored: Vec<_> = ["N10", "N11", "N12", "N13", "N14"]
            .iter()
            .map(|raw| menu.path(&id("F1"), Some(&id(raw))))
            .collect();
        assert_eq!(ignored, vec![Vec::<String>::new(); 5]);
        assert_eq!(menu.len(), 5);
        assert_eq!(menu.path(&id("F1"), Some(&id("N3"))), ["Argent", "Banque", "Compte bancaire"]);
        assert_eq!(menu.path(&id("F1"), Some(&id("N4"))), ["Argent"]);
        assert_eq!(menu.path(&id("N7"), None), ["Argent"]);
    }

    #[test]
    fn titles_are_flat_and_bounded() {
        let long = "é".repeat(MENU_TITLE_MAX_CHARS);
        let menu = GuidanceMenu::build([
            entry(1, "N1", " Argent\n et\tbanque "),
            entry(2, "N2", &long),
            entry(3, "N3", &format!("{long}x")),
        ]);
        let shortened = format!("{}…", "é".repeat(MENU_TITLE_MAX_CHARS - 1));
        assert_eq!(menu.path(&id("F1"), Some(&id("N3"))), ["Argent et banque", &long, &shortened]);
    }

    #[test]
    fn entries_beyond_the_bound_are_ignored() {
        let entries = (1..=MENU_ENTRIES_MAX + 1).map(|n| entry(1, &format!("N{n}"), "Thème"));
        let menu = GuidanceMenu::build(entries);
        assert_eq!(menu.len(), MENU_ENTRIES_MAX);
        assert!(!menu.is_empty());
        assert!(GuidanceMenu::default().is_empty());
    }

    /// Éléments du menu réel de Particuliers (fixture de T-MS-02), lus dans l'ordre : profondeur
    /// suivie à l'ouverture et à la fermeture de `ItemMenu`, intitulé = premier `Titre` qui suit
    /// l'ouverture (ceux des renvois `VoirAussi` viennent après et sont ignorés).
    fn fixture_entries(xml: &str) -> Vec<MenuEntry> {
        let token =
            Regex::new(r#"<ItemMenu[^>]*\bID="([^"]*)"[^>]*>|</ItemMenu>|<Titre>([^<]*)</Titre>"#)
                .unwrap();
        let (mut depth, mut pending, mut entries) = (0, None, Vec::new());
        for found in token.captures_iter(xml) {
            match (found.get(1), found.get(2)) {
                (Some(id), _) => {
                    depth = usize::saturating_add(depth, 1);
                    pending = Some(id.as_str().to_owned());
                }
                (None, Some(title)) => {
                    if let Some(id) = pending.take() {
                        entries.push(MenuEntry { depth, id, title: title.as_str().to_owned() });
                    }
                }
                (None, None) => depth = usize::saturating_sub(depth, 1),
            }
        }
        entries
    }

    #[test]
    fn the_real_menu_places_a_sheet_under_its_folder() {
        let xml =
            include_str!("../../../lf-adapter-service-public/tests/fixtures/xml/part/menu.xml");
        let entries = fixture_entries(xml);
        assert_eq!(entries.len(), 304);
        let menu = GuidanceMenu::build(entries);
        assert_eq!(menu.len(), 304);
        // F1437 (Solde bancaire insaisissable) nomme son dossier N277 ; la fiche écrit aussi son
        // thème (N19807) et son sous-thème (N20385), que le menu confirme.
        assert_eq!(
            menu.path(&id("F1437"), Some(&id("N277"))),
            ["Justice", "Réparation du préjudice", "Saisies et recouvrements"]
        );
        assert_eq!(menu.path(&id("N277"), None), ["Justice", "Réparation du préjudice"]);
    }
}
