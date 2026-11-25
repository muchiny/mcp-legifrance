//! Tests du lecteur XML : arbre, entités, bornes, refus, et lecture partielle d'un en-tête.

use pretty_assertions::assert_eq;
use proptest::prelude::*;

use super::*;

const F1437: &[u8] = include_bytes!("../../tests/fixtures/xml/part/F1437.xml");

fn text(raw: &str) -> Child {
    Child::Text(raw.to_owned())
}

#[test]
fn text_and_elements_keep_their_order() {
    let root =
        parse(br#"<?xml version="1.0"?><P a=" x "> Le <b>SBI</b> est <i/>d&#xFB;. </P>"#).unwrap();
    assert_eq!(root.name, "P");
    assert_eq!(root.attr("a"), Some("x"));
    assert_eq!(root.attr("z"), None);
    assert_eq!(root.children.len(), 5);
    assert_eq!(root.children[0], text(" Le "));
    assert_eq!(root.child("b").unwrap().text(), "SBI");
    assert_eq!(root.children[2], text(" est "));
    assert_eq!(root.children[4], text("dû. "));
    assert_eq!(root.elements().map(|node| node.name.as_str()).collect::<Vec<_>>(), ["b", "i"]);
    assert_eq!(root.text(), " Le SBI est dû. ");
}

#[test]
fn predefined_entities_character_references_and_cdata_are_resolved() {
    let root =
        parse(br#"<a v="&lt;&#233;&gt;">&lt;&gt;&amp;&apos;&quot;&#x2014;<![CDATA[<x>]]></a>"#)
            .unwrap();
    assert_eq!(root.attr("v"), Some("<é>"));
    assert_eq!(root.text(), "<>&'\"—<x>");
    // Une valeur vide, ou de blancs seuls, est absente.
    assert_eq!(parse(br#"<a v=" "/>"#).unwrap().attr("v"), None);
    // Namespaces : le nom gardé est le nom qualifié.
    let root = parse(br#"<P xmlns:dc="u"><dc:title>T</dc:title></P>"#).unwrap();
    assert_eq!(root.child("dc:title").unwrap().text(), "T");
}

#[test]
fn doctype_unknown_entities_and_bad_bytes_are_refused() {
    let bomb = br#"<?xml version="1.0"?><!DOCTYPE a [<!ENTITY x "y">]><a>&x;</a>"#;
    assert_eq!(parse(bomb), Err(XmlError::DocType));
    assert_eq!(parse_head(bomb), Err(XmlError::DocType));
    assert_eq!(parse(b"<a>&nbsp;</a>"), Err(XmlError::Entity));
    assert_eq!(parse(b"<a>&#xD800;</a>"), Err(XmlError::Malformed));
    assert_eq!(parse(b"<a>\xFF</a>"), Err(XmlError::NotUtf8));
    // Un octet invalide avant la fin refuse aussi l'en-tête.
    assert_eq!(parse_head(b"<a>\xFF</a>"), Err(XmlError::NotUtf8));
    assert_eq!(parse(br#"<a b="1" b="2"/>"#), Err(XmlError::Malformed));
    assert_eq!(parse(br#"<a b="&nbsp;"/>"#), Err(XmlError::Malformed));
}

#[test]
fn a_document_must_have_one_root_and_be_closed() {
    assert_eq!(parse(b""), Err(XmlError::NoRoot));
    assert_eq!(parse(b"<?xml version=\"1.0\"?>\n<!-- c --><?pi x?>"), Err(XmlError::NoRoot));
    assert_eq!(parse(b"<a/><b/>"), Err(XmlError::TrailingContent));
    assert_eq!(parse(b"<a/><b></b>"), Err(XmlError::TrailingContent));
    assert_eq!(parse(b"<a><b></a>"), Err(XmlError::Malformed));
    assert_eq!(parse(b"<a><b>"), Err(XmlError::Malformed));
    assert_eq!(parse(b"<a><b x"), Err(XmlError::Malformed));
    // Le texte hors de tout élément est perdu.
    assert_eq!(parse(b"avant<a>x</a>").unwrap().text(), "x");
}

#[test]
fn depth_and_element_count_are_bounded() {
    let nested =
        |depth: usize| format!("{}{}", "<a>".repeat(depth), "</a>".repeat(depth)).into_bytes();
    assert!(parse(&nested(XML_DEPTH_MAX)).is_ok());
    assert_eq!(parse(&nested(XML_DEPTH_MAX + 1)), Err(XmlError::TooDeep));
    let flat = |count: usize| format!("<r>{}</r>", "<a/>".repeat(count)).into_bytes();
    assert!(parse(&flat(XML_ELEMENTS_MAX - 1)).is_ok());
    assert_eq!(parse(&flat(XML_ELEMENTS_MAX)), Err(XmlError::TooManyElements));
}

#[test]
fn the_text_of_a_deep_tree_stops_at_the_depth_bound() {
    let mut node = Node { name: "a".to_owned(), attrs: Vec::new(), children: vec![text("x")] };
    for _ in 0..XML_DEPTH_MAX {
        node =
            Node { name: "a".to_owned(), attrs: Vec::new(), children: vec![Child::Element(node)] };
    }
    assert_eq!(node.text(), "");
    assert_eq!(Node::default().text(), "");
}

#[test]
fn a_head_cut_anywhere_keeps_what_was_read() {
    // Coupé dans une balise, dans un texte, au milieu d'un caractère.
    let root = parse_head(b"<P><t>Titre</t><d>R\xC3\xA9sum").unwrap();
    assert_eq!(root.child("t").unwrap().text(), "Titre");
    assert_eq!(root.child("d").unwrap().text(), "Résum");
    let root = parse_head(b"<P><t>\xC3").unwrap();
    assert_eq!(root.child("t").unwrap().text(), "");
    let root = parse_head(b"<P><t>Titre</t><Texte att").unwrap();
    assert_eq!(root.elements().count(), 1);
    // Un second élément racine reste refusé.
    assert_eq!(parse_head(b"<a/><b>"), Err(XmlError::TrailingContent));
    assert_eq!(parse_head(b"<a"), Err(XmlError::NoRoot));
    // Une entrée entière se lit pareil en mode partiel.
    assert_eq!(parse_head(F1437), parse(F1437));
}

#[test]
fn a_real_sheet_reads_whole() {
    let root = parse(F1437).unwrap();
    assert_eq!(root.name, "Publication");
    assert_eq!(root.attr("ID"), Some("F1437"));
    assert_eq!(root.child("dc:title").unwrap().text(), "Solde bancaire insaisissable (SBI)");
    let head = parse_head(&F1437[..2_000]).unwrap();
    assert_eq!(head.child("dc:title").unwrap().text(), "Solde bancaire insaisissable (SBI)");
    assert!(head.child("Texte").is_none());
}

#[test]
fn errors_name_the_refusal_without_any_text_read() {
    let texts: Vec<String> = [
        XmlError::NotUtf8,
        XmlError::Malformed,
        XmlError::DocType,
        XmlError::Entity,
        XmlError::TooDeep,
        XmlError::TooManyElements,
        XmlError::TrailingContent,
        XmlError::NoRoot,
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    assert_eq!(
        texts,
        [
            "entry is not valid UTF-8",
            "entry is not well-formed XML",
            "DOCTYPE and entity declarations are forbidden",
            "unknown entity reference",
            "element nesting is too deep",
            "too many elements",
            "content after the root element",
            "no root element",
        ]
    );
}

proptest! {
    /// Des octets quelconques, ou une vraie fiche coupée n'importe où, ne font jamais paniquer.
    #[test]
    fn any_bytes_or_cut_are_read_or_refused_without_panic(
        bytes in proptest::collection::vec(any::<u8>(), 0..256),
        cut in 0..F1437.len(),
    ) {
        let _ = parse(&bytes);
        let _ = parse_head(&bytes);
        // Toute coupure après la balise ouvrante de la racine rend un arbre.
        let first_child = std::str::from_utf8(F1437).unwrap().find("<dc:title>").unwrap();
        let head = parse_head(&F1437[..cut]);
        prop_assert!(head.is_ok() || cut < first_child);
    }
}
