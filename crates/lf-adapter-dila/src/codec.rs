//! Charge utile d'un document dans l'index : un encodage binaire maison.
//!
//! L'index tantivy garde, pour chaque document LEGI, le [`Document`] analysé par l'ingestion
//! (champ stocké `payload`). Le crate n'a pas `serde` et n'en a pas besoin : l'encodage est une
//! suite de champs à longueur explicite (entiers petit-boutiens, chaînes UTF-8 précédées de leur
//! longueur, options précédées d'un octet 0 ou 1), précédée d'un octet de version.
//!
//! La charge ne contient ni le chemin (stocké à part, sous la clé `key`, d'où [`DocPath`] est
//! reclassé) ni rien qui dépende de l'index : elle ne vaut que pour cette version du crate. Un
//! index d'une autre version est refusé par la lecture (version inconnue), jamais mal lu.
//!
//! Le décodeur n'a confiance en rien : tout est borné par les octets restants (une longueur
//! annoncée de 4 Gio ne réserve rien), tout débordement est un `None`.

use lf_domain::date::CivilDate;

use crate::{
    model::{
        Article, Chronicle, Document, Link, LinkTarget, Section, SectionRef, StructLink,
        TextContext, TextStruct, TextVersion, TextVersionLink,
    },
    path::doc_for_key,
    store::StoredDoc,
};

/// Version de l'encodage ; à changer à toute modification de la forme.
const VERSION: u8 = 1;

/// Encode un document (sans son chemin).
pub(crate) fn encode(doc: &StoredDoc) -> Vec<u8> {
    let mut out = Out(Vec::with_capacity(512));
    out.u8(VERSION);
    out.u64(doc.mtime);
    match &doc.document {
        Document::Article(article) => {
            out.u8(0);
            article.write(&mut out);
        }
        Document::Section(section) => {
            out.u8(1);
            section.write(&mut out);
        }
        Document::TextStruct(text) => {
            out.u8(2);
            text.write(&mut out);
        }
        Document::TextVersion(text) => {
            out.u8(3);
            text.write(&mut out);
        }
    }
    out.0
}

/// Décode une charge ; `key` est la clé de stockage dont le chemin est reclassé. `None` si la
/// charge est d'une autre version, tronquée, incohérente ou si la clé n'est plus un chemin du
/// corpus.
pub(crate) fn decode(bytes: &[u8], key: &str) -> Option<StoredDoc> {
    let mut input = In { buf: bytes, pos: 0 };
    if input.u8()? != VERSION {
        return None;
    }
    let mtime = input.u64()?;
    let document = match input.u8()? {
        0 => Document::Article(Article::read(&mut input)?),
        1 => Document::Section(Section::read(&mut input)?),
        2 => Document::TextStruct(TextStruct::read(&mut input)?),
        3 => Document::TextVersion(TextVersion::read(&mut input)?),
        _ => return None,
    };
    if input.pos != bytes.len() {
        return None;
    }
    let path = doc_for_key(key)?;
    if path.id != document.id() || path.kind != document.kind() {
        return None;
    }
    Some(StoredDoc { path, mtime, document })
}

/// Écrivain d'octets.
struct Out(Vec<u8>);

impl Out {
    fn u8(&mut self, value: u8) {
        self.0.push(value);
    }

    fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn len(&mut self, len: usize) {
        // Un membre fait 16 Mio au plus : une longueur ne dépasse jamais `u32`.
        self.0.extend_from_slice(&u32::try_from(len).unwrap_or(u32::MAX).to_le_bytes());
    }

    fn str(&mut self, text: &str) {
        self.len(text.len());
        self.0.extend_from_slice(text.as_bytes());
    }

    fn opt_str(&mut self, text: Option<&String>) {
        match text {
            Some(text) => {
                self.u8(1);
                self.str(text);
            }
            None => self.u8(0),
        }
    }

    fn date(&mut self, date: Option<CivilDate>) {
        match date {
            Some(date) => {
                self.u8(1);
                self.0.extend_from_slice(&date.year().to_le_bytes());
                self.u8(date.month());
                self.u8(date.day());
            }
            None => self.u8(0),
        }
    }

    fn seq<T>(&mut self, items: &[T], write: impl Fn(&T, &mut Self)) {
        self.len(items.len());
        for item in items {
            write(item, self);
        }
    }
}

/// Lecteur d'octets : chaque lecture rend `None` au premier débordement.
struct In<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl In<'_> {
    fn take(&mut self, count: usize) -> Option<&[u8]> {
        let end = self.pos.saturating_add(count);
        let slice = self.buf.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|bytes| bytes[0])
    }

    fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|bytes| u64::from_le_bytes(array(bytes)))
    }

    fn len(&mut self) -> Option<usize> {
        let bytes = self.take(4)?;
        usize::try_from(u32::from_le_bytes(array(bytes))).ok()
    }

    fn str(&mut self) -> Option<String> {
        let len = self.len()?;
        String::from_utf8(self.take(len)?.to_vec()).ok()
    }

    #[expect(
        clippy::option_option,
        reason = "l'extérieur dit « charge illisible », l'intérieur « champ absent »"
    )]
    fn opt_str(&mut self) -> Option<Option<String>> {
        match self.u8()? {
            0 => Some(None),
            1 => self.str().map(Some),
            _ => None,
        }
    }

    #[expect(
        clippy::option_option,
        reason = "l'extérieur dit « charge illisible », l'intérieur « date absente »"
    )]
    fn date(&mut self) -> Option<Option<CivilDate>> {
        match self.u8()? {
            0 => Some(None),
            1 => {
                let year = u16::from_le_bytes(array(self.take(2)?));
                let month = self.u8()?;
                let day = self.u8()?;
                CivilDate::new(year, month, day).map(Some)
            }
            _ => None,
        }
    }

    /// Suite d'éléments ; chaque élément pèse au moins un octet, ce qui borne la réservation.
    fn seq<T>(&mut self, read: impl Fn(&mut Self) -> Option<T>) -> Option<Vec<T>> {
        let len = self.len()?;
        let mut items = Vec::with_capacity(len.min(self.buf.len() - self.pos));
        for _ in 0..len {
            items.push(read(self)?);
        }
        Some(items)
    }
}

/// Les `N` premiers octets de `bytes`, dont la longueur vient d'un `take(N)`.
fn array<const N: usize>(bytes: &[u8]) -> [u8; N] {
    let mut out = [0; N];
    out.copy_from_slice(bytes);
    out
}

impl TextContext {
    fn write(&self, out: &mut Out) {
        out.opt_str(self.cid.as_ref());
        out.opt_str(self.nature.as_ref());
        out.opt_str(self.title.as_ref());
        out.seq(&self.sections, |section, out| {
            out.str(&section.id);
            out.str(&section.title);
            out.date(section.date_debut);
            out.date(section.date_fin);
        });
    }

    fn read(input: &mut In<'_>) -> Option<Self> {
        Some(Self {
            cid: input.opt_str()?,
            nature: input.opt_str()?,
            title: input.opt_str()?,
            sections: input.seq(|input| {
                Some(SectionRef {
                    id: input.str()?,
                    title: input.str()?,
                    date_debut: input.date()?,
                    date_fin: input.date()?,
                })
            })?,
        })
    }
}

impl StructLink {
    fn write(&self, out: &mut Out) {
        out.u8(match self.target {
            LinkTarget::Article => 0,
            LinkTarget::Section => 1,
        });
        out.str(&self.id);
        out.opt_str(self.cid.as_ref());
        out.opt_str(self.num.as_ref());
        match self.level {
            Some(level) => {
                out.u8(1);
                out.0.extend_from_slice(&level.to_le_bytes());
            }
            None => out.u8(0),
        }
        out.opt_str(self.etat.as_ref());
        out.date(self.debut);
        out.date(self.fin);
        out.opt_str(self.title.as_ref());
    }

    fn read(input: &mut In<'_>) -> Option<Self> {
        let target = match input.u8()? {
            0 => LinkTarget::Article,
            1 => LinkTarget::Section,
            _ => return None,
        };
        let id = input.str()?;
        let cid = input.opt_str()?;
        let num = input.opt_str()?;
        let level = match input.u8()? {
            0 => None,
            1 => Some(u16::from_le_bytes(array(input.take(2)?))),
            _ => return None,
        };
        Some(Self {
            target,
            id,
            cid,
            num,
            level,
            etat: input.opt_str()?,
            debut: input.date()?,
            fin: input.date()?,
            title: input.opt_str()?,
        })
    }
}

impl Link {
    fn write(&self, out: &mut Out) {
        out.opt_str(self.link_type.as_ref());
        out.opt_str(self.direction.as_ref());
        out.opt_str(self.id.as_ref());
        out.opt_str(self.text_cid.as_ref());
        out.opt_str(self.text_nature.as_ref());
        out.opt_str(self.text_number.as_ref());
        out.opt_str(self.text_nor.as_ref());
        out.date(self.text_signed);
        out.str(&self.title);
    }

    fn read(input: &mut In<'_>) -> Option<Self> {
        Some(Self {
            link_type: input.opt_str()?,
            direction: input.opt_str()?,
            id: input.opt_str()?,
            text_cid: input.opt_str()?,
            text_nature: input.opt_str()?,
            text_number: input.opt_str()?,
            text_nor: input.opt_str()?,
            text_signed: input.date()?,
            title: input.str()?,
        })
    }
}

impl Chronicle {
    fn write(&self, out: &mut Out) {
        out.opt_str(self.cid.as_ref());
        out.opt_str(self.num.as_ref());
        out.opt_str(self.nor.as_ref());
        out.date(self.date_publi);
        out.date(self.date_texte);
        out.date(self.last_modified);
    }

    fn read(input: &mut In<'_>) -> Option<Self> {
        Some(Self {
            cid: input.opt_str()?,
            num: input.opt_str()?,
            nor: input.opt_str()?,
            date_publi: input.date()?,
            date_texte: input.date()?,
            last_modified: input.date()?,
        })
    }
}

impl Article {
    fn write(&self, out: &mut Out) {
        out.str(&self.id);
        out.opt_str(self.num.as_ref());
        out.opt_str(self.etat.as_ref());
        out.opt_str(self.kind.as_ref());
        out.date(self.date_debut);
        out.date(self.date_fin);
        self.context.write(out);
        out.seq(&self.versions, StructLink::write);
        out.str(&self.nota);
        out.str(&self.content);
        out.seq(&self.links, Link::write);
    }

    fn read(input: &mut In<'_>) -> Option<Self> {
        Some(Self {
            id: input.str()?,
            num: input.opt_str()?,
            etat: input.opt_str()?,
            kind: input.opt_str()?,
            date_debut: input.date()?,
            date_fin: input.date()?,
            context: TextContext::read(input)?,
            versions: input.seq(StructLink::read)?,
            nota: input.str()?,
            content: input.str()?,
            links: input.seq(Link::read)?,
        })
    }
}

impl Section {
    fn write(&self, out: &mut Out) {
        out.str(&self.id);
        out.opt_str(self.title.as_ref());
        self.context.write(out);
        out.seq(&self.structure, StructLink::write);
    }

    fn read(input: &mut In<'_>) -> Option<Self> {
        Some(Self {
            id: input.str()?,
            title: input.opt_str()?,
            context: TextContext::read(input)?,
            structure: input.seq(StructLink::read)?,
        })
    }
}

impl TextStruct {
    fn write(&self, out: &mut Out) {
        out.str(&self.id);
        out.opt_str(self.nature.as_ref());
        self.chronicle.write(out);
        out.seq(&self.versions, |version, out| {
            out.str(&version.id);
            out.opt_str(version.etat.as_ref());
            out.date(version.debut);
            out.date(version.fin);
            out.opt_str(version.num.as_ref());
        });
        out.seq(&self.structure, StructLink::write);
    }

    fn read(input: &mut In<'_>) -> Option<Self> {
        Some(Self {
            id: input.str()?,
            nature: input.opt_str()?,
            chronicle: Chronicle::read(input)?,
            versions: input.seq(|input| {
                Some(TextVersionLink {
                    id: input.str()?,
                    etat: input.opt_str()?,
                    debut: input.date()?,
                    fin: input.date()?,
                    num: input.opt_str()?,
                })
            })?,
            structure: input.seq(StructLink::read)?,
        })
    }
}

impl TextVersion {
    fn write(&self, out: &mut Out) {
        out.str(&self.id);
        out.opt_str(self.nature.as_ref());
        self.chronicle.write(out);
        out.opt_str(self.title.as_ref());
        out.opt_str(self.title_full.as_ref());
        out.opt_str(self.etat.as_ref());
        out.date(self.date_debut);
        out.date(self.date_fin);
        out.opt_str(self.authority.as_ref());
        out.opt_str(self.ministry.as_ref());
        for html in [&self.visas, &self.signatories, &self.tp, &self.nota, &self.abro, &self.rect] {
            out.str(html);
        }
    }

    fn read(input: &mut In<'_>) -> Option<Self> {
        Some(Self {
            id: input.str()?,
            nature: input.opt_str()?,
            chronicle: Chronicle::read(input)?,
            title: input.opt_str()?,
            title_full: input.opt_str()?,
            etat: input.opt_str()?,
            date_debut: input.date()?,
            date_fin: input.date()?,
            authority: input.opt_str()?,
            ministry: input.opt_str()?,
            visas: input.str()?,
            signatories: input.str()?,
            tp: input.str()?,
            nota: input.str()?,
            abro: input.str()?,
            rect: input.str()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        path::{DocKind, DocPath, doc_for_key},
        testing::fixture,
        xml,
    };

    /// Le document réel d'une fixture, rangé sous la clé que lui donne son chemin d'archive.
    fn stored(relative: &str, member: &str, kind: DocKind, mtime: u64) -> StoredDoc {
        let document = xml::parse(&fixture(relative), kind).unwrap();
        let path = doc_for_key(member.trim_end_matches(".xml")).unwrap();
        StoredDoc { path, mtime, document }
    }

    const CODE: &str = "legi/global/code_et_TNC_en_vigueur/code_en_vigueur/LEGI/TEXT/00/00/06/07/42/LEGITEXT000006074232";

    fn architects() -> Vec<StoredDoc> {
        let base = "xml/global-20250713/code_en_vigueur";
        vec![
            stored(
                &format!("{base}/LEGIARTI000006842411.xml"),
                &format!("{CODE}/article/LEGI/ARTI/00/00/06/84/24/LEGIARTI000006842411.xml"),
                DocKind::Article,
                1_700_000_000,
            ),
            stored(
                &format!("{base}/LEGISCTA000006143868.xml"),
                &format!("{CODE}/section_ta/LEGI/SCTA/00/00/06/14/38/LEGISCTA000006143868.xml"),
                DocKind::Section,
                7,
            ),
            stored(
                &format!("{base}/struct_LEGITEXT000006074232.xml"),
                &format!("{CODE}/texte/struct/LEGITEXT000006074232.xml"),
                DocKind::TextStruct,
                0,
            ),
            stored(
                &format!("{base}/version_LEGITEXT000006074232.xml"),
                &format!("{CODE}/texte/version/LEGITEXT000006074232.xml"),
                DocKind::TextVersion,
                u64::MAX,
            ),
        ]
    }

    fn round_trip(doc: &StoredDoc) -> Option<StoredDoc> {
        decode(&encode(doc), &doc.path.key)
    }

    #[test]
    fn every_kind_of_real_document_round_trips() {
        for doc in architects() {
            assert_eq!(round_trip(&doc).as_ref(), Some(&doc), "{:?}", doc.path.kind);
        }
    }

    #[test]
    fn a_fully_populated_document_round_trips() {
        let mut doc = architects().remove(0);
        let mut article = doc.document.as_article().unwrap().clone();
        article.context.sections = vec![SectionRef {
            id: "LEGISCTA000006108694".into(),
            title: "Titre é œ".into(),
            date_debut: CivilDate::new(1980, 3, 25),
            date_fin: None,
        }];
        article.versions = vec![StructLink {
            target: LinkTarget::Section,
            id: "LEGISCTA000006108694".into(),
            cid: Some("LEGISCTA000006108694".into()),
            num: Some("L. 1".into()),
            level: Some(3),
            etat: Some("VIGUEUR".into()),
            debut: CivilDate::new(2222, 2, 22),
            fin: CivilDate::new(2999, 1, 1),
            title: Some("Intitulé".into()),
        }];
        article.links = vec![Link {
            link_type: Some("CITATION".into()),
            direction: Some("cible".into()),
            id: Some("JORFTEXT000000519770".into()),
            text_cid: Some("JORFTEXT000000519770".into()),
            text_nature: Some("DECRET".into()),
            text_number: Some("80-217".into()),
            text_nor: Some("ABCD1234567X".into()),
            text_signed: CivilDate::new(1980, 3, 20),
            title: "Décret 80-217".into(),
        }];
        doc.document = Document::Article(article);
        assert_eq!(round_trip(&doc).as_ref(), Some(&doc));
    }

    #[test]
    fn a_text_version_keeps_its_six_html_blocks_in_order() {
        let mut doc = architects().remove(3);
        let mut text = doc.document.as_text_version().unwrap().clone();
        text.visas = "1".into();
        text.signatories = "2".into();
        text.tp = "3".into();
        text.nota = "4".into();
        text.abro = "5".into();
        text.rect = "6".into();
        text.authority = Some("a".into());
        text.ministry = Some("m".into());
        doc.document = Document::TextVersion(text);
        let back = round_trip(&doc).unwrap();
        let blocks = back.document.as_text_version().unwrap();
        assert_eq!(
            [
                &blocks.visas,
                &blocks.signatories,
                &blocks.tp,
                &blocks.nota,
                &blocks.abro,
                &blocks.rect
            ],
            ["1", "2", "3", "4", "5", "6"]
        );
    }

    #[test]
    fn a_text_struct_keeps_its_version_links() {
        let mut doc = architects().remove(2);
        let mut text = doc.document.as_text_struct().unwrap().clone();
        text.versions = vec![TextVersionLink {
            id: "LEGITEXT000006074232".into(),
            etat: Some("ABROGE".into()),
            debut: CivilDate::new(1980, 3, 25),
            fin: CivilDate::new(2001, 1, 1),
            num: Some("2".into()),
        }];
        doc.document = Document::TextStruct(text);
        assert_eq!(round_trip(&doc).as_ref(), Some(&doc));
    }

    #[test]
    fn every_truncation_of_a_payload_is_refused() {
        for doc in architects() {
            let bytes = encode(&doc);
            for end in 0..bytes.len() {
                assert!(decode(&bytes[..end], &doc.path.key).is_none(), "cut at {end}");
            }
        }
    }

    #[test]
    fn trailing_bytes_are_refused() {
        let doc = architects().remove(0);
        let mut bytes = encode(&doc);
        bytes.push(0);
        assert!(decode(&bytes, &doc.path.key).is_none());
    }

    #[test]
    fn another_version_or_an_unknown_document_tag_is_refused() {
        let doc = architects().remove(0);
        let mut other_version = encode(&doc);
        other_version[0] = VERSION + 1;
        assert!(decode(&other_version, &doc.path.key).is_none());
        let mut unknown_tag = encode(&doc);
        unknown_tag[9] = 9;
        assert!(decode(&unknown_tag, &doc.path.key).is_none());
    }

    #[test]
    fn a_key_that_no_longer_matches_the_payload_is_refused() {
        let docs = architects();
        let bytes = encode(&docs[0]);
        // une autre clé de document (identifiant et nature différents)
        assert!(decode(&bytes, &docs[1].path.key).is_none());
        // une clé qui n'est plus un chemin du corpus
        assert!(decode(&bytes, "legi/global/eli/x").is_none());
        // même identifiant, autre nature : la clé d'un texte ne vaut pas pour une version
        let struct_key = docs[2].path.key.clone();
        let version_bytes = encode(&docs[3]);
        assert!(decode(&version_bytes, &struct_key).is_none());
    }

    #[test]
    fn invalid_flags_and_values_inside_a_payload_are_refused() {
        // Un octet d'option autre que 0 ou 1, une date impossible, une cible de lien inconnue,
        // de l'UTF-8 invalide : chacun fait échouer la lecture, jamais paniquer.
        let docs = architects();
        let article = encode(&docs[0]);
        // 1 (version) + 8 (mtime) + 1 (balise) puis id : 4 + 20 octets, puis l'option `num`
        let num_flag = 10 + 4 + 20;
        let mut bad_flag = article.clone();
        bad_flag[num_flag] = 2;
        assert!(decode(&bad_flag, &docs[0].path.key).is_none());

        // date : année 0 (invalide)
        let mut zero_year = In { buf: &[1, 0, 0, 1, 1], pos: 0 };
        assert_eq!(zero_year.date(), None);
        // date : drapeau inconnu
        assert_eq!(In { buf: &[2], pos: 0 }.date(), None);
        // chaîne : UTF-8 invalide
        assert_eq!(In { buf: &[1, 0, 0, 0, 0xFF], pos: 0 }.str(), None);
        // longueur annoncée énorme, aucun octet derrière
        assert_eq!(In { buf: &[0xFF, 0xFF, 0xFF, 0xFF], pos: 0 }.str(), None);
        assert!(In { buf: &[0xFF, 0xFF, 0xFF, 0xFF], pos: 0 }.seq(In::u8).is_none());
        // lien de structure : cible inconnue, drapeau de niveau inconnu
        assert!(StructLink::read(&mut In { buf: &[7], pos: 0 }).is_none());
        let mut level = Vec::new();
        level.push(0); // article
        level.extend_from_slice(&0u32.to_le_bytes()); // id vide
        level.extend_from_slice(&[0, 0, 9]); // cid, num absents, drapeau de niveau 9
        assert!(StructLink::read(&mut In { buf: &level, pos: 0 }).is_none());
    }

    #[test]
    fn the_path_of_a_decoded_document_is_reclassified_from_its_key() {
        let doc = architects().remove(0);
        let back: DocPath = round_trip(&doc).unwrap().path;
        assert_eq!(back, doc.path);
    }
}
