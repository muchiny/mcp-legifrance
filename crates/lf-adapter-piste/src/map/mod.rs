//! Mapping des DTO de PISTE vers les types du domaine (04 §9) : socle commun des mappers.
//!
//! Ce module porte ce que partagent les mappers : les gabarits de chemin des endpoints
//! ([`endpoint`]), les compteurs d'anomalies d'un mapping ([`Diagnostics`], 04 §9.3), versés
//! ensuite à `Stats::record_mapping`, et l'erreur d'un champ refusé ([`Field`], [`not_found`]).
//!
//! Le domaine rend ses anomalies comme valeurs (03 §1.1) : le mapper les compte ici, il ne les
//! journalise pas lui-même. Aucune valeur du corps ne sort d'ici : le `detail` d'un
//! [`CorpusError::Decode`] a la forme `<endpoint>: <chemin>: <catégorie>`, chemin fait de noms
//! de champs déclarés, catégorie à texte fixe, jamais le `Display` d'une [`DomainError`] (il
//! recopie jusqu'à 64 caractères de l'entrée).

use lf_application::ports::CorpusError;
use lf_domain::DomainError;

pub(crate) mod act;
pub(crate) mod body;
pub(crate) mod convention;
pub(crate) mod dates;
#[cfg(test)]
pub(crate) mod fixtures;
pub(crate) mod rules;
pub(crate) mod search;
pub(crate) mod toc;

/// Gabarits de chemin des endpoints, passés à `classify` et à `post_json` : ni URL ni paramètre.
pub(crate) mod endpoint {
    /// Recherche.
    pub(crate) const SEARCH: &str = "/search";
    /// Texte de loi ou de décret, et code.
    pub(crate) const LAW_DECREE: &str = "/consult/lawDecree";
    /// Convention collective.
    pub(crate) const KALI_TEXT: &str = "/consult/kaliText";
    /// Article.
    pub(crate) const GET_ARTICLE: &str = "/consult/getArticle";
    /// Décision de justice (judiciaire, administrative ou constitutionnelle).
    pub(crate) const JURI: &str = "/consult/juri";
    /// Délibération de la CNIL.
    pub(crate) const CNIL: &str = "/consult/cnil";
    /// Accord d'entreprise.
    pub(crate) const ACCO: &str = "/consult/acco";
}

/// Anomalies relevées pendant un mapping (04 §9.3) : seulement des nombres, jamais un
/// identifiant ni une valeur du corps.
///
/// Les champs suivent l'ordre de `StatsSnapshot`. Chaque compteur sature à `u32::MAX`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Diagnostics {
    /// Résultats de recherche écartés : identifiant invalide ou d'un autre fond (04 §9.1 i).
    pub(crate) dropped_hits: u32,
    /// Identifiant renvoyé absent ou différent de celui demandé (04 §9.1 a et b).
    pub(crate) id_mismatch: u32,
    /// Horodatages qui ne tombent pas à minuit UTC, lus comme jours civils UTC (04 §9.1 e).
    pub(crate) non_midnight_timestamps: u32,
    /// Suffixes de version retirés d'identifiants autres que `LEGITEXT`.
    pub(crate) upstream_suffix_stripped: u32,
    /// Champs d'origine en désaccord avec la famille de l'identifiant (04 §9.1 d).
    pub(crate) origin_mismatch: u32,
    /// ECLI natifs refusés et omis.
    pub(crate) invalid_native_ecli: u32,
    /// NOR refusés et omis.
    pub(crate) invalid_nor: u32,
    /// Entrées de table des matières écartées : identifiant invalide ou en double.
    pub(crate) toc_dropped: u32,
    /// Caractères invisibles ou bidirectionnels retirés des textes.
    pub(crate) invisible_chars_removed: u32,
}

impl Diagnostics {
    /// Compte un résultat de recherche écarté.
    pub(crate) fn note_dropped_hit(&mut self) {
        self.dropped_hits = self.dropped_hits.saturating_add(1);
    }

    /// Compte un identifiant renvoyé absent ou différent de celui demandé.
    pub(crate) fn note_id_mismatch(&mut self) {
        self.id_mismatch = self.id_mismatch.saturating_add(1);
    }

    /// Compte un horodatage qui ne tombe pas à minuit UTC.
    pub(crate) fn note_non_midnight(&mut self) {
        self.non_midnight_timestamps = self.non_midnight_timestamps.saturating_add(1);
    }

    /// Compte un suffixe de version retiré d'un identifiant autre que `LEGITEXT`.
    pub(crate) fn note_suffix_stripped(&mut self) {
        self.upstream_suffix_stripped = self.upstream_suffix_stripped.saturating_add(1);
    }

    /// Compte un champ d'origine en désaccord avec la famille de l'identifiant.
    pub(crate) fn note_origin_mismatch(&mut self) {
        self.origin_mismatch = self.origin_mismatch.saturating_add(1);
    }

    /// Compte un ECLI natif refusé.
    pub(crate) fn note_invalid_ecli(&mut self) {
        self.invalid_native_ecli = self.invalid_native_ecli.saturating_add(1);
    }

    /// Compte un NOR refusé.
    pub(crate) fn note_invalid_nor(&mut self) {
        self.invalid_nor = self.invalid_nor.saturating_add(1);
    }

    /// Ajoute `dropped` entrées de table des matières écartées.
    pub(crate) fn add_toc_dropped(&mut self, dropped: u32) {
        self.toc_dropped = self.toc_dropped.saturating_add(dropped);
    }

    /// Ajoute `removed` caractères invisibles ou bidirectionnels retirés.
    pub(crate) fn add_invisible(&mut self, removed: u32) {
        self.invisible_chars_removed = self.invisible_chars_removed.saturating_add(removed);
    }
}

/// Champ d'une réponse, désigné par son endpoint et son chemin, pour l'erreur qui le refuse.
///
/// `path` est fait de noms de champs déclarés par les DTO, `[]` pour un élément de liste
/// (`text.dateTexte`, `results[].titles[].id`) : jamais d'une valeur du corps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Field {
    /// Gabarit de l'endpoint (voir [`endpoint`]).
    pub(crate) endpoint: &'static str,
    /// Chemin du champ dans la réponse.
    pub(crate) path: &'static str,
}

impl Field {
    /// [`CorpusError::Decode`] de ce champ, de détail `<endpoint>: <chemin>: <catégorie>`.
    ///
    /// `category` est un texte fixe (`unreadable date`), jamais une valeur lue.
    pub(crate) fn decode(self, category: &'static str) -> CorpusError {
        CorpusError::Decode { detail: format!("{}: {}: {category}", self.endpoint, self.path) }
    }

    /// [`CorpusError::Decode`] de ce champ refusé par le domaine : la catégorie vient de la
    /// variante de `error` (`category`), jamais de son `Display`, qui recopie l'entrée.
    pub(crate) fn domain(self, error: &DomainError) -> CorpusError {
        self.decode(category(error))
    }
}

/// Catégorie à texte fixe d'une erreur du domaine levée en lisant une réponse (04 §9.1 h).
///
/// `TextTooLarge` et `TocTooDeep` sont des données amont incohérentes, pas une erreur de
/// l'utilisateur : comme toute variante, elles deviennent un `Decode`.
fn category(error: &DomainError) -> &'static str {
    match error {
        DomainError::InvalidId { .. } | DomainError::BadSuffix { .. } => "invalid identifier",
        DomainError::InvalidDate { .. } => "invalid date",
        DomainError::TextTooLarge { .. } => "text too large",
        DomainError::TocTooDeep { .. } => "table of contents too deep",
        _ => "invalid value",
    }
}

/// [`CorpusError::NotFound`] de l'endpoint (04 §11) : `what` reçoit le gabarit, jamais
/// l'identifiant demandé ni une valeur du corps.
pub(crate) fn not_found(endpoint: &str) -> CorpusError {
    CorpusError::NotFound { what: endpoint.to_owned() }
}

#[cfg(test)]
mod tests {
    use lf_domain::{date::CivilDate, ecli::Ecli, id::LegiTextId};

    use super::*;

    /// Une des méthodes `note_*`.
    type Note = fn(&mut Diagnostics);

    /// Un cas par `note_*` : la fonction et les compteurs attendus après un appel depuis zéro.
    fn notes() -> [(Note, Diagnostics); 7] {
        let zero = Diagnostics::default();
        [
            (Diagnostics::note_dropped_hit, Diagnostics { dropped_hits: 1, ..zero }),
            (Diagnostics::note_id_mismatch, Diagnostics { id_mismatch: 1, ..zero }),
            (Diagnostics::note_non_midnight, Diagnostics { non_midnight_timestamps: 1, ..zero }),
            (
                Diagnostics::note_suffix_stripped,
                Diagnostics { upstream_suffix_stripped: 1, ..zero },
            ),
            (Diagnostics::note_origin_mismatch, Diagnostics { origin_mismatch: 1, ..zero }),
            (Diagnostics::note_invalid_ecli, Diagnostics { invalid_native_ecli: 1, ..zero }),
            (Diagnostics::note_invalid_nor, Diagnostics { invalid_nor: 1, ..zero }),
        ]
    }

    /// Les neuf compteurs à `u32::MAX`.
    fn saturated() -> Diagnostics {
        Diagnostics {
            dropped_hits: u32::MAX,
            id_mismatch: u32::MAX,
            non_midnight_timestamps: u32::MAX,
            upstream_suffix_stripped: u32::MAX,
            origin_mismatch: u32::MAX,
            invalid_native_ecli: u32::MAX,
            invalid_nor: u32::MAX,
            toc_dropped: u32::MAX,
            invisible_chars_removed: u32::MAX,
        }
    }

    #[test]
    fn diagnostics_default_is_all_zero() {
        assert_eq!(
            Diagnostics::default(),
            Diagnostics {
                dropped_hits: 0,
                id_mismatch: 0,
                non_midnight_timestamps: 0,
                upstream_suffix_stripped: 0,
                origin_mismatch: 0,
                invalid_native_ecli: 0,
                invalid_nor: 0,
                toc_dropped: 0,
                invisible_chars_removed: 0,
            }
        );
    }

    #[test]
    fn each_note_bumps_only_its_own_counter() {
        for (note, expected) in notes() {
            let mut diags = Diagnostics::default();
            note(&mut diags);
            assert_eq!(diags, expected);
        }
    }

    #[test]
    fn add_counters_accumulate_and_saturate() {
        let mut diags = Diagnostics::default();
        diags.add_invisible(3);
        diags.add_invisible(4);
        diags.add_toc_dropped(2);
        diags.add_toc_dropped(5);
        assert_eq!(
            diags,
            Diagnostics { toc_dropped: 7, invisible_chars_removed: 7, ..Diagnostics::default() }
        );

        let mut full = Diagnostics::default();
        full.add_invisible(u32::MAX);
        full.add_invisible(1);
        full.add_toc_dropped(u32::MAX);
        full.add_toc_dropped(1);
        full.dropped_hits = u32::MAX;
        full.note_dropped_hit();
        assert_eq!(
            full,
            Diagnostics {
                dropped_hits: u32::MAX,
                toc_dropped: u32::MAX,
                invisible_chars_removed: u32::MAX,
                ..Diagnostics::default()
            }
        );

        for (note, _) in notes() {
            let mut diags = saturated();
            note(&mut diags);
            assert_eq!(diags, saturated());
        }
    }

    #[test]
    fn not_found_is_the_endpoint_template() {
        assert_eq!(
            not_found("/consult/juri"),
            CorpusError::NotFound { what: "/consult/juri".into() }
        );
    }

    #[test]
    fn field_decode_writes_endpoint_path_and_category() {
        let field = Field { endpoint: "/consult/juri", path: "text.dateTexte" };
        assert_eq!(
            field.decode("unreadable date"),
            CorpusError::Decode { detail: "/consult/juri: text.dateTexte: unreadable date".into() }
        );
    }

    #[test]
    fn domain_errors_map_to_a_fixed_category() {
        let field = Field { endpoint: "/consult/juri", path: "text.id" };
        let cases = [
            (LegiTextId::parse("x").map(drop), "invalid identifier"),
            (LegiTextId::parse("LEGITEXT000033205014_foo").map(drop), "invalid identifier"),
            (CivilDate::parse("x").map(drop), "invalid date"),
            (Err(DomainError::TextTooLarge { chars: 16_000_001 }), "text too large"),
            (Err(DomainError::TocTooDeep { max: 64 }), "table of contents too deep"),
            (Ecli::parse("x").map(drop), "invalid value"),
        ];
        for (result, category) in cases {
            let error = result.unwrap_err();
            assert_eq!(
                field.domain(&error),
                CorpusError::Decode { detail: format!("/consult/juri: text.id: {category}") }
            );
        }
    }

    #[test]
    fn domain_detail_never_echoes_the_rejected_value() {
        let error = LegiTextId::parse("SECRET-VALUE-0001").unwrap_err();
        assert!(error.to_string().contains("SECRET"), "{error}");
        let mapped = Field { endpoint: "/consult/juri", path: "text.id" }.domain(&error);
        let shown = format!("{mapped:?} {mapped}");
        assert!(!shown.contains("SECRET"), "{shown}");
    }

    #[test]
    fn endpoint_templates_are_the_api_paths() {
        assert_eq!(
            [
                endpoint::SEARCH,
                endpoint::LAW_DECREE,
                endpoint::KALI_TEXT,
                endpoint::GET_ARTICLE,
                endpoint::JURI,
                endpoint::CNIL,
                endpoint::ACCO,
            ],
            [
                "/search",
                "/consult/lawDecree",
                "/consult/kaliText",
                "/consult/getArticle",
                "/consult/juri",
                "/consult/cnil",
                "/consult/acco",
            ]
        );
    }
}
