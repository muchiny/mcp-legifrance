//! Table des matières d'un texte consolidé : brouillon rempli depuis les DTO (04 §9.2 ; 03 §4.2).
//!
//! Le mapper ne fait que recopier : chaque [`TocSectionDto`] devient un [`SectionDraft`], chaque
//! [`TocArticleDto`] un [`ArticleDraft`] (`content` y devient `content_html`), dans l'ordre de
//! l'API. Les chaînes sont déplacées, jamais clonées (`content` peut peser plusieurs Mio) ; les
//! dates et les liens des DTO ne sont pas lus. Filtrage, nettoyage des titres et des numéros,
//! tri, dédoublonnage et garde de profondeur appartiennent à [`Toc::build`] (03 §4.2).
//!
//! [`build`] verse ensuite deux compteurs aux [`Diagnostics`] : les articles écartés par le
//! domaine (`toc_dropped`) et les invisibles retirés des textes en ligne des articles retenus
//! (`invisible_chars_removed`, D17). Une erreur du domaine devient un [`CorpusError::Decode`]
//! dont le chemin ne nomme qu'un champ déclaré, jamais une valeur du corps.

use lf_application::ports::CorpusError;
use lf_domain::DomainError;
use lf_domain::entity::{ArticleDraft, SectionDraft, Toc, TocDraft, TocNode};
use lf_domain::text::LongText;

use super::{Diagnostics, Field};
use crate::dto::consult_text::{TocArticleDto, TocSectionDto};

/// Table des matières de `sections` et `articles` (racine d'un texte consolidé), lue pour
/// `endpoint` ; ses articles écartés et ses invisibles retirés sont comptés dans `diags`.
///
/// # Errors
///
/// [`CorpusError::Decode`] si [`Toc::build`] refuse le brouillon :
/// `<endpoint>: sections: table of contents too deep` au-delà de 64 niveaux,
/// `<endpoint>: sections[].articles[].content: text too large` pour un texte en ligne trop long.
/// Les compteurs ne sont alors pas touchés.
pub(super) fn build(
    sections: Vec<TocSectionDto>,
    articles: Vec<TocArticleDto>,
    endpoint: &'static str,
    diags: &mut Diagnostics,
) -> Result<Toc, CorpusError> {
    let toc = Toc::build(draft(sections, articles))
        .map_err(|error| Field { endpoint, path: failure_path(&error) }.domain(&error))?;
    diags.add_toc_dropped(toc.dropped());
    diags.add_invisible(invisible_in(toc.roots()));
    Ok(toc)
}

/// Brouillon de la racine : sections puis articles de premier niveau, dans l'ordre de l'API.
fn draft(sections: Vec<TocSectionDto>, articles: Vec<TocArticleDto>) -> TocDraft {
    TocDraft {
        sections: sections.into_iter().map(section_draft).collect(),
        articles: articles.into_iter().map(article_draft).collect(),
    }
}

/// Section brute, sous-sections et articles compris ; les DTO ne déclarent ni dates ni liens.
/// La récursion suit le DTO, dont `serde_json` borne la profondeur (62 ou 63 niveaux).
fn section_draft(dto: TocSectionDto) -> SectionDraft {
    let TocSectionDto { id, title, etat, int_ordre, sections, articles } = dto;
    SectionDraft {
        id,
        title,
        int_ordre,
        etat,
        sections: sections.into_iter().map(section_draft).collect(),
        articles: articles.into_iter().map(article_draft).collect(),
    }
}

/// Article brut : `content` devient `content_html`, déplacé ; le DTO ne déclare ni dates ni liens.
fn article_draft(dto: TocArticleDto) -> ArticleDraft {
    let TocArticleDto { id, num, etat, content, int_ordre } = dto;
    ArticleDraft { id, num, int_ordre, etat, content_html: content }
}

/// Chemin du champ refusé par `error` : le contenu d'un article pour un texte en ligne trop long,
/// la liste des sections pour toute autre erreur (profondeur comprise).
fn failure_path(error: &DomainError) -> &'static str {
    match error {
        DomainError::TextTooLarge { .. } => "sections[].articles[].content",
        _ => "sections",
    }
}

/// Invisibles retirés des textes en ligne des articles de `nodes`, sous-sections comprises ;
/// somme saturée à `u32::MAX`. La récursion suit l'arbre validé, d'au plus 64 niveaux.
fn invisible_in(nodes: &[TocNode]) -> u32 {
    nodes.iter().fold(0, |total: u32, node| {
        let removed = match node {
            TocNode::Article(article) => {
                article.inline_text.as_ref().map_or(0, LongText::invisible_removed)
            }
            TocNode::Section(section) => invisible_in(&section.children),
        };
        total.saturating_add(removed)
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::num::NonZeroU32;

    use lf_domain::entity::{TocArticle, TocEntry, TocEntryRef};
    use lf_domain::status::LegalStatus;

    use super::*;
    use crate::dto::consult_text::ConsultTextDto;
    use crate::map::endpoint;
    use crate::map::fixtures::{decode_as, golden, text};

    /// Article de table des matières d'identifiant `id`, sans autre champ.
    fn article(id: &str) -> TocArticleDto {
        TocArticleDto { id: text!(id), ..TocArticleDto::default() }
    }

    /// Article `id` dont le contenu HTML est `content`.
    fn with_content(id: &str, content: &str) -> TocArticleDto {
        TocArticleDto { content: text!(content), ..article(id) }
    }

    /// `levels` sections imbriquées, titrées `S1` à `S<levels>`, sans article. Construites à la
    /// main : `serde_json` plafonne le JSON à 62 ou 63 niveaux de sections (`dto/consult_text.rs`).
    fn chain(levels: usize) -> Vec<TocSectionDto> {
        let mut sections = Vec::new();
        for level in (1..=levels).rev() {
            let title = Some(format!("S{level}"));
            sections = vec![TocSectionDto { title, sections, ..TocSectionDto::default() }];
        }
        sections
    }

    /// Une page assez grande pour toute table des fixtures.
    fn whole() -> NonZeroU32 {
        NonZeroU32::new(1000).unwrap()
    }

    /// Table des matières de la fixture `name`, construite pour `endpoint`, et ses compteurs.
    fn fixture_toc(name: &str, endpoint: &'static str) -> (Toc, Diagnostics) {
        let dto = decode_as::<ConsultTextDto>(name);
        let mut diags = Diagnostics::default();
        let toc = build(dto.sections, dto.articles, endpoint, &mut diags).unwrap();
        (toc, diags)
    }

    /// Entrée lisible : profondeur, puis titre de la section ou identifiant de l'article.
    fn shown<'a>(entry: &TocEntry<'a>) -> (u16, &'a str) {
        match entry.node {
            TocEntryRef::Section { title } => (entry.depth, title),
            TocEntryRef::Article(article) => (entry.depth, article.article_id.as_str()),
        }
    }

    /// Articles de `entries`, dans l'ordre, avec leur profondeur.
    fn articles_in<'a>(entries: &[TocEntry<'a>]) -> Vec<(u16, &'a TocArticle)> {
        entries
            .iter()
            .filter_map(|entry| match entry.node {
                TocEntryRef::Article(article) => Some((entry.depth, article)),
                TocEntryRef::Section { .. } => None,
            })
            .collect()
    }

    /// Titres des sections de `entries`, dans l'ordre, avec leur profondeur.
    fn sections_in<'a>(entries: &[TocEntry<'a>]) -> Vec<(u16, &'a str)> {
        entries
            .iter()
            .filter_map(|entry| match entry.node {
                TocEntryRef::Section { title } => Some((entry.depth, title)),
                TocEntryRef::Article(_) => None,
            })
            .collect()
    }

    /// Section d'identifiant `id`, cherchée en préordre dans `sections`.
    fn find_section<'a>(sections: &'a [TocSectionDto], id: &str) -> Option<&'a TocSectionDto> {
        sections.iter().find_map(|section| {
            if section.id.as_deref() == Some(id) {
                Some(section)
            } else {
                find_section(&section.sections, id)
            }
        })
    }

    #[test]
    fn section_draft_copies_every_field() {
        let dto = TocSectionDto {
            id: text!("LEGISCTA000033205040"),
            title: text!(" Titre Ier "),
            etat: text!("MODIFIE"),
            int_ordre: Some(3),
            sections: vec![TocSectionDto {
                id: text!("LEGISCTA000033205015"),
                ..TocSectionDto::default()
            }],
            articles: vec![article("LEGIARTI000033205126")],
        };
        assert_eq!(
            section_draft(dto),
            SectionDraft {
                id: text!("LEGISCTA000033205040"),
                title: text!(" Titre Ier "),
                int_ordre: Some(3),
                etat: text!("MODIFIE"),
                sections: vec![SectionDraft {
                    id: text!("LEGISCTA000033205015"),
                    ..SectionDraft::default()
                }],
                articles: vec![ArticleDraft {
                    id: text!("LEGIARTI000033205126"),
                    ..ArticleDraft::default()
                }],
            }
        );
        assert_eq!(section_draft(TocSectionDto::default()), SectionDraft::default());
    }

    #[test]
    fn article_draft_takes_content_as_content_html() {
        let dto = TocArticleDto {
            id: text!("KALIARTI000049886059"),
            num: text!("19"),
            etat: text!("VIGUEUR_ETEN"),
            content: text!("<p>x</p>"),
            int_ordre: Some(1_073_741_823),
        };
        let expected = ArticleDraft {
            id: text!("KALIARTI000049886059"),
            num: text!("19"),
            int_ordre: Some(1_073_741_823),
            etat: text!("VIGUEUR_ETEN"),
            content_html: text!("<p>x</p>"),
        };
        assert_eq!(article_draft(dto), expected);
        let bare = TocArticleDto { content: None, ..article("LEGIARTI000033205126") };
        assert_eq!(
            article_draft(bare),
            ArticleDraft { id: text!("LEGIARTI000033205126"), ..ArticleDraft::default() }
        );

        let sections = vec![
            TocSectionDto { id: text!("LEGISCTA000000000001"), ..TocSectionDto::default() },
            TocSectionDto { id: text!("LEGISCTA000000000002"), ..TocSectionDto::default() },
        ];
        assert_eq!(
            draft(sections, vec![article("LEGIARTI000000000001")]),
            TocDraft {
                sections: vec![
                    SectionDraft { id: text!("LEGISCTA000000000001"), ..SectionDraft::default() },
                    SectionDraft { id: text!("LEGISCTA000000000002"), ..SectionDraft::default() },
                ],
                articles: vec![ArticleDraft {
                    id: text!("LEGIARTI000000000001"),
                    ..ArticleDraft::default()
                }],
            }
        );
    }

    #[test]
    fn build_counts_dropped_articles_into_the_diagnostics() {
        let articles = || {
            vec![article("LEGIARTI000000000001"), article("XYZ"), article("LEGIARTI000000000001")]
        };
        let mut diags = Diagnostics::default();
        let toc = build(vec![], articles(), endpoint::LAW_DECREE, &mut diags).unwrap();
        assert_eq!(toc.dropped(), 2);
        assert_eq!(diags, Diagnostics { toc_dropped: 2, ..Diagnostics::default() });

        let mut diags = Diagnostics { toc_dropped: 5, ..Diagnostics::default() };
        build(vec![], articles(), endpoint::LAW_DECREE, &mut diags).unwrap();
        assert_eq!(diags, Diagnostics { toc_dropped: 7, ..Diagnostics::default() });
    }

    #[test]
    fn build_counts_invisible_characters_of_inline_texts() {
        let section = TocSectionDto {
            id: text!("KALISCTA000000000001"),
            title: text!("Chapitre"),
            articles: vec![with_content("KALIARTI000000000002", "<p>c\u{200b}\u{200b}d</p>")],
            ..TocSectionDto::default()
        };
        let root = with_content("KALIARTI000000000001", "<p>a\u{200b}b</p>");
        let mut diags = Diagnostics::default();
        let toc = build(vec![section], vec![root], endpoint::KALI_TEXT, &mut diags).unwrap();
        assert_eq!(invisible_in(toc.roots()), 3);
        assert_eq!(diags, Diagnostics { invisible_chars_removed: 3, ..Diagnostics::default() });

        assert_eq!(invisible_in(&[]), 0);
        let mut diags = Diagnostics::default();
        let bare =
            build(vec![], vec![article("KALIARTI000000000001")], endpoint::KALI_TEXT, &mut diags)
                .unwrap();
        assert_eq!(invisible_in(bare.roots()), 0);
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn a_toc_of_64_levels_is_built() {
        let mut diags = Diagnostics::default();
        let toc = build(chain(64), vec![], endpoint::LAW_DECREE, &mut diags).unwrap();
        assert_eq!(toc.len(), 64);
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn a_toc_of_65_levels_is_a_decode_error() {
        let mut diags = Diagnostics { toc_dropped: 1, ..Diagnostics::default() };
        assert_eq!(
            build(chain(65), vec![], endpoint::LAW_DECREE, &mut diags),
            Err(CorpusError::Decode {
                detail: "/consult/lawDecree: sections: table of contents too deep".into()
            })
        );
        assert_eq!(diags, Diagnostics { toc_dropped: 1, ..Diagnostics::default() });
    }

    #[test]
    fn failure_path_names_the_field_of_every_error() {
        assert_eq!(failure_path(&DomainError::TocTooDeep { max: 64 }), "sections");
        assert_eq!(
            failure_path(&DomainError::TextTooLarge { chars: 16_000_001 }),
            "sections[].articles[].content"
        );
        let other = DomainError::EmptyQuery;
        assert_eq!(failure_path(&other), "sections");
        let field = Field { endpoint: endpoint::LAW_DECREE, path: failure_path(&other) };
        assert_eq!(
            field.domain(&other),
            CorpusError::Decode { detail: "/consult/lawDecree: sections: invalid value".into() }
        );
    }

    #[test]
    fn an_oversized_inline_text_is_a_decode_error_on_the_content_path() {
        let huge = format!("<p>{}</p>", "a".repeat(16_000_001));
        let mut diags = Diagnostics::default();
        let error = build(
            vec![],
            vec![TocArticleDto { content: Some(huge), ..article("KALIARTI000000000001") }],
            endpoint::KALI_TEXT,
            &mut diags,
        )
        .unwrap_err();
        assert_eq!(
            error,
            CorpusError::Decode {
                detail: "/consult/kaliText: sections[].articles[].content: text too large".into()
            }
        );
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn toc_article_projection_matches_reference_order() {
        // Discriminant : cette section porte 5 articles puis 3 sous-sections ; les articles
        // passent avant les sous-sections (A7), comme dans la référence.
        let loda = decode_as::<ConsultTextDto>("consult_loda");
        let discriminant = find_section(&loda.sections, "LEGISCTA000033205020").unwrap();
        assert_eq!((discriminant.articles.len(), discriminant.sections.len()), (5, 3));

        let golden = golden();
        let fixtures =
            [("consult_loda", endpoint::LAW_DECREE), ("consult_kali", endpoint::KALI_TEXT)];
        let mut projections = Vec::new();
        for (name, endpoint) in fixtures {
            let (toc, _) = fixture_toc(name, endpoint);
            let page = toc.page(0, whole()).unwrap();
            let ids: Vec<String> = articles_in(&page.entries)
                .into_iter()
                .map(|(_, article)| article.article_id.as_str().to_owned())
                .collect();
            let expected: Vec<String> = golden["normalizer"][format!("{name}.json")]["articles"]
                .as_array()
                .unwrap()
                .iter()
                .map(|article| article["article_id"].as_str().unwrap().to_owned())
                .collect();
            assert_eq!(ids, expected, "{name}");
            projections.push(ids);
        }
        let [loda_ids, kali_ids] = <[Vec<String>; 2]>::try_from(projections).unwrap();
        assert_eq!(loda_ids.len(), 62);
        assert_eq!(loda_ids.first().map(String::as_str), Some("LEGIARTI000033205126"));
        assert_eq!(loda_ids.last().map(String::as_str), Some("LEGIARTI000033205268"));
        assert_eq!(kali_ids, ["KALIARTI000049886059"]);
    }

    #[test]
    fn toc_preserves_section_titles() {
        let (toc, diags) = fixture_toc("consult_loda", endpoint::LAW_DECREE);
        let page = toc.page(0, whole()).unwrap();
        assert_eq!((toc.len(), page.total, page.next_offset, toc.dropped()), (88, 88, None, 0));
        assert_eq!(diags, Diagnostics::default());

        let sections = sections_in(&page.entries);
        let articles = articles_in(&page.entries);
        assert_eq!((sections.len(), articles.len()), (26, 62));
        let mut depths = BTreeMap::new();
        for entry in &page.entries {
            *depths.entry(entry.depth).or_insert(0_u32) += 1;
        }
        assert_eq!(depths, BTreeMap::from([(1, 3), (2, 7), (3, 26), (4, 52)]));

        let first: Vec<_> = page.entries.iter().take(4).map(shown).collect();
        assert_eq!(
            first,
            [
                (1, "Titre Ier : LA CIRCULATION DES DONNÉES ET DU SAVOIR"),
                (2, "Chapitre Ier : Economie de la donnée"),
                (3, "Section 1 : Ouverture de l'accès aux données publiques"),
                (4, "LEGIARTI000033205126"),
            ]
        );
        let (_, first_article) = articles[0];
        assert_eq!(first_article.num.as_deref(), Some("1"));
        let without_text = articles.iter().filter(|(_, article)| article.inline_text.is_none());
        assert_eq!(without_text.count(), 62);

        // Trois racines, et les trois entrées de profondeur 1 sont des sections titrées.
        assert_eq!(toc.roots().len(), 3);
        let roots: Vec<&str> =
            sections.iter().filter(|(depth, _)| *depth == 1).map(|(_, title)| *title).collect();
        assert_eq!(
            roots,
            [
                "Titre Ier : LA CIRCULATION DES DONNÉES ET DU SAVOIR",
                "Titre II : LA PROTECTION DES DROITS DANS LA SOCIÉTÉ NUMÉRIQUE",
                "Titre III : L'ACCÈS AU NUMÉRIQUE",
            ]
        );
    }

    #[test]
    fn kali_inline_content_is_available_without_extra_call() {
        let (toc, diags) = fixture_toc("consult_kali", endpoint::KALI_TEXT);
        assert_eq!((toc.len(), toc.dropped()), (2, 0));
        assert_eq!(diags, Diagnostics::default());
        let page = toc.page(0, whole()).unwrap();
        let entries: Vec<_> = page.entries.iter().map(shown).collect();
        assert_eq!(
            entries,
            [(1, "Chapitre 1er : Recours au CDD d'usage"), (2, "KALIARTI000049886059")]
        );

        let [(_, article)] = <[_; 1]>::try_from(articles_in(&page.entries)).unwrap();
        assert_eq!(article.etat, Some(LegalStatus::VigueurEtendue));
        let inline = article.inline_text.as_ref().unwrap();
        let canonical = inline.as_str();
        assert!(
            canonical.starts_with(
                "Les parties conviennent de préciser au niveau de la branche les conditions \
                 d'un recours légitime"
            ),
            "{canonical}"
        );
        assert!(!canonical.contains('<'), "{canonical}");
        assert!(!canonical.contains("href"), "{canonical}");
        assert_eq!(inline.paragraphs().count(), 1);
    }
}
