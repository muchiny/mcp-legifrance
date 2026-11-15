//! Passage des réponses du portail aux valeurs du domaine, et contrôle de ce qu'il rend
//! (ADR-0029, décision 6).
//!
//! Le portail n'est pas cru sur parole. Une réponse est refusée ([`DoctrineError::Decode`]) si :
//!
//! - elle n'est pas le JSON attendu, ou porte plus d'enregistrements qu'une page n'en tient (le
//!   décodage s'arrête alors au premier de trop, voir `dto`) ;
//! - elle porte plus d'enregistrements que demandé, ou un total incohérent avec eux ;
//! - un enregistrement n'a pas d'identifiant valide, pas de numéro de publication lisible dans
//!   son permalien, ou une nature inconnue ;
//! - en recherche, un résultat sort de la nature demandée ;
//! - en lecture, une publication porte un autre identifiant que celui demandé, ou deux
//!   publications portent le même numéro (il ne départagerait plus rien).
//!
//! Le reste est toléré et **compté** ([`Anomalies`]) : une série ou une date illisible est
//! omise, une adresse hors de la grammaire stricte n'est pas rendue, une entité inconnue reste
//! telle quelle dans le texte.
//!
//! Aucun `detail` d'erreur ne cite une valeur du corps : seulement un texte fixe et, pour un
//! JSON invalide, la position de la faute.

use std::collections::BTreeSet;

use lf_application::ports::DoctrineError;
use lf_domain::{
    date::CivilDate,
    doctrine::{
        BofipUrl, BoiId, DoctrineDoc, DoctrineHit, DoctrineKind, DoctrinePage, KindFilter,
        Permalink, PublicationId, Serie, clean_line, clean_text,
    },
};

use crate::{
    dto::{Page, Record},
    query::{GET_LIMIT, portal_kind},
};

/// Ce qu'une réponse acceptée avait d'inattendu. Des compteurs, jamais des valeurs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Anomalies {
    /// Entités HTML inconnues laissées dans un texte.
    pub(crate) unknown_entities: u32,
    /// Champs facultatifs omis parce qu'illisibles : série, début de validité, adresse.
    pub(crate) dropped_fields: u32,
}

impl Anomalies {
    /// Vrai s'il n'y a rien à signaler.
    pub(crate) fn is_empty(self) -> bool {
        self == Self::default()
    }

    /// Compte un champ facultatif omis.
    fn drop_field(&mut self) {
        self.dropped_fields = self.dropped_fields.saturating_add(1);
    }
}

/// Valeur décodée et ce que la réponse avait d'inattendu.
#[derive(Debug, PartialEq)]
pub(crate) struct Decoded<R> {
    /// La valeur du domaine.
    pub(crate) value: R,
    /// Les anomalies comptées en chemin.
    pub(crate) anomalies: Anomalies,
}

/// Page de résultats d'une recherche qui demandait `limit` résultats de nature `filter`.
///
/// # Errors
///
/// [`DoctrineError::Decode`] pour une réponse non conforme (voir le module).
pub(crate) fn page(
    body: &[u8],
    limit: u32,
    filter: KindFilter,
) -> Result<Decoded<DoctrinePage>, DoctrineError> {
    let page = parse(body, "search")?;
    let count = bounded_count(&page, limit)?;
    if page.total_count < count {
        return Err(decode("search response: total lower than the number of results"));
    }
    let mut anomalies = Anomalies::default();
    let hits = page
        .results
        .iter()
        .map(|record| hit(record, filter, &mut anomalies))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Decoded { value: DoctrinePage { total: page.total_count, hits }, anomalies })
}

/// Publications qui portent l'identifiant `requested`, avec leur texte.
///
/// # Errors
///
/// [`DoctrineError::NotFound`] si aucune publication ne le porte ; [`DoctrineError::Decode`]
/// pour une réponse non conforme (voir le module) ; [`DoctrineError::TooLarge`] si un texte
/// dépasse la borne du domaine.
pub(crate) fn docs(
    body: &[u8],
    requested: &BoiId,
) -> Result<Decoded<Vec<DoctrineDoc>>, DoctrineError> {
    let page = parse(body, "get")?;
    let count = bounded_count(&page, GET_LIMIT)?;
    // Un total plus grand que ce qui est rendu serait une liste tronquée : refusée, pas servie.
    if page.total_count != count {
        return Err(decode("get response: total differs from the number of results"));
    }
    if page.results.is_empty() {
        return Err(DoctrineError::NotFound);
    }
    let mut anomalies = Anomalies::default();
    let docs = page
        .results
        .iter()
        .map(|record| doc(record, requested, &mut anomalies))
        .collect::<Result<Vec<_>, _>>()?;
    // Le numéro départage les publications d'un même identifiant : deux fois le même, et
    // `publication_id` ne désignerait que la première.
    let mut numbers = BTreeSet::new();
    if !docs.iter().all(|doc| numbers.insert(doc.publication_id)) {
        return Err(decode("get response: two publications share a publication number"));
    }
    Ok(Decoded { value: docs, anomalies })
}

/// Lit le JSON d'une réponse. Le `detail` ne donne que la position de la faute : le message de
/// `serde_json` citerait une valeur du corps.
fn parse(body: &[u8], route: &str) -> Result<Page, DoctrineError> {
    serde_json::from_slice(body).map_err(|error| {
        decode(&format!(
            "{route} response is not the expected JSON (line {}, column {})",
            error.line(),
            error.column()
        ))
    })
}

/// Nombre d'enregistrements de `page`, refusé s'il dépasse `limit`.
fn bounded_count(page: &Page, limit: u32) -> Result<u64, DoctrineError> {
    let count = u64::try_from(page.results.len()).unwrap_or(u64::MAX);
    if count > u64::from(limit) {
        return Err(decode("more results than the requested limit"));
    }
    Ok(count)
}

/// Erreur de décodage au texte fixe `detail`.
fn decode(detail: &str) -> DoctrineError {
    DoctrineError::Decode { detail: detail.to_owned() }
}

/// Ce qu'une recherche et une lecture lisent de la même façon dans un enregistrement.
struct Head {
    id: BoiId,
    publication_id: PublicationId,
    source_url: Option<BofipUrl>,
    title: String,
    kind: DoctrineKind,
    serie: Option<Serie>,
    valid_from: Option<CivilDate>,
}

/// Lit l'en-tête d'un enregistrement ; ce qui désigne la publication est exigé, le reste toléré.
fn head(record: &Record, anomalies: &mut Anomalies) -> Result<Head, DoctrineError> {
    let id = record
        .identifiant_juridique
        .as_deref()
        .and_then(|raw| BoiId::parse(raw).ok())
        .ok_or_else(|| decode("record without a valid identifier"))?;
    let link = record
        .permalien
        .as_deref()
        .and_then(Permalink::parse)
        .ok_or_else(|| decode("record without a publication number in its permalink"))?;
    let kind = record
        .kind
        .as_deref()
        .and_then(kind_of)
        .ok_or_else(|| decode("record of an unknown type"))?;
    if link.url.is_none() {
        anomalies.drop_field();
    }
    Ok(Head {
        id,
        publication_id: link.publication_id,
        source_url: link.url,
        title: clean_line(record.titre.as_deref().unwrap_or_default()),
        kind,
        serie: lenient(record.serie.as_deref(), |raw| Serie::parse(raw).ok(), anomalies),
        valid_from: lenient(
            record.debut_de_validite.as_deref(),
            |raw| CivilDate::parse(raw).ok(),
            anomalies,
        ),
    })
}

/// Nature d'un enregistrement, d'après le nom que lui donne le portail ; `None` pour un autre nom.
fn kind_of(raw: &str) -> Option<DoctrineKind> {
    [DoctrineKind::Doctrine, DoctrineKind::News].into_iter().find(|kind| portal_kind(*kind) == raw)
}

/// Champ facultatif : absent ou blanc, il est `None` sans rien compter ; présent mais illisible,
/// il est omis et compté.
fn lenient<V>(
    raw: Option<&str>,
    read: impl Fn(&str) -> Option<V>,
    anomalies: &mut Anomalies,
) -> Option<V> {
    let raw = raw.map(str::trim).filter(|raw| !raw.is_empty())?;
    let value = read(raw);
    if value.is_none() {
        anomalies.drop_field();
    }
    value
}

/// Résultat de recherche ; refusé s'il sort de la nature demandée.
fn hit(
    record: &Record,
    filter: KindFilter,
    anomalies: &mut Anomalies,
) -> Result<DoctrineHit, DoctrineError> {
    let head = head(record, anomalies)?;
    if filter.kind().is_some_and(|wanted| wanted != head.kind) {
        return Err(decode("search result outside the requested kind"));
    }
    Ok(DoctrineHit {
        id: head.id,
        publication_id: head.publication_id,
        title: head.title,
        kind: head.kind,
        serie: head.serie,
        valid_from: head.valid_from,
        source_url: head.source_url,
    })
}

/// Publication lue ; refusée si elle ne porte pas l'identifiant `requested`.
fn doc(
    record: &Record,
    requested: &BoiId,
    anomalies: &mut Anomalies,
) -> Result<DoctrineDoc, DoctrineError> {
    let head = head(record, anomalies)?;
    if head.id != *requested {
        return Err(decode("returned identifier differs from the requested one"));
    }
    let (text, unknown) =
        clean_text(record.contenu.as_deref().unwrap_or_default()).map_err(|_| {
            DoctrineError::TooLarge { detail: "publication text over the domain limit".to_owned() }
        })?;
    anomalies.unknown_entities = anomalies.unknown_entities.saturating_add(unknown);
    Ok(DoctrineDoc {
        id: head.id,
        publication_id: head.publication_id,
        title: head.title,
        kind: head.kind,
        serie: head.serie,
        division: record
            .division
            .as_deref()
            .map(clean_line)
            .filter(|division| !division.is_empty()),
        valid_from: head.valid_from,
        source_url: head.source_url,
        text,
    })
}

#[cfg(test)]
mod tests {
    use lf_domain::text::LongText;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::testing::fixture;

    const ID: &str = "BOI-ENR-DMTG-10-50-20";
    const LINK: &str = "https://bofip.impots.gouv.fr/bofip/3369-PGP.html/identifiant=BOI-ENR-DMTG-10-50-20-20230524";

    fn id() -> BoiId {
        BoiId::parse(ID).unwrap()
    }

    /// Enregistrement JSON complet, dont `changes` remplace ou retire (`null`) des champs.
    fn record(changes: &[(&str, serde_json::Value)]) -> serde_json::Value {
        let mut record = serde_json::json!({
            "identifiant_juridique": ID,
            "titre": "ENR - Mutations à titre gratuit",
            "type": "Contenu",
            "serie": "ENR",
            "division": "DMTG",
            "debut_de_validite": "2023-05-24",
            "permalien": LINK,
            "contenu": "Premier paragraphe.\nSecond paragraphe.",
        });
        for (name, value) in changes {
            record[*name] = value.clone();
        }
        record
    }

    /// Corps d'une réponse : `total` et les enregistrements.
    fn body(total: u64, records: &[serde_json::Value]) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({ "total_count": total, "results": records }))
            .unwrap()
    }

    fn one(changes: &[(&str, serde_json::Value)]) -> Vec<u8> {
        body(1, &[record(changes)])
    }

    /// Enregistrement complet dont le permalien porte le numéro `number`.
    fn numbered(number: u32) -> serde_json::Value {
        let link = format!("https://bofip.impots.gouv.fr/bofip/{number}-PGP.html/identifiant={ID}");
        record(&[("permalien", link.into())])
    }

    /// `count` enregistrements complets, de numéros distincts.
    fn distinct(count: u32) -> Vec<serde_json::Value> {
        (1..=count).map(numbered).collect()
    }

    fn decode_error(detail: &str) -> DoctrineError {
        DoctrineError::Decode { detail: detail.to_owned() }
    }

    fn text(plain: &str) -> LongText {
        LongText::from_plain(plain).unwrap()
    }

    fn expected_doc() -> DoctrineDoc {
        let link = Permalink::parse(LINK).unwrap();
        DoctrineDoc {
            id: id(),
            publication_id: link.publication_id,
            title: "ENR - Mutations à titre gratuit".to_owned(),
            kind: DoctrineKind::Doctrine,
            serie: Some(Serie::parse("ENR").unwrap()),
            division: Some("DMTG".to_owned()),
            valid_from: CivilDate::new(2023, 5, 24),
            source_url: link.url,
            text: text("Premier paragraphe.\nSecond paragraphe."),
        }
    }

    fn expected_hit() -> DoctrineHit {
        let doc = expected_doc();
        DoctrineHit {
            id: doc.id,
            publication_id: doc.publication_id,
            title: doc.title,
            kind: doc.kind,
            serie: doc.serie,
            valid_from: doc.valid_from,
            source_url: doc.source_url,
        }
    }

    const CLEAN: Anomalies = Anomalies { unknown_entities: 0, dropped_fields: 0 };

    // ---- lecture ----------------------------------------------------------------------------

    #[test]
    fn a_complete_record_becomes_a_publication() {
        let decoded = docs(&one(&[]), &id()).unwrap();
        assert_eq!(decoded, Decoded { value: vec![expected_doc()], anomalies: CLEAN });
        assert_eq!(decoded.value[0].publication_id.get(), 3369);
        assert!(decoded.anomalies.is_empty());
    }

    #[test]
    fn returned_id_equals_requested_id() {
        let other = BoiId::parse("BOI-ENR-DMTG-10-50-30").unwrap();
        let expected = Err(decode_error("returned identifier differs from the requested one"));
        assert_eq!(docs(&one(&[]), &other), expected);
        // Une seule publication étrangère parmi d'autres suffit à refuser la réponse.
        let foreign = record(&[("identifiant_juridique", "BOI-ENR-DMTG-10-50-30".into())]);
        assert_eq!(docs(&body(2, &[record(&[]), foreign]), &id()), expected);
        // La casse rendue par le portail est normalisée avant la comparaison.
        let lower = one(&[("identifiant_juridique", ID.to_lowercase().into())]);
        assert_eq!(docs(&lower, &id()).unwrap().value, vec![expected_doc()]);
    }

    #[test]
    fn results_over_limit_is_decode() {
        let expected = decode_error("more results than the requested limit");
        // Lecture : dix publications au plus.
        assert_eq!(docs(&body(10, &distinct(10)), &id()).unwrap().value.len(), 10);
        assert_eq!(docs(&body(11, &distinct(11)), &id()), Err(expected.clone()));
        // Recherche : jamais plus que la taille de page demandée.
        let three = vec![record(&[]); 3];
        assert_eq!(page(&body(40, &three), 3, KindFilter::All).unwrap().value.hits.len(), 3);
        assert_eq!(page(&body(40, &three), 2, KindFilter::All), Err(expected));
    }

    /// Au-delà de la plus grande page, le décodage s'arrête : le refus est celui d'un JSON
    /// inattendu, avec sa position, et non celui de la limite demandée.
    #[test]
    fn a_body_with_more_records_than_a_page_is_refused_while_it_is_read() {
        let full = vec![record(&[]); 100];
        assert_eq!(page(&body(100, &full), 100, KindFilter::All).unwrap().value.hits.len(), 100);
        let over = vec![record(&[]); 101];
        for (route, refused) in [
            ("search", page(&body(101, &over), 100, KindFilter::All).map(|_| ())),
            ("get", docs(&body(101, &over), &id()).map(|_| ())),
        ] {
            let shown = format!("{refused:?}");
            let start = format!(
                "Err(Decode {{ detail: \"{route} response is not the expected JSON (line 1, column "
            );
            assert!(shown.starts_with(&start), "{shown}");
        }
    }

    #[test]
    fn two_publications_with_the_same_number_are_refused() {
        let expected = decode_error("get response: two publications share a publication number");
        let twice = [numbered(3369), numbered(3369)];
        assert_eq!(docs(&body(2, &twice), &id()), Err(expected.clone()));
        // Le doublon est vu où qu'il soit dans la liste.
        let apart = [numbered(3369), numbered(13_033), numbered(3369)];
        assert_eq!(docs(&body(3, &apart), &id()), Err(expected));
        let two = docs(&body(2, &[numbered(3369), numbered(13_033)]), &id()).unwrap().value;
        let numbers: Vec<u32> = two.iter().map(|doc| doc.publication_id.get()).collect();
        assert_eq!(numbers, [3369, 13_033]);
    }

    #[test]
    fn a_read_whose_total_differs_from_its_results_is_refused() {
        let expected = Err(decode_error("get response: total differs from the number of results"));
        // Plus de publications annoncées que rendues : une liste tronquée.
        assert_eq!(docs(&body(2, &[record(&[])]), &id()), expected);
        assert_eq!(docs(&body(0, &[record(&[])]), &id()), expected);
        assert_eq!(docs(&body(1, &[]), &id()), expected);
    }

    #[test]
    fn no_publication_is_not_found() {
        assert_eq!(docs(&body(0, &[]), &id()), Err(DoctrineError::NotFound));
    }

    #[test]
    fn a_publication_without_text_has_an_empty_text() {
        for contenu in [serde_json::Value::Null, "".into(), " \n&#13;\n".into()] {
            let decoded = docs(&one(&[("contenu", contenu)]), &id()).unwrap();
            assert_eq!(
                decoded,
                Decoded {
                    value: vec![DoctrineDoc { text: text(""), ..expected_doc() }],
                    anomalies: CLEAN
                }
            );
        }
    }

    #[test]
    fn the_text_is_cleaned_and_its_unknown_entities_are_counted() {
        let raw = "1&#13;\nTaux&nbsp;: 5 % &amp; 10 %&#13;\n&eacute;t&eacute;";
        let decoded = docs(&one(&[("contenu", raw.into())]), &id()).unwrap();
        assert_eq!(decoded.value[0].text.as_str(), "1\n\nTaux : 5 % & 10 %\n\n&eacute;t&eacute;");
        assert_eq!(decoded.anomalies, Anomalies { unknown_entities: 2, dropped_fields: 0 });
        assert!(!decoded.anomalies.is_empty());
        // Les anomalies de deux publications s'additionnent.
        let mut first = numbered(3369);
        first["contenu"] = raw.into();
        let mut second = numbered(13_033);
        second["contenu"] = "&bull;".into();
        assert_eq!(docs(&body(2, &[first, second]), &id()).unwrap().anomalies.unknown_entities, 3);
    }

    #[test]
    fn a_text_over_the_domain_limit_is_too_large() {
        let huge = "a".repeat(usize::try_from(LongText::MAX_CHARS).unwrap() + 1);
        assert_eq!(
            docs(&one(&[("contenu", huge.into())]), &id()),
            Err(DoctrineError::TooLarge {
                detail: "publication text over the domain limit".to_owned()
            })
        );
    }

    #[test]
    fn title_and_division_are_put_on_one_line() {
        let changes = [
            ("titre", " ENR -\u{a0}Mutations &amp; partages\r\n".into()),
            ("division", " DMTG\n".into()),
        ];
        let decoded = docs(&one(&changes), &id()).unwrap();
        assert_eq!(decoded.value[0].title, "ENR - Mutations & partages");
        assert_eq!(decoded.value[0].division.as_deref(), Some("DMTG"));
        // Sans titre : un titre vide, pas un refus. Division absente ou blanche : omise.
        for division in [serde_json::Value::Null, "  ".into()] {
            let changes = [("titre", serde_json::Value::Null), ("division", division)];
            let decoded = docs(&one(&changes), &id()).unwrap();
            assert_eq!((decoded.value[0].title.as_str(), &decoded.value[0].division), ("", &None));
            assert_eq!(decoded.anomalies, CLEAN);
        }
    }

    // ---- ce qui désigne la publication est exigé ------------------------------------------------

    #[test]
    fn a_record_without_a_valid_identifier_is_refused() {
        let expected = decode_error("record without a valid identifier");
        for value in [serde_json::Value::Null, "".into(), "BOI ENR".into(), "BOI-ENR\"".into()] {
            let changes = [("identifiant_juridique", value)];
            assert_eq!(docs(&one(&changes), &id()), Err(expected.clone()));
            assert_eq!(page(&one(&changes), 10, KindFilter::All), Err(expected.clone()));
        }
    }

    #[test]
    fn a_record_without_a_publication_number_is_refused() {
        let expected = decode_error("record without a publication number in its permalink");
        let links = [
            serde_json::Value::Null,
            "".into(),
            "https://example.org/bofip/3369-PGP.html".into(),
            "https://bofip.impots.gouv.fr/bofip/-PGP.html".into(),
            "http://bofip.impots.gouv.fr/bofip/3369-PGP.html".into(),
        ];
        for value in links {
            let changes = [("permalien", value)];
            assert_eq!(docs(&one(&changes), &id()), Err(expected.clone()));
            assert_eq!(page(&one(&changes), 10, KindFilter::All), Err(expected.clone()));
        }
    }

    #[test]
    fn a_record_of_an_unknown_type_is_refused() {
        let expected = decode_error("record of an unknown type");
        for value in [serde_json::Value::Null, "".into(), "contenu".into(), "Rescrit".into()] {
            let changes = [("type", value)];
            assert_eq!(docs(&one(&changes), &id()), Err(expected.clone()));
            assert_eq!(page(&one(&changes), 10, KindFilter::All), Err(expected.clone()));
        }
    }

    // ---- le reste est toléré et compté -----------------------------------------------------------

    #[test]
    fn an_address_outside_the_strict_grammar_is_not_rendered_but_keeps_its_number() {
        let odd = "https://bofip.impots.gouv.fr/bofip/3369-PGP.html?x=1";
        let decoded = docs(&one(&[("permalien", odd.into())]), &id()).unwrap();
        assert_eq!(decoded.value, vec![DoctrineDoc { source_url: None, ..expected_doc() }]);
        assert_eq!(decoded.anomalies, Anomalies { unknown_entities: 0, dropped_fields: 1 });
    }

    #[test]
    fn an_unreadable_serie_or_date_is_dropped_and_counted() {
        let changes = [("serie", "EN-R".into()), ("debut_de_validite", "24/05/2023".into())];
        let decoded = docs(&one(&changes), &id()).unwrap();
        assert_eq!(
            decoded.value,
            vec![DoctrineDoc { serie: None, valid_from: None, ..expected_doc() }]
        );
        assert_eq!(decoded.anomalies, Anomalies { unknown_entities: 0, dropped_fields: 2 });
        // Absents ou blancs : omis sans rien compter (six publications n'ont pas de série).
        for blank in [serde_json::Value::Null, "".into(), "  ".into()] {
            let changes = [("serie", blank.clone()), ("debut_de_validite", blank)];
            let decoded = docs(&one(&changes), &id()).unwrap();
            assert_eq!(
                decoded.value,
                vec![DoctrineDoc { serie: None, valid_from: None, ..expected_doc() }]
            );
            assert_eq!(decoded.anomalies, CLEAN);
        }
        // Espaces autour et casse : lus.
        let changes = [("serie", " enr ".into()), ("debut_de_validite", " 2023-05-24 ".into())];
        assert_eq!(
            docs(&one(&changes), &id()).unwrap(),
            Decoded { value: vec![expected_doc()], anomalies: CLEAN }
        );
    }

    // ---- recherche ---------------------------------------------------------------------------

    #[test]
    fn a_search_page_keeps_the_total_and_the_order() {
        let news = record(&[
            ("identifiant_juridique", "ACTU-2021-00157".into()),
            ("type", "Actualité".into()),
            (
                "permalien",
                "https://bofip.impots.gouv.fr/bofip/13033-PGP.html/ACTU-2021-00157".into(),
            ),
        ]);
        let decoded = page(&body(433, &[news, record(&[])]), 10, KindFilter::All).unwrap();
        assert_eq!(decoded.anomalies, CLEAN);
        assert_eq!(decoded.value.total, 433);
        let ids: Vec<(&str, u32, DoctrineKind)> = decoded
            .value
            .hits
            .iter()
            .map(|hit| (hit.id.as_str(), hit.publication_id.get(), hit.kind))
            .collect();
        assert_eq!(
            ids,
            [("ACTU-2021-00157", 13_033, DoctrineKind::News), (ID, 3369, DoctrineKind::Doctrine)]
        );
        assert_eq!(decoded.value.hits[1], expected_hit());
    }

    #[test]
    fn an_empty_search_page_is_a_result() {
        let decoded = page(&body(0, &[]), 10, KindFilter::Doctrine).unwrap();
        assert_eq!(
            decoded,
            Decoded { value: DoctrinePage { total: 0, hits: Vec::new() }, anomalies: CLEAN }
        );
        // Une page au-delà de la fin : un total, aucun résultat.
        assert_eq!(page(&body(433, &[]), 10, KindFilter::Doctrine).unwrap().value.total, 433);
    }

    #[test]
    fn a_search_total_lower_than_its_results_is_refused() {
        let two = [record(&[]), record(&[])];
        assert_eq!(
            page(&body(1, &two), 10, KindFilter::All),
            Err(decode_error("search response: total lower than the number of results"))
        );
        assert_eq!(page(&body(2, &two), 10, KindFilter::All).unwrap().value.total, 2);
    }

    #[test]
    fn a_search_result_outside_the_requested_kind_is_refused() {
        let expected = decode_error("search result outside the requested kind");
        let doctrine = one(&[]);
        let news = one(&[("type", "Actualité".into())]);
        assert_eq!(page(&doctrine, 10, KindFilter::News), Err(expected.clone()));
        assert_eq!(page(&news, 10, KindFilter::Doctrine), Err(expected));
        assert_eq!(page(&doctrine, 10, KindFilter::Doctrine).unwrap().value.hits, [expected_hit()]);
        let news_hit = DoctrineHit { kind: DoctrineKind::News, ..expected_hit() };
        for filter in [KindFilter::News, KindFilter::All] {
            assert_eq!(
                page(&news, 10, filter).unwrap().value.hits,
                std::slice::from_ref(&news_hit)
            );
        }
    }

    #[test]
    fn search_anomalies_are_counted_across_the_page() {
        let odd =
            record(&[("permalien", "https://bofip.impots.gouv.fr/bofip/3369-PGP.html?x=1".into())]);
        let undated = record(&[("debut_de_validite", "hier".into())]);
        let decoded = page(&body(3, &[odd, undated, record(&[])]), 10, KindFilter::All).unwrap();
        assert_eq!(decoded.anomalies, Anomalies { unknown_entities: 0, dropped_fields: 2 });
        assert_eq!(decoded.value.hits[0].source_url, None);
        assert_eq!(decoded.value.hits[1].valid_from, None);
    }

    // ---- JSON ----------------------------------------------------------------------------------

    /// Le détail donne la position de la faute, jamais la valeur fautive.
    #[test]
    fn invalid_json_is_refused_without_quoting_the_body() {
        let rows: [(&[u8], &str); 5] = [
            (b"", "line 1, column 0"),
            (b"<html>secret</html>", "line 1, column 1"),
            (br#"{"total_count": "secret", "results": []}"#, "line 1, column 24"),
            (br#"{"results": []}"#, "line 1, column 15"),
            (b"{\"total_count\": 1,\n \"results\": [\xff]}", "line 2, column 14"),
        ];
        for (body, position) in rows {
            let search = format!("search response is not the expected JSON ({position})");
            assert_eq!(page(body, 10, KindFilter::All), Err(decode_error(&search)));
            let get = format!("get response is not the expected JSON ({position})");
            assert_eq!(docs(body, &id()), Err(decode_error(&get)));
        }
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let mut extended = record(&[]);
        extended["contenu_html"] = "<p>ignoré</p>".into();
        extended["nouveau_champ"] = serde_json::json!({ "a": [1, 2] });
        let body = serde_json::to_vec(&serde_json::json!({
            "total_count": 1, "results": [extended], "pagination": { "next": null }
        }))
        .unwrap();
        assert_eq!(docs(&body, &id()).unwrap().value, vec![expected_doc()]);
    }

    // ---- réponses réelles du portail (tests/fixtures) --------------------------------------------

    #[test]
    fn real_search_pages_are_decoded_whole() {
        let rows = [
            (
                "search-donation-page1.json",
                10,
                KindFilter::Doctrine,
                433,
                10,
                "BOI-ENR-DMTG-20-30-20-30",
                2699,
            ),
            (
                "search-donation-page2.json",
                10,
                KindFilter::Doctrine,
                433,
                10,
                "BOI-IF-CFE-20-20-20-10",
                4325,
            ),
            ("search-empty.json", 10, KindFilter::Doctrine, 0, 0, "", 0),
            (
                "search-serie-enr.json",
                5,
                KindFilter::Doctrine,
                46,
                5,
                "BOI-ENR-DMTG-10-60-10",
                1500,
            ),
            ("search-news.json", 5, KindFilter::News, 40, 5, "ACTU-2012-00012", 7946),
            ("search-all.json", 5, KindFilter::All, 347, 5, "BOI-ENR-DMTG-10-20-30-20", 2236),
        ];
        for (name, limit, filter, total, count, first_id, first_number) in rows {
            let decoded = page(&fixture(name), limit, filter).unwrap();
            assert_eq!(decoded.anomalies, CLEAN, "{name}");
            assert_eq!((decoded.value.total, decoded.value.hits.len()), (total, count), "{name}");
            let first =
                decoded.value.hits.first().map(|hit| (hit.id.as_str(), hit.publication_id.get()));
            assert_eq!(first.unwrap_or_default(), (first_id, first_number), "{name}");
            // Chaque résultat réel a un titre, une date et une adresse rendue.
            let complete = decoded.value.hits.iter().filter(|hit| {
                !hit.title.is_empty() && hit.valid_from.is_some() && hit.source_url.is_some()
            });
            assert_eq!(complete.count(), count, "{name}");
        }
    }

    #[test]
    fn the_mixed_search_page_holds_both_kinds() {
        let decoded = page(&fixture("search-all.json"), 5, KindFilter::All).unwrap();
        let kinds: Vec<DoctrineKind> = decoded.value.hits.iter().map(|hit| hit.kind).collect();
        let (doctrine, news) = (DoctrineKind::Doctrine, DoctrineKind::News);
        assert_eq!(kinds, [doctrine, doctrine, news, news, doctrine]);
        // La même page, lue comme une réponse de doctrine seule, serait refusée.
        assert_eq!(
            page(&fixture("search-all.json"), 5, KindFilter::Doctrine),
            Err(decode_error("search result outside the requested kind"))
        );
    }

    #[test]
    fn a_real_publication_is_read_with_its_text() {
        let decoded = docs(&fixture("get-single.json"), &id()).unwrap();
        assert_eq!((decoded.anomalies, decoded.value.len()), (CLEAN, 1));
        let doc = &decoded.value[0];
        assert_eq!(
            (doc.id.as_str(), doc.publication_id.get(), doc.kind, doc.valid_from),
            (ID, 3369, DoctrineKind::Doctrine, CivilDate::new(2023, 5, 24))
        );
        assert_eq!(doc.serie.as_ref().map(Serie::as_str), Some("ENR"));
        assert_eq!(doc.division.as_deref(), Some("DMTG"));
        assert_eq!(doc.source_url.as_ref().map(BofipUrl::as_str), Some(LINK));
        assert!(doc.title.starts_with("ENR - Mutations à titre gratuit de meubles ou d'immeubles"));
        assert!(doc.text.as_str().starts_with("Actualité liée : 24/05/2023 : ENR"));
        // Ni entité de fin de ligne ni retour chariot dans le texte canonique.
        assert_eq!(
            (doc.text.as_str().contains("&#13;"), doc.text.as_str().contains('\r')),
            (false, false)
        );
        assert_eq!(doc.citation(), "BOFiP, BOI-ENR-DMTG-10-50-20, 24 mai 2023");
    }

    #[test]
    fn two_real_publications_share_an_identifier_and_a_date() {
        let id = BoiId::parse("BOI-IF-TH-10-50-40").unwrap();
        let decoded = docs(&fixture("get-double-same-date.json"), &id).unwrap();
        let seen: Vec<(u32, Option<CivilDate>)> =
            decoded.value.iter().map(|doc| (doc.publication_id.get(), doc.valid_from)).collect();
        let date = CivilDate::new(2020, 12, 22);
        assert_eq!(seen, [(11_040, date), (648, date)]);
        assert_ne!(decoded.value[0].title, decoded.value[1].title);
    }

    #[test]
    fn the_longest_real_publication_fits() {
        let id = BoiId::parse("BOI-ANNX-000050").unwrap();
        let decoded = docs(&fixture("get-longest.json"), &id).unwrap();
        let doc = &decoded.value[0];
        assert_eq!((decoded.value.len(), doc.publication_id.get()), (1, 6273));
        // 167 535 caractères dans le jeu ; un peu plus une fois le texte mis en paragraphes
        // (168 548), moins un : un seul saut de ligne y est recollé. C'est la limite connue
        // d'AM-95, un nom coupé à son trait d'union (« Savignésous- », puis « le-Lude »), qui
        // reçoit une espace de trop.
        assert_eq!(doc.text.as_str().chars().count(), 168_547);
    }

    /// Publication de 2018 dont le portail sert le texte avec des sauts de ligne au milieu des
    /// phrases : six sont recollés (49 lignes, 43 paragraphes). Les titres, les numéros de
    /// paragraphe et les cellules du tableau restent séparés ; une ligne qui commence par un
    /// chiffre n'est pas recollée (« L'article », puis « 977 du CGI »).
    #[test]
    fn a_real_wrapped_publication_is_read_in_whole_sentences() {
        let id = BoiId::parse("BOI-PAT-IFI-40-10").unwrap();
        let decoded = docs(&fixture("get-wrapped.json"), &id).unwrap();
        assert_eq!((decoded.anomalies, decoded.value.len()), (CLEAN, 1));
        let text = &decoded.value[0].text;
        let paragraphs: Vec<&str> = text.paragraphs().map(|(_, paragraph)| paragraph).collect();
        assert_eq!((paragraphs.len(), text.as_str().chars().count()), (43, 1_868));
        assert_eq!(
            paragraphs[..9],
            [
                "I. Seuil d'imposition",
                "1",
                "Le seuil d’assujettissement à l'impôt sur la fortune immobilière (IFI) est prévu \
                 à l'article 964 du code général des impôts (CGI). Ainsi, sont redevables de \
                 l'IFI les personnes physiques dont la valeur nette des actifs immobiliers \
                 imposables au 1er janvier de l'année d'imposition est supérieure à 1,3 M€.",
                "II. Tarif",
                "10",
                "L'article",
                "977 du CGI fixe le tarif de l'impôt, qui est calculé selon le barème progressif \
                 suivant :",
                "Fraction de la valeur nette taxable du patrimoine immobilier",
                "Tarif applicable",
            ]
        );
        assert_eq!(
            paragraphs[24],
            "Le 2 de l'article 977 du CGI prévoit un mécanisme de décote pour atténuer l’effet de \
             seuil à l’entrée du barème de l’IFI. Ainsi, pour les redevables dont le patrimoine \
             immobilier a une valeur nette taxable égale ou supérieure à 1 300 000 € et \
             inférieure à 1 400 000 €, le montant de l'impôt calculé selon le barème progressif \
             est réduit d'une somme égale à 17 500 € - 1,25 % P, où P est la valeur nette \
             taxable du patrimoine immobilier."
        );
    }

    #[test]
    fn a_real_publication_without_text_keeps_its_metadata() {
        let id = BoiId::parse("BOI-LETTRE-000150").unwrap();
        let decoded = docs(&fixture("get-no-text.json"), &id).unwrap();
        let doc = &decoded.value[0];
        assert_eq!((decoded.value.len(), doc.publication_id.get()), (1, 3189));
        assert!(doc.text.is_empty());
        assert_eq!(doc.serie.as_ref().map(Serie::as_str), Some("INT"));
        assert!(doc.source_url.is_some() && doc.title.starts_with("LETTRE - INT"));
    }

    #[test]
    fn a_real_unknown_identifier_is_not_found() {
        let id = BoiId::parse("BOI-ZZ-INCONNU-99").unwrap();
        assert_eq!(docs(&fixture("get-unknown.json"), &id), Err(DoctrineError::NotFound));
    }
}
