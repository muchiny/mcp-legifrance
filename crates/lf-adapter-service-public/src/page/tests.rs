//! Tests de la traduction en valeurs du domaine : fixtures réelles de T-MS-02 (attendus relevés
//! dans le XML par un script indépendant, `python3 -I`), puis petits documents écrits ici, une
//! règle à la fois.

use lf_domain::{guidance::LegalReference, text::LongText};
use pretty_assertions::assert_eq;

use super::*;
use crate::xml::{parse, parse_head};

const F1437: &[u8] = include_bytes!("../../tests/fixtures/xml/part/F1437.xml");
const F1239: &[u8] = include_bytes!("../../tests/fixtures/xml/part/F1239.xml");
const F2408: &[u8] = include_bytes!("../../tests/fixtures/xml/part/F2408.xml");
const N31675: &[u8] = include_bytes!("../../tests/fixtures/xml/part/N31675.xml");
const N19804: &[u8] = include_bytes!("../../tests/fixtures/xml/part/N19804.xml");
const MENU: &[u8] = include_bytes!("../../tests/fixtures/xml/part/menu.xml");

fn id(raw: &str) -> SheetId {
    SheetId::parse(raw).unwrap()
}

fn real_menu() -> GuidanceMenu {
    GuidanceMenu::build(menu(&parse(MENU).unwrap()))
}

fn read(bytes: &[u8], raw: &str) -> GuidanceDoc {
    page(&parse(bytes).unwrap(), Audience::Particuliers, &id(raw), &real_menu()).unwrap()
}

fn text_of(doc: &GuidanceDoc) -> String {
    doc.text().unwrap().as_str().to_owned()
}

/// Entrées d'un dossier ou d'un thème ; aucune pour une fiche.
fn entries(doc: &GuidanceDoc) -> &[NavigationEntry] {
    match &doc.content {
        GuidanceContent::Folder(entries) | GuidanceContent::Theme(entries) => entries,
        GuidanceContent::Sheet { .. } => &[],
    }
}

/// Titres des chapitres d'une fiche ; aucun pour un dossier ou un thème.
fn chapters(doc: &GuidanceDoc) -> &[String] {
    match &doc.content {
        GuidanceContent::Sheet { text, .. } => &text.chapters,
        GuidanceContent::Folder(_) | GuidanceContent::Theme(_) => &[],
    }
}

/// Une petite fiche : `body` est placé sous `Publication`, après les métadonnées.
fn sheet(body: &str) -> GuidanceDoc {
    let xml = format!(
        r#"<Publication ID="F1" type="Fiche"><dc:title>Titre</dc:title>{body}</Publication>"#
    );
    page(
        &parse(xml.as_bytes()).unwrap(),
        Audience::Particuliers,
        &id("F1"),
        &GuidanceMenu::default(),
    )
    .unwrap()
}

fn blocks_of(body: &str) -> Vec<Block> {
    let xml = format!(r#"<Publication ID="F1"><dc:title>T</dc:title>{body}</Publication>"#);
    super::body(&parse(xml.as_bytes()).unwrap())
}

fn words(raw: &str) -> Vec<Inline> {
    vec![Inline::Text(raw.to_owned())]
}

#[test]
fn a_real_sheet_becomes_its_page() {
    let doc = read(F1437, "F1437");
    assert_eq!(doc.id, id("F1437"));
    assert_eq!(doc.audience, Audience::Particuliers);
    assert_eq!(doc.kind(), GuidanceKind::Sheet);
    assert_eq!(doc.sheet_type.as_deref(), Some("Fiche d'information conditionnée"));
    assert_eq!(doc.title, "Solde bancaire insaisissable (SBI)");
    assert!(doc.description.as_deref().unwrap().starts_with("En cas de saisie sur un compte"));
    assert_eq!(doc.theme.as_deref(), Some("Justice"));
    assert_eq!(doc.updated, CivilDate::new(2026, 4, 1));
    assert_eq!(
        doc.source_url.as_str(),
        "https://www.service-public.gouv.fr/particuliers/vosdroits/F1437"
    );
    assert_eq!(doc.path, ["Justice", "Réparation du préjudice", "Saisies et recouvrements"]);
    let text = text_of(&doc);
    assert!(text.starts_with("Lorsque votre compte bancaire fait l'objet d'une saisie"), "{text}");
    let lines: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
    for expected in [
        "Situation : Moins de 651,69 €",
        "Situation : Plus de 651,69 €",
        "Cas : Compte bancaire avec sommes insaisissables",
        "Cas : Sommes insaisissables inférieures à 651,69 €",
        "Cas : Sommes insaisissables supérieures à 651,69 €",
        "Cas : Autre cas",
        "Exemple",
    ] {
        assert!(lines.contains(&expected), "{expected}");
    }
    assert!(text.contains("sommes insaisissables (F31185)"));
    // Ni renvois, ni contacts, ni titres de références dans le texte.
    assert!(!text.contains("Code des procédures civiles"));
    assert_eq!(chapters(&doc), Vec::<String>::new());
    assert_eq!(entries(&doc), []);
    // `dc:source` d'abord, puis les `Reference` qui donnent les titres des mêmes textes.
    let references = doc.legal_references().unwrap().references();
    let ids: Vec<Option<&str>> = references.iter().map(LegalReference::id).collect();
    assert_eq!(ids, [None, Some("LEGIARTI000025938454"), None]);
    let titles: Vec<&str> = references.iter().map(|r| r.title().unwrap()).collect();
    assert_eq!(titles[0], "Code des procédures civiles de l'exécution : articles L162-1 à L162-2");
    assert_eq!(titles[1], "Code des procédures civiles d'exécution : article R162-7");
    assert!(titles[2].starts_with("Décret n°2025-293 du 29 mars 2025"));
}

#[test]
fn the_header_of_a_sheet_is_its_metadata_for_the_index() {
    let menu = real_menu();
    let whole = page(&parse(F1437).unwrap(), Audience::Particuliers, &id("F1437"), &menu).unwrap();
    // `DossierPere` est dans les 6 premiers Ko ; l'en-tête est coupé là.
    let head = parse_head(&F1437[..6_000]).unwrap();
    let indexed = header(&head, Audience::Particuliers, &id("F1437"), &menu).unwrap();
    assert_eq!(
        indexed,
        IndexedPage {
            id: whole.id,
            kind: GuidanceKind::Sheet,
            title: whole.title,
            path: whole.path,
            summary: whole.description.unwrap(),
            updated: whole.updated,
            source_url: whole.source_url,
        }
    );
    // Sans description, le résumé est vide.
    let bare = parse(br#"<Publication ID="F1"><dc:title>T</dc:title></Publication>"#).unwrap();
    let indexed = header(&bare, Audience::Entreprendre, &id("F1"), &menu).unwrap();
    assert_eq!((indexed.summary.as_str(), indexed.path.len()), ("", 0));
    assert_eq!(
        indexed.source_url.as_str(),
        "https://entreprendre.service-public.gouv.fr/vosdroits/F1"
    );
}

#[test]
fn a_box_loses_a_title_that_repeats_its_label() {
    let text = text_of(&read(F1239, "F1239"));
    let lines: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
    let at = lines.iter().position(|line| *line == "À savoir").unwrap();
    assert!(lines[at + 1].starts_with("Pour faciliter les échanges de logement"));
    assert!(!text.contains("À savoir : À savoir"));
    // Un avertissement garde son titre propre.
    let doc = sheet(
        "<Avertissement><Titre>Réforme</Titre><Texte><Paragraphe>Depuis 2025.</Paragraphe></Texte></Avertissement>",
    );
    assert_eq!(text_of(&doc), "Attention : Réforme\n\nDepuis 2025.");
    // Un titre fait d'un lien se compare sur le texte du lien, sans son numéro.
    let doc = sheet(
        "<Texte><ANoter><Titre><LienInterne LienPublication=\"F2\">À noter</LienInterne></Titre><Paragraphe>A.</Paragraphe></ANoter>\
         <Cas><TitreRiche>Voir <LienInterne LienPublication=\"F3\">la fiche</LienInterne></TitreRiche></Cas></Texte>",
    );
    assert_eq!(text_of(&doc), "À noter\n\nA.\n\nCas : Voir la fiche (F3)");
}

#[test]
fn a_table_is_read_row_by_row() {
    let text = text_of(&read(F2408, "F2408"));
    assert!(text.contains("Différences entre frais professionnels et avantages en nature"));
    assert!(text.contains(
        "Frais ou avantage | Intégré à la rémunération ? | Soumis aux cotisations sociales, CSG et CRDS"
    ));
}

#[test]
fn a_folder_lists_its_sheets_and_a_theme_its_folders_by_group() {
    let folder = read(N31675, "N31675");
    assert_eq!(folder.kind(), GuidanceKind::Folder);
    // Le flux écrit `type="Dossier"` : il répète la nature, `sheet_type` ne le rend pas.
    let marker = br#"type="Dossier""#;
    assert!(N31675.windows(marker.len()).any(|bytes| bytes == marker));
    assert_eq!(folder.sheet_type, None);
    assert_eq!(chapters(&folder), Vec::<String>::new());
    let listed = entries(&folder);
    assert_eq!(listed.len(), 3);
    assert_eq!(
        listed[0],
        NavigationEntry {
            id: id("F2384"),
            kind: GuidanceKind::Sheet,
            title: "Virement bancaire".to_owned(),
            group: None,
        }
    );
    let theme = read(N19804, "N19804");
    assert_eq!(theme.kind(), GuidanceKind::Theme);
    assert_eq!(theme.sheet_type, None);
    // Un thème est à la racine du menu.
    assert_eq!(theme.path, Vec::<String>::new());
    let listed = entries(&theme);
    assert_eq!(
        listed[0],
        NavigationEntry {
            id: id("N105"),
            kind: GuidanceKind::Folder,
            title: "Entrée en France".to_owned(),
            group: Some("Étranger en France".to_owned()),
        }
    );
    assert_eq!(listed.last().unwrap().group.as_deref(), Some("Français à l'étranger"));
}

#[test]
fn navigation_entries_skip_bad_numbers_and_take_their_titles() {
    let xml = r#"<Publication ID="N1" type="Dossier"><dc:title>D</dc:title>
        <SousDossier ID="N1-A"><Fiche ID="F2">Sans groupe titré</Fiche><Fiche>Sans numéro</Fiche></SousDossier>
        <Fiche ID="X9">Numéro invalide</Fiche>
        <Fiche ID="F3"><Titre> Titre  propre </Titre></Fiche>
        <Dossier ID="N4">Hors de propos dans un dossier</Dossier>
    </Publication>"#;
    let doc = page(
        &parse(xml.as_bytes()).unwrap(),
        Audience::Particuliers,
        &id("N1"),
        &GuidanceMenu::default(),
    )
    .unwrap();
    assert_eq!(doc.kind(), GuidanceKind::Folder);
    let listed: Vec<(&str, &str, Option<&str>)> = entries(&doc)
        .iter()
        .map(|entry| (entry.id.as_str(), entry.title.as_str(), entry.group.as_deref()))
        .collect();
    assert_eq!(listed, [("F2", "Sans groupe titré", None), ("F3", "Titre propre", None)]);
}

#[test]
fn a_page_that_is_not_the_requested_publication_is_refused() {
    let menu = GuidanceMenu::default();
    let refused = |xml: &[u8], raw: &str| {
        page(&parse(xml).unwrap(), Audience::Particuliers, &id(raw), &menu).unwrap_err()
    };
    assert_eq!(refused(MENU, "N1"), PageError::NotPublication);
    assert_eq!(refused(F1437, "F1438"), PageError::WrongId);
    assert_eq!(
        refused(br"<Publication><dc:title>T</dc:title></Publication>", "F1"),
        PageError::WrongId
    );
    assert_eq!(
        refused(br#"<Publication ID="Z1"><dc:title>T</dc:title></Publication>"#, "F1"),
        PageError::WrongId
    );
    assert_eq!(
        refused(br#"<Publication ID="F1"><dc:title> </dc:title></Publication>"#, "F1"),
        PageError::NoTitle
    );
    assert_eq!(refused(br#"<Publication ID="F1"/>"#, "F1"), PageError::NoTitle);
    let head = parse(b"<Menu/>").unwrap();
    assert_eq!(
        header(&head, Audience::Particuliers, &id("F1"), &menu),
        Err(PageError::NotPublication)
    );
}

#[test]
fn a_text_beyond_the_long_text_bound_is_refused() {
    let mut root = parse(br#"<Publication ID="F1"><dc:title>T</dc:title></Publication>"#).unwrap();
    let mut texte = parse(b"<Texte/>").unwrap();
    let size = usize::try_from(LongText::MAX_CHARS).unwrap() + 1;
    texte.children.push(Child::Text("a".repeat(size)));
    root.children.push(Child::Element(texte));
    let error = page(&root, Audience::Particuliers, &id("F1"), &GuidanceMenu::default());
    assert_eq!(error, Err(PageError::TooLong));
}

#[test]
fn metadata_tolerates_odd_values() {
    let doc = sheet("");
    assert_eq!(
        (doc.sheet_type.as_deref(), doc.description, doc.theme),
        (Some("Fiche"), None, None)
    );
    let xml = br#"<Publication ID="F1" type=" " dateDerniereModificationImportante="2026-13-01T00:00:00" spUrl="https://ailleurs.example/F1"><dc:title>T</dc:title><DossierPere ID="bad"/><Theme ID="N1"/></Publication>"#;
    let doc = page(&parse(xml).unwrap(), Audience::Particuliers, &id("F1"), &real_menu()).unwrap();
    assert_eq!((doc.sheet_type, doc.updated, doc.theme), (None, None, None));
    assert_eq!(
        doc.source_url.as_str(),
        "https://www.service-public.gouv.fr/particuliers/vosdroits/F1"
    );
    assert_eq!(doc.path, Vec::<String>::new());
    let short = br#"<Publication ID="F1" dateDerniereModificationImportante="2026"><dc:title>T</dc:title></Publication>"#;
    let doc =
        page(&parse(short).unwrap(), Audience::Particuliers, &id("F1"), &real_menu()).unwrap();
    assert_eq!(doc.updated, None);
}

#[test]
fn chapters_sections_lists_and_floating_titles_become_blocks() {
    let blocks = blocks_of(
        "<Texte><Chapitre><Titre><Paragraphe>Montant</Paragraphe><Paragraphe>annuel</Paragraphe></Titre>\
         <TitreAlternatif>court</TitreAlternatif>\
         <SousChapitre><Titre><Paragraphe>Calcul</Paragraphe></Titre><Paragraphe>Texte.</Paragraphe></SousChapitre>\
         <TitreFlottant><Paragraphe>Plafond</Paragraphe></TitreFlottant>\
         <Liste type=\"puce\"><Item><Paragraphe>un</Paragraphe></Item><Item><Paragraphe>deux</Paragraphe></Item></Liste>\
         </Chapitre><Chapitre><Paragraphe>Sans titre.</Paragraphe></Chapitre></Texte>",
    );
    let joined = |parts: &[&str]| {
        parts.iter().map(|part| Inline::Text((*part).to_owned())).collect::<Vec<_>>()
    };
    assert_eq!(
        blocks,
        [
            Block::Heading {
                level: HeadingLevel::Chapter,
                text: joined(&["Montant", " ", "annuel"])
            },
            Block::Heading { level: HeadingLevel::Section, text: words("Calcul") },
            Block::Paragraph(words("Texte.")),
            Block::Heading { level: HeadingLevel::Floating, text: words("Plafond") },
            Block::List(vec![
                vec![Block::Paragraph(words("un"))],
                vec![Block::Paragraph(words("deux"))]
            ]),
            Block::Paragraph(words("Sans titre.")),
        ]
    );
}

/// Un titre posé comme un bloc forme un seul paragraphe, ses paragraphes joints ; une image ne
/// donne aucun texte, même quand elle en porte.
#[test]
fn a_block_title_is_one_paragraph_and_an_image_gives_no_text() {
    let blocks = blocks_of(
        "<Texte><Titre><Paragraphe>Montant</Paragraphe><Paragraphe>annuel</Paragraphe></Titre>\
         <Paragraphe>Avant <Image LienPublication=\"R2\">légende</Image>après</Paragraphe></Texte>",
    );
    assert_eq!(
        blocks,
        [
            Block::Paragraph(vec![
                Inline::Text("Montant".to_owned()),
                Inline::Text(" ".to_owned()),
                Inline::Text("annuel".to_owned()),
            ]),
            Block::Paragraph(vec![
                Inline::Text("Avant ".to_owned()),
                Inline::Text("après".to_owned()),
            ]),
        ]
    );
}

#[test]
fn inline_elements_stay_in_their_line_and_links_keep_their_number() {
    let blocks = blocks_of(
        "<Texte><Paragraphe>Le <MiseEnEvidence>1<Exposant>er</Exposant> janvier</MiseEnEvidence>, \
         voir <LienInterne LienPublication=\"F31185\">les sommes</LienInterne>, \
         <LienInterne LienPublication=\"R1\">une ressource</LienInterne>\
         <Image LienPublication=\"R2\"/>.</Paragraphe></Texte>",
    );
    assert_eq!(
        blocks,
        [Block::Paragraph(vec![
            Inline::Text("Le ".to_owned()),
            Inline::Text("1".to_owned()),
            Inline::Text("er".to_owned()),
            Inline::Text(" janvier".to_owned()),
            Inline::Text(", voir ".to_owned()),
            Inline::Link { text: "les sommes".to_owned(), id: id("F31185") },
            Inline::Text(", ".to_owned()),
            Inline::Text("une ressource".to_owned()),
            Inline::Text(".".to_owned()),
        ])]
    );
}

#[test]
fn cases_situations_and_boxes_are_labelled() {
    let blocks = blocks_of(
        "<ListeSituations><Situation><TitreRiche><Paragraphe>Vous êtes salarié</Paragraphe></TitreRiche>\
         <Texte><BlocCas><Cas><TitreRiche>Moins de 25 ans</TitreRiche><Paragraphe>A.</Paragraphe></Cas></BlocCas>\
         <ANoter><Titre><Paragraphe>À noter</Paragraphe></Titre><Paragraphe>B.</Paragraphe></ANoter>\
         <Exemple><Paragraphe>C.</Paragraphe></Exemple><Rappel><Titre>Rappel</Titre></Rappel>\
         <ASavoir><Paragraphe>D.</Paragraphe></ASavoir><Attention><Paragraphe>E.</Paragraphe></Attention>\
         </Texte></Situation></ListeSituations>",
    );
    let boxed = |label, title: Vec<Inline>, blocks| Block::Labelled { label, title, blocks };
    let para = |raw: &str| vec![Block::Paragraph(words(raw))];
    assert_eq!(
        blocks,
        [boxed(
            Label::Situation,
            words("Vous êtes salarié"),
            vec![
                boxed(Label::Case, words("Moins de 25 ans"), para("A.")),
                boxed(Label::Note, Vec::new(), para("B.")),
                boxed(Label::Example, Vec::new(), para("C.")),
                boxed(Label::Reminder, Vec::new(), Vec::new()),
                boxed(Label::ToKnow, Vec::new(), para("D.")),
                boxed(Label::Warning, Vec::new(), para("E.")),
            ],
        )]
    );
}

#[test]
fn skipped_elements_leave_the_text_and_unknown_ones_keep_theirs() {
    let blocks = blocks_of(
        "<Introduction><Texte><Paragraphe>Intro.</Paragraphe></Texte></Introduction>\
         <Texte>\n  <Definition><Titre>Terme</Titre></Definition>\
         <Reference URL=\"https://www.legifrance.gouv.fr/jorf/id/JORFTEXT000051393086\"><Titre>Décret</Titre></Reference>\
         <Condition><estVrai var=\"x\"/></Condition>\
         <NouvelElement>Texte gardé <LienInterne LienPublication=\"F2\">lien</LienInterne><Paragraphe>Suite.</Paragraphe></NouvelElement>\
         <Tableau><Rangée><Cellule><Paragraphe>a</Paragraphe></Cellule><Cellule><Paragraphe>b</Paragraphe></Cellule></Rangée></Tableau>\
         <Titre><Paragraphe>Titre libre</Paragraphe></Titre></Texte>\
         <VoirAussi><Fiche ID=\"F9\">Renvoi</Fiche></VoirAussi>",
    );
    assert_eq!(
        blocks,
        [
            Block::Paragraph(words("Intro.")),
            Block::Paragraph(vec![
                Inline::Text("Texte gardé ".to_owned()),
                Inline::Link { text: "lien".to_owned(), id: id("F2") },
            ]),
            Block::Paragraph(words("Suite.")),
            Block::Table(vec![vec![
                vec![Block::Paragraph(words("a"))],
                vec![Block::Paragraph(words("b"))],
            ]]),
            Block::Paragraph(words("Titre libre")),
        ]
    );
}

#[test]
fn references_come_from_reference_elements_and_dc_source_in_order() {
    let doc = sheet(
        "<dc:source>https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288, https://ailleurs.example/x</dc:source>\
         <Texte><Chapitre><Reference URL=\"https://www.legifrance.gouv.fr/jorf/id/JORFTEXT000051393086\"><Titre>Décret</Titre></Reference></Chapitre></Texte>\
         <Reference><Titre>Sans adresse</Titre></Reference>\
         <Reference URL=\"https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288\"/>",
    );
    let references = doc.legal_references().unwrap().references();
    let found: Vec<(Option<&str>, Option<&str>)> =
        references.iter().map(|r| (r.id(), r.title())).collect();
    assert_eq!(found, [(Some("LEGIARTI000006419288"), None), (None, Some("Décret"))]);
}

#[test]
fn the_menu_is_read_with_its_depths() {
    let entries = menu(&parse(MENU).unwrap());
    assert_eq!(entries.len(), 304);
    assert_eq!(
        entries[..2],
        [
            MenuEntry {
                depth: 1,
                id: "N19810".to_owned(),
                title: "Papiers - Citoyenneté - Élections".to_owned()
            },
            MenuEntry { depth: 2, id: "N31785".to_owned(), title: "État civil".to_owned() },
        ]
    );
    assert_eq!(entries.iter().map(|entry| entry.depth).max(), Some(3));
    assert_eq!(menu(&parse(F1437).unwrap()), Vec::new());
    let odd = menu(&parse(b"<Menu><ItemMenu><ItemMenu ID=\"N2\"/></ItemMenu></Menu>").unwrap());
    assert_eq!(
        odd,
        [
            MenuEntry { depth: 1, id: String::new(), title: String::new() },
            MenuEntry { depth: 2, id: "N2".to_owned(), title: String::new() },
        ]
    );
}

#[test]
fn every_fixture_page_is_read() {
    let part: [(&[u8], &str); 11] = [
        (include_bytes!("../../tests/fixtures/xml/part/F10041.xml"), "F10041"),
        (F1239, "F1239"),
        (F1437, "F1437"),
        (include_bytes!("../../tests/fixtures/xml/part/F1903.xml"), "F1903"),
        (F2408, "F2408"),
        (include_bytes!("../../tests/fixtures/xml/part/F31613.xml"), "F31613"),
        (include_bytes!("../../tests/fixtures/xml/part/F34680.xml"), "F34680"),
        (include_bytes!("../../tests/fixtures/xml/part/F468.xml"), "F468"),
        (N19804, "N19804"),
        (N31675, "N31675"),
        (include_bytes!("../../tests/fixtures/xml/pro/F37954.xml"), "F37954"),
    ];
    let menu = real_menu();
    for (bytes, raw) in part {
        let doc = page(&parse(bytes).unwrap(), Audience::Particuliers, &id(raw), &menu).unwrap();
        let readable = match &doc.content {
            GuidanceContent::Sheet { text, .. } => !text.text.as_str().is_empty(),
            GuidanceContent::Folder(entries) | GuidanceContent::Theme(entries) => {
                !entries.is_empty()
            }
        };
        assert!(readable, "{raw}");
    }
}

#[test]
fn errors_name_the_refusal_without_any_text_read() {
    let texts: Vec<String> =
        [PageError::NotPublication, PageError::WrongId, PageError::NoTitle, PageError::TooLong]
            .iter()
            .map(ToString::to_string)
            .collect();
    assert_eq!(
        texts,
        [
            "entry is not a publication",
            "publication id is missing, invalid or not the entry's",
            "publication has no title",
            "sheet text is too long",
        ]
    );
}
