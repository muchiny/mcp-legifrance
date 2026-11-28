//! Textes du serveur : instructions et surface annoncée (05 §4, Q14, Q15).
//!
//! Source unique de ces textes. Les titres et descriptions d'outils (anglais) et les textes des
//! prompts (français) rejoignent ce module avec leurs tranches.
//!
//! Les instructions sont servies telles quelles dans `server/discover` (ère moderne) et dans le
//! résultat d'`initialize` (ère legacy) : le même texte, en anglais, au plus 4 000 caractères,
//! attribution des données comprise. Une surface plus riche cite plus d'outils : seules les
//! lignes de la section « Workflow » changent (05 §4.2). Le texte ne dépend jamais de
//! l'environnement (05 §4.3, règle 5).

/// Surface d'outils annoncée par le serveur : chaque jalon étend la précédente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// v0.1 : recherche, lecture, couverture (M3 à M5).
    V01,
    /// v0.2 : plus la vérification et la résolution de citations (M6).
    V02,
    /// v0.3 : plus la recherche multi-fonds, les prompts et les ressources (M7) ; depuis le jalon
    /// MB, plus les deux outils du BOFiP, depuis le jalon MS, les deux de Service-Public, et depuis
    /// le jalon MJ, les deux de Judilibre (pas de surface `V04` : celle-ci, seule servie,
    /// s'étend).
    V03,
}

/// Surface servie par ce binaire : la complète, qui annonce les dix-neuf outils de
/// `ToolName::ALL`, les resources, les prompts et les complétions.
pub const SURFACE: Surface = Surface::V03;

impl Surface {
    /// Instructions annoncées par cette surface (05 §4.2).
    pub(crate) const fn instructions(self) -> &'static str {
        match self {
            Self::V01 => INSTRUCTIONS_V01,
            Self::V02 => INSTRUCTIONS_V02,
            Self::V03 => INSTRUCTIONS_V03,
        }
    }
}

/// Plafond des instructions, en caractères Unicode (00 §6, 05 §4.3 règle 2 ; relevé de 3 500 à
/// 3 700 par AM-93 pour la ligne du BOFiP, puis à 4 000 par ADR-0033, décision 14, pour celle de
/// Service-Public : Claude Code coupe à 4 096). Éprouvé par `instructions_fit_budget` et, sur le
/// binaire, par `cargo xtask instructions-size`.
#[cfg(test)]
pub(crate) const INSTRUCTIONS_BUDGET_CHARS: usize = 4_000;

// Les trois textes ne diffèrent que par les lignes 1, 3 et 4 de « Workflow » : les morceaux communs
// sont des macros qui se développent en littéraux, que `concat!` assemble à la compilation.

/// Présentation du serveur et titre « Workflow ».
macro_rules! head {
    () => {
        concat!(
            "French law from Légifrance through the official PISTE API (read-only): consolidated \
             laws and decrees (LODA), codes (CODE), Cour de cassation and courts of appeal \
             (JURI), Conseil constitutionnel (CONSTIT), Conseil d'État and administrative courts \
             (CETAT), CNIL deliberations (CNIL), collective labour agreements (KALI) and company \
             agreements (ACCO). Use them for any question on French law or case law.\n",
            "\n",
            "## Workflow\n"
        )
    };
}

/// Ligne 1 de « Workflow » en v0.1 et v0.2.
macro_rules! find {
    () => {
        "1. Find: `fr_search` searches one fond, paged with `page` and `has_more`."
    };
}

/// Phrase de `fr_resolve`, ajoutée à la ligne 1 dès la v0.2.
macro_rules! resolve {
    () => {
        "`fr_resolve` turns a free-text citation (code article, law number, ECLI, case number) \
         into Legifrance ids."
    };
}

/// Ligne 2 de « Workflow » : lecture par préfixe d'identifiant, la même dans les trois versions.
macro_rules! read_by_prefix {
    () => {
        "2. Read by id prefix: LEGITEXT -> `fr_get_act` (metadata and table of contents); \
         LEGIARTI or KALIARTI -> `fr_get_text`; JURITEXT, CONSTEXT or CETATEXT -> \
         `fr_get_decision`; CNILTEXT -> `fr_get_deliberation`; KALITEXT -> \
         `fr_get_convention`; ACCOTEXT -> `fr_get_company_agreement` (metadata only: the \
         full text is a .docx on Légifrance).\n"
    };
}

/// Contrôle des citations en v0.2 : sans navigation ni comparaison.
macro_rules! check_v02 {
    () => {
        "3. Check: `fr_verify_citations` checks every citation of a draft against the official \
         record."
    };
}

/// Contrôle et navigation en v0.3.
macro_rules! check_v03 {
    () => {
        "3. Check and navigate: `fr_verify_citations` checks every citation of a draft against \
         the official record; `fr_get_related` lists what an article or a decision cites or is \
         cited by; `fr_diff_article` compares two versions of an article."
    };
}

/// Ligne « Limits » (`fr_coverage`) : le numéro dépend de la version.
macro_rules! limits {
    () => {
        "Limits: `fr_coverage` says what this server does not cover and whether it runs on the \
         PISTE sandbox."
    };
}

/// Ligne du BOFiP (jalon MB), en v0.3 seulement : les deux outils, ce qu'est la doctrine fiscale
/// et sa limite. Aucune adresse : seuls les liens vers Légifrance sont admis. Resserrée au jalon
/// MJ, sans changer de sens, pour faire place à la ligne 7.
macro_rules! tax_doctrine {
    () => {
        "Tax doctrine: `fr_search_bofip` then `fr_get_bofip` read BOFiP, the tax administration's \
         reading of tax law in force today, not the law."
    };
}

/// Ligne 6 de « Workflow » à la v0.3, depuis le jalon MS (ADR-0033, 05 §4.2) : les deux outils de
/// Service-Public, sans adresse (seuls les liens vers Légifrance sont admis dans ce texte) ;
/// resserrée au jalon MJ, sans changer de sens, pour faire place à la ligne 7.
macro_rules! public_guidance {
    () => {
        "Public guidance: `fr_search_service_public` then `fr_get_service_public` read \
         Service-Public sheets (DILA): plain-language guidance, not the law; `legal_references` \
         lists its texts."
    };
}

/// Ligne 7 de « Workflow » à la v0.3, depuis le jalon MJ (ADR-0035, 05 §4.2) : les deux outils de
/// Judilibre, ce que couvre la source et l'interdiction de profiler les magistrats (décision 7).
macro_rules! court_rulings {
    () => {
        "Court rulings: `fr_search_judilibre` then `fr_get_judilibre` read Judilibre, the Cour de \
         cassation open data, with appeal and lower courts; never to assess a judge."
    };
}

/// Sections « Long texts », « Hard rules » et « Errors », les mêmes dans les trois versions.
/// Pas de saut de ligne final.
macro_rules! tail {
    () => {
        concat!(
            "\n",
            "## Long texts\n",
            "`fr_get_text`, `fr_get_decision` and `fr_get_deliberation` return a short summary \
             by default. A summary is never the whole text: pass `detail=\"full\"` before quoting a \
             threshold, amount or rate; while `next_offset` is present, call again with `offset` \
             set to it. To reach the relevant part directly, \
             pass `focus` with a few words: you get the best passages and their `offset`.\n",
            "\n",
            "## Hard rules\n",
            "- Cite every source with its `citation` and `source_url`; for case law add the \
             `ecli` when present.\n",
            "- Never invent an id, an ECLI, an ELI or a citation: take each one from tool \
             output. Many CAA and TA decisions have no ECLI.\n",
            "- ELI: none from the API for legislation. `source_url` is the stable Légifrance \
             page, not an ELI; an `eli` field appears only when the API provides one.\n",
            "- Attribution: Source : Légifrance / DILA — Licence Ouverte 2.0. When you reuse the \
             data, give this source and the consultation date (`consulted_on` in \
             `fr_coverage`).\n",
            "- Versions default to today's date in Paris; to get the law in force on another \
             day, pass `date` to `fr_get_act` (`version_date` in `fr_search`) as YYYY-MM-DD, \
             then read the article ids of that version with `fr_get_text`. Texts are verbatim (table \
             cells are joined with \" | \"): this server does not decide whether a provision \
             applies to a case.\n",
            "- `etat`: VIGUEUR, VIGUEUR_ETEN, VIGUEUR_NON_ETEN, ABROGE_DIFF in force; VIGUEUR_DIFF \
             not yet; MODIFIE, ABROGE, PERIME no longer; DENONCE may still apply (list: `etat` \
             field).\n",
            "- An empty result does not mean the law is silent: call `fr_coverage` and tell the \
             user what is missing.\n",
            "\n",
            "## Errors\n",
            "A failed call returns isError with a text that starts with a code:\n",
            "- `[invalid_arg]` an argument is missing or malformed: fix it as the message \
             says.\n",
            "- `[not_found]` nothing exists for that id: search again.\n",
            "- `[rate_limited]` PISTE quota reached: wait the delay given, then retry.\n",
            "- `[upstream_error]` Légifrance/PISTE failure: retry if the message asks, else \
             report it.\n",
            "- `[config_error]` credentials or settings are missing or rejected: ask the user \
             to fix the configuration.\n",
            "- `[audit_error]` the local audit log could not be written, so the result was \
             withheld: tell the user."
        )
    };
}

/// Instructions de la surface v0.1 : 8 outils cités, 2 879 caractères (05 §4.2).
pub(crate) const INSTRUCTIONS_V01: &str =
    concat!(head!(), find!(), "\n", read_by_prefix!(), "3. ", limits!(), "\n", tail!());

/// Instructions de la surface v0.2 : 10 outils cités, 3 080 caractères (05 §4.2).
pub(crate) const INSTRUCTIONS_V02: &str = concat!(
    head!(),
    find!(),
    " ",
    resolve!(),
    "\n",
    read_by_prefix!(),
    check_v02!(),
    "\n4. ",
    limits!(),
    "\n",
    tail!()
);

/// Instructions de la surface v0.3 : 19 outils cités (05 §4.1 et §4.2). Depuis le jalon MB, une
/// ligne 5 nomme les deux outils du BOFiP, depuis le jalon MS, une ligne 6 ceux de Service-Public,
/// et depuis le jalon MJ, une ligne 7 ceux de Judilibre ; les textes des v0.1 et v0.2 n'ont pas
/// bougé.
pub(crate) const INSTRUCTIONS_V03: &str = concat!(
    head!(),
    "1. Find: `fr_search` searches one fond, paged with `page` and `has_more`; \
     `fr_search_all` returns a few hits from several fonds. ",
    resolve!(),
    "\n",
    read_by_prefix!(),
    check_v03!(),
    "\n4. ",
    limits!(),
    "\n5. ",
    tax_doctrine!(),
    "\n6. ",
    public_guidance!(),
    "\n7. ",
    court_rulings!(),
    "\n",
    tail!()
);

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use lf_application::{error::ErrorCode, tools::ToolName};
    use pretty_assertions::assert_eq;

    use super::*;

    /// Les trois surfaces, de la plus petite à la plus riche.
    const SURFACES: [Surface; 3] = [Surface::V01, Surface::V02, Surface::V03];

    /// Outils cités, par surface : les 8 de la v0.1, puis M6, puis M7, les deux du BOFiP, les deux
    /// de Service-Public et les deux de Judilibre (05 §4.2).
    fn cited_tools(surface: Surface) -> BTreeSet<&'static str> {
        let v01 = ToolName::ALL[..8].iter().map(|tool| tool.as_str());
        let m6 = [ToolName::FR_VERIFY_CITATIONS, ToolName::FR_RESOLVE].map(ToolName::as_str);
        let m7 = [ToolName::FR_GET_RELATED, ToolName::FR_DIFF_ARTICLE, ToolName::FR_SEARCH_ALL]
            .map(ToolName::as_str);
        let mb = [ToolName::FR_SEARCH_BOFIP, ToolName::FR_GET_BOFIP].map(ToolName::as_str);
        let ms = [ToolName::FR_SEARCH_SERVICE_PUBLIC, ToolName::FR_GET_SERVICE_PUBLIC]
            .map(ToolName::as_str);
        let mj = [ToolName::FR_SEARCH_JUDILIBRE, ToolName::FR_GET_JUDILIBRE].map(ToolName::as_str);
        match surface {
            Surface::V01 => v01.collect(),
            Surface::V02 => v01.chain(m6).collect(),
            Surface::V03 => v01.chain(m6).chain(m7).chain(mb).chain(ms).chain(mj).collect(),
        }
    }

    /// Les jetons entre accents graves d'un texte, dans l'ordre.
    fn tokens(text: &str) -> Vec<&str> {
        text.split('`').skip(1).step_by(2).collect()
    }

    #[test]
    fn the_surface_is_the_complete_v03() {
        assert_eq!(SURFACE, Surface::V03);
        // Les outils que la surface cite sont exactement ceux que le serveur enregistre.
        let registered: BTreeSet<&str> = ToolName::ALL.iter().map(|tool| tool.as_str()).collect();
        assert_eq!(cited_tools(SURFACE), registered);
    }

    #[test]
    fn surfaces_are_distinct_and_copyable() {
        let surface = SURFACE;
        let copy = surface;
        assert_eq!(surface, copy);
        assert_ne!(Surface::V02, Surface::V03);
        assert_eq!(format!("{:?}", Surface::V02), "V02");
    }

    #[test]
    fn each_surface_serves_its_own_text() {
        assert_eq!(Surface::V01.instructions(), INSTRUCTIONS_V01);
        assert_eq!(Surface::V02.instructions(), INSTRUCTIONS_V02);
        assert_eq!(Surface::V03.instructions(), INSTRUCTIONS_V03);
    }

    #[test]
    fn instructions_fit_budget() {
        for surface in SURFACES {
            let size = surface.instructions().chars().count();
            assert!(size <= INSTRUCTIONS_BUDGET_CHARS, "{surface:?} : {size} caractères");
        }
        assert_eq!(INSTRUCTIONS_BUDGET_CHARS, 4_000);
    }

    #[test]
    fn instruction_sizes_match_the_counts_of_the_spec() {
        // 05 §4.2 : caractères et octets UTF-8, sans saut de ligne final, refaits avec Python.
        // D-12 (1.1.0) : +220 caractères (glossaire de `etat`, résumé jamais complet, tableaux),
        // pris sur 3 500 ; revue finale de la PR 2 : glossaire sans joker (règle courte, la règle complète est
        // dans la description du champ `etat`), marge restante : 1 caractère.
        let measured = SURFACES.map(|surface| {
            let text = surface.instructions();
            (text.chars().count(), text.len())
        });
        // Jalon MB (AM-93) : la v0.3 gagne la ligne du BOFiP, 189 caractères avec son numéro ;
        // jalon MS (ADR-0033) : celle de Service-Public, 249 avec son saut de ligne, toute en
        // ASCII ; jalon MJ (ADR-0035, décision 13) : les lignes 5 et 6 passent de 185 à 136 et de
        // 245 à 181 caractères, sans changer de sens, et la ligne 7 de Judilibre en ajoute 168
        // avec son saut de ligne et son numéro, toute en ASCII : 3 992, 8 de marge sous 4 000. Les
        // v0.1 et v0.2 sont identiques à l'octet (G1 bis).
        assert_eq!(measured, [(3_100, 3_108), (3_301, 3_309), (3_992, 4_000)]);
    }

    /// D-12 : glossaire de `etat`, résumé jamais complet, nuance de « verbatim » pour les tableaux.
    /// Les trois ères de surface partagent ces sections.
    #[test]
    fn instructions_carry_the_etat_glossary_the_summary_warning_and_the_table_nuance() {
        for surface in SURFACES {
            let text = surface.instructions();
            for state in [
                "VIGUEUR, VIGUEUR_ETEN, VIGUEUR_NON_ETEN, ABROGE_DIFF in force",
                "VIGUEUR_DIFF not yet",
                "MODIFIE, ABROGE, PERIME no longer",
                "DENONCE may still apply (list: `etat` field)",
            ] {
                assert!(text.contains(state), "{surface:?} : {state}");
            }
            assert!(
                text.contains(
                    "A summary is never the whole text: pass `detail=\"full\"` before quoting a \
                     threshold, amount or rate"
                ),
                "{surface:?}"
            );
            assert!(
                text.contains("Texts are verbatim (table cells are joined with \" | \")"),
                "{surface:?}"
            );
            // L'ancienne phrase, qui laissait croire qu'un résumé suffit, a disparu.
            assert!(!text.contains("For the text itself pass"), "{surface:?}");
        }
    }

    #[test]
    fn instructions_carry_attribution() {
        for surface in SURFACES {
            let text = surface.instructions();
            assert!(
                text.contains("Source : Légifrance / DILA — Licence Ouverte 2.0"),
                "{surface:?}"
            );
            // La date de consultation n'est pas dans le texte statique : `fr_coverage` la rend.
            assert!(text.contains("`consulted_on` in `fr_coverage`"), "{surface:?}");
        }
    }

    #[test]
    fn instructions_cite_exactly_the_tools_of_their_surface() {
        for surface in SURFACES {
            let cited: BTreeSet<&str> = tokens(surface.instructions())
                .into_iter()
                .filter(|token| token.starts_with("fr_"))
                .collect();
            assert_eq!(cited, cited_tools(surface), "{surface:?}");
        }
        let counts = SURFACES.map(|surface| cited_tools(surface).len());
        assert_eq!(counts, [8, 10, 19]);
    }

    #[test]
    fn instructions_cite_exactly_the_six_error_codes() {
        let expected: BTreeSet<String> =
            ErrorCode::ALL.iter().map(|code| format!("[{}]", code.as_str())).collect();
        for surface in SURFACES {
            let cited: BTreeSet<String> = tokens(surface.instructions())
                .into_iter()
                .filter(|token| token.starts_with('['))
                .map(str::to_owned)
                .collect();
            assert_eq!(cited, expected, "{surface:?}");
        }
    }

    #[test]
    fn instructions_cite_only_known_parameters_and_fields() {
        // Tout jeton qui n'est ni un outil ni un code d'erreur nomme un paramètre ou une
        // propriété de sortie (05 §4.3 règle 3). Cette liste est la garde de dérive côté texte ;
        // le rapprochement avec les descripteurs vient avec le verrou (T-M4-11).
        let expected = BTreeSet::from([
            "citation",
            "consulted_on",
            "date",
            "detail=\"full\"",
            "ecli",
            "eli",
            "etat",
            "focus",
            "has_more",
            "next_offset",
            "offset",
            "page",
            "source_url",
            "version_date",
        ]);
        for surface in SURFACES {
            let cited: BTreeSet<&str> = tokens(surface.instructions())
                .into_iter()
                .filter(|token| !token.starts_with("fr_") && !token.starts_with('['))
                .collect();
            // La ligne de Service-Public, à la v0.3 seulement, nomme le champ de sortie de
            // `fr_get_service_public` qui donne les textes cités.
            let mut expected = expected.clone();
            if surface == Surface::V03 {
                expected.insert("legal_references");
            }
            assert_eq!(cited, expected, "{surface:?}");
        }
    }

    #[test]
    fn each_surface_extends_the_previous_one() {
        let sets = SURFACES.map(cited_tools);
        assert!(sets[0].is_subset(&sets[1]));
        assert!(sets[1].is_subset(&sets[2]));
        // Les sections communes restent identiques : seules les lignes de « Workflow » changent.
        for surface in SURFACES {
            let text = surface.instructions();
            assert!(text.starts_with(head!()), "{surface:?}");
            assert!(text.ends_with(tail!()), "{surface:?}");
            assert!(text.contains(read_by_prefix!()), "{surface:?}");
        }
    }

    #[test]
    fn instructions_are_served_verbatim() {
        for surface in SURFACES {
            let text = surface.instructions();
            // LF seulement, aucun saut de ligne final, aucun blanc de fin de ligne (05 §4.1).
            assert!(!text.contains('\r'), "{surface:?}");
            assert!(!text.ends_with('\n'), "{surface:?}");
            assert!(text.lines().all(|line| line == line.trim_end()), "{surface:?}");
            // Les sauts de ligne viennent des `\n` du source, pas de continuations oubliées.
            assert!(!text.contains("  "), "{surface:?}");
        }
    }
}
