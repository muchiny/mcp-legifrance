//! Liens d'un article ou d'une décision (`fr_get_related`) (annexe B §7.11, 05 §5.11).
//!
//! Deux sources, deux traitements, et le même résultat : des [`LinkOut`].
//!
//! - **Article** ([`article_links`]) : les listes `liensCite` et `liensCitePar` du port
//!   (`RelatedLinks`, 03 §4.11) deviennent des liens `cites` et `cited_by`. La famille (`kind`)
//!   suit la table nature → kind de 05 §5.11 ; `id` et `source_url` ne sont rendus que pour une
//!   cible typée qu'un outil servi sait lire, jamais fabriqués.
//! - **Décision** ([`decision_links`]) : `citationJp` et les titres de `liens` passent par le
//!   parseur de citations (`strip_html: true`, `repair_mojibake: false` : le corps est déjà
//!   réparé par l'adapter). Une décision ou un ECLI de `citationJp` est `cites_case_law`, toute
//!   autre citation est `applies_text`. Les fragments sans ancre (« 1 du code civil. », « article
//!   2270- ») ne donnent aucune citation, donc aucun lien ; les doublons sont fusionnés, le plus
//!   précis l'emportant. Aucune résolution : ces liens n'ont ni `id` ni `source_url`.

use std::collections::HashSet;

use lf_domain::{
    citation::{CitationKind, CitationRef, ParseOptions, Spanned, format::canonical, parse_with},
    entity::{RelatedLink, RelatedLinks, RelatedNature},
    id::DocumentId,
    text::clean_field,
    url::WEB_BASE,
};

use crate::output::get_related::{LinkOut, Relation};

/// Nombre maximal de liens rendus, sans suite (00 §7, 05 §5.11).
pub(crate) const MAX_LINKS: usize = 100;

/// Longueur maximale d'une `citation`, en caractères : un titre démesuré de l'API ne doit pas
/// vider la liste à lui seul (il serait retiré avec tous ceux qui le suivent).
const CITATION_MAX_CHARS: usize = 500;

// ---------------------------------------------------------------------------------------------
// Article
// ---------------------------------------------------------------------------------------------

/// Les liens d'un article : ceux qu'il cite d'abord, puis ceux qui le citent, chacun dans l'ordre
/// reçu. Un lien sans titre ni identifiant n'a rien à dire et n'est pas rendu ; un lien identique
/// à un précédent (même sens, même famille, même citation, même cible) est fusionné avec lui.
pub(crate) fn article_links(links: &RelatedLinks) -> Vec<LinkOut> {
    let cites = links.cites.iter().map(|link| article_link(Relation::Cites, link));
    let cited_by = links.cited_by.iter().map(|link| article_link(Relation::CitedBy, link));
    let mut seen = HashSet::new();
    cites.chain(cited_by).flatten().filter(|link| seen.insert(link.clone())).collect()
}

/// Un lien d'article, ou `None` s'il n'a ni titre ni identifiant à montrer.
fn article_link(relation: Relation, link: &RelatedLink) -> Option<LinkOut> {
    let located = link.target.as_ref().and_then(|target| located(&link.nature, target));
    let citation = clean_field(link.title.as_deref())
        .or_else(|| link.target.as_ref().map(|target| target.as_str().to_owned()))?;
    Some(LinkOut {
        relation,
        kind: kind_of(&link.nature, link.target.as_ref()),
        citation: clip(citation),
        id: located.as_ref().map(|(id, _)| id.clone()),
        source_url: located.map(|(_, url)| url),
    })
}

/// Famille du lien d'après la table nature → kind de 05 §5.11. Toute autre association est
/// `unsupported` : un `LODA` vers un `JORFTEXT`, par exemple, que rien ne sait lire.
fn kind_of(nature: &RelatedNature, target: Option<&DocumentId>) -> CitationKind {
    use RelatedNature::{Code, Jorf, JorfCont, Kali, Loda};
    match (nature, target) {
        (Code, Some(DocumentId::LegiArti(_))) => CitationKind::CodeArticle,
        (Code | Loda, Some(DocumentId::LegiText(_))) => CitationKind::Text,
        (Loda, Some(DocumentId::LegiArti(_))) => CitationKind::TextArticle,
        (Kali, Some(DocumentId::KaliArti(_) | DocumentId::KaliText(_))) => CitationKind::Convention,
        // `JORFCONT` et `JORFARTI` n'ont pas de cible typée (03 §4.11) : « ou aucune ».
        (Jorf | JorfCont, None | Some(DocumentId::JorfText(_))) => CitationKind::Jorf,
        _ => CitationKind::Unsupported,
    }
}

/// Identifiant et page Légifrance de la cible, si un outil servi sait la lire (`LEGIARTI`,
/// `LEGITEXT`, `KALIARTI`, `KALITEXT`). Le `JORFTEXT` n'en est pas : la surface JORF n'est pas
/// retenue (T-M7-01), et les autres familles n'apparaissent pas dans ces listes.
///
/// Les chemins sont ceux de `lf_domain::url` (`ResourceKind`), dont la construction est réservée
/// au domaine : une demande partagée propose de l'exposer pour supprimer cette copie.
fn located(nature: &RelatedNature, target: &DocumentId) -> Option<(String, String)> {
    let path = match target {
        DocumentId::LegiArti(_) => "codes/article_lc",
        DocumentId::LegiText(_) if *nature == RelatedNature::Code => "codes/texte_lc",
        DocumentId::LegiText(_) => "loda/id",
        DocumentId::KaliArti(_) | DocumentId::KaliText(_) => "conv_coll/id",
        _ => return None,
    };
    let id = target.as_str();
    Some((id.to_owned(), format!("{WEB_BASE}/{path}/{id}")))
}

/// Coupe à [`CITATION_MAX_CHARS`] caractères, suivis de `…`.
fn clip(mut text: String) -> String {
    if let Some((byte, _)) = text.char_indices().nth(CITATION_MAX_CHARS) {
        text.truncate(byte);
        text.push('…');
    }
    text
}

// ---------------------------------------------------------------------------------------------
// Décision
// ---------------------------------------------------------------------------------------------

/// Ce que l'on sait analyser d'une décision : `citationJp` et les titres des liens de citation
/// (`liens[].title`, 03 §4.4).
#[derive(Debug, Clone, Copy)]
pub(crate) struct DecisionReferences<'a> {
    /// Texte de `citationJp`, absent s'il est vide.
    pub(crate) cited_case_law: Option<&'a str>,
    /// Titres des `liens` de type `CITATION`.
    pub(crate) applied_texts: &'a [String],
}

/// D'où vient un texte analysé.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// `citationJp` : une décision ou un ECLI y est une jurisprudence citée.
    CitationJp,
    /// Un titre de `liens` : tout y est un texte appliqué.
    Liens,
}

/// Un lien en cours de dédoublonnage : son identité (l'acte visé) et sa précision (l'article de
/// cet acte, s'il y en a un).
struct Draft {
    link: LinkOut,
    /// Acte visé, en minuscules, préfixé par sa famille.
    act: String,
    /// Article de l'acte, quand le lien en désigne un.
    article: Option<String>,
}

/// Les liens d'une décision : jurisprudence citée d'abord, textes appliqués ensuite, chaque groupe
/// dans l'ordre du texte (`citationJp` puis `liens`).
pub(crate) fn decision_links(references: &DecisionReferences<'_>) -> Vec<LinkOut> {
    let options =
        ParseOptions { strip_html: true, repair_mojibake: false, ..ParseOptions::default() };
    let cited = references.cited_case_law.map(|text| (Source::CitationJp, text));
    let applied = references.applied_texts.iter().map(|text| (Source::Liens, text.as_str()));
    let found = cited.into_iter().chain(applied).flat_map(|(source, text)| {
        let citations = parse_with(text, &options);
        citations.into_iter().filter_map(move |citation| draft_of(source, text, &citation))
    });
    let mut drafts: Vec<Draft> = Vec::new();
    for draft in found {
        merge(&mut drafts, draft);
    }
    // Tri stable : la jurisprudence citée passe devant, l'ordre du texte est gardé dans chaque groupe.
    drafts.sort_by_key(|draft| draft.link.relation == Relation::AppliesText);
    drafts.into_iter().map(|draft| draft.link).collect()
}

/// Le brouillon de lien d'une citation reconnue dans `text`, ou `None` si elle n'a rien de lisible
/// à montrer.
fn draft_of(source: Source, text: &str, found: &Spanned<CitationRef>) -> Option<Draft> {
    let raw = text.get(found.span.start..found.span.end).unwrap_or_default();
    let citation = shown(&found.value, raw)?;
    let kind = found.value.kind();
    let relation = match (source, kind) {
        (Source::CitationJp, CitationKind::Decision | CitationKind::Ecli) => Relation::CitesCaseLaw,
        _ => Relation::AppliesText,
    };
    let (act, article) = identity(&found.value, &citation);
    Some(Draft {
        link: LinkOut { relation, kind, citation: clip(citation), id: None, source_url: None },
        act,
        article,
    })
}

/// Ce que le lien montre : la forme canonique de la citation, sinon le texte reconnu, nettoyé
/// (balises et entités retirées). `None` si ce texte est vide une fois nettoyé.
fn shown(value: &CitationRef, raw: &str) -> Option<String> {
    canonical(value).or_else(|| clean_field(Some(raw)))
}

/// L'acte visé par la citation et l'article de cet acte, s'il est désigné.
///
/// Deux citations visent le même acte quand leur clé d'acte est égale : un texte et un article de
/// ce texte, ou deux mentions d'une même directive (clé `85/374/CEE`). Les autres familles se
/// reconnaissent à leur forme affichée.
fn identity(value: &CitationRef, citation: &str) -> (String, Option<String>) {
    let lower = citation.to_lowercase();
    match value {
        CitationRef::Text(_) => (format!("text:{lower}"), None),
        CitationRef::TextArticle { text: Some(text), .. } => {
            let act = canonical(&CitationRef::Text(text.clone()));
            let act = act.map_or(lower.clone(), |form| form.to_lowercase());
            (format!("text:{act}"), Some(lower))
        }
        CitationRef::Unsupported(unsupported) => {
            let key = unsupported.key.as_deref().map_or(lower.clone(), str::to_lowercase);
            (format!("{}:{key}", unsupported.family.code()), leading_article(citation))
        }
        other => (format!("{}:{lower}", other.kind().snake_case()), None),
    }
}

/// Le numéro d'article par lequel commence un texte (« Article 10 de la directive… » donne
/// `10`), ou `None` s'il ne commence pas par un article.
fn leading_article(text: &str) -> Option<String> {
    let lower = text.trim_start().to_lowercase();
    let rest = ["articles ", "article ", "art. ", "art "]
        .iter()
        .find_map(|prefix| lower.strip_prefix(prefix))?;
    rest.split_whitespace().next().map(|number| number.trim_end_matches(',').to_owned())
}

/// Ajoute `draft`, sauf s'il vise le même acte et le même article qu'un brouillon déjà là (même
/// sens). Quand le nouveau désigne un article et que l'ancien vise l'acte seul, le plus précis
/// prend la place du premier ; l'inverse écarte le nouveau.
fn merge(drafts: &mut Vec<Draft>, draft: Draft) {
    let same = |other: &Draft| {
        other.act == draft.act
            && other.link.relation == draft.link.relation
            && (other.article == draft.article
                || other.article.is_none()
                || draft.article.is_none())
    };
    match drafts.iter_mut().find(|other| same(other)) {
        Some(existing) if existing.article.is_none() && draft.article.is_some() => {
            *existing = draft;
        }
        Some(_) => {}
        None => drafts.push(draft),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lf_domain::{citation::UnsupportedCite, date::CivilDate};
    use pretty_assertions::assert_eq;

    const ARTI: &str = "LEGIARTI000006419288";
    const TEXT: &str = "LEGITEXT000006070721";
    const KALIARTI: &str = "KALIARTI000049886059";
    const KALITEXT: &str = "KALITEXT000005652329";
    const JORFTEXT: &str = "JORFTEXT000019109412";

    fn target(raw: &str) -> DocumentId {
        DocumentId::parse(raw).unwrap()
    }

    fn related(nature: RelatedNature, raw: Option<&str>, title: Option<&str>) -> RelatedLink {
        RelatedLink {
            target: raw.map(target),
            nature,
            title: title.map(str::to_owned),
            date: CivilDate::new(2020, 1, 1),
        }
    }

    // ---- nature -> kind ------------------------------------------------------------------

    #[test]
    fn nature_to_kind_mapping() {
        use RelatedNature::{Code, Jorf, JorfCont, Kali, Loda};
        let other = || RelatedNature::Other("AUTRE".to_owned());
        // La table de 05 §5.11, ligne par ligne.
        let table = [
            (Code, Some(ARTI), CitationKind::CodeArticle),
            (Code, Some(TEXT), CitationKind::Text),
            (Loda, Some(TEXT), CitationKind::Text),
            (Loda, Some(ARTI), CitationKind::TextArticle),
            (Kali, Some(KALIARTI), CitationKind::Convention),
            (Kali, Some(KALITEXT), CitationKind::Convention),
            (Jorf, Some(JORFTEXT), CitationKind::Jorf),
            (Jorf, None, CitationKind::Jorf),
            (JorfCont, None, CitationKind::Jorf),
            (JorfCont, Some(JORFTEXT), CitationKind::Jorf),
        ];
        for (nature, id, kind) in table {
            assert_eq!(kind_of(&nature, id.map(target).as_ref()), kind, "{nature:?} {id:?}");
        }
        // Toute autre association est `unsupported`, y compris le `LODA` vers un `JORFTEXT` des
        // fixtures de la sandbox et une nature inconnue.
        let unsupported = [
            (Loda, Some(JORFTEXT)),
            (Loda, None),
            (Code, None),
            (Code, Some(KALIARTI)),
            (Kali, Some(ARTI)),
            (Kali, None),
            (Jorf, Some(ARTI)),
            (other(), Some(ARTI)),
            (other(), None),
        ];
        for (nature, id) in unsupported {
            let kind = kind_of(&nature, id.map(target).as_ref());
            assert_eq!(kind, CitationKind::Unsupported, "{nature:?} {id:?}");
        }
    }

    // ---- liens d'un article --------------------------------------------------------------

    #[test]
    fn only_readable_targets_carry_an_id_and_a_source_url() {
        let page = "https://www.legifrance.gouv.fr";
        let cases = [
            (RelatedNature::Code, ARTI, Some(format!("{page}/codes/article_lc/{ARTI}"))),
            (RelatedNature::Loda, ARTI, Some(format!("{page}/codes/article_lc/{ARTI}"))),
            (RelatedNature::Code, TEXT, Some(format!("{page}/codes/texte_lc/{TEXT}"))),
            (RelatedNature::Loda, TEXT, Some(format!("{page}/loda/id/{TEXT}"))),
            (RelatedNature::Kali, KALIARTI, Some(format!("{page}/conv_coll/id/{KALIARTI}"))),
            (RelatedNature::Kali, KALITEXT, Some(format!("{page}/conv_coll/id/{KALITEXT}"))),
            // La surface JORF n'est pas retenue : un JORFTEXT garde son titre, sans id.
            (RelatedNature::Jorf, JORFTEXT, None),
        ];
        for (nature, raw, url) in cases {
            let links = RelatedLinks {
                cites: vec![related(nature.clone(), Some(raw), Some("Un lien"))],
                cited_by: vec![],
            };
            let out = article_links(&links);
            assert_eq!(out.len(), 1, "{nature:?} {raw}");
            assert_eq!(out[0].id.as_deref(), url.as_ref().map(|_| raw), "{nature:?} {raw}");
            assert_eq!(out[0].source_url, url, "{nature:?} {raw}");
            assert_eq!(out[0].citation, "Un lien");
        }
    }

    #[test]
    fn article_links_follow_the_sandbox_shape_cites_first_then_cited_by() {
        // Les trois formes vues sur l'article 9 du Code civil (liensCitePar de la sandbox).
        let links = RelatedLinks {
            cites: vec![related(RelatedNature::Code, Some(ARTI), Some("Code civil, art. 1240"))],
            cited_by: vec![
                related(
                    RelatedNature::Loda,
                    Some(JORFTEXT),
                    Some("Délibération n° 2008-161  du 3 juin 2008, v. init."),
                ),
                // JORFARTI : pas de cible typée (03 §4.11), le titre reste.
                related(RelatedNature::Loda, None, Some("Circulaire du 14 septembre 2011 - art.")),
                related(RelatedNature::Loda, Some(ARTI), Some("Arrêté du 21 juin 2016 - art. (V)")),
            ],
        };
        let out = article_links(&links);
        let summary: Vec<_> = out
            .iter()
            .map(|link| (link.relation.as_str(), link.kind.snake_case(), link.id.is_some()))
            .collect();
        assert_eq!(
            summary,
            [
                ("cites", "code_article", true),
                ("cited_by", "unsupported", false),
                ("cited_by", "unsupported", false),
                ("cited_by", "text_article", true),
            ]
        );
        assert_eq!(out[1].citation, "Délibération n° 2008-161 du 3 juin 2008, v. init.");
    }

    #[test]
    fn a_link_without_title_falls_back_to_its_id_and_one_with_neither_is_dropped() {
        let links = RelatedLinks {
            cites: vec![
                related(RelatedNature::Code, Some(ARTI), None),
                related(RelatedNature::Code, Some(TEXT), Some("  \n ")),
                related(RelatedNature::Other("X".to_owned()), None, None),
            ],
            cited_by: vec![],
        };
        let out = article_links(&links);
        let citations: Vec<_> = out.iter().map(|link| link.citation.as_str()).collect();
        assert_eq!(citations, [ARTI, TEXT]);
    }

    #[test]
    fn identical_article_links_are_merged_but_both_directions_are_kept() {
        let same = || related(RelatedNature::Code, Some(ARTI), Some("Code civil, art. 1240"));
        let links = RelatedLinks { cites: vec![same(), same()], cited_by: vec![same()] };
        let out = article_links(&links);
        let relations: Vec<_> = out.iter().map(|link| link.relation).collect();
        assert_eq!(relations, [Relation::Cites, Relation::CitedBy]);
    }

    #[test]
    fn a_huge_title_is_clipped_so_it_cannot_empty_the_list() {
        let title = "é".repeat(CITATION_MAX_CHARS + 10);
        let links = RelatedLinks {
            cites: vec![related(RelatedNature::Code, Some(ARTI), Some(&title))],
            cited_by: vec![],
        };
        let out = article_links(&links);
        assert_eq!(out[0].citation.chars().count(), CITATION_MAX_CHARS + 1);
        assert!(out[0].citation.ends_with('…'));
        // À la limite exacte, rien n'est coupé.
        let exact = "e".repeat(CITATION_MAX_CHARS);
        assert_eq!(clip(exact.clone()), exact);
    }

    // ---- liens d'une décision ------------------------------------------------------------

    /// `citationJp` de `JURITEXT000051743650` (`consult_juri.json`, mojibake réparé).
    const CITATION_JP: &str = "Sur l'interprétation du droit interne à la lumière de la directive \
        85/374/CEE du Conseil du 25 juillet 1985, à rapprocher : 1re Civ., 25 mai 2023, pourvoi \
        n° 21-23.174 (cassation partielle) et l'arrêt cité.";

    /// Les deux titres de `liens` de la même décision.
    fn liens() -> Vec<String> {
        vec![
            "1 du code civil.".to_owned(),
            "Article 10 de la directive 85/374/CEE du Conseil, du 25 juillet 1985, relative au \
             rapprochement des dispositions législatives, réglementaires et administratives des \
             Etats membres en matière de responsabilité du fait des produits défectueux ;  \
             article 2270-"
                .to_owned(),
        ]
    }

    fn links_of(cited: Option<&str>, applied: &[String]) -> Vec<LinkOut> {
        decision_links(&DecisionReferences { cited_case_law: cited, applied_texts: applied })
    }

    fn shape(links: &[LinkOut]) -> Vec<(&'static str, &'static str, &str)> {
        links
            .iter()
            .map(|link| (link.relation.as_str(), link.kind.snake_case(), link.citation.as_str()))
            .collect()
    }

    #[test]
    fn citation_jp_and_liens_become_the_two_links_of_the_documented_example() {
        let out = links_of(Some(CITATION_JP), &liens());
        // « 1 du code civil. » (N01) n'a pas d'ancre : aucun lien. La directive de citationJp et
        // celle des liens visent le même acte : la plus précise (l'article 10) est gardée.
        assert_eq!(
            shape(&out),
            [
                ("cites_case_law", "decision", "Cass. 1re civ., 25 mai 2023, n° 21-23.174"),
                (
                    "applies_text",
                    "unsupported",
                    "Article 10 de la directive 85/374/CEE du Conseil, du 25 juillet 1985"
                ),
            ]
        );
        assert!(out.iter().all(|link| link.id.is_none() && link.source_url.is_none()));
    }

    #[test]
    fn nothing_to_analyse_gives_no_link() {
        assert_eq!(links_of(None, &[]), vec![]);
        assert_eq!(links_of(Some(""), &["1 du code civil.".to_owned()]), vec![]);
        assert_eq!(links_of(Some("Sur l'interprétation du droit interne"), &[]), vec![]);
    }

    #[test]
    fn a_decision_or_an_ecli_is_case_law_only_in_citation_jp() {
        let jp = "Cass. 1re civ., 4 juin 2025, n° 24-13.470 ; ECLI:FR:CCASS:2025:C100389 ; \
                  article 1240 du code civil";
        let out = links_of(Some(jp), &[]);
        assert_eq!(
            shape(&out),
            [
                ("cites_case_law", "decision", "Cass. 1re civ., 4 juin 2025, n° 24-13.470"),
                ("cites_case_law", "ecli", "ECLI:FR:CCASS:2025:C100389"),
                ("applies_text", "code_article", "Code civil, art. 1240"),
            ]
        );
        // Dans `liens`, tout est un texte appliqué, une décision comprise.
        let out = links_of(None, &["Cass. 1re civ., 4 juin 2025, n° 24-13.470".to_owned()]);
        assert_eq!(
            shape(&out),
            [("applies_text", "decision", "Cass. 1re civ., 4 juin 2025, n° 24-13.470")]
        );
    }

    #[test]
    fn case_law_comes_first_whatever_the_order_of_the_text() {
        let jp = "article 1240 du code civil ; Cass. 1re civ., 4 juin 2025, n° 24-13.470 ; \
                  article 1241 du code civil";
        let out = links_of(Some(jp), &[]);
        let relations: Vec<_> = out.iter().map(|link| link.relation.as_str()).collect();
        assert_eq!(relations, ["cites_case_law", "applies_text", "applies_text"]);
        assert_eq!(out[1].citation, "Code civil, art. 1240");
        assert_eq!(out[2].citation, "Code civil, art. 1241");
    }

    #[test]
    fn html_in_the_references_is_stripped() {
        let jp = "<p>Voir&nbsp;: <b>article 1240</b> du code civil</p>";
        let out = links_of(Some(jp), &[]);
        assert_eq!(shape(&out), [("applies_text", "code_article", "Code civil, art. 1240")]);
    }

    #[test]
    fn duplicates_are_merged_and_the_more_precise_link_wins_in_any_order() {
        let act = "loi n° 2016-1321 du 7 octobre 2016";
        let article = "art. 4 de la loi n° 2016-1321 du 7 octobre 2016";
        let precise = "Loi n° 2016-1321 du 7 octobre 2016, art. 4";
        // L'acte seul puis l'article : l'article remplace l'acte.
        let out = links_of(None, &[act.to_owned(), article.to_owned(), article.to_owned()]);
        assert_eq!(shape(&out), [("applies_text", "text_article", precise)]);
        // L'article puis l'acte seul : l'acte seul est écarté.
        let out = links_of(None, &[article.to_owned(), act.to_owned()]);
        assert_eq!(shape(&out), [("applies_text", "text_article", precise)]);
        // Deux articles différents du même acte restent deux liens.
        let other = "art. 5 de la loi n° 2016-1321 du 7 octobre 2016";
        let out = links_of(None, &[article.to_owned(), other.to_owned(), act.to_owned()]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].citation, "Loi n° 2016-1321 du 7 octobre 2016, art. 5");
    }

    #[test]
    fn two_articles_of_one_directive_stay_apart_and_the_bare_mention_is_absorbed() {
        let directive = "directive 85/374/CEE du Conseil du 25 juillet 1985".to_owned();
        let art10 = "Article 10 de la directive 85/374/CEE du Conseil".to_owned();
        let art12 = "Article 12 de la directive 85/374/CEE du Conseil".to_owned();
        let out = links_of(None, &[directive, art10, art12]);
        let citations: Vec<_> = out.iter().map(|link| link.citation.as_str()).collect();
        assert_eq!(
            citations,
            [
                "Article 10 de la directive 85/374/CEE du Conseil",
                "Article 12 de la directive 85/374/CEE du Conseil"
            ]
        );
    }

    #[test]
    fn the_same_text_cited_as_case_law_and_as_text_is_not_merged() {
        // Même clé d'acte, sens différents : deux liens.
        let jp = "Cass. 1re civ., 4 juin 2025, n° 24-13.470";
        let out = links_of(Some(jp), &[jp.to_owned()]);
        let relations: Vec<_> = out.iter().map(|link| link.relation).collect();
        assert_eq!(relations, [Relation::CitesCaseLaw, Relation::AppliesText]);
    }

    #[test]
    fn a_recognised_text_with_nothing_visible_is_dropped() {
        let unsupported = CitationRef::Unsupported(UnsupportedCite {
            family: lf_domain::citation::UnsupportedFamily::Doctrine,
            key: None,
        });
        assert_eq!(shown(&unsupported, "<b></b>  "), None);
        assert_eq!(shown(&unsupported, "Dalloz 2020"), Some("Dalloz 2020".to_owned()));
        let spanned = Spanned::new(unsupported, lf_domain::citation::Span { start: 0, end: 7 });
        // Un span qui ne tombe pas sur le texte donné (frontière invalide) ne fait pas paniquer.
        assert!(draft_of(Source::Liens, "<b></b>", &spanned).is_none());
        assert!(draft_of(Source::Liens, "é", &spanned).is_none());
    }

    #[test]
    fn leading_article_reads_the_number_after_the_word() {
        assert_eq!(leading_article("Article 10 de la directive"), Some("10".to_owned()));
        assert_eq!(leading_article("  art. 4, de la loi"), Some("4".to_owned()));
        assert_eq!(leading_article("Articles 4 et 5"), Some("4".to_owned()));
        assert_eq!(leading_article("art 7"), Some("7".to_owned()));
        assert_eq!(leading_article("directive 85/374/CEE"), None);
        assert_eq!(leading_article("Article "), None);
        assert_eq!(leading_article(""), None);
    }
}
