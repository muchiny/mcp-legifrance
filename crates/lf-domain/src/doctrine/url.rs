//! Adresse d'une publication sur le site du BOFiP (ADR-0029, décision 5).
//!
//! Le jeu `bofip-vigueur` donne un permalien par publication. Mesure du 2026-11-15 : les 9 173
//! permaliens suivent `https://bofip.impots.gouv.fr/bofip/<numéro>-PGP.html`, suivi d'au plus un
//! segment ; le numéro (15 227 au plus) est unique ; le plus long permalien fait 97 octets ; l'un
//! d'eux porte un `%20` parasite, gardé tel quel.
//!
//! L'adresse vient d'un tiers et finit sous les yeux du modèle : elle n'est rendue que si elle
//! suit cette grammaire, à l'octet près. Elle n'est jamais appelée. [`BofipUrl`] est un type à
//! part de [`SourceUrl`](crate::url::SourceUrl), dont l'invariant (« sous l'adresse de
//! Légifrance, sans échappement ») ne tient pas ici.

use std::fmt;

use super::id::PublicationId;

/// Début de toute adresse de publication.
pub const BOFIP_PAGE_BASE: &str = "https://bofip.impots.gouv.fr/bofip/";

/// Longueur maximale d'une adresse rendue, en octets.
pub const BOFIP_URL_MAX_BYTES: usize = 512;

/// Ce qui suit le numéro de publication dans une adresse.
const PAGE_SUFFIX: &str = "-PGP.html";

/// Nombre maximal de chiffres du numéro de publication.
const NUMBER_MAX_DIGITS: usize = 7;

/// Adresse publique d'une publication du BOFiP (`source_url` des deux outils BOFiP) : toujours
/// `https://bofip.impots.gouv.fr/bofip/<1 à 7 chiffres>-PGP.html`, puis au plus un segment de
/// lettres ASCII, chiffres et `. _ % = -`, [`BOFIP_URL_MAX_BYTES`] octets au plus. Seul
/// [`Permalink::parse`] la construit.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BofipUrl(String);

impl BofipUrl {
    /// L'adresse.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BofipUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Ce qu'on lit du permalien d'une publication : son numéro, et son adresse quand elle suit la
/// grammaire entière.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Permalink {
    /// Numéro de la publication.
    pub publication_id: PublicationId,
    /// Adresse à rendre ; `None` si ce qui suit `-PGP.html` sort de la grammaire, ou si l'adresse
    /// dépasse [`BOFIP_URL_MAX_BYTES`] octets : le numéro reste lisible, l'adresse n'est pas
    /// montrée.
    pub url: Option<BofipUrl>,
}

impl Permalink {
    /// Lit un permalien, à l'octet près : ni espace autour, ni autre casse, ni autre schéma.
    ///
    /// `None` si `raw` ne commence pas par [`BOFIP_PAGE_BASE`], un numéro de 1 à 7 chiffres ASCII
    /// (zéro exclu) et `-PGP.html` : sans numéro, la publication ne peut pas être désignée.
    ///
    /// ```
    /// use lf_domain::doctrine::Permalink;
    ///
    /// let raw = "https://bofip.impots.gouv.fr/bofip/3350-PGP.html/identifiant=BOI-ENR-DMTG-10-50-20-20230524";
    /// let link = Permalink::parse(raw).unwrap();
    /// assert_eq!(link.publication_id.get(), 3350);
    /// assert_eq!(link.url.unwrap().as_str(), raw);
    ///
    /// let odd = Permalink::parse("https://bofip.impots.gouv.fr/bofip/3350-PGP.html?x=1").unwrap();
    /// assert_eq!((odd.publication_id.get(), odd.url), (3350, None));
    ///
    /// assert_eq!(Permalink::parse("https://example.org/bofip/3350-PGP.html"), None);
    /// ```
    pub fn parse(raw: &str) -> Option<Self> {
        let (digits, tail) = raw.strip_prefix(BOFIP_PAGE_BASE)?.split_once(PAGE_SUFFIX)?;
        let publication_id = publication_number(digits)?;
        let url = is_rendered(raw.len(), tail).then(|| BofipUrl(raw.to_owned()));
        Some(Self { publication_id, url })
    }
}

/// Numéro écrit en 1 à [`NUMBER_MAX_DIGITS`] chiffres ASCII, dans la plage de [`PublicationId`].
/// Le contrôle des chiffres vient avant la conversion, qui admettrait un signe `+`.
fn publication_number(digits: &str) -> Option<PublicationId> {
    let well_formed = (1..=NUMBER_MAX_DIGITS).contains(&digits.len())
        && digits.bytes().all(|byte| byte.is_ascii_digit());
    let number = well_formed.then(|| digits.parse::<u32>().ok()).flatten()?;
    PublicationId::new(number).ok()
}

/// L'adresse entière, de `len` octets, est-elle rendue ? Oui si elle tient dans
/// [`BOFIP_URL_MAX_BYTES`] et si `tail`, ce qui suit `-PGP.html`, est vide ou fait d'un seul
/// segment.
fn is_rendered(len: usize, tail: &str) -> bool {
    len <= BOFIP_URL_MAX_BYTES
        && (tail.is_empty() || tail.strip_prefix('/').is_some_and(is_segment))
}

/// Un segment de chemin non vide, fait de lettres ASCII, de chiffres et de `. _ % = -`.
fn is_segment(segment: &str) -> bool {
    !segment.is_empty() && segment.bytes().all(is_segment_byte)
}

/// Octet admis dans le segment final : ni `/` (un second segment), ni `?`, ni `#`, ni espace,
/// ni guillemet, ni chevron, ni octet non ASCII.
fn is_segment_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'%' | b'=' | b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    const PAGE: &str = "https://bofip.impots.gouv.fr/bofip/3350-PGP.html";

    /// Numéro et adresse rendue, ou `None` quand le permalien n'en est pas un.
    fn read(raw: &str) -> Option<(u32, Option<String>)> {
        Permalink::parse(raw)
            .map(|link| (link.publication_id.get(), link.url.map(|url| url.to_string())))
    }

    #[test]
    fn measured_forms_are_rendered_as_written() {
        for (raw, number) in [
            (PAGE, 3350),
            ("https://bofip.impots.gouv.fr/bofip/1-PGP.html", 1),
            ("https://bofip.impots.gouv.fr/bofip/15227-PGP.html", 15_227),
            ("https://bofip.impots.gouv.fr/bofip/9999999-PGP.html", 9_999_999),
            (
                "https://bofip.impots.gouv.fr/bofip/3350-PGP.html/identifiant=BOI-ENR-DMTG-10-50-20-20230524",
                3350,
            ),
            (
                "https://bofip.impots.gouv.fr/bofip/3350-PGP.html/identifiant%3DBOI-ENR-DMTG-10-50-20-20230524",
                3350,
            ),
            ("https://bofip.impots.gouv.fr/bofip/77-PGP.html/identifiant=BOI-X%20-20200101", 77),
            ("https://bofip.impots.gouv.fr/bofip/77-PGP.html/a.b_c%d=e-F9", 77),
            // Zéros de tête : même numéro, adresse gardée telle qu'écrite.
            ("https://bofip.impots.gouv.fr/bofip/0000077-PGP.html", 77),
        ] {
            assert_eq!(read(raw), Some((number, Some(raw.to_owned()))));
            assert_eq!(Permalink::parse(raw).unwrap().url.unwrap().as_str(), raw);
        }
    }

    /// Formes qui ne désignent aucune publication du site : ni numéro, ni adresse.
    #[test]
    fn permalink_grammar_rejects_hostile_forms() {
        for raw in [
            "",
            "3350-PGP.html",
            "http://bofip.impots.gouv.fr/bofip/3350-PGP.html",
            "HTTPS://bofip.impots.gouv.fr/bofip/3350-PGP.html",
            "https://BOFIP.impots.gouv.fr/bofip/3350-PGP.html",
            " https://bofip.impots.gouv.fr/bofip/3350-PGP.html",
            "https://bofip.impots.gouv.fr.evil.example/bofip/3350-PGP.html",
            "https://evil.example/https://bofip.impots.gouv.fr/bofip/3350-PGP.html",
            "https://bofip.impots.gouv.fr@evil.example/bofip/3350-PGP.html",
            "https://evil.example@bofip.impots.gouv.fr/bofip/3350-PGP.html",
            "https://bofip.impots.gouv.fr:8443/bofip/3350-PGP.html",
            "https://bofip.impots.gouv.fr//bofip/3350-PGP.html",
            "https://bofip.impots.gouv.fr/bofip/../bofip/3350-PGP.html",
            "https://bofip.impots.gouv.fr/BOFIP/3350-PGP.html",
            "javascript:alert(1)//https://bofip.impots.gouv.fr/bofip/3350-PGP.html",
            // Numéro absent, nul, trop long, signé, non décimal, non ASCII.
            "https://bofip.impots.gouv.fr/bofip/-PGP.html",
            "https://bofip.impots.gouv.fr/bofip/0-PGP.html",
            "https://bofip.impots.gouv.fr/bofip/0000000-PGP.html",
            "https://bofip.impots.gouv.fr/bofip/12345678-PGP.html",
            "https://bofip.impots.gouv.fr/bofip/00000077-PGP.html",
            "https://bofip.impots.gouv.fr/bofip/+77-PGP.html",
            "https://bofip.impots.gouv.fr/bofip/-77-PGP.html",
            "https://bofip.impots.gouv.fr/bofip/7a-PGP.html",
            "https://bofip.impots.gouv.fr/bofip/0x7-PGP.html",
            "https://bofip.impots.gouv.fr/bofip/７７-PGP.html",
            "https://bofip.impots.gouv.fr/bofip/7 7-PGP.html",
            "https://bofip.impots.gouv.fr/bofip/x/77-PGP.html",
            // Suffixe de page absent ou d'une autre casse.
            "https://bofip.impots.gouv.fr/bofip/3350",
            "https://bofip.impots.gouv.fr/bofip/3350-PGP.htm",
            "https://bofip.impots.gouv.fr/bofip/3350-pgp.html",
            "https://bofip.impots.gouv.fr/bofip/3350.html",
        ] {
            assert_eq!(read(raw), None);
        }
    }

    /// Le numéro est lisible, mais la suite sort de la grammaire : l'adresse n'est pas rendue.
    #[test]
    fn odd_tails_keep_the_number_and_drop_the_address() {
        for tail in [
            "/",
            "//",
            "/a/b",
            "/a/",
            "?x=1",
            "/a?x=1",
            "#frag",
            "/a#frag",
            "/a b",
            "/a\"b",
            "/a'b",
            "/<script>",
            "/a\\b",
            "/a\nb",
            "/a\u{0}",
            "/é",
            "/a@b",
            "/a:b",
            "/a;b",
            "/a,b",
            "/a+b",
            "/a&b",
            "x",
            ".bak",
            "-PGP.html",
        ] {
            assert_eq!(read(&format!("{PAGE}{tail}")), Some((3350, None)));
        }
    }

    #[test]
    fn address_length_is_bounded_in_bytes() {
        let segment = |total: usize| "a".repeat(total - PAGE.len() - 1);
        let longest = format!("{PAGE}/{}", segment(BOFIP_URL_MAX_BYTES));
        assert_eq!(longest.len(), 512);
        assert_eq!(read(&longest), Some((3350, Some(longest.clone()))));
        let too_long = format!("{PAGE}/{}", segment(BOFIP_URL_MAX_BYTES + 1));
        assert_eq!(read(&too_long), Some((3350, None)));
    }

    proptest! {
        /// Toute adresse de la grammaire est rendue telle qu'écrite, avec son numéro.
        #[test]
        fn well_formed_permalinks_are_rendered_as_written(
            number in 1_u32..=9_999_999,
            tail in "(/[A-Za-z0-9._%=-]{1,40})?",
        ) {
            let raw = format!("{BOFIP_PAGE_BASE}{number}{PAGE_SUFFIX}{tail}");
            prop_assert_eq!(read(&raw), Some((number, Some(raw.clone()))));
        }

        /// Après `-PGP.html`, seul un segment unique de la grammaire laisse rendre l'adresse : ni
        /// requête, ni fragment, ni second segment, ni espace, ni balisage.
        #[test]
        fn only_a_single_plain_segment_is_rendered(tail in "/?[A-Za-z0-9._%=?#/ \"<-]{0,8}") {
            let raw = format!("{PAGE}{tail}");
            let rendered = tail.is_empty()
                | (tail.starts_with('/')
                    & (tail.len() > 1)
                    & tail.bytes().skip(1).all(is_segment_byte));
            prop_assert_eq!(read(&raw), Some((3350, rendered.then_some(raw.clone()))));
        }
    }
}
