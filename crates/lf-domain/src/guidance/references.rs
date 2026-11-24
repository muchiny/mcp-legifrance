//! Références de Légifrance d'une fiche de Service-Public (ADR-0033, décision 8 ; Q40 = A).
//!
//! Une fiche cite les textes qu'elle résume : éléments `Reference` (adresse et titre) et champ
//! `dc:source` (adresses séparées par des virgules). Relevé du 2026-11-23 sur les deux archives :
//! 45 269 liens vers Légifrance, sous 118 formes ; la grammaire ci-dessous en est tirée.
//!
//! - **Hôte exact** : `https://www.legifrance.gouv.fr/`. Les quelques liens `http://` ou sans
//!   `www` (8), les circulaires (`circulaire/id/…`, `download/pdf…`) et les fautes (`Ioda/id/…`)
//!   sont omis.
//! - **Chemin** : `/{fonds}/{vue}/{ID}`, ou deux identifiants (`/codes/section_lc/LEGITEXT…/
//!   LEGISCTA…`), puis une date `AAAA-MM-JJ` facultative ; une barre finale est admise. Le couple
//!   fonds et vue est pris dans une liste fermée.
//! - **Requête et ancre ignorées** : plus d'une centaine de liens portent `?idSecParent=…`,
//!   `?isSuggest=true`, une recherche entière, ou une ancre `#:~:text=…` ; rien n'en est gardé.
//! - **Adresse rendue reconstruite** à partir des parties lues, jamais recopiée.
//! - **Cible** : le dernier identifiant. Son numéro n'est rendu que si un outil en place le lit ;
//!   une section (`LEGISCTA`) ne donne jamais celui du code entier.

use crate::{
    date::CivilDate,
    id::{ID_DIGITS, PREFIX_LEN},
    url::WEB_BASE,
};

use super::line::clean_line;

/// Nombre maximal de références rendues par fiche (05 §5.17). Relevé du 2026-11-23 : 4 liens
/// distincts en médiane, 37 au 99ᵉ centile, 56 au plus ; trois fiches dépassent 50.
pub const LEGAL_REFERENCES_MAX: usize = 50;

/// Couples fonds et vue admis, triés : ceux du relevé (dont `conv_coll/article`, trois liens vers
/// des articles de convention), plus `cons/id` pour les décisions du Conseil constitutionnel, que
/// `fr_get_decision` lit.
const VIEWS: [(&str, &str); 13] = [
    ("ceta", "id"),
    ("cnil", "id"),
    ("codes", "article_lc"),
    ("codes", "id"),
    ("codes", "section_lc"),
    ("cons", "id"),
    ("conv_coll", "article"),
    ("conv_coll", "id"),
    ("jorf", "article_jo"),
    ("jorf", "id"),
    ("juri", "id"),
    ("loda", "article_lc"),
    ("loda", "id"),
];

/// Préfixes qu'un outil en place lit, triés : `LEGIARTI` et `KALIARTI` (`fr_get_text`),
/// `LEGITEXT` (`fr_get_act`), `KALITEXT` (`fr_get_convention`), `JURITEXT`, `CETATEXT`,
/// `CONSTEXT` (`fr_get_decision`), `CNILTEXT` (`fr_get_deliberation`).
const READABLE: [&str; 8] = [
    "CETATEXT", "CNILTEXT", "CONSTEXT", "JURITEXT", "KALIARTI", "KALITEXT", "LEGIARTI", "LEGITEXT",
];

/// Un texte de Légifrance cité par une fiche.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegalReference {
    id: Option<String>,
    title: Option<String>,
    source_url: String,
}

impl LegalReference {
    /// Numéro du texte cité, quand un outil en place le lit ; `None` sinon (`JORFTEXT`,
    /// `LEGISCTA`, `JORFARTI`…).
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// Titre donné par la fiche (élément `Titre` d'une `Reference`), sur une ligne.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Adresse Légifrance reconstruite : `https://www.legifrance.gouv.fr/{fonds}/{vue}/{ID}…`.
    pub fn source_url(&self) -> &str {
        &self.source_url
    }
}

/// Les références d'une fiche, dans l'ordre de première apparition, et si la borne en a coupé.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LegalReferences {
    references: Vec<LegalReference>,
    truncated: bool,
}

impl LegalReferences {
    /// Les références gardées, [`LEGAL_REFERENCES_MAX`] au plus.
    pub fn references(&self) -> &[LegalReference] {
        &self.references
    }

    /// Vrai quand la fiche cite plus de [`LEGAL_REFERENCES_MAX`] textes distincts.
    pub fn truncated(&self) -> bool {
        self.truncated
    }
}

/// Les adresses d'un champ `dc:source`, séparées par des virgules, bords retirés. Un morceau qui
/// n'est pas une adresse (une virgule écrite dans une ancre `#:~:text=…` coupe l'adresse) sera
/// écarté par [`legal_references`].
///
/// ```
/// use lf_domain::guidance::source_urls;
///
/// let urls: Vec<&str> = source_urls("https://a.example/x, https://b.example/y").collect();
/// assert_eq!(urls, ["https://a.example/x", "https://b.example/y"]);
/// ```
pub fn source_urls(dc_source: &str) -> impl Iterator<Item = &str> {
    dc_source.split(',').map(str::trim).filter(|url| !url.is_empty())
}

/// Références de Légifrance tirées des liens d'une fiche, chacun donné avec son titre éventuel,
/// dans l'ordre du document.
///
/// Un lien hors de la grammaire est omis. Un même texte (même cible, même date) n'est rendu
/// qu'une fois, à sa première apparition ; il prend le premier titre qu'un de ses liens donne.
///
/// ```
/// use lf_domain::guidance::legal_references;
///
/// let found = legal_references([
///     (None, "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000025938454/"),
///     (Some("Code : article R162-7"), "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000025938454?x=1"),
///     (None, "https://www.legifrance.gouv.fr/codes/id/LEGISCTA000006167042"),
///     (None, "https://www.legifrance.gouv.fr/circulaire/id/34124"),
/// ]);
/// let references = found.references();
/// assert_eq!(references.len(), 2);
/// assert_eq!(references[0].id(), Some("LEGIARTI000025938454"));
/// assert_eq!(references[0].title(), Some("Code : article R162-7"));
/// assert_eq!(references[0].source_url(), "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000025938454");
/// assert_eq!(references[1].id(), None);
/// ```
pub fn legal_references<'a>(
    links: impl IntoIterator<Item = (Option<&'a str>, &'a str)>,
) -> LegalReferences {
    let mut found = LegalReferences::default();
    // Clé de chaque référence gardée, au même rang : cible et date.
    let mut keys: Vec<(&'a str, Option<&'a str>)> = Vec::new();
    for (title, url) in links {
        let Some(link) = Link::parse(url) else { continue };
        let key = (link.target(), link.date);
        let title = title.map(clean_line).filter(|title| !title.is_empty());
        let known = keys.iter().position(|seen| *seen == key);
        if let Some(known) = known.and_then(|index| found.references.get_mut(index)) {
            if known.title.is_none() {
                known.title = title;
            }
        } else if found.references.len() < LEGAL_REFERENCES_MAX {
            keys.push(key);
            found.references.push(link.into_reference(title));
        } else {
            found.truncated = true;
        }
    }
    found
}

/// Un lien reconnu par la grammaire.
struct Link<'a> {
    fonds: &'a str,
    view: &'a str,
    ids: Vec<&'a str>,
    date: Option<&'a str>,
}

impl<'a> Link<'a> {
    /// Lit `url` selon la grammaire du module ; `None` hors grammaire.
    fn parse(url: &'a str) -> Option<Self> {
        let rest = url.trim().strip_prefix(WEB_BASE)?.strip_prefix('/')?;
        let path = rest.split(['?', '#']).next().unwrap_or_default();
        let path = path.strip_suffix('/').unwrap_or(path);
        let mut segments = path.split('/');
        let fonds = segments.next()?;
        let view = segments.next()?;
        VIEWS.binary_search(&(fonds, view)).ok()?;
        let mut ids = Vec::new();
        let mut date = None;
        for segment in segments {
            if date.is_some() {
                return None;
            }
            if is_id(segment) && ids.len() < 2 {
                ids.push(segment);
            } else if !ids.is_empty() && CivilDate::parse_iso(segment.as_bytes()).is_some() {
                date = Some(segment);
            } else {
                return None;
            }
        }
        (!ids.is_empty()).then_some(Self { fonds, view, ids, date })
    }

    /// Le dernier identifiant du chemin.
    fn target(&self) -> &'a str {
        self.ids.last().copied().unwrap_or_default()
    }

    /// Adresse reconstruite : base, fonds, vue, identifiants, date.
    fn url(&self) -> String {
        let mut url = format!("{WEB_BASE}/{}/{}", self.fonds, self.view);
        for part in self.ids.iter().chain(&self.date) {
            url.push('/');
            url.push_str(part);
        }
        url
    }

    fn into_reference(self, title: Option<String>) -> LegalReference {
        let target = self.target();
        let readable =
            target.get(..PREFIX_LEN).is_some_and(|prefix| READABLE.binary_search(&prefix).is_ok());
        LegalReference { id: readable.then(|| target.to_owned()), title, source_url: self.url() }
    }
}

/// Identifiant de Légifrance : [`PREFIX_LEN`] majuscules ASCII puis [`ID_DIGITS`] chiffres.
fn is_id(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    bytes.len() == PREFIX_LEN + ID_DIGITS
        && bytes.iter().take(PREFIX_LEN).all(u8::is_ascii_uppercase)
        && bytes.iter().skip(PREFIX_LEN).all(u8::is_ascii_digit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use regex::Regex;

    const ARTICLE: &str = "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000025938454";
    const SECTION: &str =
        "https://www.legifrance.gouv.fr/codes/section_lc/LEGITEXT000006070721/LEGISCTA000006150009";

    /// `(id, title, source_url)` d'une référence.
    type Row = (Option<String>, Option<String>, String);

    /// Les références, et la troncature.
    fn found(links: &[(Option<&str>, &str)]) -> (Vec<Row>, bool) {
        let found = legal_references(links.iter().copied());
        let rows = found
            .references()
            .iter()
            .map(|r| {
                (r.id().map(str::to_owned), r.title().map(str::to_owned), r.source_url().to_owned())
            })
            .collect();
        (rows, found.truncated())
    }

    fn urls(links: &[&str]) -> Vec<String> {
        let pairs: Vec<(Option<&str>, &str)> = links.iter().map(|url| (None, *url)).collect();
        found(&pairs).0.into_iter().map(|(_, _, url)| url).collect()
    }

    #[test]
    fn tables_are_sorted_for_binary_search() {
        assert!(VIEWS.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(READABLE.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn only_legifrance_addresses_are_kept() {
        assert_eq!(
            urls(&[
                "http://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000025938454",
                "http://legifrance.gouv.fr/codes/article_lc/LEGIARTI000025938454",
                "https://legifrance.gouv.fr/codes/article_lc/LEGIARTI000025938454",
                "https://www.legifrance.gouv.fr.example.org/codes/article_lc/LEGIARTI000025938454",
                "https://www.legifrance.gouv.fr@example.org/codes/article_lc/LEGIARTI000025938454",
                "https://www.legifrance.gouv.fr:443/codes/article_lc/LEGIARTI000025938454",
                "HTTPS://WWW.LEGIFRANCE.GOUV.FR/codes/article_lc/LEGIARTI000025938454",
                "https://eur-lex.europa.eu/legal-content/FR/TXT/?uri=CELEX:32004L0038",
                "https://www.service-public.gouv.fr/particuliers/vosdroits/F1437",
                "https://www.legifrance.gouv.fr/circulaire/id/34124",
                "https://www.legifrance.gouv.fr/download/pdf/circ?id=32884",
                "https://www.legifrance.gouv.fr/Ioda/id/JORFTEXT000051330783",
                "https://www.legifrance.gouv.fr/dossierlegislatif/JORFDOLE000038748352/",
                "https://www.legifrance.gouv.fr/liste/idcc",
                "https://www.legifrance.gouv.fr/codes",
                "https://www.legifrance.gouv.fr/",
                "ult%C3%A9rieure%20en%20ordonnant",
                "",
            ]),
            Vec::<String>::new()
        );
        assert_eq!(urls(&[ARTICLE]), [ARTICLE]);
    }

    #[test]
    fn the_path_follows_the_strict_grammar() {
        let base = "https://www.legifrance.gouv.fr";
        for (input, expected) in [
            (format!("{ARTICLE}/"), ARTICLE.to_owned()),
            (format!("{ARTICLE}/2022-09-01"), format!("{ARTICLE}/2022-09-01")),
            (format!("{ARTICLE}/2022-09-01/"), format!("{ARTICLE}/2022-09-01")),
            (
                format!("{SECTION}/2022-06-24/#LEGISCTA000006167080"),
                format!("{SECTION}/2022-06-24"),
            ),
            (format!("  {SECTION}/  "), SECTION.to_owned()),
            (
                format!(
                    "{base}/conv_coll/id/KALITEXT000005676569/?idConteneur=KALICONT000005635614"
                ),
                format!("{base}/conv_coll/id/KALITEXT000005676569"),
            ),
            (
                format!("{base}/conv_coll/article/KALIARTI000018563911"),
                format!("{base}/conv_coll/article/KALIARTI000018563911"),
            ),
            (
                format!("{base}/cnil/id/CNILTEXT000053352643"),
                format!("{base}/cnil/id/CNILTEXT000053352643"),
            ),
        ] {
            assert_eq!(urls(&[&input]), [expected], "{input}");
        }
        for input in [
            format!("{ARTICLE}//"),
            format!("{base}/codes/article_lc//LEGIARTI000025938454"),
            format!("{base}/codes/article_lc/2022-09-01/LEGIARTI000025938454"),
            format!("{base}/codes/article_lc/2022-09-01"),
            format!("{ARTICLE}/2022-02-30"),
            format!("{ARTICLE}/2022-9-1"),
            format!("{ARTICLE}/2022-09-01/2022-09-02"),
            format!("{ARTICLE}/2022-09-01/LEGIARTI000025938454"),
            format!("{SECTION}/LEGIARTI000025938454"),
            format!("{base}/codes/article_lc/legiarti000025938454"),
            format!("{base}/codes/article_lc/LEGIARTI00002593845"),
            format!("{base}/codes/article_lc/LEGIARTI0000259384541"),
            format!("{base}/codes/article_lc/LEGIART1000025938454"),
            format!("{base}/codes/article_lc/LÉGIARTI000025938454"),
            format!("{base}/codes/texte_lc/LEGIARTI000025938454"),
            format!("{base}/loda/section_lc/LEGIARTI000025938454"),
            format!("{base}/codes/article_lc/LEGIARTI000025938454%2F"),
            format!("{base}/codes/article_lc/../juri/id/JURITEXT000037676914"),
        ] {
            assert_eq!(urls(&[&input]), Vec::<String>::new(), "{input}");
        }
    }

    #[test]
    fn query_and_anchor_are_never_copied() {
        for suffix in [
            "?init=true&page=1&query=R1336-5&searchField=ALL",
            "?idSecParent=LEGISCTA000051930600",
            "#:~:text=Sur%20l'ordre%20de%20l",
            "/?isSuggest=true",
            "/#LEGIARTI000025938454",
            "?",
            "#",
        ] {
            assert_eq!(urls(&[&format!("{ARTICLE}{suffix}")]), [ARTICLE], "{suffix}");
        }
    }

    #[test]
    fn a_section_reference_never_gives_the_code_id() {
        let (rows, _) = found(&[(None, SECTION)]);
        assert_eq!(rows, [(None, None, SECTION.to_owned())]);
        // Le code entier, lui, donne son numéro.
        let code = "https://www.legifrance.gouv.fr/loda/id/LEGITEXT000006070721";
        assert_eq!(
            found(&[(None, code)]).0,
            [(Some("LEGITEXT000006070721".to_owned()), None, code.to_owned())]
        );
    }

    #[test]
    fn ids_are_given_only_when_a_tool_reads_them() {
        let base = "https://www.legifrance.gouv.fr";
        for (path, readable) in [
            ("codes/article_lc/LEGIARTI000025938454", true),
            ("conv_coll/article/KALIARTI000018563911", true),
            ("loda/id/LEGITEXT000005632482", true),
            ("conv_coll/id/KALITEXT000005676569", true),
            ("juri/id/JURITEXT000037676914", true),
            ("ceta/id/CETATEXT000033194812", true),
            ("cons/id/CONSTEXT000017667487", true),
            ("cnil/id/CNILTEXT000053352643", true),
            ("jorf/id/JORFTEXT000051393086", false),
            ("jorf/article_jo/JORFARTI000045133788", false),
            ("codes/id/LEGISCTA000006167042", false),
            ("conv_coll/id/KALICONT000005635614", false),
            ("loda/id/JORFTEXT000020608894", false),
        ] {
            let url = format!("{base}/{path}");
            let (rows, _) = found(&[(None, &url)]);
            let target = path.rsplit('/').next().map(str::to_owned);
            assert_eq!(rows, [(target.filter(|_| readable), None, url.clone())], "{path}");
        }
    }

    #[test]
    fn a_text_is_listed_once_with_the_first_title_given() {
        let (rows, truncated) = found(&[
            (None, &format!("{ARTICLE}/")),
            (Some("  "), ARTICLE),
            (Some(" Code : \tarticle R162-7 "), &format!("{ARTICLE}?x=1")),
            (Some("Autre titre"), ARTICLE),
            (Some("À une date"), &format!("{ARTICLE}/2022-09-01")),
            (Some("Section"), SECTION),
            (None, "https://www.legifrance.gouv.fr/codes/id/LEGISCTA000006150009"),
        ]);
        assert!(!truncated);
        assert_eq!(
            rows,
            [
                (
                    Some("LEGIARTI000025938454".to_owned()),
                    Some("Code : article R162-7".to_owned()),
                    ARTICLE.to_owned()
                ),
                (
                    Some("LEGIARTI000025938454".to_owned()),
                    Some("À une date".to_owned()),
                    format!("{ARTICLE}/2022-09-01")
                ),
                (None, Some("Section".to_owned()), SECTION.to_owned()),
            ]
        );
    }

    #[test]
    fn references_stop_at_fifty_and_say_so() {
        let links: Vec<String> = (0..=LEGAL_REFERENCES_MAX)
            .map(|n| format!("https://www.legifrance.gouv.fr/jorf/id/JORFTEXT{n:012}"))
            .collect();
        let pairs: Vec<(Option<&str>, &str)> =
            links.iter().map(|url| (None, url.as_str())).collect();
        let (rows, truncated) = found(pairs.get(..LEGAL_REFERENCES_MAX).unwrap_or_default());
        assert_eq!((rows.len(), truncated), (LEGAL_REFERENCES_MAX, false));
        // Un doublon au-delà de la borne ne la dépasse pas ; un texte nouveau, si.
        let mut repeated = pairs.get(..LEGAL_REFERENCES_MAX).unwrap_or_default().to_vec();
        repeated.push((Some("titre"), links.first().map(String::as_str).unwrap_or_default()));
        assert_eq!(found(&repeated).1, false);
        let (rows, truncated) = found(&pairs);
        assert_eq!((rows.len(), truncated), (LEGAL_REFERENCES_MAX, true));
        assert_eq!(
            rows.last().map(|row| row.2.as_str()),
            links.get(LEGAL_REFERENCES_MAX - 1).map(String::as_str)
        );
    }

    #[test]
    fn dc_source_is_split_at_commas() {
        let urls: Vec<&str> =
            source_urls(" https://a.example/x ,https://b.example/y,, ,").collect();
        assert_eq!(urls, ["https://a.example/x", "https://b.example/y"]);
        assert_eq!(source_urls("").count(), 0);
    }

    /// Liens d'une fixture, dans l'ordre du document : chaque adresse de `dc:source`, puis
    /// l'adresse et le titre de chaque `Reference`, entités XML décodées.
    fn fixture_links(xml: &str) -> Vec<(Option<String>, String)> {
        let unescape = |text: &str| {
            text.replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&apos;", "'")
                .replace("&amp;", "&")
        };
        let pattern = Regex::new(
            r#"(?s)<dc:source>(.*?)</dc:source>|<Reference\b[^>]*URL="([^"]*)"[^>]*>(.*?)</Reference>"#,
        )
        .unwrap();
        let title = Regex::new(r"(?s)<Titre>(.*?)</Titre>").unwrap();
        let mut links = Vec::new();
        for found in pattern.captures_iter(xml) {
            if let Some(source) = found.get(1) {
                links.extend(
                    source_urls(&unescape(source.as_str())).map(|url| (None, url.to_owned())),
                );
            } else {
                let name = title.captures(&found[3]).map(|t| unescape(&t[1]));
                links.push((name, unescape(&found[2])));
            }
        }
        links
    }

    /// Les fiches réelles de T-MS-02 (archive du 2026-11-22) : chaque lien classé, résultat
    /// attendu calculé par une lecture indépendante (script hors dépôt, `python3 -I`).
    #[test]
    fn every_fixture_address_is_classified() {
        let fixtures = [
            (
                include_str!(
                    "../../../lf-adapter-service-public/tests/fixtures/xml/part/F1239.xml"
                ),
                4,
                2,
                2,
            ),
            (
                include_str!(
                    "../../../lf-adapter-service-public/tests/fixtures/xml/part/F1437.xml"
                ),
                6,
                3,
                1,
            ),
            (
                include_str!(
                    "../../../lf-adapter-service-public/tests/fixtures/xml/part/F1903.xml"
                ),
                37,
                19,
                8,
            ),
            (
                include_str!(
                    "../../../lf-adapter-service-public/tests/fixtures/xml/part/F2408.xml"
                ),
                8,
                4,
                1,
            ),
            (
                include_str!("../../../lf-adapter-service-public/tests/fixtures/xml/part/F468.xml"),
                6,
                3,
                0,
            ),
            (
                include_str!("../../../lf-adapter-service-public/tests/fixtures/xml/pro/F2408.xml"),
                8,
                4,
                1,
            ),
            (
                include_str!(
                    "../../../lf-adapter-service-public/tests/fixtures/xml/pro/F37954.xml"
                ),
                6,
                2,
                2,
            ),
            (
                include_str!(
                    "../../../lf-adapter-service-public/tests/fixtures/xml/part/F10041.xml"
                ),
                0,
                0,
                0,
            ),
            (
                include_str!(
                    "../../../lf-adapter-service-public/tests/fixtures/xml/part/F31613.xml"
                ),
                0,
                0,
                0,
            ),
            (
                include_str!(
                    "../../../lf-adapter-service-public/tests/fixtures/xml/part/F34680.xml"
                ),
                0,
                0,
                0,
            ),
        ];
        for (xml, links, kept, with_id) in fixtures {
            let found_links = fixture_links(xml);
            assert_eq!(found_links.len(), links);
            let found = legal_references(
                found_links.iter().map(|(title, url)| (title.as_deref(), url.as_str())),
            );
            assert!(!found.truncated());
            assert_eq!(found.references().len(), kept);
            assert_eq!(found.references().iter().filter(|r| r.id().is_some()).count(), with_id);
            // Toute référence gardée a un titre : chaque texte cité l'est aussi par une `Reference`.
            assert!(found.references().iter().all(|r| r.title().is_some()));
        }
    }

    /// Le détail d'une fiche réelle : une section, un article, un décret du JORF, chacun avec le
    /// titre de sa `Reference`, même quand `dc:source` le cite d'abord sans titre.
    #[test]
    fn a_real_sheet_gives_its_section_article_and_decree() {
        let xml =
            include_str!("../../../lf-adapter-service-public/tests/fixtures/xml/part/F1437.xml");
        let links = fixture_links(xml);
        let (rows, _) = found(
            &links.iter().map(|(title, url)| (title.as_deref(), url.as_str())).collect::<Vec<_>>(),
        );
        assert_eq!(
            rows,
            [
                (
                    None,
                    Some("Code des procédures civiles de l'exécution : articles L162-1 à L162-2".to_owned()),
                    "https://www.legifrance.gouv.fr/codes/section_lc/LEGITEXT000025024948/LEGISCTA000025025791".to_owned()
                ),
                (
                    Some("LEGIARTI000025938454".to_owned()),
                    Some("Code des procédures civiles d'exécution : article R162-7".to_owned()),
                    ARTICLE.to_owned()
                ),
                (
                    None,
                    Some(
                        "Décret n°2025-293 du 29 mars 2025 portant revalorisation du montant forfaitaire du \
                         revenu de solidarité active"
                            .to_owned()
                    ),
                    "https://www.legifrance.gouv.fr/jorf/id/JORFTEXT000051393086".to_owned()
                ),
            ]
        );
    }

    proptest! {
        /// Quel que soit le lien, une adresse rendue suit la grammaire, se relit à l'identique, et
        /// ne garde ni requête ni ancre ; son numéro, quand il y en a un, est sa cible.
        #[test]
        fn rendered_addresses_follow_the_grammar(
            tail in "[a-z_]{1,10}/[a-z_]{1,10}(/[A-Z]{8}[0-9]{12}){0,3}(/[0-9]{4}-[0-9]{2}-[0-9]{2})?/?[?#]?\\PC{0,20}",
            noise in "\\PC{0,40}",
        ) {
            for url in [format!("https://www.legifrance.gouv.fr/{tail}"), noise] {
                let found = legal_references([(None, url.as_str())]);
                for reference in found.references() {
                    let rendered = reference.source_url();
                    prop_assert!(rendered.starts_with("https://www.legifrance.gouv.fr/"));
                    prop_assert!(!rendered.contains(['?', '#']) && !rendered.ends_with('/'));
                    let again = legal_references([(None, rendered)]);
                    prop_assert_eq!(again.references(), found.references());
                    if let Some(id) = reference.id() {
                        prop_assert!(rendered.contains(id));
                        prop_assert!(READABLE.contains(&id.get(..PREFIX_LEN).unwrap_or_default()));
                    }
                }
            }
        }
    }
}
