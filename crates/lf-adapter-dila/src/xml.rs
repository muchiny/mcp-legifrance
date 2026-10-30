//! Analyse des XML LEGI avec `quick-xml` (rapport 10 §7 et §10).
//!
//! Chaque membre est lu en entier (16 Mio au plus, le maximum observé est de 2,9 Mo), analysé en un
//! petit arbre borné (profondeur et nombre d'éléments), puis converti en [`Document`]. Les
//! éléments `CONTENU` ne sont pas descendus : leur contenu est gardé tel quel (HTML brut, entités
//! et références de caractères comprises).
//!
//! Sécurité (rapport 10 §10.5) : tout `DOCTYPE` et toute déclaration d'entité sont refusés
//! (aucun des 2,5 millions de fichiers du global n'en a), seules les cinq entités prédéfinies et
//! les références de caractères sont résolues, et les erreurs ne citent jamais le contenu lu.
//!
//! Tolérance : `ETAT`, `NATURE`, `TYPE` et `typelien` inconnus sont gardés bruts ; un champ vide
//! est un champ absent ; les nœuds de texte faits d'espaces sont ignorés (les lignes vides sont
//! partout dans les métadonnées). Une date non vide qui n'en est pas une refuse le document :
//! elle gouverne la fenêtre de vigueur et ne doit pas disparaître en silence.

use lf_domain::date::CivilDate;
use quick_xml::{
    Reader, XmlVersion,
    events::{BytesRef, BytesStart, Event},
};
use thiserror::Error;

use crate::{
    model::{
        Article, Chronicle, Document, Link, LinkTarget, Section, SectionRef, StructLink,
        TextContext, TextStruct, TextVersion, TextVersionLink,
    },
    path::DocKind,
};

/// Profondeur maximale des éléments (hors `CONTENU`) ; les plus profonds du global en ont une dizaine.
pub const MAX_DEPTH: usize = 64;

/// Nombre maximal d'éléments d'un membre (un `TEXTELR` de code en compte quelques milliers).
pub const MAX_NODES: usize = 250_000;

/// Élément dont le contenu est gardé brut.
const RAW_ELEMENT: &str = "CONTENU";

/// Motif de refus d'un membre XML. Aucun ne porte de texte lu dans le membre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum XmlError {
    /// Le membre n'est pas de l'UTF-8 (V-188 : tous les fichiers LEGI le sont).
    #[error("member is not valid UTF-8")]
    NotUtf8,
    /// Le XML est mal formé.
    #[error("member is not well-formed XML")]
    Malformed,
    /// `DOCTYPE` ou déclaration d'entité.
    #[error("DOCTYPE and entity declarations are forbidden")]
    DocType,
    /// Référence d'entité autre que les cinq prédéfinies.
    #[error("unknown entity reference")]
    Entity,
    /// Éléments imbriqués au-delà de [`MAX_DEPTH`].
    #[error("element nesting is too deep")]
    TooDeep,
    /// Plus de [`MAX_NODES`] éléments.
    #[error("too many elements")]
    TooManyNodes,
    /// Un second élément racine suit le premier.
    #[error("content after the root element")]
    TrailingContent,
    /// Aucun élément racine.
    #[error("no root element")]
    NoRoot,
    /// La racine n'est pas celle que le dossier annonce.
    #[error("unexpected root element")]
    UnexpectedRoot,
    /// Un champ obligatoire manque (le nom est celui de la balise ou de l'attribut).
    #[error("missing field {0}")]
    MissingField(&'static str),
    /// Une date non vide n'est pas une date `AAAA-MM-JJ` valide.
    #[error("invalid date in {0}")]
    BadDate(&'static str),
}

/// Élément de l'arbre : nom, attributs, texte direct, contenu brut et enfants.
#[derive(Debug)]
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    text: String,
    raw: Option<String>,
    children: Vec<Node>,
}

impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    fn at(&self, path: &[&str]) -> Option<&Node> {
        path.iter().try_fold(self, |node, name| node.child(name))
    }

    fn each<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// Valeur d'un attribut, `None` s'il manque ou s'il est vide.
    fn attr(&self, name: &str) -> Option<&str> {
        let value = self.attrs.iter().find(|(key, _)| key == name)?.1.trim();
        (!value.is_empty()).then_some(value)
    }

    fn attr_owned(&self, name: &str) -> Option<String> {
        self.attr(name).map(str::to_owned)
    }

    fn attr_date(&self, name: &'static str) -> Result<Option<CivilDate>, XmlError> {
        date_of(self.attr(name), name)
    }

    /// Texte direct sans espaces de bord, `None` s'il est vide.
    fn value(&self) -> Option<String> {
        let text = self.text.trim();
        (!text.is_empty()).then(|| text.to_owned())
    }

    /// Contenu brut de l'élément `CONTENU` sous `path`, vide s'il manque.
    fn raw_at(&self, path: &[&str]) -> String {
        self.at(path).and_then(|n| n.raw.clone()).unwrap_or_default()
    }
}

/// Texte d'un enfant `name` de `node`, `None` s'il manque ou s'il est vide.
fn field(node: Option<&Node>, name: &str) -> Option<String> {
    node?.child(name)?.value()
}

/// Date d'un enfant `name` de `node` ; `label` nomme le champ dans l'erreur.
fn date_field(
    node: Option<&Node>,
    name: &str,
    label: &'static str,
) -> Result<Option<CivilDate>, XmlError> {
    date_of(node.and_then(|n| n.child(name)).map(|n| n.text.as_str()), label)
}

/// Lit une date `AAAA-MM-JJ` ; vide ou absente donne `None`.
fn date_of(text: Option<&str>, label: &'static str) -> Result<Option<CivilDate>, XmlError> {
    match text.map(str::trim).filter(|t| !t.is_empty()) {
        None => Ok(None),
        Some(text) => CivilDate::parse(text).map(Some).map_err(|_| XmlError::BadDate(label)),
    }
}

/// Ouvre un élément : compte, lit le nom et les attributs (valeurs normalisées).
fn open(start: &BytesStart<'_>, count: &mut usize) -> Result<Node, XmlError> {
    *count = count.saturating_add(1);
    if *count > MAX_NODES {
        return Err(XmlError::TooManyNodes);
    }
    let mut attrs = Vec::new();
    for attr in start.attributes() {
        let attr = attr.map_err(|_| XmlError::Malformed)?;
        let value =
            attr.normalized_value(XmlVersion::Implicit1_0).map_err(|_| XmlError::Malformed)?;
        let key: &str = attr.key.as_ref();
        attrs.push((key.to_owned(), value.into_owned()));
    }
    let qname = start.name();
    let name: &str = qname.as_ref();
    Ok(Node { name: name.to_owned(), attrs, text: String::new(), raw: None, children: Vec::new() })
}

/// Rattache un élément clos à son parent, ou en fait la racine.
fn attach(stack: &mut [Node], root: &mut Option<Node>, node: Node) -> Result<(), XmlError> {
    match stack.last_mut() {
        Some(parent) => parent.children.push(node),
        None if root.is_none() => *root = Some(node),
        None => return Err(XmlError::TrailingContent),
    }
    Ok(())
}

/// Ajoute au texte d'un élément une référence de caractère ou l'une des cinq entités prédéfinies.
fn push_ref(out: &mut String, reference: &BytesRef<'_>) -> Result<(), XmlError> {
    let resolved = match reference.resolve_char_ref() {
        Ok(Some(c)) => c,
        Ok(None) => match &**reference {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "apos" => '\'',
            "quot" => '"',
            _ => return Err(XmlError::Entity),
        },
        Err(_) => return Err(XmlError::Malformed),
    };
    out.push(resolved);
    Ok(())
}

/// Lit jusqu'à la fin de `CONTENU` et rend son contenu brut, sans l'interpréter.
fn raw_content(
    reader: &mut Reader<&[u8]>,
    start: &BytesStart<'_>,
    text: &str,
) -> Result<String, XmlError> {
    let span = reader.read_to_end(start.to_end().name()).map_err(|_| XmlError::Malformed)?;
    let bound = |position: u64| usize::try_from(position).unwrap_or(usize::MAX);
    let raw = text.get(bound(span.start)..bound(span.end)).unwrap_or_default();
    if raw.contains("<!DOCTYPE") || raw.contains("<!ENTITY") {
        return Err(XmlError::DocType);
    }
    Ok(raw.to_owned())
}

/// Analyse un membre en arbre borné.
fn parse_tree(src: &[u8]) -> Result<Node, XmlError> {
    let text = std::str::from_utf8(src).map_err(|_| XmlError::NotUtf8)?;
    let mut reader = Reader::from_str(text);
    let mut stack: Vec<Node> = Vec::new();
    let mut root: Option<Node> = None;
    let mut outside = String::new();
    let mut count = 0usize;
    loop {
        match reader.read_event().map_err(|_| XmlError::Malformed)? {
            Event::Start(start) => {
                let mut node = open(&start, &mut count)?;
                if node.name == RAW_ELEMENT {
                    node.raw = Some(raw_content(&mut reader, &start, text)?);
                    attach(&mut stack, &mut root, node)?;
                } else if stack.len() >= MAX_DEPTH {
                    return Err(XmlError::TooDeep);
                } else {
                    stack.push(node);
                }
            }
            Event::Empty(start) => {
                let mut node = open(&start, &mut count)?;
                if node.name == RAW_ELEMENT {
                    node.raw = Some(String::new());
                }
                attach(&mut stack, &mut root, node)?;
            }
            // quick-xml refuse une fin sans début : la pile n'est jamais vide ici
            Event::End(_) => {
                let closed = stack.pop();
                closed.map_or(Ok(()), |node| attach(&mut stack, &mut root, node))?;
            }
            // hors de tout élément (prologue, épilogue), le texte va dans `outside` et se perd
            Event::Text(text) => {
                let target = stack.last_mut().map_or(&mut outside, |top| &mut top.text);
                target.push_str(&text.xml10_content());
            }
            Event::CData(data) => {
                let target = stack.last_mut().map_or(&mut outside, |top| &mut top.text);
                target.push_str(&data.xml10_content());
            }
            Event::GeneralRef(reference) => {
                let target = stack.last_mut().map_or(&mut outside, |top| &mut top.text);
                push_ref(target, &reference)?;
            }
            Event::DocType(_) => return Err(XmlError::DocType),
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) => {}
            Event::Eof => break,
        }
    }
    if !stack.is_empty() {
        return Err(XmlError::Malformed);
    }
    root.ok_or(XmlError::NoRoot)
}

/// `CONTEXTE/TEXTE` : texte parent et chaîne de sections.
fn context(root: &Node) -> Result<TextContext, XmlError> {
    let Some(texte) = root.at(&["CONTEXTE", "TEXTE"]) else {
        return Ok(TextContext::default());
    };
    let mut sections = Vec::new();
    let mut cursor = texte.child("TM");
    while let Some(tm) = cursor {
        if let Some(title) = tm.child("TITRE_TM") {
            sections.push(SectionRef {
                id: title.attr_owned("id").unwrap_or_default(),
                title: title.text.trim().to_owned(),
                date_debut: title.attr_date("debut")?,
                date_fin: title.attr_date("fin")?,
            });
        }
        cursor = tm.child("TM");
    }
    Ok(TextContext {
        cid: texte.attr_owned("cid"),
        nature: texte.attr_owned("nature"),
        title: field(Some(texte), "TITRE_TXT"),
        sections,
    })
}

/// Un lien `LIEN_ART` ou `LIEN_SECTION_TA` ; `None` pour tout autre élément.
fn struct_link(node: &Node) -> Result<Option<StructLink>, XmlError> {
    let target = match node.name.as_str() {
        "LIEN_ART" => LinkTarget::Article,
        "LIEN_SECTION_TA" => LinkTarget::Section,
        _ => return Ok(None),
    };
    Ok(Some(StructLink {
        target,
        id: node.attr_owned("id").ok_or(XmlError::MissingField("LIEN@id"))?,
        cid: node.attr_owned("cid"),
        num: node.attr_owned("num"),
        level: node.attr("niv").and_then(|niv| niv.parse().ok()),
        etat: node.attr_owned("etat"),
        debut: node.attr_date("debut")?,
        fin: node.attr_date("fin")?,
        title: node.value(),
    }))
}

/// Tous les liens de structure enfants de `parent`, dans l'ordre du fichier.
fn struct_links(parent: Option<&Node>) -> Result<Vec<StructLink>, XmlError> {
    let Some(parent) = parent else {
        return Ok(Vec::new());
    };
    parent.children.iter().filter_map(|node| struct_link(node).transpose()).collect()
}

/// `LIENS/LIEN` d'un article.
fn links(root: &Node) -> Result<Vec<Link>, XmlError> {
    let Some(parent) = root.child("LIENS") else {
        return Ok(Vec::new());
    };
    parent
        .each("LIEN")
        .map(|node| {
            Ok(Link {
                link_type: node.attr_owned("typelien"),
                direction: node.attr_owned("sens"),
                id: node.attr_owned("id"),
                text_cid: node.attr_owned("cidtexte"),
                text_nature: node.attr_owned("naturetexte"),
                text_number: node.attr_owned("numtexte"),
                text_nor: node.attr_owned("nortexte"),
                text_signed: node.attr_date("datesignatexte")?,
                title: node.text.trim().to_owned(),
            })
        })
        .collect()
}

/// Identifiant obligatoire de `META/META_COMMUN/ID`.
fn common_id(root: &Node) -> Result<String, XmlError> {
    field(root.at(&["META", "META_COMMUN"]), "ID").ok_or(XmlError::MissingField("ID"))
}

fn article(root: &Node) -> Result<Document, XmlError> {
    let meta = root.at(&["META", "META_SPEC", "META_ARTICLE"]);
    let mut versions = Vec::new();
    for version in root.child("VERSIONS").into_iter().flat_map(|v| v.each("VERSION")) {
        versions.extend(struct_links(Some(version))?);
    }
    Ok(Document::Article(Article {
        id: common_id(root)?,
        num: field(meta, "NUM"),
        etat: field(meta, "ETAT"),
        kind: field(meta, "TYPE"),
        date_debut: date_field(meta, "DATE_DEBUT", "DATE_DEBUT")?,
        date_fin: date_field(meta, "DATE_FIN", "DATE_FIN")?,
        context: context(root)?,
        versions,
        nota: root.raw_at(&["NOTA", "CONTENU"]),
        content: root.raw_at(&["BLOC_TEXTUEL", "CONTENU"]),
        links: links(root)?,
    }))
}

fn section(root: &Node) -> Result<Document, XmlError> {
    Ok(Document::Section(Section {
        id: root.child("ID").and_then(Node::value).ok_or(XmlError::MissingField("ID"))?,
        title: field(Some(root), "TITRE_TA"),
        context: context(root)?,
        structure: struct_links(root.child("STRUCTURE_TA"))?,
    }))
}

fn chronicle(root: &Node) -> Result<Chronicle, XmlError> {
    let meta = root.at(&["META", "META_SPEC", "META_TEXTE_CHRONICLE"]);
    Ok(Chronicle {
        cid: field(meta, "CID"),
        num: field(meta, "NUM"),
        nor: field(meta, "NOR"),
        date_publi: date_field(meta, "DATE_PUBLI", "DATE_PUBLI")?,
        date_texte: date_field(meta, "DATE_TEXTE", "DATE_TEXTE")?,
        last_modified: date_field(meta, "DERNIERE_MODIFICATION", "DERNIERE_MODIFICATION")?,
    })
}

fn text_struct(root: &Node) -> Result<Document, XmlError> {
    let mut versions = Vec::new();
    for version in root.child("VERSIONS").into_iter().flat_map(|v| v.each("VERSION")) {
        for link in version.each("LIEN_TXT") {
            versions.push(TextVersionLink {
                id: link.attr_owned("id").ok_or(XmlError::MissingField("LIEN_TXT@id"))?,
                etat: version.attr_owned("etat"),
                debut: link.attr_date("debut")?,
                fin: link.attr_date("fin")?,
                num: link.attr_owned("num"),
            });
        }
    }
    Ok(Document::TextStruct(TextStruct {
        id: common_id(root)?,
        nature: nature(root),
        chronicle: chronicle(root)?,
        versions,
        structure: struct_links(root.child("STRUCT"))?,
    }))
}

/// `META/META_COMMUN/NATURE`.
fn nature(root: &Node) -> Option<String> {
    field(root.at(&["META", "META_COMMUN"]), "NATURE")
}

fn text_version(root: &Node) -> Result<Document, XmlError> {
    let meta = root.at(&["META", "META_SPEC", "META_TEXTE_VERSION"]);
    Ok(Document::TextVersion(TextVersion {
        id: common_id(root)?,
        nature: nature(root),
        chronicle: chronicle(root)?,
        title: field(meta, "TITRE"),
        title_full: field(meta, "TITREFULL"),
        etat: field(meta, "ETAT"),
        date_debut: date_field(meta, "DATE_DEBUT", "DATE_DEBUT")?,
        date_fin: date_field(meta, "DATE_FIN", "DATE_FIN")?,
        authority: field(meta, "AUTORITE"),
        ministry: field(meta, "MINISTERE"),
        visas: root.raw_at(&["VISAS", "CONTENU"]),
        signatories: root.raw_at(&["SIGNATAIRES", "CONTENU"]),
        tp: root.raw_at(&["TP", "CONTENU"]),
        nota: root.raw_at(&["NOTA", "CONTENU"]),
        abro: root.raw_at(&["ABRO", "CONTENU"]),
        rect: root.raw_at(&["RECT", "CONTENU"]),
    }))
}

/// Convertit l'arbre d'une racine reconnue en [`Document`].
type Builder = fn(&Node) -> Result<Document, XmlError>;

/// Analyse un membre XML du dossier `expected` et le convertit en [`Document`].
///
/// # Errors
///
/// [`XmlError`] si le membre n'est pas de l'UTF-8, est mal formé, déclare un `DOCTYPE` ou une
/// entité, dépasse les bornes, n'a pas la racine annoncée par son dossier, manque d'un champ
/// obligatoire ou porte une date invalide.
pub fn parse(src: &[u8], expected: DocKind) -> Result<Document, XmlError> {
    let root = parse_tree(src)?;
    let (name, build): (&str, Builder) = match expected {
        DocKind::Article => ("ARTICLE", article),
        DocKind::Section => ("SECTION_TA", section),
        DocKind::TextStruct => ("TEXTELR", text_struct),
        DocKind::TextVersion => ("TEXTE_VERSION", text_version),
    };
    if root.name != name {
        return Err(XmlError::UnexpectedRoot);
    }
    build(&root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{article_xml, fixture, fixture_path};

    const GLOBAL: &str = "xml/global-20250713";
    const TEXT_ID: &str = "LEGITEXT000000000001";
    const ARTICLE_ID: &str = "LEGIARTI000000000001";

    fn date(year: u16, month: u8, day: u8) -> Option<CivilDate> {
        CivilDate::new(year, month, day)
    }

    fn article_of(rel: &str) -> Article {
        parse(&fixture(rel), DocKind::Article).unwrap().as_article().unwrap().clone()
    }

    fn section_of(rel: &str) -> Section {
        parse(&fixture(rel), DocKind::Section).unwrap().as_section().unwrap().clone()
    }

    fn fails(xml: &str, kind: DocKind) -> XmlError {
        parse(xml.as_bytes(), kind).unwrap_err()
    }

    #[test]
    fn a_real_article_is_read_field_by_field() {
        let article = article_of(&format!("{GLOBAL}/code_en_vigueur/LEGIARTI000006842417.xml"));
        assert_eq!(article.id, "LEGIARTI000006842417");
        assert_eq!(article.num.as_deref(), Some("6"));
        assert_eq!(article.etat.as_deref(), Some("VIGUEUR"));
        assert_eq!(article.kind.as_deref(), Some("AUTONOME"));
        assert_eq!(article.date_debut, date(1980, 3, 25));
        assert_eq!(article.date_fin, date(2999, 1, 1));
        assert_eq!(article.context.cid.as_deref(), Some("LEGITEXT000006074232"));
        assert_eq!(article.context.nature.as_deref(), Some("CODE"));
        assert_eq!(article.context.title.as_deref(), Some("Code de déontologie des architectes"));
        let chain: Vec<(&str, &str)> =
            article.context.sections.iter().map(|s| (s.id.as_str(), s.title.as_str())).collect();
        assert_eq!(
            chain,
            [
                ("LEGISCTA000006108696", "TITRE II : Devoirs professionnels"),
                ("LEGISCTA000006129093", "CHAPITRE 1er : Règles générales"),
                ("LEGISCTA000006143863", "Section 1 : Règles personnelles."),
            ]
        );
        assert_eq!(article.context.sections[0].date_debut, date(1980, 3, 25));
        assert_eq!(article.context.sections[0].date_fin, date(2999, 1, 1));
        assert_eq!(article.versions.len(), 1);
        let version = &article.versions[0];
        assert_eq!(version.target, LinkTarget::Article);
        assert_eq!(version.id, "LEGIARTI000006842417");
        assert_eq!(version.num.as_deref(), Some("6"));
        assert_eq!(version.etat.as_deref(), Some("VIGUEUR"));
        assert_eq!((version.debut, version.fin), (date(1980, 3, 25), date(2999, 1, 1)));
        assert_eq!(article.nota, "");
        assert!(article.content.contains("Tout architecte se doit de prêter son concours"));
        assert!(article.content.starts_with("\n<p>"), "raw HTML keeps its markup");
        assert_eq!(article.links.len(), 1);
        let link = &article.links[0];
        assert_eq!(link.link_type.as_deref(), Some("CODIFICATION"));
        assert_eq!(link.direction.as_deref(), Some("source"));
        assert_eq!(link.id.as_deref(), Some("JORFTEXT000000519770"));
        assert_eq!(link.text_cid.as_deref(), Some("JORFTEXT000000519770"));
        assert_eq!(link.text_nature.as_deref(), Some("DECRET"));
        assert_eq!(link.text_number.as_deref(), Some("80-217"));
        assert_eq!(link.text_nor, None);
        assert_eq!(link.text_signed, date(1980, 3, 20));
        assert_eq!(link.title, "Décret 80-217 1980-03-20");
    }

    #[test]
    fn an_entirely_modifying_article_has_no_state_and_keeps_sentinel_dates() {
        let rel = format!("{GLOBAL}/TNC_non_vigueur/LEGIARTI000006493843.xml");
        let article = article_of(&rel);
        assert_eq!(article.etat, None, "<ETAT/> is an absent state");
        assert_eq!(article.kind.as_deref(), Some("ENTIEREMENT_MODIF"));
        assert_eq!((article.date_debut, article.date_fin), (date(2999, 1, 1), date(2999, 1, 1)));
        assert_eq!(article.context.title.as_deref(), Some("Loi du 22 mai 1915 sur le recel."));
        assert_eq!(article.context.sections.len(), 0);
        assert_eq!(article.versions[0].etat, None);
        assert_eq!(article.content, "a modifié les dispositions suivantes<br/>\n<br/>\n");
    }

    #[test]
    fn deferred_and_empty_values_of_the_real_dump_are_kept() {
        let deferred = article_of(&format!("{GLOBAL}/TNC_non_vigueur/LEGIARTI000041976403.xml"));
        assert_eq!(deferred.date_debut, date(2222, 2, 22));
        let no_number = article_of(&format!("{GLOBAL}/TNC_en_vigueur/LEGIARTI000006283817.xml"));
        assert_eq!(no_number.num, None, "<NUM/> is an absent number");
        let table = article_of(&format!("{GLOBAL}/TNC_en_vigueur/LEGIARTI000006560843.xml"));
        assert!(table.content.contains("<table"));
        let hyphen = article_of(&format!("{GLOBAL}/TNC_en_vigueur/LEGIARTI000042240551.xml"));
        assert!(hyphen.content.contains('\u{ad}'));
    }

    #[test]
    fn a_section_keeps_sections_and_articles_in_file_order() {
        let section = section_of(&format!("{GLOBAL}/code_en_vigueur/LEGISCTA000045418469.xml"));
        assert_eq!(section.id, "LEGISCTA000045418469");
        assert!(section.title.as_deref().unwrap().starts_with("Titre préliminaire"));
        assert_eq!(section.context.cid.as_deref(), Some("LEGITEXT000006074220"));
        assert_eq!(section.context.sections.len(), 2);
        assert_eq!(section.structure.len(), 18);
        let first = &section.structure[0];
        assert_eq!(first.target, LinkTarget::Section);
        assert_eq!(first.id, "LEGISCTA000045417602");
        assert_eq!(first.cid.as_deref(), Some("LEGISCTA000045417602"));
        assert_eq!(first.level, Some(4));
        assert_eq!(first.title.as_deref(), Some("Chapitre Ier : Enquêtes techniques"));
        let article = &section.structure[2];
        assert_eq!(article.target, LinkTarget::Article);
        assert_eq!(article.num.as_deref(), Some("D510-1"));
        assert_eq!(article.etat.as_deref(), Some("MODIFIE"));
        assert_eq!((article.debut, article.fin), (date(2011, 12, 19), date(2013, 12, 30)));
        assert_eq!(article.title, None);
        let minimal = section_of(&format!("{GLOBAL}/code_en_vigueur/LEGISCTA000006108694.xml"));
        assert_eq!(minimal.title.as_deref(), Some("Code de déontologie des architectes."));
        assert_eq!(minimal.structure.len(), 1);
        assert_eq!(minimal.structure[0].num.as_deref(), Some("1"));
        assert_eq!(minimal.context.sections.len(), 0);
    }

    #[test]
    fn a_text_structure_lists_versions_and_top_level_sections() {
        let src = fixture(&format!("{GLOBAL}/code_en_vigueur/struct_LEGITEXT000006074232.xml"));
        let document = parse(&src, DocKind::TextStruct).unwrap();
        let text = document.as_text_struct().unwrap();
        assert_eq!(text.id, "LEGITEXT000006074232");
        assert_eq!(text.nature.as_deref(), Some("CODE"));
        assert_eq!(text.chronicle.cid.as_deref(), Some("LEGITEXT000006074232"));
        assert_eq!((&text.chronicle.num, &text.chronicle.nor), (&None, &None));
        assert_eq!(text.chronicle.date_publi, date(2999, 1, 1));
        assert_eq!(text.chronicle.date_texte, date(2999, 1, 1));
        assert_eq!(text.chronicle.last_modified, date(1992, 9, 23));
        assert_eq!(text.versions.len(), 1);
        let version = &text.versions[0];
        assert_eq!(version.id, "LEGITEXT000006074232");
        assert_eq!(version.etat.as_deref(), Some("VIGUEUR"));
        assert_eq!((version.debut, version.fin), (date(1980, 3, 25), date(2999, 1, 1)));
        assert_eq!(version.num, None);
        assert_eq!(text.structure.len(), 4);
        assert!(
            text.structure.iter().all(|l| l.target == LinkTarget::Section && l.level == Some(1))
        );
    }

    #[test]
    fn a_text_version_carries_its_title_dates_and_blocks() {
        let src = fixture(&format!("{GLOBAL}/code_en_vigueur/version_LEGITEXT000006074232.xml"));
        let document = parse(&src, DocKind::TextVersion).unwrap();
        let text = document.as_text_version().unwrap();
        assert_eq!(text.id, "LEGITEXT000006074232");
        assert_eq!(text.nature.as_deref(), Some("CODE"));
        assert_eq!(text.title.as_deref(), Some("Code de déontologie des architectes"));
        assert_eq!(text.title_full.as_deref(), Some("Code de déontologie des architectes"));
        assert_eq!(text.etat.as_deref(), Some("VIGUEUR"));
        assert_eq!((text.date_debut, text.date_fin), (date(1980, 3, 25), date(2999, 1, 1)));
        assert_eq!((&text.authority, &text.ministry), (&None, &None));
        assert_eq!(text.chronicle.last_modified, date(1992, 9, 23));
        for block in [&text.visas, &text.signatories, &text.tp, &text.nota, &text.abro, &text.rect]
        {
            assert_eq!(block, "");
        }
    }

    #[test]
    fn every_real_fixture_parses_into_a_document_with_the_id_of_its_file_name() {
        let mut checked = 0_usize;
        for tree in ["xml/global-20250713", "xml/incr-20261028"] {
            for dir in std::fs::read_dir(fixture_path(tree)).unwrap() {
                let dir = dir.unwrap().path();
                if dir.ends_with("eli") {
                    continue;
                }
                for file in std::fs::read_dir(&dir).unwrap() {
                    let path = file.unwrap().path();
                    if path.extension().is_none_or(|e| e != "xml") {
                        continue;
                    }
                    let name = path.file_stem().unwrap().to_str().unwrap().to_owned();
                    let (kind, id) = if let Some(id) = name.strip_prefix("struct_") {
                        (DocKind::TextStruct, id)
                    } else if let Some(id) = name.strip_prefix("version_") {
                        (DocKind::TextVersion, id)
                    } else if name.starts_with("LEGISCTA") {
                        (DocKind::Section, name.as_str())
                    } else {
                        (DocKind::Article, name.as_str())
                    };
                    let parsed = parse(&std::fs::read(&path).unwrap(), kind);
                    assert!(parsed.is_ok(), "{name}: {parsed:?}");
                    let document = parsed.unwrap();
                    assert_eq!((document.id(), document.kind()), (id, kind), "{name}");
                    checked = checked.saturating_add(1);
                }
            }
        }
        assert_eq!(checked, 50, "the 50 non-empty XML fixtures of V1-16");
    }

    #[test]
    fn the_empty_and_eli_fixtures_are_not_documents_of_the_corpus() {
        let empty = fixture(&format!(
            "{GLOBAL}/eli/eli_ordonnance_20230831_ECOT2316929P_texte_versions_VIDE.xml"
        ));
        assert_eq!(empty.len(), 0);
        assert_eq!(parse(&empty, DocKind::Article).unwrap_err(), XmlError::NoRoot);
        let eli = fixture(&format!("{GLOBAL}/eli/eli_jo_20131030_versions.xml"));
        assert_eq!(parse(&eli, DocKind::Article).unwrap_err(), XmlError::UnexpectedRoot);
    }

    #[test]
    fn bytes_that_are_not_utf8_are_refused() {
        let error = parse(b"<ARTICLE>\xff</ARTICLE>", DocKind::Article).unwrap_err();
        assert_eq!(error, XmlError::NotUtf8);
    }

    #[test]
    fn a_doctype_or_an_entity_declaration_is_refused() {
        let bomb = r#"<?xml version="1.0"?><!DOCTYPE ARTICLE [<!ENTITY x "y">]><ARTICLE/>"#;
        assert_eq!(fails(bomb, DocKind::Article), XmlError::DocType);
        let inside =
            "<ARTICLE><BLOC_TEXTUEL><CONTENU><!DOCTYPE x></CONTENU></BLOC_TEXTUEL></ARTICLE>";
        assert_eq!(fails(inside, DocKind::Article), XmlError::DocType);
        // quick-xml refuse déjà une déclaration d'entité hors prologue : jamais une expansion
        let entity = "<ARTICLE><CONTENU>a<!ENTITY y 'z'>b</CONTENU></ARTICLE>";
        assert_eq!(fails(entity, DocKind::Article), XmlError::Malformed);
    }

    #[test]
    fn unknown_entities_and_bad_character_references_are_refused() {
        let xml = |inner: &str| format!("<SECTION_TA><ID>{inner}</ID></SECTION_TA>");
        assert_eq!(fails(&xml("&nbsp;"), DocKind::Section), XmlError::Entity);
        assert_eq!(fails(&xml("&#0;"), DocKind::Section), XmlError::Malformed);
        let in_attribute = r#"<TEXTELR><VERSIONS><VERSION><LIEN_TXT id="&bogus;"/></VERSION></VERSIONS></TEXTELR>"#;
        assert_eq!(fails(in_attribute, DocKind::TextStruct), XmlError::Malformed);
    }

    #[test]
    fn predefined_entities_character_references_and_cdata_are_decoded_in_metadata() {
        let xml = "<SECTION_TA><ID>LEGISCTA000000000001</ID>\
            <TITRE_TA>a &lt;b&gt; &amp; &apos;c&apos; &quot;d&quot; &#233;&#x41; \
            <![CDATA[<e>]]><!-- x --><?pi y?></TITRE_TA></SECTION_TA>";
        let document = parse(xml.as_bytes(), DocKind::Section).unwrap();
        let section = document.as_section().unwrap();
        assert_eq!(section.title.as_deref(), Some("a <b> & 'c' \"d\" éA <e>"));
    }

    #[test]
    fn raw_html_keeps_its_entities_and_references_untouched() {
        let xml = article_xml(
            "LEGITEXT000000000001",
            "LEGIARTI000000000001",
            "1",
            "<p>a &lt;b&gt;&#13;&nbsp;c</p>",
        );
        let document = parse(xml.as_bytes(), DocKind::Article).unwrap();
        assert_eq!(document.as_article().unwrap().content, "<p>a &lt;b&gt;&#13;&nbsp;c</p>");
    }

    #[test]
    fn broken_structure_is_refused() {
        let cases = [
            ("<ARTICLE><a></ARTICLE>", XmlError::Malformed),
            ("<ARTICLE><a>text", XmlError::Malformed),
            ("<ARTICLE><CONTENU><p></CONTENU></ARTICLE>", XmlError::Malformed),
            (r#"<ARTICLE a="1" a="2"/>"#, XmlError::Malformed),
            ("<ARTICLE/><ARTICLE/>", XmlError::TrailingContent),
            ("", XmlError::NoRoot),
            ("<?xml version=\"1.0\"?><!-- rien -->", XmlError::NoRoot),
        ];
        for (xml, expected) in cases {
            assert_eq!(fails(xml, DocKind::Article), expected, "{xml}");
        }
    }

    #[test]
    fn nesting_and_size_are_bounded() {
        let deep = format!("{}{}", "<a>".repeat(MAX_DEPTH + 1), "</a>".repeat(MAX_DEPTH + 1));
        assert_eq!(fails(&deep, DocKind::Article), XmlError::TooDeep);
        let levels = MAX_DEPTH - 1;
        let ok = format!("<ARTICLE>{}{}</ARTICLE>", "<a>".repeat(levels), "</a>".repeat(levels));
        assert_eq!(fails(&ok, DocKind::Article), XmlError::MissingField("ID"));
        let wide = format!("<ARTICLE>{}</ARTICLE>", "<a/>".repeat(MAX_NODES));
        assert_eq!(fails(&wide, DocKind::Article), XmlError::TooManyNodes);
    }

    #[test]
    fn the_root_must_match_the_folder() {
        assert_eq!(fails("<SECTION_TA/>", DocKind::Article), XmlError::UnexpectedRoot);
        assert_eq!(fails("<ARTICLE/>", DocKind::TextVersion), XmlError::UnexpectedRoot);
    }

    #[test]
    fn mandatory_fields_and_valid_dates_are_enforced() {
        let missing_id = XmlError::MissingField("ID");
        assert_eq!(fails("<ARTICLE/>", DocKind::Article), missing_id);
        assert_eq!(fails("<TEXTELR/>", DocKind::TextStruct), missing_id);
        assert_eq!(fails("<TEXTE_VERSION/>", DocKind::TextVersion), missing_id);
        assert_eq!(fails("<SECTION_TA><ID> </ID></SECTION_TA>", DocKind::Section), missing_id);
        let bad_date = article_xml("LEGITEXT000000000001", "LEGIARTI000000000001", "1", "")
            .replace("<DATE_FIN>2999-01-01", "<DATE_FIN>2999-13-01");
        assert_eq!(fails(&bad_date, DocKind::Article), XmlError::BadDate("DATE_FIN"));
        let bad_attr = r#"<SECTION_TA><ID>LEGISCTA000000000001</ID><STRUCTURE_TA><LIEN_ART id="LEGIARTI000000000001" debut="demain"/></STRUCTURE_TA></SECTION_TA>"#;
        assert_eq!(fails(bad_attr, DocKind::Section), XmlError::BadDate("debut"));
    }

    #[test]
    fn links_need_an_id_and_unknown_siblings_are_skipped() {
        let no_id = r#"<SECTION_TA><ID>LEGISCTA000000000001</ID><STRUCTURE_TA><LIEN_ART num="1"/></STRUCTURE_TA></SECTION_TA>"#;
        assert_eq!(fails(no_id, DocKind::Section), XmlError::MissingField("LIEN@id"));
        let no_txt_id = r#"<TEXTELR><META><META_COMMUN><ID>LEGITEXT000000000001</ID></META_COMMUN></META><VERSIONS><VERSION><LIEN_TXT num="1"/></VERSION></VERSIONS></TEXTELR>"#;
        assert_eq!(fails(no_txt_id, DocKind::TextStruct), XmlError::MissingField("LIEN_TXT@id"));
        let mixed = r#"<SECTION_TA><ID>LEGISCTA000000000001</ID><STRUCTURE_TA><AUTRE/><LIEN_SECTION_TA id="LEGISCTA000000000002" niv="x"/></STRUCTURE_TA></SECTION_TA>"#;
        let document = parse(mixed.as_bytes(), DocKind::Section).unwrap();
        let section = document.as_section().unwrap();
        assert_eq!(section.structure.len(), 1);
        assert_eq!(section.structure[0].level, None, "a non-numeric level is dropped, not fatal");
        assert_eq!(section.context, TextContext::default());
    }

    #[test]
    fn bad_dates_in_links_and_section_chains_refuse_the_document() {
        let article = |links: &str, tm: &str| {
            format!(
                "<ARTICLE><META><META_COMMUN><ID>LEGIARTI000000000001</ID></META_COMMUN></META>\
                 <CONTEXTE><TEXTE cid=\"LEGITEXT000000000001\">{tm}</TEXTE></CONTEXTE>{links}</ARTICLE>"
            )
        };
        let bad_signed = article(r#"<LIENS><LIEN datesignatexte="hier">x</LIEN></LIENS>"#, "");
        assert_eq!(fails(&bad_signed, DocKind::Article), XmlError::BadDate("datesignatexte"));
        let bad_start =
            r#"<TM><TITRE_TM id="LEGISCTA000000000001" debut="2020-02-30">x</TITRE_TM></TM>"#;
        assert_eq!(fails(&article("", bad_start), DocKind::Article), XmlError::BadDate("debut"));
        let bad_end = r#"<TM><TITRE_TM fin="nope">x</TITRE_TM></TM>"#;
        assert_eq!(fails(&article("", bad_end), DocKind::Article), XmlError::BadDate("fin"));
        let nested = r#"<TM><TM><TITRE_TM id="LEGISCTA000000000009">Bas</TITRE_TM></TM></TM>"#;
        let document = parse(article("", nested).as_bytes(), DocKind::Article).unwrap();
        let parsed = document.as_article().unwrap();
        assert_eq!(parsed.context.sections.len(), 1);
        assert_eq!(parsed.context.sections[0].title, "Bas");
    }

    fn text_struct_xml(chronicle: &str, version: &str, structure: &str) -> String {
        format!(
            "<TEXTELR><META><META_COMMUN><ID>{TEXT_ID}</ID></META_COMMUN><META_SPEC>\
             <META_TEXTE_CHRONICLE>{chronicle}</META_TEXTE_CHRONICLE></META_SPEC></META>\
             <VERSIONS><VERSION etat=\"VIGUEUR\">{version}</VERSION></VERSIONS>\
             <STRUCT>{structure}</STRUCT></TEXTELR>"
        )
    }

    fn text_version_xml(chronicle: &str, version_meta: &str) -> String {
        format!(
            "<TEXTE_VERSION><META><META_COMMUN><ID>{TEXT_ID}</ID></META_COMMUN><META_SPEC>\
             <META_TEXTE_CHRONICLE>{chronicle}</META_TEXTE_CHRONICLE>\
             <META_TEXTE_VERSION>{version_meta}</META_TEXTE_VERSION></META_SPEC></META>\
             </TEXTE_VERSION>"
        )
    }

    #[test]
    fn invalid_dates_and_links_refuse_every_kind_of_document() {
        let article = article_xml(TEXT_ID, ARTICLE_ID, "1", "");
        let bad_article_start = article.replace("<DATE_DEBUT>2020-01-01", "<DATE_DEBUT>2020-13-01");
        let linkless_version = article.replace(&format!("id=\"{ARTICLE_ID}\" num"), "num");
        let section_context = format!(
            "<SECTION_TA><ID>LEGISCTA000000000001</ID><CONTEXTE><TEXTE cid=\"{TEXT_ID}\"><TM>\
             <TITRE_TM id=\"LEGISCTA000000000002\" debut=\"demain\">t</TITRE_TM></TM></TEXTE>\
             </CONTEXTE></SECTION_TA>"
        );
        let section_link = format!(
            "<SECTION_TA><ID>LEGISCTA000000000001</ID><STRUCTURE_TA>\
             <LIEN_ART id=\"{ARTICLE_ID}\" fin=\"x\"/></STRUCTURE_TA></SECTION_TA>"
        );
        let publi = "<DATE_PUBLI>x</DATE_PUBLI>";
        let texte = "<DATE_TEXTE>x</DATE_TEXTE>";
        let modified = "<DERNIERE_MODIFICATION>x</DERNIERE_MODIFICATION>";
        let text_link = |attribute: &str| format!("<LIEN_TXT id=\"{TEXT_ID}\" {attribute}=\"x\"/>");
        let cases = [
            (DocKind::Article, bad_article_start, XmlError::BadDate("DATE_DEBUT")),
            (DocKind::Article, linkless_version, XmlError::MissingField("LIEN@id")),
            (DocKind::Section, section_context, XmlError::BadDate("debut")),
            (DocKind::Section, section_link, XmlError::BadDate("fin")),
            (DocKind::TextStruct, text_struct_xml(publi, "", ""), XmlError::BadDate("DATE_PUBLI")),
            (DocKind::TextStruct, text_struct_xml(texte, "", ""), XmlError::BadDate("DATE_TEXTE")),
            (
                DocKind::TextStruct,
                text_struct_xml(modified, "", ""),
                XmlError::BadDate("DERNIERE_MODIFICATION"),
            ),
            (
                DocKind::TextStruct,
                text_struct_xml("", &text_link("debut"), ""),
                XmlError::BadDate("debut"),
            ),
            (
                DocKind::TextStruct,
                text_struct_xml("", &text_link("fin"), ""),
                XmlError::BadDate("fin"),
            ),
            (
                DocKind::TextStruct,
                text_struct_xml("", "", "<LIEN_ART/>"),
                XmlError::MissingField("LIEN@id"),
            ),
            (DocKind::TextVersion, text_version_xml(publi, ""), XmlError::BadDate("DATE_PUBLI")),
            (
                DocKind::TextVersion,
                text_version_xml("", "<DATE_DEBUT>x</DATE_DEBUT>"),
                XmlError::BadDate("DATE_DEBUT"),
            ),
            (
                DocKind::TextVersion,
                text_version_xml("", "<DATE_FIN>x</DATE_FIN>"),
                XmlError::BadDate("DATE_FIN"),
            ),
        ];
        for (kind, xml, expected) in cases {
            assert_eq!(fails(&xml, kind), expected, "{xml}");
        }
    }

    #[test]
    fn start_tags_fail_like_empty_tags_and_a_second_root_is_refused_everywhere() {
        let duplicate = r#"<ARTICLE a="1" a="2"></ARTICLE>"#;
        assert_eq!(fails(duplicate, DocKind::Article), XmlError::Malformed);
        let many = format!("<ARTICLE>{}</ARTICLE>", "<a></a>".repeat(MAX_NODES));
        assert_eq!(fails(&many, DocKind::Article), XmlError::TooManyNodes);
        let raw_twice = "<CONTENU>a</CONTENU><CONTENU>b</CONTENU>";
        assert_eq!(fails(raw_twice, DocKind::Article), XmlError::TrailingContent);
        let twice = "<ARTICLE></ARTICLE><ARTICLE></ARTICLE>";
        assert_eq!(fails(twice, DocKind::Article), XmlError::TrailingContent);
    }

    #[test]
    fn text_outside_the_root_element_is_ignored() {
        let xml = "x<![CDATA[y]]>&amp;<SECTION_TA><ID>LEGISCTA000000000001</ID></SECTION_TA>z";
        let document = parse(xml.as_bytes(), DocKind::Section).unwrap();
        assert_eq!(document.id(), "LEGISCTA000000000001");
        assert_eq!(fails("&nbsp;<SECTION_TA/>", DocKind::Section), XmlError::Entity);
    }

    #[test]
    fn errors_never_quote_the_content() {
        assert_eq!(XmlError::NotUtf8.to_string(), "member is not valid UTF-8");
        assert_eq!(XmlError::MissingField("ID").to_string(), "missing field ID");
        assert_eq!(XmlError::BadDate("DATE_FIN").to_string(), "invalid date in DATE_FIN");
        for error in [
            XmlError::Malformed,
            XmlError::DocType,
            XmlError::Entity,
            XmlError::TooDeep,
            XmlError::TooManyNodes,
            XmlError::TrailingContent,
            XmlError::NoRoot,
            XmlError::UnexpectedRoot,
        ] {
            assert_ne!(error.to_string(), "");
        }
    }
}
