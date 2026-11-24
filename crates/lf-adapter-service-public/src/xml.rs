//! Lecture d'une entrée XML de l'archive en un arbre borné (ADR-0033 décision 5, T-MS-22).
//!
//! Lecteur de `quick-xml`, sans `serde`. L'arbre garde l'ordre du texte et des éléments : une
//! fiche mêle texte et liens dans un même paragraphe. Sécurité, comme pour l'adapter DILA : tout
//! `DOCTYPE` est refusé (aucune entrée des archives du 2026-11-23 n'en a), seules les cinq entités
//! prédéfinies et les références de caractères sont résolues, la profondeur et le nombre
//! d'éléments sont bornés, et aucune erreur ne cite le contenu lu.
//!
//! Relevé du 2026-11-24 sur les deux archives (4 102 fiches et pages de navigation de moins de
//! 2 Mio) : 24 niveaux et 10 101 éléments au plus.
//!
//! L'en-tête d'une entrée, coupé à 64 Kio par le lecteur d'archive, se lit en mode partiel
//! ([`parse_head`]) : à la première erreur de lecture, les éléments ouverts sont fermés et l'arbre
//! lu jusque-là est rendu. Les refus de sécurité restent des refus.

use quick_xml::{
    Reader, XmlVersion,
    events::{BytesRef, BytesStart, Event},
};
use thiserror::Error;

/// Profondeur maximale des éléments (24 mesurés).
pub(crate) const XML_DEPTH_MAX: usize = 64;

/// Nombre maximal d'éléments d'une entrée (10 101 mesurés).
pub(crate) const XML_ELEMENTS_MAX: usize = 100_000;

/// Refus d'une entrée XML. Aucun ne porte de texte lu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum XmlError {
    /// L'entrée n'est pas de l'UTF-8.
    #[error("entry is not valid UTF-8")]
    NotUtf8,
    /// Le XML est mal formé.
    #[error("entry is not well-formed XML")]
    Malformed,
    /// `DOCTYPE`, donc déclaration d'entité possible.
    #[error("DOCTYPE and entity declarations are forbidden")]
    DocType,
    /// Référence d'entité autre que les cinq prédéfinies.
    #[error("unknown entity reference")]
    Entity,
    /// Éléments imbriqués au-delà de [`XML_DEPTH_MAX`].
    #[error("element nesting is too deep")]
    TooDeep,
    /// Plus de [`XML_ELEMENTS_MAX`] éléments.
    #[error("too many elements")]
    TooManyElements,
    /// Un second élément racine suit le premier.
    #[error("content after the root element")]
    TrailingContent,
    /// Aucun élément racine.
    #[error("no root element")]
    NoRoot,
}

/// Un enfant d'élément : du texte, ou un élément.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Child {
    /// Texte, entités résolues.
    Text(String),
    /// Élément.
    Element(Node),
}

/// Un élément : nom qualifié (`dc:title`), attributs normalisés, enfants dans l'ordre.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Node {
    pub(crate) name: String,
    attrs: Vec<(String, String)>,
    pub(crate) children: Vec<Child>,
}

impl Node {
    /// Valeur d'un attribut, bords retirés ; `None` s'il manque ou s'il est vide.
    pub(crate) fn attr(&self, name: &str) -> Option<&str> {
        let value = self.attrs.iter().find(|(key, _)| key == name)?.1.trim();
        (!value.is_empty()).then_some(value)
    }

    /// Éléments enfants, dans l'ordre.
    pub(crate) fn elements(&self) -> impl Iterator<Item = &Node> {
        self.children.iter().filter_map(|child| match child {
            Child::Element(node) => Some(node),
            Child::Text(_) => None,
        })
    }

    /// Premier élément enfant de ce nom.
    pub(crate) fn child(&self, name: &str) -> Option<&Node> {
        self.elements().find(|node| node.name == name)
    }

    /// Texte de l'élément et de ses descendants, mis bout à bout.
    pub(crate) fn text(&self) -> String {
        let mut out = String::new();
        self.push_text(&mut out, 0);
        out
    }

    fn push_text(&self, out: &mut String, depth: usize) {
        if depth >= XML_DEPTH_MAX {
            return;
        }
        for child in &self.children {
            match child {
                Child::Text(text) => out.push_str(text),
                Child::Element(node) => node.push_text(out, depth.saturating_add(1)),
            }
        }
    }
}

/// Lit une entrée entière.
pub(crate) fn parse(bytes: &[u8]) -> Result<Node, XmlError> {
    read(bytes, false)
}

/// Lit l'en-tête d'une entrée, coupé : à la première erreur de lecture, rend ce qui est lu.
pub(crate) fn parse_head(bytes: &[u8]) -> Result<Node, XmlError> {
    read(bytes, true)
}

/// État de la lecture : pile des éléments ouverts, racine, éléments comptés.
#[derive(Default)]
struct Tree {
    stack: Vec<Node>,
    root: Option<Node>,
    count: usize,
}

impl Tree {
    /// Ouvre un élément : compte, lit le nom et les attributs (valeurs normalisées).
    fn open(&mut self, start: &BytesStart<'_>) -> Result<Node, XmlError> {
        self.count = self.count.saturating_add(1);
        if self.count > XML_ELEMENTS_MAX {
            return Err(XmlError::TooManyElements);
        }
        if self.stack.len() >= XML_DEPTH_MAX {
            return Err(XmlError::TooDeep);
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
        Ok(Node { name: name.to_owned(), attrs, children: Vec::new() })
    }

    /// Rattache un élément clos à son parent, ou en fait la racine.
    fn attach(&mut self, node: Node) -> Result<(), XmlError> {
        match self.stack.last_mut() {
            Some(parent) => parent.children.push(Child::Element(node)),
            None if self.root.is_none() => self.root = Some(node),
            None => return Err(XmlError::TrailingContent),
        }
        Ok(())
    }

    /// Ajoute du texte à l'élément ouvert ; hors de tout élément, il est perdu.
    fn text(&mut self, text: &str) {
        let Some(top) = self.stack.last_mut() else { return };
        match top.children.last_mut() {
            Some(Child::Text(last)) => last.push_str(text),
            _ => top.children.push(Child::Text(text.to_owned())),
        }
    }

    /// Ferme les éléments encore ouverts et rend la racine.
    fn close_all(mut self) -> Result<Node, XmlError> {
        while let Some(node) = self.stack.pop() {
            self.attach(node)?;
        }
        self.root.ok_or(XmlError::NoRoot)
    }
}

/// Lit `bytes` ; en mode `partial`, une erreur de lecture arrête la lecture sans refuser.
fn read(bytes: &[u8], partial: bool) -> Result<Node, XmlError> {
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        // Une coupure peut tomber au milieu du dernier caractère : seul le préfixe valide est lu.
        // Un octet invalide plus tôt refuse l'entrée, en-tête compris.
        Err(error) if partial && error.error_len().is_none() => bytes
            .get(..error.valid_up_to())
            .and_then(|valid| std::str::from_utf8(valid).ok())
            .unwrap_or_default(),
        Err(_) => return Err(XmlError::NotUtf8),
    };
    let mut reader = Reader::from_str(text);
    let mut tree = Tree::default();
    loop {
        let event = match reader.read_event() {
            Ok(event) => event,
            Err(_) if partial => break,
            Err(_) => return Err(XmlError::Malformed),
        };
        match event {
            Event::Start(start) => {
                let node = tree.open(&start)?;
                tree.stack.push(node);
            }
            Event::Empty(start) => {
                let node = tree.open(&start)?;
                tree.attach(node)?;
            }
            // quick-xml refuse une fin sans début : la pile n'est jamais vide ici.
            Event::End(_) => tree.stack.pop().map_or(Ok(()), |node| tree.attach(node))?,
            Event::Text(text) => tree.text(&text.xml10_content()),
            Event::CData(data) => tree.text(&data.xml10_content()),
            Event::GeneralRef(reference) => tree.text(&resolve(&reference)?),
            Event::DocType(_) => return Err(XmlError::DocType),
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) => {}
            Event::Eof => break,
        }
    }
    if !partial && !tree.stack.is_empty() {
        return Err(XmlError::Malformed);
    }
    tree.close_all()
}

/// Une référence de caractère, ou l'une des cinq entités prédéfinies.
fn resolve(reference: &BytesRef<'_>) -> Result<String, XmlError> {
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
    Ok(resolved.to_string())
}

#[cfg(test)]
mod tests;
