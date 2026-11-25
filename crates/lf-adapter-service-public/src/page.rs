//! Traduction d'une entrée lue en valeurs du domaine (ADR-0033 décisions 5, 6 et 8 ; 05 §5.17
//! « Texte » ; T-MS-22).
//!
//! - **En-tête** d'une page, pour l'index ([`header`]) : numéro, nature, titre, résumé, date,
//!   adresse publique et chemin dans le site (Q41), tirés des premiers éléments de la page.
//! - **Page** lue ([`page`]) : les mêmes métadonnées ; pour une fiche, son texte en blocs du
//!   domaine et ses références de Légifrance ; pour un dossier ou un thème, ses entrées.
//! - **Menu** ([`menu`]) : ses éléments avec leur profondeur, pour `GuidanceMenu`.
//!
//! Texte d'une fiche : les parties [`BODY`] de `Publication`, dans l'ordre du document ; les
//! autres (liens « voir aussi », services en ligne, définitions, contacts…) n'en sont pas. Dans ces
//! parties, les éléments connus deviennent des blocs (chapitres, sous-chapitres, paragraphes,
//! listes, tableaux, cas et situations, encadrés) ; ceux de [`SKIPPED`] sont omis (logique des
//! conditions, médias, renvois, références) ; tout autre élément est traversé, et son texte
//! gardé : la DILA annonce que des éléments peuvent s'ajouter. Un lien vers une autre page garde
//! son numéro.
//!
//! Références légales : les éléments `Reference` (titre et adresse) et `dc:source` (liste
//! d'adresses à virgules), dans l'ordre du document ; le domaine ne garde que les adresses de
//! Légifrance de sa grammaire.

use lf_domain::{
    date::CivilDate,
    guidance::{
        Audience, Block, GuidanceContent, GuidanceDoc, GuidanceKind, GuidanceMenu, HeadingLevel,
        IndexedPage, Inline, Label, LegalReferences, MenuEntry, NavigationEntry, ServicePublicUrl,
        SheetId, clean_line, legal_references, render, source_urls,
    },
};
use thiserror::Error;

use crate::xml::{Child, Node};

/// Racine d'une fiche ou d'une page de navigation.
const ROOT: &str = "Publication";

/// Racine du menu.
const MENU_ROOT: &str = "Menu";

/// Parties de `Publication` qui forment le texte d'une fiche.
pub(crate) const BODY: [&str; 7] = [
    "Avertissement",
    "Introduction",
    "Texte",
    "ListeSituations",
    "Conclusion",
    "LienExterneCommente",
    "Problematiques",
];

/// Éléments omis du texte : références (lues à part), définitions et abréviations du glossaire,
/// logique des conditions et des recherches guidées, colonnes de tableau, médias, contacts,
/// services en ligne et renvois.
pub(crate) const SKIPPED: [&str; 24] = [
    "Reference",
    "Definition",
    "Abreviation",
    "Condition",
    "SiSelectionne",
    "Colonne",
    "Resume",
    "TitreAlternatif",
    "Video",
    "Image",
    "Infographie",
    "ImageIllustration",
    "Credits",
    "Legende",
    "TexteDeRemplacement",
    "OuSAdresser",
    "QuiPeutMAider",
    "PourEnSavoirPlus",
    "ServiceEnLigne",
    "Source",
    "RessourceWeb",
    "PivotLocal",
    "VoirAussi",
    "Questionnaire",
];

/// Éléments d'une ligne de texte : ils restent dans le paragraphe qui les porte.
const INLINE: [&str; 10] = [
    "LienInterne",
    "LienIntra",
    "LienExterne",
    "MiseEnEvidence",
    "Valeur",
    "Expression",
    "Exposant",
    "Indice",
    "TermeEtranger",
    "Date",
];

/// Titres propres d'un bloc : ils deviennent son titre, pas un paragraphe.
const TITLES: [&str; 2] = ["Titre", "TitreRiche"];

/// Refus d'une page. Aucun ne porte de texte lu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum PageError {
    /// La racine n'est pas `Publication`.
    #[error("entry is not a publication")]
    NotPublication,
    /// Le numéro manque, est invalide, ou n'est pas celui de l'entrée.
    #[error("publication id is missing, invalid or not the entry's")]
    WrongId,
    /// Aucun titre.
    #[error("publication has no title")]
    NoTitle,
    /// Texte au-delà de la taille d'un texte long.
    #[error("sheet text is too long")]
    TooLong,
}

/// Métadonnées communes à l'en-tête et à la page.
struct Meta {
    id: SheetId,
    kind: GuidanceKind,
    sheet_type: Option<String>,
    title: String,
    description: Option<String>,
    theme: Option<String>,
    updated: Option<CivilDate>,
    source_url: ServicePublicUrl,
    path: Vec<String>,
}

/// En-tête de la page `expected` de `audience`, pour l'index.
pub(crate) fn header(
    root: &Node,
    audience: Audience,
    expected: &SheetId,
    menu: &GuidanceMenu,
) -> Result<IndexedPage, PageError> {
    let meta = meta(root, audience, expected, menu)?;
    Ok(IndexedPage {
        id: meta.id,
        kind: meta.kind,
        title: meta.title,
        path: meta.path,
        summary: meta.description.unwrap_or_default(),
        updated: meta.updated,
        source_url: meta.source_url,
    })
}

/// La page `expected` de `audience`, lue en entier.
pub(crate) fn page(
    root: &Node,
    audience: Audience,
    expected: &SheetId,
    menu: &GuidanceMenu,
) -> Result<GuidanceDoc, PageError> {
    let meta = meta(root, audience, expected, menu)?;
    let content = match meta.kind {
        GuidanceKind::Sheet => GuidanceContent::Sheet {
            text: render(&body(root)).map_err(|_| PageError::TooLong)?,
            references: references(root),
        },
        GuidanceKind::Folder => GuidanceContent::Folder(entries(root, "SousDossier", "Fiche")),
        GuidanceKind::Theme => GuidanceContent::Theme(entries(root, "SousTheme", "Dossier")),
    };
    Ok(GuidanceDoc {
        id: meta.id,
        audience,
        sheet_type: meta.sheet_type,
        title: meta.title,
        description: meta.description,
        theme: meta.theme,
        path: meta.path,
        updated: meta.updated,
        source_url: meta.source_url,
        content,
    })
}

/// Éléments du menu, dans l'ordre du document, avec leur profondeur (1 au premier niveau). Une
/// autre racine que `Menu` ne donne rien.
pub(crate) fn menu(root: &Node) -> Vec<MenuEntry> {
    let mut out = Vec::new();
    if root.name == MENU_ROOT {
        menu_items(root, 1, &mut out);
    }
    out
}

fn menu_items(node: &Node, depth: usize, out: &mut Vec<MenuEntry>) {
    for item in node.elements().filter(|node| node.name == "ItemMenu") {
        out.push(MenuEntry {
            depth,
            id: item.attr("ID").unwrap_or_default().to_owned(),
            title: item.child("Titre").map(Node::text).unwrap_or_default(),
        });
        menu_items(item, depth.saturating_add(1), out);
    }
}

fn meta(
    root: &Node,
    audience: Audience,
    expected: &SheetId,
    menu: &GuidanceMenu,
) -> Result<Meta, PageError> {
    if root.name != ROOT {
        return Err(PageError::NotPublication);
    }
    let id = root
        .attr("ID")
        .and_then(|raw| SheetId::parse(raw).ok())
        .filter(|id| id == expected)
        .ok_or(PageError::WrongId)?;
    let title = line_of(root.child("dc:title")).ok_or(PageError::NoTitle)?;
    let kind = if !id.is_navigation() {
        GuidanceKind::Sheet
    } else if root.attr("type") == Some("Theme") {
        GuidanceKind::Theme
    } else {
        GuidanceKind::Folder
    };
    // `2026-04-01T00:00:00` : la date seule.
    let updated = root
        .attr("dateDerniereModificationImportante")
        .and_then(|raw| raw.get(..10))
        .and_then(|date| CivilDate::parse(date).ok());
    let parent = root
        .child("DossierPere")
        .and_then(|node| node.attr("ID"))
        .and_then(|raw| SheetId::parse(raw).ok());
    Ok(Meta {
        path: menu.path(&id, parent.as_ref()),
        source_url: ServicePublicUrl::read(root.attr("spUrl"), audience, &id),
        // Le type du flux n'a de sens que pour une fiche ; celui d'un dossier ou d'un thème
        // répète sa nature (`kind`).
        sheet_type: root
            .attr("type")
            .filter(|_| kind == GuidanceKind::Sheet)
            .map(clean_line)
            .filter(|value| !value.is_empty()),
        description: line_of(root.child("dc:description")),
        theme: line_of(root.child("Theme").and_then(|theme| theme.child("Titre"))),
        id,
        kind,
        title,
        updated,
    })
}

/// Texte d'un élément sur une ligne ; `None` s'il manque ou s'il est vide.
fn line_of(node: Option<&Node>) -> Option<String> {
    node.map(|node| clean_line(&node.text())).filter(|line| !line.is_empty())
}

/// Entrées d'une page de navigation : celles des groupes `group` (sous-dossier, sous-thème), avec
/// leur intitulé, puis ou avant celles posées sans groupe, dans l'ordre du document.
fn entries(root: &Node, group: &str, item: &str) -> Vec<NavigationEntry> {
    let mut out = Vec::new();
    for node in root.elements() {
        if node.name == group {
            let title = line_of(node.child("Titre"));
            let items = node.elements().filter(|node| node.name == item);
            out.extend(items.filter_map(|node| entry(node, title.clone())));
        } else if node.name == item {
            out.extend(entry(node, None));
        }
    }
    out
}

/// Une entrée : son numéro (attribut `ID`), son titre (élément `Titre`, sinon son texte).
fn entry(node: &Node, group: Option<String>) -> Option<NavigationEntry> {
    let id = SheetId::parse(node.attr("ID")?).ok()?;
    let title = node.child("Titre").map_or_else(|| node.text(), Node::text);
    let kind = if id.is_navigation() { GuidanceKind::Folder } else { GuidanceKind::Sheet };
    Some(NavigationEntry { id, kind, title: clean_line(&title), group })
}

/// Blocs du texte d'une fiche.
fn body(root: &Node) -> Vec<Block> {
    let mut out = Vec::new();
    for part in root.elements().filter(|node| BODY.contains(&node.name.as_str())) {
        block(part, &mut out);
    }
    out
}

/// Ajoute les blocs d'un élément de niveau bloc.
fn block(node: &Node, out: &mut Vec<Block>) {
    match node.name.as_str() {
        "Paragraphe" => paragraph(inlines(node), out),
        "Titre" | "TitreRiche" => paragraph(title(node), out),
        "Chapitre" => section(HeadingLevel::Chapter, node, out),
        "SousChapitre" => section(HeadingLevel::Section, node, out),
        "TitreFlottant" => heading(HeadingLevel::Floating, title(node), out),
        "Liste" => {
            let items = node.elements().filter(|node| node.name == "Item");
            out.push(Block::List(items.map(|item| blocks(item, &[])).collect()));
        }
        "Tableau" => table(node, out),
        "Cas" => labelled(Label::Case, node, out),
        "Situation" => labelled(Label::Situation, node, out),
        "ASavoir" => labelled(Label::ToKnow, node, out),
        "Attention" | "Avertissement" => labelled(Label::Warning, node, out),
        "ANoter" => labelled(Label::Note, node, out),
        "Exemple" => labelled(Label::Example, node, out),
        "Rappel" => labelled(Label::Reminder, node, out),
        name if SKIPPED.contains(&name) => {}
        _ => out.extend(blocks(node, &[])),
    }
}

/// Blocs des enfants d'un élément, sauf ceux nommés dans `skip` : le texte et les éléments d'une
/// ligne qui se suivent forment un paragraphe ; chaque autre élément ajoute ses blocs.
fn blocks(node: &Node, skip: &[&str]) -> Vec<Block> {
    let mut out = Vec::new();
    let mut run: Vec<Inline> = Vec::new();
    for child in &node.children {
        match child {
            Child::Text(text) => run.push(Inline::Text(text.clone())),
            Child::Element(element) if INLINE.contains(&element.name.as_str()) => {
                run.extend(inline(element));
            }
            Child::Element(element) => {
                paragraph(std::mem::take(&mut run), &mut out);
                if !skip.contains(&element.name.as_str()) {
                    block(element, &mut out);
                }
            }
        }
    }
    paragraph(run, &mut out);
    out
}

/// Titre puis contenu d'un chapitre ou d'un sous-chapitre.
fn section(level: HeadingLevel, node: &Node, out: &mut Vec<Block>) {
    heading(level, node.child("Titre").map(title).unwrap_or_default(), out);
    out.extend(blocks(node, &TITLES));
}

/// Bloc annoncé : son intitulé, son titre propre (`Titre` ou `TitreRiche`), son contenu. Un titre
/// qui répète l'intitulé est omis : relevé du 2026-11-24, c'est le cas de tous les encadrés
/// (« À savoir », « À noter », « Attention », exemples, rappels : 28 471 sur 28 471), pas des
/// avertissements, des cas ni des situations.
fn labelled(label: Label, node: &Node, out: &mut Vec<Block>) {
    let own = node.elements().find(|node| TITLES.contains(&node.name.as_str()));
    let own = own.map(title).filter(|own| plain(own) != label.as_str());
    out.push(Block::Labelled {
        label,
        title: own.unwrap_or_default(),
        blocks: blocks(node, &TITLES),
    });
}

/// Texte d'une ligne sur une ligne, liens compris, sans leur numéro.
fn plain(text: &[Inline]) -> String {
    let raw: String = text
        .iter()
        .map(|inline| match inline {
            Inline::Text(text) | Inline::Link { text, .. } => text.as_str(),
        })
        .collect();
    clean_line(&raw)
}

/// Tableau : sa légende (`TitreRiche`) en paragraphe, puis ses rangées de cellules.
fn table(node: &Node, out: &mut Vec<Block>) {
    if let Some(caption) = node.child("TitreRiche") {
        paragraph(title(caption), out);
    }
    let rows = node.elements().filter(|node| node.name == "Rangée").map(|row| {
        row.elements().filter(|node| node.name == "Cellule").map(|cell| blocks(cell, &[])).collect()
    });
    out.push(Block::Table(rows.collect()));
}

/// Ajoute un titre s'il porte du texte.
fn heading(level: HeadingLevel, text: Vec<Inline>, out: &mut Vec<Block>) {
    if visible(&text) {
        out.push(Block::Heading { level, text });
    }
}

/// Ajoute un paragraphe s'il porte du texte.
fn paragraph(text: Vec<Inline>, out: &mut Vec<Block>) {
    if visible(&text) {
        out.push(Block::Paragraph(text));
    }
}

/// Vrai si une ligne porte autre chose que des blancs.
fn visible(text: &[Inline]) -> bool {
    text.iter().any(|inline| match inline {
        Inline::Text(text) => !text.trim().is_empty(),
        Inline::Link { .. } => true,
    })
}

/// Ligne d'un titre : ses paragraphes mis bout à bout, ou son texte s'il n'en a pas.
fn title(node: &Node) -> Vec<Inline> {
    let paragraphs: Vec<Vec<Inline>> =
        node.elements().filter(|node| node.name == "Paragraphe").map(inlines).collect();
    if paragraphs.is_empty() {
        return inlines(node);
    }
    paragraphs.join(&Inline::Text(" ".to_owned()))
}

/// Morceaux de ligne des enfants d'un élément.
fn inlines(node: &Node) -> Vec<Inline> {
    node.children
        .iter()
        .flat_map(|child| match child {
            Child::Text(text) => vec![Inline::Text(text.clone())],
            Child::Element(element) => inline(element),
        })
        .collect()
}

/// Morceaux de ligne d'un élément : un lien vers une page garde son numéro ; une image n'en
/// donne aucun ; tout autre élément donne ses propres morceaux.
fn inline(element: &Node) -> Vec<Inline> {
    match element.name.as_str() {
        "LienInterne" => {
            let text = element.text();
            match element.attr("LienPublication").and_then(|raw| SheetId::parse(raw).ok()) {
                Some(id) => vec![Inline::Link { text, id }],
                None => vec![Inline::Text(text)],
            }
        }
        "Image" => Vec::new(),
        _ => inlines(element),
    }
}

/// Références de Légifrance de la page, dans l'ordre du document.
fn references(root: &Node) -> LegalReferences {
    let mut links: Vec<(Option<String>, String)> = Vec::new();
    collect_links(root, &mut links);
    legal_references(links.iter().map(|(title, url)| (title.as_deref(), url.as_str())))
}

fn collect_links(node: &Node, links: &mut Vec<(Option<String>, String)>) {
    for element in node.elements() {
        match element.name.as_str() {
            "dc:source" => {
                let text = element.text();
                links.extend(source_urls(&text).map(|url| (None, url.to_owned())));
            }
            "Reference" => {
                if let Some(url) = element.attr("URL") {
                    links.push((element.child("Titre").map(Node::text), url.to_owned()));
                }
            }
            _ => collect_links(element, links),
        }
    }
}

#[cfg(test)]
mod tests;
