//! Requêtes envoyées au portail : la clause `where` et l'adresse (ADR-0029, décision 3).
//!
//! Le portail lit un langage de filtre (ODSQL) dans le paramètre `where`. Un guillemet non
//! échappé dans une valeur changerait le filtre : c'est une injection. Deux barrières, l'une
//! derrière l'autre :
//!
//! 1. seules des **valeurs typées par le domaine** entrent dans une clause ([`Literal`]) : des
//!    mots déjà réduits aux lettres, aux chiffres et aux espaces, un identifiant, une série, une
//!    nature ; jamais une chaîne libre ;
//! 2. **une seule fonction** écrit les littéraux ([`push_literal`]), et elle échappe `\` puis
//!    `"` même si le domaine ne peut pas en produire.
//!
//! L'adresse est bâtie par l'analyseur d'adresses (`Url::query_pairs_mut`), qui encode chaque
//! paramètre ; jamais par concaténation.

use lf_application::ports::DoctrineError;
use lf_domain::doctrine::{BoiId, DoctrineKind, DoctrineQuery, DoctrineTerms, Serie};
use reqwest::Url;

/// Champs demandés par une recherche : jamais le texte (`contenu`, `contenu_html`).
pub(crate) const SEARCH_SELECT: &str =
    "identifiant_juridique,titre,type,serie,debut_de_validite,permalien";

/// Champs demandés par une lecture : le texte sous ses deux formes, dans la même requête. Le
/// champ HTML (`contenu_html`) est lu ; le champ brut (`contenu`) le garde et le remplace s'il
/// est incomplet (ADR-0030).
pub(crate) const GET_SELECT: &str = "identifiant_juridique,titre,type,serie,division,debut_de_validite,permalien,contenu,\
     contenu_html";

/// Nombre de publications demandées par une lecture. Un identifiant en désigne une, parfois deux
/// (mesure du 2026-11-15) : une réponse qui en annonce plus est refusée, jamais tronquée.
pub(crate) const GET_LIMIT: u32 = 10;

/// Nom que le portail donne à une nature, dans son champ `type`.
pub(crate) const fn portal_kind(kind: DoctrineKind) -> &'static str {
    match kind {
        DoctrineKind::Doctrine => "Contenu",
        DoctrineKind::News => "Actualité",
    }
}

/// Valeur du domaine qu'une clause peut porter en littéral. Aucune variante ne reçoit une chaîne
/// libre : c'est le type qui ferme la porte.
#[derive(Debug, Clone, Copy)]
enum Literal<'a> {
    /// Mots cherchés.
    Terms(&'a DoctrineTerms),
    /// Identifiant d'une publication.
    Id(&'a BoiId),
    /// Code de série.
    Serie(&'a Serie),
    /// Nature, écrite sous son nom du portail.
    Kind(DoctrineKind),
}

impl Literal<'_> {
    /// Le texte de la valeur, avant échappement.
    fn text(&self) -> &str {
        match self {
            Self::Terms(terms) => terms.as_str(),
            Self::Id(id) => id.as_str(),
            Self::Serie(serie) => serie.as_str(),
            Self::Kind(kind) => portal_kind(*kind),
        }
    }
}

/// Ajoute `value` à `clause` sous la forme d'un littéral de chaîne. **Seule** fonction du crate
/// qui écrit un littéral dans une clause.
fn push_literal(clause: &mut String, value: Literal<'_>) {
    push_quoted(clause, value.text());
}

/// Ajoute `text` entre guillemets doubles, `\` et `"` précédés d'un `\`.
fn push_quoted(clause: &mut String, text: &str) {
    clause.push('"');
    for character in text.chars() {
        if matches!(character, '\\' | '"') {
            clause.push('\\');
        }
        clause.push(character);
    }
    clause.push('"');
}

/// Clause d'une recherche : tous les mots dans le texte, puis la nature et la série demandées.
pub(crate) fn search_where(query: &DoctrineQuery) -> String {
    let mut clause = String::from("search(contenu, ");
    push_literal(&mut clause, Literal::Terms(query.terms()));
    clause.push(')');
    if let Some(kind) = query.kind().kind() {
        clause.push_str(" and type = ");
        push_literal(&mut clause, Literal::Kind(kind));
    }
    if let Some(serie) = query.serie() {
        clause.push_str(" and serie = ");
        push_literal(&mut clause, Literal::Serie(serie));
    }
    clause
}

/// Clause d'une lecture : l'identifiant exact.
pub(crate) fn get_where(id: &BoiId) -> String {
    let mut clause = String::from("identifiant_juridique = ");
    push_literal(&mut clause, Literal::Id(id));
    clause
}

/// Adresse d'une recherche sur `base`.
///
/// # Errors
///
/// [`DoctrineError::Upstream`] si `base` n'est pas une adresse.
pub(crate) fn search_url(base: &str, query: &DoctrineQuery) -> Result<Url, DoctrineError> {
    records_url(
        base,
        &[
            ("select", SEARCH_SELECT),
            ("where", &search_where(query)),
            ("limit", &query.page_size().get().to_string()),
            ("offset", &query.offset().to_string()),
        ],
    )
}

/// Adresse de la lecture de `id` sur `base`.
///
/// # Errors
///
/// [`DoctrineError::Upstream`] si `base` n'est pas une adresse.
pub(crate) fn get_url(base: &str, id: &BoiId) -> Result<Url, DoctrineError> {
    records_url(
        base,
        &[("select", GET_SELECT), ("where", &get_where(id)), ("limit", &GET_LIMIT.to_string())],
    )
}

/// `base` suivie des paramètres `pairs`, chacun encodé par l'analyseur d'adresses.
fn records_url(base: &str, pairs: &[(&str, &str)]) -> Result<Url, DoctrineError> {
    let mut url = Url::parse(base).map_err(|_| DoctrineError::Upstream {
        status: None,
        retryable: false,
        detail: "the portal address is not a URL".to_owned(),
    })?;
    url.query_pairs_mut().clear().extend_pairs(pairs);
    Ok(url)
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use lf_domain::{doctrine::KindFilter, search::PageSize};
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    use super::*;
    use crate::doctrine::RECORDS_URL;

    fn query(
        words: &str,
        serie: Option<&str>,
        kind: KindFilter,
        page: u32,
        size: u32,
    ) -> DoctrineQuery {
        DoctrineQuery::new(
            DoctrineTerms::parse(words).unwrap(),
            serie.map(|code| Serie::parse(code).unwrap()),
            kind,
            NonZeroU32::new(page).unwrap(),
            PageSize::new(size).unwrap(),
        )
        .unwrap()
    }

    /// Lit `clause` comme le fait le portail : son squelette, où chaque littéral est remplacé par
    /// `?`, et les littéraux décodés.
    fn lex(clause: &str) -> (String, Vec<String>) {
        let mut skeleton = String::new();
        let mut literals = Vec::new();
        let mut characters = clause.chars();
        while let Some(character) = characters.next() {
            if character == '"' {
                let mut literal = String::new();
                loop {
                    match characters.next() {
                        Some('\\') => literal.extend(characters.next()),
                        Some('"') | None => break,
                        Some(other) => literal.push(other),
                    }
                }
                literals.push(literal);
                skeleton.push('?');
            } else {
                skeleton.push(character);
            }
        }
        (skeleton, literals)
    }

    #[test]
    fn the_search_clause_follows_the_requested_kind_and_serie() {
        let rows = [
            (
                query("donation abattement", None, KindFilter::Doctrine, 1, 10),
                r#"search(contenu, "donation abattement") and type = "Contenu""#,
            ),
            (
                query("donation", None, KindFilter::News, 1, 5),
                r#"search(contenu, "donation") and type = "Actualité""#,
            ),
            (query("donation", None, KindFilter::All, 1, 5), r#"search(contenu, "donation")"#),
            (
                query(
                    "déclaration de succession délai dépôt",
                    Some("ENR"),
                    KindFilter::Doctrine,
                    1,
                    5,
                ),
                r#"search(contenu, "déclaration de succession délai dépôt") and type = "Contenu" and serie = "ENR""#,
            ),
            (
                query("bail", Some("RFPI"), KindFilter::All, 3, 20),
                r#"search(contenu, "bail") and serie = "RFPI""#,
            ),
        ];
        for (query, clause) in rows {
            assert_eq!(search_where(&query), clause);
        }
    }

    #[test]
    fn the_get_clause_is_the_exact_identifier() {
        let id = BoiId::parse("BOI-ENR-DMTG-10-50-20").unwrap();
        assert_eq!(get_where(&id), r#"identifiant_juridique = "BOI-ENR-DMTG-10-50-20""#);
    }

    #[test]
    fn each_kind_has_its_portal_name() {
        assert_eq!(
            [DoctrineKind::Doctrine, DoctrineKind::News].map(portal_kind),
            ["Contenu", "Actualité"]
        );
    }

    /// Un guillemet ou une barre oblique inverse ne ferme jamais le littéral : le portail relit
    /// exactement le texte donné, dans un seul littéral.
    #[test]
    fn quotes_and_backslashes_are_escaped() {
        let rows = [
            ("donation", r#""donation""#),
            ("", r#""""#),
            (r#"a" or "1"="1"#, r#""a\" or \"1\"=\"1""#),
            (r"fin\", r#""fin\\""#),
            (r#"\""#, r#""\\\"""#),
        ];
        for (text, written) in rows {
            let mut clause = String::new();
            push_quoted(&mut clause, text);
            assert_eq!(clause, written);
            assert_eq!(lex(&clause), ("?".to_owned(), vec![text.to_owned()]));
        }
    }

    #[test]
    fn the_search_address_encodes_every_parameter() {
        let url = search_url(
            RECORDS_URL,
            &query("donation abattement", None, KindFilter::Doctrine, 2, 10),
        );
        assert_eq!(
            url.unwrap().as_str(),
            "https://data.economie.gouv.fr/api/explore/v2.1/catalog/datasets/bofip-vigueur/records\
             ?select=identifiant_juridique%2Ctitre%2Ctype%2Cserie%2Cdebut_de_validite%2Cpermalien\
             &where=search%28contenu%2C+%22donation+abattement%22%29+and+type+%3D+%22Contenu%22\
             &limit=10&offset=10"
        );
        let news = search_url(RECORDS_URL, &query("été", Some("IR"), KindFilter::News, 1, 100));
        assert_eq!(
            news.unwrap().query(),
            Some(
                "select=identifiant_juridique%2Ctitre%2Ctype%2Cserie%2Cdebut_de_validite%2Cpermalien\
                 &where=search%28contenu%2C+%22%C3%A9t%C3%A9%22%29+and+type+%3D+%22Actualit%C3%A9%22\
                 +and+serie+%3D+%22IR%22&limit=100&offset=0"
            )
        );
    }

    #[test]
    fn the_get_address_asks_for_the_text_and_ten_publications() {
        let id = BoiId::parse("BOI-IF-TH-10-50-40").unwrap();
        assert_eq!(
            get_url(RECORDS_URL, &id).unwrap().as_str(),
            "https://data.economie.gouv.fr/api/explore/v2.1/catalog/datasets/bofip-vigueur/records\
             ?select=identifiant_juridique%2Ctitre%2Ctype%2Cserie%2Cdivision%2Cdebut_de_validite\
             %2Cpermalien%2Ccontenu%2Ccontenu_html\
             &where=identifiant_juridique+%3D+%22BOI-IF-TH-10-50-40%22&limit=10"
        );
    }

    /// Le texte du portail n'est jamais demandé par une recherche ; une lecture le demande sous
    /// ses deux formes (ADR-0030).
    #[test]
    fn the_text_is_only_selected_by_a_read() {
        assert_eq!(SEARCH_SELECT.split(',').find(|field| field.starts_with("contenu")), None);
        let read: Vec<&str> =
            GET_SELECT.split(',').filter(|field| field.starts_with("contenu")).collect();
        assert_eq!(read, ["contenu", "contenu_html"]);
        // La lecture ne demande rien d'autre de plus que la recherche, sauf la division.
        let extra: Vec<&str> = GET_SELECT
            .split(',')
            .filter(|field| !SEARCH_SELECT.split(',').any(|known| known == *field))
            .collect();
        assert_eq!(extra, ["division", "contenu", "contenu_html"]);
    }

    /// Les paramètres déjà présents dans la base sont remplacés, pas complétés.
    #[test]
    fn parameters_of_the_base_are_replaced() {
        let id = BoiId::parse("BOI-A-1").unwrap();
        let url = get_url("http://127.0.0.1:9/records?where=1&limit=100", &id).unwrap();
        assert_eq!(url.query_pairs().count(), 3);
        assert_eq!(url.query_pairs().filter(|(name, _)| name == "where").count(), 1);
    }

    #[test]
    fn a_base_that_is_not_an_address_is_an_upstream_error() {
        let expected = Err(DoctrineError::Upstream {
            status: None,
            retryable: false,
            detail: "the portal address is not a URL".to_owned(),
        });
        let id = BoiId::parse("BOI-A-1").unwrap();
        assert_eq!(get_url("pas une adresse", &id), expected);
        let query = query("donation", None, KindFilter::All, 1, 10);
        assert_eq!(search_url("", &query), expected);
    }

    /// Quarante caractères au plus, pris dans tout Unicode.
    fn any_text() -> impl Strategy<Value = String> {
        proptest::collection::vec(proptest::char::any(), 0..=40).prop_map(String::from_iter)
    }

    fn any_kind() -> impl Strategy<Value = KindFilter> {
        prop_oneof![Just(KindFilter::Doctrine), Just(KindFilter::News), Just(KindFilter::All)]
    }

    proptest! {
        /// Quelle que soit l'entrée, la clause garde son squelette et porte exactement un
        /// littéral par valeur : les mots, puis la nature et la série quand elles sont
        /// demandées. Aucune entrée ne ferme un littéral ni n'ajoute un opérateur.
        ///
        /// Les caractères sont pris dans tout Unicode, contrôles et caractères de format compris
        /// (`\0`, saut de ligne, U+202E) : le domaine les retire ou en fait des espaces.
        #[test]
        fn where_has_exactly_n_literals_for_any_input(
            before in any_text(),
            word in "[a-z0-9éèàùç]{1,12}",
            after in any_text(),
            serie in proptest::option::of("[A-Za-z0-9]{1,16}"),
            kind in any_kind(),
        ) {
            let terms = DoctrineTerms::parse(&format!("{before} {word} {after}")).unwrap();
            let serie = serie.map(|code| Serie::parse(&code).unwrap());
            let query = DoctrineQuery::new(
                terms.clone(),
                serie.clone(),
                kind,
                NonZeroU32::MIN,
                PageSize::DEFAULT,
            )
            .unwrap();

            let mut skeleton = String::from("search(contenu, ?)");
            let mut literals = vec![terms.as_str().to_owned()];
            literals.extend(kind.kind().map(|kind| portal_kind(kind).to_owned()));
            skeleton.push_str(kind.kind().map_or("", |_| " and type = ?"));
            literals.extend(serie.as_ref().map(|serie| serie.as_str().to_owned()));
            skeleton.push_str(serie.as_ref().map_or("", |_| " and serie = ?"));
            prop_assert_eq!(lex(&search_where(&query)), (skeleton, literals));
        }

        /// L'échappement seul, sur une chaîne libre que le domaine ne laisserait pas passer : un
        /// littéral, relu à l'identique, et rien autour.
        #[test]
        fn any_text_is_read_back_as_one_literal(text in "\\PC{0,80}") {
            let mut clause = String::new();
            push_quoted(&mut clause, &text);
            prop_assert_eq!(lex(&clause), ("?".to_owned(), vec![text]));
        }

        /// Un identifiant quelconque reste un seul littéral.
        #[test]
        fn the_get_clause_has_exactly_one_literal(id in "[A-Z]{1,6}(-[A-Z0-9]{1,8}){1,6}") {
            let id = BoiId::parse(&id).unwrap();
            prop_assert_eq!(
                lex(&get_where(&id)),
                ("identifiant_juridique = ?".to_owned(), vec![id.as_str().to_owned()])
            );
        }
    }
}
