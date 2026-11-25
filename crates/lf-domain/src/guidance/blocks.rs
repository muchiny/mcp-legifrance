//! Blocs d'une fiche de Service-Public et leur rendu en texte (05 §5.17 « Texte » ; ADR-0033,
//! décision 5).
//!
//! L'adapter traduit le XML de la fiche en [`Block`] ; le domaine les rend en texte long, un
//! paragraphe par ligne :
//!
//! - titres de chapitre, de sous-chapitre et titres flottants, paragraphes : une ligne chacun ;
//! - liste : chaque élément commence par `- ` ;
//! - tableau : une ligne par rangée, cellules séparées par ` | ` ;
//! - situation, cas et encadrés : une ligne d'intitulé (« Cas : Vous êtes locataire »,
//!   « À savoir »), puis leurs blocs ;
//! - lien vers une autre fiche : son texte suivi du numéro, « titre (F1234) ».
//!
//! Relevé du 2026-11-24 sur les deux archives : 25 119 cas, 2 568 situations, 18 113 encadrés
//! (« À savoir », « Attention », « À noter »), 2 514 exemples et 499 rappels titrés.

use crate::{
    error::DomainError,
    text::{LongText, flatten_whitespace},
};

use super::{id::SheetId, line::clean_line};

/// Profondeur d'imbrication rendue au plus. Le lecteur XML de l'adapter borne la profondeur des
/// documents plus bas ; au-delà, les blocs ne sont pas parcourus : la pile reste bornée quel que
/// soit l'arbre reçu.
pub const BLOCK_DEPTH_MAX: usize = 64;

/// Morceau d'une ligne de texte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline {
    /// Texte, rendu tel quel, blancs repliés.
    Text(String),
    /// Lien vers une autre fiche ou page de navigation : son texte, puis son numéro entre
    /// parenthèses.
    Link {
        /// Texte du lien.
        text: String,
        /// Numéro de la fiche visée.
        id: SheetId,
    },
}

/// Niveau d'un titre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadingLevel {
    /// Titre de chapitre : il entre dans le résumé.
    Chapter,
    /// Titre de sous-chapitre.
    Section,
    /// Titre flottant, au fil du texte.
    Floating,
}

/// Intitulé d'un bloc annoncé.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Label {
    /// Situation (`Situation`).
    Situation,
    /// Cas (`Cas`).
    Case,
    /// Encadré « À savoir » (`ASavoir`).
    ToKnow,
    /// Encadré « Attention » (`Attention`).
    Warning,
    /// Encadré « À noter » (`ANoter`).
    Note,
    /// Exemple (`Exemple`).
    Example,
    /// Rappel (`Rappel`).
    Reminder,
}

impl Label {
    /// Intitulé tel que le site l'affiche.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Situation => "Situation",
            Self::Case => "Cas",
            Self::ToKnow => "À savoir",
            Self::Warning => "Attention",
            Self::Note => "À noter",
            Self::Example => "Exemple",
            Self::Reminder => "Rappel",
        }
    }
}

/// Bloc d'une fiche.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// Titre.
    Heading {
        /// Niveau.
        level: HeadingLevel,
        /// Texte du titre.
        text: Vec<Inline>,
    },
    /// Paragraphe.
    Paragraph(Vec<Inline>),
    /// Liste : les blocs de chaque élément.
    List(Vec<Vec<Block>>),
    /// Tableau : rangées de cellules, chaque cellule faite de blocs.
    Table(Vec<Vec<Vec<Block>>>),
    /// Bloc annoncé par son intitulé, puis son titre éventuel.
    Labelled {
        /// Intitulé.
        label: Label,
        /// Titre propre du bloc (vide s'il n'en a pas).
        title: Vec<Inline>,
        /// Contenu.
        blocks: Vec<Block>,
    },
}

/// Texte rendu d'une fiche.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SheetText {
    /// Texte long, un paragraphe par ligne rendue.
    pub text: LongText,
    /// Titres des chapitres, dans l'ordre, chacun sur une ligne : ils entrent dans le résumé.
    pub chapters: Vec<String>,
}

/// Rend `blocks` en texte long (05 §5.17 « Texte »).
///
/// ```
/// use lf_domain::guidance::{Block, HeadingLevel, Inline, Label, SheetId, render};
///
/// let text = |s: &str| vec![Inline::Text(s.to_owned())];
/// let rendered = render(&[
///     Block::Heading { level: HeadingLevel::Chapter, text: text("Montant") },
///     Block::Labelled {
///         label: Label::Case,
///         title: text("Vous êtes locataire"),
///         blocks: vec![Block::Paragraph(vec![
///             Inline::Text("Voir ".to_owned()),
///             Inline::Link { text: "l'aide au logement".to_owned(), id: SheetId::parse("F12006")? },
///             Inline::Text(".".to_owned()),
///         ])],
///     },
/// ])?;
/// assert_eq!(
///     rendered.text.as_str(),
///     "Montant\n\nCas : Vous êtes locataire\n\nVoir l'aide au logement (F12006)."
/// );
/// assert_eq!(rendered.chapters, ["Montant"]);
/// # Ok::<(), lf_domain::DomainError>(())
/// ```
///
/// # Errors
///
/// `DomainError::TextTooLarge` si le texte dépasse `LongText::MAX_CHARS` caractères.
pub fn render(blocks: &[Block]) -> Result<SheetText, DomainError> {
    let mut out = Lines::default();
    out.blocks(blocks, 0);
    let Lines { lines, chapters } = out;
    LongText::from_plain(&lines.join("\n")).map(|text| SheetText { text, chapters })
}

/// Lignes rendues et titres de chapitre rencontrés.
#[derive(Default)]
struct Lines {
    lines: Vec<String>,
    chapters: Vec<String>,
}

impl Lines {
    fn blocks(&mut self, blocks: &[Block], depth: usize) {
        if depth >= BLOCK_DEPTH_MAX {
            return;
        }
        for block in blocks {
            self.block(block, depth);
        }
    }

    fn block(&mut self, block: &Block, depth: usize) {
        let inner = depth.saturating_add(1);
        match block {
            Block::Heading { level, text } => {
                let line = phrase(text);
                if *level == HeadingLevel::Chapter {
                    let title = clean_line(&line);
                    if !title.is_empty() {
                        self.chapters.push(title);
                    }
                }
                self.push(line);
            }
            Block::Paragraph(text) => self.push(phrase(text)),
            Block::List(items) => {
                for item in items {
                    let first = self.lines.len();
                    self.blocks(item, inner);
                    if let Some(line) = self.lines.get_mut(first) {
                        line.insert_str(0, "- ");
                    }
                }
            }
            Block::Table(rows) => {
                for row in rows {
                    let cells: Vec<String> = row.iter().map(|cell| flat(cell, inner)).collect();
                    if cells.iter().any(|cell| !cell.is_empty()) {
                        self.push(cells.join(" | "));
                    }
                }
            }
            Block::Labelled { label, title, blocks } => {
                let title = phrase(title);
                if title.is_empty() {
                    self.push(label.as_str().to_owned());
                } else {
                    self.push(format!("{} : {title}", label.as_str()));
                }
                self.blocks(blocks, inner);
            }
        }
    }

    /// Ajoute `line` si elle n'est pas vide.
    fn push(&mut self, line: String) {
        if !line.is_empty() {
            self.lines.push(line);
        }
    }
}

/// Contenu d'une cellule sur une ligne : ses lignes rendues, jointes par une espace.
fn flat(blocks: &[Block], depth: usize) -> String {
    let mut cell = Lines::default();
    cell.blocks(blocks, depth);
    cell.lines.join(" ")
}

/// Une ligne : morceaux mis bout à bout, blancs repliés en une espace, bords retirés.
fn phrase(inlines: &[Inline]) -> String {
    let mut raw = String::new();
    for inline in inlines {
        match inline {
            Inline::Text(text) => raw.push_str(text),
            Inline::Link { text, id } => {
                let shown = text.trim_end();
                if shown.trim_start().is_empty() {
                    raw.push_str(id.as_str());
                } else {
                    // Les blancs autour du texte du lien séparent les mots voisins : ils restent.
                    raw.push_str(shown);
                    raw.push_str(" (");
                    raw.push_str(id.as_str());
                    raw.push(')');
                }
                raw.push_str(text.get(shown.len()..).unwrap_or_default());
            }
        }
    }
    flatten_whitespace(&raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn text(s: &str) -> Vec<Inline> {
        vec![Inline::Text(s.to_owned())]
    }

    fn para(s: &str) -> Block {
        Block::Paragraph(text(s))
    }

    fn link(s: &str, id: &str) -> Inline {
        Inline::Link { text: s.to_owned(), id: SheetId::parse(id).unwrap() }
    }

    fn rendered(blocks: &[Block]) -> String {
        render(blocks).unwrap().text.as_str().to_owned()
    }

    #[test]
    fn cases_are_announced_by_their_label() {
        let blocks = [
            Block::Labelled {
                label: Label::Situation,
                title: text("Vous êtes salarié"),
                blocks: vec![Block::Labelled {
                    label: Label::Case,
                    title: text(" Vous êtes\tlocataire "),
                    blocks: vec![para("Le loyer est payé.")],
                }],
            },
            Block::Labelled { label: Label::ToKnow, title: Vec::new(), blocks: vec![para("Un.")] },
            Block::Labelled { label: Label::Warning, title: text("Délai"), blocks: Vec::new() },
            Block::Labelled { label: Label::Note, title: text("  "), blocks: vec![para("Deux.")] },
            Block::Labelled { label: Label::Example, title: text("Calcul"), blocks: Vec::new() },
            Block::Labelled { label: Label::Reminder, title: Vec::new(), blocks: Vec::new() },
        ];
        assert_eq!(
            rendered(&blocks),
            "Situation : Vous êtes salarié\n\nCas : Vous êtes locataire\n\nLe loyer est payé.\n\n\
             À savoir\n\nUn.\n\nAttention : Délai\n\nÀ noter\n\nDeux.\n\nExemple : Calcul\n\nRappel"
        );
    }

    #[test]
    fn table_rows_render_one_per_paragraph() {
        let table = Block::Table(vec![
            vec![vec![para("Revenu")], vec![para("Montant")]],
            vec![vec![para("Moins de 10 000 €")], vec![para("635,71 €"), para("par mois")]],
            vec![vec![para(" ")], vec![]],
            vec![
                vec![para("Seul")],
                vec![],
                vec![Block::List(vec![vec![para("a")], vec![para("b")]])],
            ],
        ]);
        assert_eq!(
            rendered(&[table]),
            "Revenu | Montant\n\nMoins de 10 000 € | 635,71 € par mois\n\nSeul | | - a - b"
        );
    }

    #[test]
    fn list_items_start_with_a_dash() {
        let list = Block::List(vec![
            vec![para("Premier"), para("suite du premier")],
            vec![],
            vec![para(""), para("Deuxième")],
            vec![Block::List(vec![vec![para("imbriqué")]])],
        ]);
        assert_eq!(
            rendered(&[list]),
            "- Premier\n\nsuite du premier\n\n- Deuxième\n\n- - imbriqué"
        );
    }

    #[test]
    fn a_link_keeps_its_sheet_id() {
        let line = Block::Paragraph(vec![
            Inline::Text("Voir".to_owned()),
            link(" la fiche ", "f1234"),
            Inline::Text("ou ".to_owned()),
            link("", "N31675"),
            Inline::Text(", et ".to_owned()),
            link(" \n ", "F1"),
            link("titre", "F2"),
            Inline::Text(".".to_owned()),
        ]);
        assert_eq!(rendered(&[line]), "Voir la fiche (F1234) ou N31675, et F1 titre (F2).");
    }

    #[test]
    fn headings_render_as_lines_and_chapters_feed_the_summary() {
        let blocks = [
            Block::Heading { level: HeadingLevel::Chapter, text: text("Bénéficiaires") },
            Block::Heading { level: HeadingLevel::Section, text: text("Conditions") },
            Block::Heading { level: HeadingLevel::Floating, text: text("Exemple") },
            Block::Heading { level: HeadingLevel::Chapter, text: text(" \u{200b} ") },
            Block::Labelled {
                label: Label::Situation,
                title: Vec::new(),
                blocks: vec![Block::Heading {
                    level: HeadingLevel::Chapter,
                    text: text("Mon\u{200b}tant\n"),
                }],
            },
        ];
        let rendered = render(&blocks).unwrap();
        assert_eq!(
            rendered.text.as_str(),
            "Bénéficiaires\n\nConditions\n\nExemple\n\nSituation\n\nMontant"
        );
        assert_eq!(rendered.chapters, ["Bénéficiaires", "Montant"]);
    }

    #[test]
    fn nothing_renders_to_an_empty_text() {
        let rendered = render(&[]).unwrap();
        assert!(rendered.text.is_empty());
        assert_eq!(rendered.chapters, Vec::<String>::new());
        assert!(
            render(&[para(" "), Block::List(Vec::new()), Block::Table(Vec::new())])
                .unwrap()
                .text
                .is_empty()
        );
    }

    #[test]
    fn nesting_stops_at_the_depth_bound() {
        let case = |inner: Block| Block::Labelled {
            label: Label::Case,
            title: text("niveau"),
            blocks: vec![inner],
        };
        let mut deep = para("fond");
        for _ in 1..BLOCK_DEPTH_MAX {
            deep = case(deep);
        }
        // Un niveau de moins que la borne : tout est rendu.
        assert!(rendered(&[deep.clone()]).ends_with("fond"));
        let text = rendered(&[case(deep)]);
        assert_eq!(text.matches("Cas : niveau").count(), BLOCK_DEPTH_MAX);
        assert!(!text.contains("fond"));
        assert_eq!(flat(&[para("x")], BLOCK_DEPTH_MAX), "");
    }

    #[test]
    fn a_text_too_large_is_refused() {
        let huge = "a".repeat(usize::try_from(LongText::MAX_CHARS).unwrap());
        // Le texte, le séparateur et « b » : trois caractères de trop.
        assert_eq!(
            render(&[para(&huge), para("b")]),
            Err(DomainError::TextTooLarge { chars: 16_000_003 })
        );
    }

    #[test]
    fn labels_name_what_the_site_shows() {
        let all = [
            Label::Situation,
            Label::Case,
            Label::ToKnow,
            Label::Warning,
            Label::Note,
            Label::Example,
            Label::Reminder,
        ];
        assert_eq!(
            all.map(Label::as_str),
            ["Situation", "Cas", "À savoir", "Attention", "À noter", "Exemple", "Rappel"]
        );
    }

    fn inline() -> impl Strategy<Value = Inline> {
        prop_oneof![
            "\\PC{0,12}".prop_map(Inline::Text),
            ("\\PC{0,8}", "[FN][0-9]{1,7}")
                .prop_map(|(text, id)| Inline::Link { text, id: SheetId::parse(&id).unwrap() }),
        ]
    }

    fn block() -> impl Strategy<Value = Block> {
        let leaf = prop_oneof![
            prop::collection::vec(inline(), 0..4).prop_map(Block::Paragraph),
            (
                prop::sample::select(vec![
                    HeadingLevel::Chapter,
                    HeadingLevel::Section,
                    HeadingLevel::Floating,
                ]),
                prop::collection::vec(inline(), 0..3),
            )
                .prop_map(|(level, text)| Block::Heading { level, text }),
        ];
        leaf.prop_recursive(5, 48, 4, |inner| {
            prop_oneof![
                prop::collection::vec(prop::collection::vec(inner.clone(), 0..3), 0..3)
                    .prop_map(Block::List),
                prop::collection::vec(
                    prop::collection::vec(prop::collection::vec(inner.clone(), 0..2), 0..3),
                    0..3
                )
                .prop_map(Block::Table),
                (
                    prop::sample::select(vec![
                        Label::Situation,
                        Label::Case,
                        Label::ToKnow,
                        Label::Note
                    ]),
                    prop::collection::vec(inline(), 0..2),
                    prop::collection::vec(inner, 0..3),
                )
                    .prop_map(|(label, title, blocks)| Block::Labelled {
                        label,
                        title,
                        blocks
                    }),
            ]
        })
    }

    proptest! {
        /// Le rendu ne panique sur aucun arbre de blocs ; chaque paragraphe tient sur une ligne
        /// sans blanc de bord, et chaque chapitre est non vide.
        #[test]
        fn rendering_never_panics(blocks in prop::collection::vec(block(), 0..5)) {
            let rendered = render(&blocks).unwrap();
            for (_, paragraph) in rendered.text.paragraphs() {
                prop_assert!(!paragraph.is_empty() && !paragraph.contains('\n'));
                prop_assert_eq!(paragraph, paragraph.trim());
            }
            prop_assert!(rendered.chapters.iter().all(|title| !title.is_empty()));
        }
    }
}
