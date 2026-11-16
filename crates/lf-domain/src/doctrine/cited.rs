//! Articles de code cités par une publication du BOFiP (ADR-0032).
//!
//! Une publication peut être plus ancienne que la loi qu'elle commente. La lecture ne le juge
//! pas : elle liste les articles de code que cite le texte, et l'appelant les contrôle avec
//! `fr_verify_citations` (date de début de la version en vigueur), puis `fr_diff_article`.
//!
//! La liste vient du parseur de citations du domaine ([`parse_with`], options par défaut) :
//!
//! - seuls les articles d'un code du dictionnaire ([`CodeRef::Known`]) sont gardés : un article de
//!   loi ou de décret, un article sans code et un code inconnu ne le sont pas ;
//! - chaque article sous sa forme canonique ([`format::canonical`], « Code civil, art. 9 »), une
//!   seule fois, dans l'ordre de sa première apparition ;
//! - une plage par ses deux bornes : la forme canonique d'une plage (« art. 1078-1 à 1078-3 »)
//!   serait relue comme sa seule première borne ;
//! - [`CITED_ARTICLES_MAX`] articles au plus ;
//! - les [`CITED_TEXT_MAX_CHARS`] premiers caractères du texte au plus, coupés à la fin d'un
//!   paragraphe (note du 2026-11-16 d'ADR-0032).
//!
//! Mesure du 2026-11-16 (export du jeu du 2026-11-15, 9 173 publications) : chaque liste, jointe
//! par « ; » ou par des retours à la ligne, est relue à l'identique par le parseur ; c'est la forme
//! sous laquelle un appelant la passe à `fr_verify_citations`.

use crate::citation::{
    ArticleNum, ArticleRef, CitationRef, CodeRef, ParseOptions, format, parse_with,
};

/// Nombre maximal d'articles listés : le défaut de `max_citations` de `fr_verify_citations`
/// (05 §5.9), qui contrôle ainsi la liste entière en un appel (ADR-0032, décision 3).
pub const CITED_ARTICLES_MAX: usize = 50;

/// Longueur du texte lu par le parseur, en caractères. La plus longue publication du jeu en
/// compte 167 694 (2026-11-16) : aucune n'est coupée. Mesure du même jour, binaire optimisé :
/// sur un texte hostile de 4 Mio (le plafond d'une réponse du portail), la lecture des citations
/// prend jusqu'à 5,1 s sans borne et 0,9 s avec elle, quand la préparation du même texte en prend
/// 41 ms ; 65 ms au plus pour une publication du jeu.
pub const CITED_TEXT_MAX_CHARS: usize = 200_000;

/// Les articles de code cités par un texte.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CitedArticles {
    /// Formes canoniques, sans doublon, dans l'ordre de première apparition ;
    /// [`CITED_ARTICLES_MAX`] au plus.
    pub articles: Vec<String>,
    /// Vrai quand la liste peut manquer des articles : le texte en cite plus de
    /// [`CITED_ARTICLES_MAX`], il dépasse [`CITED_TEXT_MAX_CHARS`] caractères, ou le parseur a
    /// atteint son garde-fou ([`ParseOptions::max_citations`]).
    pub truncated: bool,
}

/// Les articles de code que cite `text` (voir le module).
///
/// ```
/// use lf_domain::doctrine::cited_articles;
///
/// let cited = cited_articles(
///     "L'article 9 du code civil et les articles 1078-1 à 1078-3 du même code ; \
///      l'article 3 de la loi n° 89-462 du 6 juillet 1989 n'est pas un article de code.",
/// );
/// assert_eq!(cited.articles, ["Code civil, art. 9", "Code civil, art. 1078-1", "Code civil, art. 1078-3"]);
/// assert!(!cited.truncated);
/// ```
#[must_use]
pub fn cited_articles(text: &str) -> CitedArticles {
    let (read, cut) = within_bound(text);
    let options = ParseOptions::default();
    let found = parse_with(read, &options);
    let mut cited = CitedArticles {
        articles: Vec::new(),
        truncated: cut || found.len() >= options.max_citations,
    };
    for citation in &found {
        for article in code_articles(&citation.value) {
            if cited.articles.contains(&article) {
                continue;
            }
            if cited.articles.len() == CITED_ARTICLES_MAX {
                cited.truncated = true;
                return cited;
            }
            cited.articles.push(article);
        }
    }
    cited
}

/// Le texte lu par le parseur, et `true` s'il a été coupé. Tout le texte s'il tient en
/// [`CITED_TEXT_MAX_CHARS`] caractères ; sinon les paragraphes entiers qui y tiennent, pour ne
/// jamais couper une citation (« CGI, art. 1135 bis » coupé après « 113 » serait un autre
/// article) ; rien si le premier paragraphe dépasse à lui seul la borne.
fn within_bound(text: &str) -> (&str, bool) {
    let Some((end, _)) = text.char_indices().nth(CITED_TEXT_MAX_CHARS) else {
        return (text, false);
    };
    let head = text.get(..end).unwrap_or_default();
    (head.rfind('\n').and_then(|at| head.get(..at)).unwrap_or_default(), true)
}

/// Formes canoniques d'un article d'un code du dictionnaire : une, ou les deux bornes d'une
/// plage. Rien pour toute autre citation.
fn code_articles(citation: &CitationRef) -> Vec<String> {
    let CitationRef::CodeArticle { code: code @ CodeRef::Known(_), article, .. } = citation else {
        return Vec::new();
    };
    std::iter::once(&article.num)
        .chain(article.range_to.as_ref())
        .filter_map(|num| format::canonical(&single(code, num)))
        .collect()
}

/// L'article `num` de `code`, sans subdivision ni plage.
fn single(code: &CodeRef, num: &ArticleNum) -> CitationRef {
    CitationRef::CodeArticle {
        code: code.clone(),
        article: ArticleRef { num: num.clone(), sub: Vec::new(), range_to: None },
        version: None,
        code_via: None,
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    use super::*;
    use crate::citation::parse;

    /// Les formes canoniques que le parseur relit dans `text`.
    fn reread(text: &str) -> Vec<String> {
        parse(text).iter().filter_map(|c| format::canonical(&c.value)).collect()
    }

    /// Une liste de `n` articles distincts du code civil, un par phrase.
    fn civil_articles(n: usize) -> String {
        (1..=n).map(|i| format!("Voir l'article {i} du code civil. ")).collect::<Vec<_>>().concat()
    }

    #[test]
    fn articles_come_once_each_in_order_of_first_appearance() {
        let cited = cited_articles(
            "L'article 1135 bis du code général des impôts renvoie à l'article 9 du code civil ; \
             l'article 1135 bis du code général des impôts s'applique, et l'article L. 80 A du \
             livre des procédures fiscales aussi.",
        );
        assert_eq!(
            cited,
            CitedArticles {
                articles: vec![
                    "Code général des impôts, art. 1135 bis".to_owned(),
                    "Code civil, art. 9".to_owned(),
                    "Livre des procédures fiscales, art. L80 A".to_owned(),
                ],
                truncated: false,
            }
        );
    }

    #[test]
    fn only_articles_of_a_code_of_the_dictionary_are_kept() {
        let cited = cited_articles(
            "L'article 3 de la loi n° 89-462 du 6 juillet 1989, l'article 12 du code des \
             impositions imaginaires, l'article 5, la décision n° 2019-791 QPC et \
             ECLI:FR:CCASS:2019:CO00412.",
        );
        assert_eq!(cited, CitedArticles::default());
    }

    #[test]
    fn a_range_is_listed_by_its_two_bounds() {
        let cited = cited_articles(
            "Les articles 1078-1 à 1078-3 du code civil, puis l'article 1078-3 du code civil.",
        );
        assert_eq!(cited.articles, ["Code civil, art. 1078-1", "Code civil, art. 1078-3"]);
        assert!(!cited.truncated);
    }

    #[test]
    fn a_text_without_citation_lists_nothing() {
        assert_eq!(cited_articles(""), CitedArticles::default());
        assert_eq!(cited_articles("Aucun article n'est cité ici."), CitedArticles::default());
    }

    #[test]
    fn fifty_articles_fit_and_the_fifty_first_marks_the_list_truncated() {
        let fifty = cited_articles(&civil_articles(CITED_ARTICLES_MAX));
        assert_eq!(fifty.articles.len(), CITED_ARTICLES_MAX);
        assert_eq!(fifty.articles.last().map(String::as_str), Some("Code civil, art. 50"));
        assert!(!fifty.truncated);

        // Un doublon après la cinquantième n'est pas un article de plus.
        let repeated = cited_articles(&format!(
            "{}Voir l'article 1 du code civil.",
            civil_articles(CITED_ARTICLES_MAX)
        ));
        assert_eq!(repeated, fifty);

        let more = cited_articles(&civil_articles(CITED_ARTICLES_MAX + 1));
        assert_eq!(more.articles, fifty.articles);
        assert!(more.truncated);
    }

    #[test]
    fn the_parser_guard_marks_the_list_truncated() {
        let guard = ParseOptions::default().max_citations;
        let below = "Voir l'article 9 du code civil. ".repeat(guard - 1);
        let at = "Voir l'article 9 du code civil. ".repeat(guard);
        assert_eq!(parse(&at).len(), guard);

        let cited = cited_articles(&below);
        assert_eq!(cited.articles, ["Code civil, art. 9"]);
        assert!(!cited.truncated);

        let cited = cited_articles(&at);
        assert_eq!(cited.articles, ["Code civil, art. 9"]);
        assert!(cited.truncated);
    }

    /// Un texte de `CITED_TEXT_MAX_CHARS` caractères ou plus : une citation, une fin de
    /// paragraphe à la position `newline` (ou aucune), du remplissage, puis une autre citation
    /// qui finit au caractère `last`.
    fn long_text(newline: Option<usize>, last: usize) -> String {
        let first = "Voir l'article 9 du code civil.";
        let second = "Voir l'article 1135 bis du code général des impôts.";
        let chars = |s: &str| s.chars().count();
        let mut text = first.to_owned();
        if let Some(at) = newline {
            text.push_str(&" ".repeat(at.saturating_sub(chars(first))));
            text.push('\n');
        }
        text.push_str(&" ".repeat(last.saturating_sub(chars(&text)).saturating_sub(chars(second))));
        text.push_str(second);
        assert_eq!(text.chars().count(), last);
        text
    }

    #[test]
    fn a_text_at_the_bound_is_read_whole() {
        let cited = cited_articles(&long_text(Some(1_000), CITED_TEXT_MAX_CHARS));
        assert_eq!(
            cited.articles,
            ["Code civil, art. 9", "Code général des impôts, art. 1135 bis"]
        );
        assert!(!cited.truncated);
    }

    #[test]
    fn a_longer_text_is_read_up_to_its_last_whole_paragraph() {
        let cited = cited_articles(&long_text(Some(1_000), CITED_TEXT_MAX_CHARS + 1));
        assert_eq!(cited.articles, ["Code civil, art. 9"]);
        assert!(cited.truncated);

        // La borne tombe dans « CGI, art. 1135 bis », après « 113 » : lu jusqu'à la borne, le
        // texte citerait l'article 113, un autre article. La coupe au paragraphe l'écarte.
        let before =
            format!("Voir l'article 9 du code civil.\n{}", " ".repeat(CITED_TEXT_MAX_CHARS - 100));
        let pad = CITED_TEXT_MAX_CHARS - "CGI, art. 113".len() - before.chars().count();
        let text = format!("{before}{}CGI, art. 1135 bis.", " ".repeat(pad));
        let at_bound: String = text.chars().take(CITED_TEXT_MAX_CHARS).collect();
        assert!(reread(&at_bound).contains(&"Code général des impôts, art. 113".to_owned()));
        let cited = cited_articles(&text);
        assert_eq!(cited.articles, ["Code civil, art. 9"]);
        assert!(cited.truncated);
    }

    #[test]
    fn a_first_paragraph_over_the_bound_lists_nothing() {
        let cited = cited_articles(&long_text(None, CITED_TEXT_MAX_CHARS + 1));
        assert_eq!(cited, CitedArticles { articles: Vec::new(), truncated: true });
    }

    /// Une citation d'article de code, écrite comme dans une publication.
    fn code_citation() -> impl Strategy<Value = String> {
        let prefix = prop_oneof![Just(""), Just("L. "), Just("R. "), Just("D. "), Just("A ")];
        let suffix = prop_oneof![
            Just(""),
            Just(" bis"),
            Just(" ter"),
            Just(" quater"),
            Just(" A"),
            Just(" B"),
            Just(" bis A"),
            Just(" 0 A"),
        ];
        let code = prop_oneof![
            Just("du code civil"),
            Just("du code général des impôts"),
            Just("du livre des procédures fiscales"),
            Just("du code de commerce"),
            Just("du code du travail"),
            Just("du code de la construction et de l'habitation"),
        ];
        let segments = prop::collection::vec(1_u16..100, 0..3);
        (prefix, 1_u16..3000, segments, suffix, code, any::<bool>()).prop_map(
            |(prefix, number, segments, suffix, code, range)| {
                let tail = segments.iter().map(|s| format!("-{s}")).collect::<Vec<_>>().concat();
                if range {
                    format!("articles {prefix}{number}{tail} à {prefix}{number}{tail}-1 {code}")
                } else {
                    format!("article {prefix}{number}{tail}{suffix} {code}")
                }
            },
        )
    }

    proptest! {
        /// Quelles que soient les citations, la liste tient sous le plafond, sans doublon, et
        /// chaque élément est relu à l'identique : seul, puis dans la liste jointe par « ; » ou
        /// par des retours à la ligne (l'usage de `fr_verify_citations`).
        #[test]
        fn every_listed_article_is_read_back_identically(
            citations in prop::collection::vec(code_citation(), 0..56),
            filler in prop_oneof![Just(" ; "), Just(". Puis "), Just(", et l'"), Just("\n")],
        ) {
            let cited = cited_articles(&citations.join(filler));
            prop_assert!(cited.articles.len() <= CITED_ARTICLES_MAX);
            for (i, article) in cited.articles.iter().enumerate() {
                prop_assert!(!cited.articles[..i].contains(article), "doublon : {article}");
                prop_assert_eq!(reread(article), vec![article.clone()]);
            }
            prop_assert_eq!(reread(&cited.articles.join("; ")), cited.articles.clone());
            prop_assert_eq!(reread(&cited.articles.join("\n")), cited.articles.clone());
        }

        /// Sur un texte quelconque, la fonction est totale et garde ses bornes.
        #[test]
        fn any_text_keeps_the_bounds(text in "\\PC{0,400}") {
            let cited = cited_articles(&text);
            prop_assert!(cited.articles.len() <= CITED_ARTICLES_MAX);
            prop_assert!(cited.articles.iter().all(|a| !a.is_empty()));
        }
    }
}
