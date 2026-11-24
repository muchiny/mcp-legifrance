//! Adresse publique d'une fiche de Service-Public (ADR-0033, décision 6 ; 05 §5.17).
//!
//! Chaque fiche de l'archive donne son adresse dans l'attribut `spUrl`. Relevé du 2026-11-24 sur
//! les deux archives : toutes suivent l'une des deux formes ci-dessous, avec le numéro de la
//! fiche ; 37 fiches ont leur page sur le site de l'autre public (12 dans l'archive Particuliers,
//! 25 dans l'archive Entreprendre).
//!
//! L'adresse vient d'un tiers et finit sous les yeux du modèle : elle n'est rendue que si elle
//! suit cette grammaire à l'octet près, sinon elle est construite à partir du public et du
//! numéro. Elle n'est jamais appelée. [`ServicePublicUrl`] est un type à part de
//! [`SourceUrl`](crate::url::SourceUrl), dont l'invariant (« sous l'adresse de Légifrance ») ne
//! tient pas ici.

use std::fmt;

use super::{audience::Audience, id::SheetId};

/// Début de l'adresse d'une page du site Particuliers.
pub const PARTICULIERS_PAGE_BASE: &str =
    "https://www.service-public.gouv.fr/particuliers/vosdroits/";

/// Début de l'adresse d'une page du site Entreprendre.
pub const ENTREPRENDRE_PAGE_BASE: &str = "https://entreprendre.service-public.gouv.fr/vosdroits/";

/// Adresse de l'archive du jour du public Particuliers (ADR-0033, décision 4) : fixée dans le
/// code, sans variable de surcharge.
pub const PARTICULIERS_ARCHIVE_URL: &str =
    "https://lecomarquage.service-public.gouv.fr/vdd/3.5/part/zip/vosdroits-latest.zip";

/// Adresse de l'archive du jour du public Entreprendre.
pub const ENTREPRENDRE_ARCHIVE_URL: &str =
    "https://lecomarquage.service-public.gouv.fr/vdd/3.5/pro/zip/vosdroits-latest.zip";

/// Nom du fichier téléchargé, dans les termes de la DILA (`name` de `source_files`).
pub const ARCHIVE_NAME: &str = "vosdroits-latest.zip";

/// Adresse de l'archive du jour de `audience` : la seule requête de la source.
///
/// ```
/// use lf_domain::guidance::{ARCHIVE_NAME, Audience, archive_url};
///
/// assert!(archive_url(Audience::Entreprendre).ends_with("/pro/zip/vosdroits-latest.zip"));
/// assert!(archive_url(Audience::Particuliers).ends_with(ARCHIVE_NAME));
/// ```
#[must_use]
pub const fn archive_url(audience: Audience) -> &'static str {
    match audience {
        Audience::Particuliers => PARTICULIERS_ARCHIVE_URL,
        Audience::Entreprendre => ENTREPRENDRE_ARCHIVE_URL,
    }
}

/// Adresse publique d'une fiche ou d'une page de navigation : toujours
/// [`PARTICULIERS_PAGE_BASE`] ou [`ENTREPRENDRE_PAGE_BASE`] suivi du numéro, rien d'autre.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServicePublicUrl(String);

impl ServicePublicUrl {
    /// L'adresse que donne la fiche (`sp_url`) si elle suit la grammaire avec ce numéro, à
    /// l'octet près ; sinon celle du site de `audience`.
    ///
    /// ```
    /// use lf_domain::guidance::{Audience, ServicePublicUrl, SheetId};
    ///
    /// let id = SheetId::parse("F2408")?;
    /// let given = "https://entreprendre.service-public.gouv.fr/vosdroits/F2408";
    /// assert_eq!(ServicePublicUrl::read(Some(given), Audience::Particuliers, &id).as_str(), given);
    /// assert_eq!(
    ///     ServicePublicUrl::read(Some("https://example.org/F2408"), Audience::Particuliers, &id).as_str(),
    ///     "https://www.service-public.gouv.fr/particuliers/vosdroits/F2408"
    /// );
    /// # Ok::<(), lf_domain::DomainError>(())
    /// ```
    pub fn read(sp_url: Option<&str>, audience: Audience, id: &SheetId) -> Self {
        sp_url
            .filter(|raw| {
                Audience::ALL
                    .iter()
                    .any(|site| raw.strip_prefix(page_base(*site)) == Some(id.as_str()))
            })
            .map_or_else(
                || Self(format!("{}{id}", page_base(audience))),
                |raw| Self(raw.to_owned()),
            )
    }

    /// L'adresse.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Le public du site de l'adresse : une page de l'archive d'un public peut être publiée sur l'autre site
    /// (`spUrl`), et la citation nomme alors ce site-là.
    ///
    /// ```
    /// use lf_domain::guidance::{Audience, ServicePublicUrl, SheetId};
    ///
    /// let id = SheetId::parse("F2408")?;
    /// let given = "https://www.service-public.gouv.fr/particuliers/vosdroits/F2408";
    /// let url = ServicePublicUrl::read(Some(given), Audience::Entreprendre, &id);
    /// assert_eq!(url.audience(), Audience::Particuliers);
    /// let rebuilt = ServicePublicUrl::read(None, Audience::Entreprendre, &id);
    /// assert_eq!(rebuilt.audience(), Audience::Entreprendre);
    /// # Ok::<(), lf_domain::DomainError>(())
    /// ```
    pub fn audience(&self) -> Audience {
        // Une adresse ne se construit que sous la base d'un des deux sites (`read`).
        if self.0.starts_with(page_base(Audience::Entreprendre)) {
            Audience::Entreprendre
        } else {
            Audience::Particuliers
        }
    }
}

impl fmt::Display for ServicePublicUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Début de l'adresse d'une page du site de `audience`.
const fn page_base(audience: Audience) -> &'static str {
    match audience {
        Audience::Particuliers => PARTICULIERS_PAGE_BASE,
        Audience::Entreprendre => ENTREPRENDRE_PAGE_BASE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use regex::Regex;

    const GRAMMAR: &str = r"\Ahttps://(www\.service-public\.gouv\.fr/particuliers|entreprendre\.service-public\.gouv\.fr)/vosdroits/[FN][0-9]{1,7}\z";

    fn read(sp_url: Option<&str>, audience: Audience, id: &str) -> String {
        ServicePublicUrl::read(sp_url, audience, &SheetId::parse(id).unwrap()).to_string()
    }

    #[test]
    fn public_url_follows_the_strict_grammar() {
        let part = "https://www.service-public.gouv.fr/particuliers/vosdroits/F1437";
        let pro = "https://entreprendre.service-public.gouv.fr/vosdroits/F1437";
        // Construite selon le public quand la fiche n'en donne pas.
        assert_eq!(read(None, Audience::Particuliers, "f1437"), part);
        assert_eq!(read(None, Audience::Entreprendre, "F1437"), pro);
        // Celle de la fiche, même sur le site de l'autre public.
        assert_eq!(read(Some(pro), Audience::Particuliers, "F1437"), pro);
        assert_eq!(read(Some(part), Audience::Entreprendre, "F1437"), part);
        // Toute autre forme est remplacée par l'adresse construite.
        for given in [
            "https://www.service-public.gouv.fr/particuliers/vosdroits/F1438",
            "https://www.service-public.gouv.fr/particuliers/vosdroits/f1437",
            "https://www.service-public.gouv.fr/particuliers/vosdroits/F1437/",
            "https://www.service-public.gouv.fr/particuliers/vosdroits/F1437?x=1",
            "https://www.service-public.gouv.fr/particuliers/vosdroits/F1437#a",
            "https://www.service-public.gouv.fr/particuliers/vosdroits/F14370",
            " https://www.service-public.gouv.fr/particuliers/vosdroits/F1437",
            "http://www.service-public.gouv.fr/particuliers/vosdroits/F1437",
            "https://service-public.gouv.fr/particuliers/vosdroits/F1437",
            "https://www.service-public.gouv.fr/professionnels-entreprises/vosdroits/F1437",
            "https://www.service-public.gouv.fr.example.org/particuliers/vosdroits/F1437",
            "https://entreprendre.service-public.gouv.fr/vosdroits/F1437\u{200b}",
            "",
        ] {
            assert_eq!(read(Some(given), Audience::Particuliers, "F1437"), part, "{given:?}");
        }
    }

    /// Les deux archives sont sur l'hôte de la DILA, en `https`, et ne diffèrent que par le
    /// public.
    #[test]
    fn archive_addresses_are_fixed_and_distinct() {
        let prefix = "https://lecomarquage.service-public.gouv.fr/vdd/3.5/";
        assert_eq!(
            Audience::ALL.map(archive_url),
            [format!("{prefix}part/zip/{ARCHIVE_NAME}"), format!("{prefix}pro/zip/{ARCHIVE_NAME}"),]
        );
    }

    /// Les adresses des fiches réelles de T-MS-02 sont gardées telles quelles.
    #[test]
    fn fixture_addresses_are_kept() {
        for (given, audience, id) in [
            (
                "https://entreprendre.service-public.gouv.fr/vosdroits/F37954",
                Audience::Entreprendre,
                "F37954",
            ),
            (
                "https://www.service-public.gouv.fr/particuliers/vosdroits/N31675",
                Audience::Particuliers,
                "N31675",
            ),
        ] {
            assert_eq!(read(Some(given), audience, id), given);
        }
    }

    proptest! {
        /// Quelle que soit l'adresse donnée, l'adresse rendue suit la grammaire et finit par le
        /// numéro.
        #[test]
        fn rendered_addresses_follow_the_grammar(
            given in proptest::option::of("\\PC{0,80}"),
            particuliers in any::<bool>(),
            prefix in "[FN]",
            digits in "[0-9]{1,7}",
        ) {
            let id = SheetId::parse(&format!("{prefix}{digits}")).unwrap();
            let audience = if particuliers { Audience::Particuliers } else { Audience::Entreprendre };
            let url = ServicePublicUrl::read(given.as_deref(), audience, &id);
            prop_assert!(Regex::new(GRAMMAR).unwrap().is_match(url.as_str()), "{url}");
            let tail = format!("/{id}");
            prop_assert!(url.as_str().ends_with(&tail));
        }
    }
}
