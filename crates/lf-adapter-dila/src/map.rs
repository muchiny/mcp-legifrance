//! Traduction des documents LEGI en entités du domaine (rapport 10 §5, annexe C §7.6).
//!
//! Les documents gardent les valeurs brutes de la DILA ([`crate::model`]) ; ce module en tire les
//! entités que le port rend. Trois règles y sont appliquées partout :
//!
//! - **sentinelles** : toute date à partir du 2222-01-01 (`2999-01-01` fin ouverte, `2222-02-22`
//!   entrée en vigueur différée) devient `None` par [`open_ended`] ;
//! - **fenêtre de vigueur** : un lien ou un article est en vigueur à `D` si `debut <= D < fin`,
//!   une borne absente ne limitant pas (rapport 10 §10.6) ;
//! - **valeurs inconnues** : un `ETAT` ou une `NATURE` que le domaine ne connaît pas est gardé
//!   (`LegalStatus::Other`), jamais une erreur.

use lf_application::ports::{ConfigKind, CorpusError};
use lf_domain::{
    contract::Nor,
    date::{CivilDate, open_ended},
    entity::{
        ArticleDraft, ArticleText, ArticleVersion, CodeArticleRef, CodeListing, ParentText,
        RelatedLink, RelatedNature, SearchHit, SectionDraft, TOC_MAX_DEPTH, TextBody, Toc,
        TocDraft,
    },
    fond::Fond,
    id::{ArticleId, DocumentId, LegiArtiId, LegiTextId},
    status::LegalStatus,
    text::{LongText, clean_field},
};

use crate::{
    index::{View, plain, unreadable_entry},
    model::{Article, Link, LinkTarget, StructLink, TextVersion},
    path::DocKind,
};

/// Plus grand nombre de nœuds d'une table des matières : le Code civil en compte environ 12 000,
/// le plus gros code environ 25 000. Au-delà, le texte est trop gros pour `get_act`.
pub(crate) const TOC_MAX_NODES: usize = 60_000;

/// Plus grand nombre d'articles cités par résultat de recherche sur un code.
pub(crate) const HIT_MAX_ARTICLES: usize = 5;

/// Taille de l'extrait d'un article, en caractères, avant la coupe en fin de mot du domaine.
const SNIPPET_SOURCE_CHARS: usize = 400;

/// L'opération demandée n'existe pas dans les dumps LEGI (décision 00 §5, M9 : LEGI seul).
///
/// Le modèle lit ce `detail` tel quel (`ConfigKind::Settings`) : texte fixe, rédigé pour lui,
/// sans valeur de l'appelant. `what` est une constante du code, jamais une entrée.
pub(crate) fn unsupported(what: &str) -> CorpusError {
    CorpusError::Config {
        kind: ConfigKind::Settings,
        detail: format!(
            "{what} is not available in the local DILA corpus, which holds the LEGI fund only \
             (codes, laws and decrees); use the PISTE source for it"
        ),
    }
}

/// Document absent du corpus ; `endpoint` est le gabarit de l'API dont l'adapter prend la place.
pub(crate) fn not_found(endpoint: &str) -> CorpusError {
    CorpusError::NotFound { what: endpoint.to_owned() }
}

/// Texte HTML en texte long ; au-delà de la borne, `TooLarge`.
fn long_text(html: &str) -> Result<LongText, CorpusError> {
    LongText::from_html(html).map_err(|_| CorpusError::TooLarge {
        detail: "text above the 16 million character bound".to_owned(),
    })
}

/// `etat` brut en statut juridique.
fn status(raw: Option<&String>) -> Option<LegalStatus> {
    raw.and_then(|raw| LegalStatus::parse(raw))
}

/// Un identifiant de document, `None` s'il n'est pas valide.
fn document_id(raw: Option<&String>) -> Option<DocumentId> {
    raw.and_then(|raw| DocumentId::parse(raw).ok())
}

/// Fenêtre de vigueur : `debut <= at < fin`, une borne absente ne limitant pas.
pub(crate) fn in_force(debut: Option<CivilDate>, fin: Option<CivilDate>, at: CivilDate) -> bool {
    debut.is_none_or(|debut| debut <= at) && fin.is_none_or(|fin| at < fin)
}

/// Titre d'un texte : le titre complet de la DILA, à défaut son titre court.
pub(crate) fn title_of(text: &TextVersion) -> Option<String> {
    clean_field(text.title_full.as_deref()).or_else(|| clean_field(text.title.as_deref()))
}

/// Un article en `ArticleText`.
///
/// # Errors
///
/// [`CorpusError::Decode`] si l'identifiant stocké n'est pas un identifiant d'article,
/// [`CorpusError::TooLarge`] si le texte dépasse la borne du domaine.
pub(crate) fn article_text(article: &Article) -> Result<ArticleText, CorpusError> {
    let article_id = ArticleId::parse(&article.id).map_err(|_| unreadable_entry())?;
    let nota = clean_field(Some(&article.nota)).map(|_| long_text(&article.nota)).transpose()?;
    Ok(ArticleText {
        article_id,
        num: clean_field(article.num.as_deref()),
        etat: status(article.etat.as_ref()),
        code_title: clean_field(article.context.title.as_deref()),
        parent: document_id(article.context.cid.as_ref())
            .map(|id| ParentText::new(id, article.context.nature.as_deref())),
        path: article
            .context
            .sections
            .iter()
            .filter_map(|section| clean_field(Some(&section.title)))
            .collect(),
        full_sections_titre: None,
        date_debut: open_ended(article.date_debut),
        date_fin: open_ended(article.date_fin),
        text: long_text(&article.content)?,
        nota,
    })
}

/// Métadonnées d'un texte consolidé et son sommaire.
pub(crate) fn text_body(text: &TextVersion, toc: Toc) -> TextBody {
    TextBody {
        cid: document_id(text.chronicle.cid.as_ref()),
        nor: text.chronicle.nor.as_deref().and_then(Nor::parse),
        title: title_of(text),
        nature: clean_field(text.nature.as_deref()),
        etat: status(text.etat.as_ref()),
        date_parution: open_ended(text.chronicle.date_publi),
        num_parution: None,
        date_debut_version: open_ended(text.date_debut),
        date_fin_version: open_ended(text.date_fin),
        jo_reference: None,
        aliases: Vec::new(),
        toc,
    }
}

/// Le résultat de recherche d'un texte ; `None` si son identifiant n'est pas celui du fond.
pub(crate) fn text_hit(fond: Fond, text: &TextVersion) -> Option<SearchHit> {
    let id = DocumentId::parse(&text.id).ok()?;
    let mut hit = SearchHit::new(fond, id, None).ok()?;
    hit.cid = document_id(text.chronicle.cid.as_ref());
    hit.title = title_of(text);
    hit.date = open_ended(text.chronicle.date_texte);
    hit.nature = clean_field(text.nature.as_deref());
    hit.etat = status(text.etat.as_ref());
    Some(hit)
}

/// L'article d'un résultat de recherche sur un code ; `None` si son identifiant est invalide.
/// L'extrait est le début de l'article.
pub(crate) fn article_ref(article: &Article) -> Option<CodeArticleRef> {
    let id = ArticleId::parse(&article.id).ok()?;
    let start: String = plain(&article.content).chars().take(SNIPPET_SOURCE_CHARS).collect();
    Some(CodeArticleRef::new(id, clean_field(article.num.as_deref()), &start))
}

/// Une entrée de la liste des codes ; `None` sans identifiant `LEGITEXT` valide ni titre.
pub(crate) fn code_listing(text: &TextVersion) -> Option<CodeListing> {
    Some(CodeListing {
        id: LegiTextId::parse(&text.id).ok()?,
        title: title_of(text)?,
        etat: status(text.etat.as_ref()),
    })
}

/// Une version d'article, d'après un lien `LIEN_ART` ; `None` si le lien n'est pas un article
/// valide.
pub(crate) fn article_version(link: &StructLink) -> Option<ArticleVersion> {
    if link.target != LinkTarget::Article {
        return None;
    }
    Some(ArticleVersion {
        id: LegiArtiId::parse(&link.id).ok()?,
        num: clean_field(link.num.as_deref()),
        etat: status(link.etat.as_ref()),
        date_debut: open_ended(link.debut),
        date_fin: open_ended(link.fin),
    })
}

/// Nature d'un lien d'après l'identifiant de sa cible, puis la nature du texte lié.
fn related_nature(link: &Link) -> RelatedNature {
    let id = link.id.as_deref().unwrap_or_default();
    let nature = link.text_nature.as_deref().map(str::trim);
    if id.starts_with("JORFTEXT") {
        RelatedNature::Jorf
    } else if id.starts_with("JORFCONT") {
        RelatedNature::JorfCont
    } else if id.starts_with("KALI") {
        RelatedNature::Kali
    } else if id.starts_with("LEGI") && nature.is_some_and(|n| n.eq_ignore_ascii_case("CODE")) {
        RelatedNature::Code
    } else if id.starts_with("LEGI") {
        RelatedNature::Loda
    } else {
        RelatedNature::Other(clean_field(nature).unwrap_or_default())
    }
}

/// Un lien d'article en `RelatedLink`.
pub(crate) fn related_link(link: &Link) -> RelatedLink {
    RelatedLink {
        target: document_id(link.id.as_ref()),
        nature: related_nature(link),
        title: clean_field(Some(&link.title)),
        date: open_ended(link.text_signed),
    }
}

/// Table des matières du texte dont `links` est la structure, en vigueur à `at`.
///
/// Les sections sont lues une à une dans l'index. Un lien hors fenêtre de vigueur est ignoré
/// avec toute sa branche ; une section dont le document manque (le dump n'en garde qu'un
/// sous-ensemble) reste, avec le titre de son lien et sans enfant.
///
/// # Errors
///
/// [`CorpusError::TooLarge`] au-delà de `max_nodes` nœuds (production : [`TOC_MAX_NODES`]),
/// [`CorpusError::Decode`] si l'arbre dépasse la profondeur du domaine (un cycle de sections y
/// mène) ou si un texte en ligne est démesuré.
pub(crate) fn build_toc(
    view: &View<'_>,
    links: &[StructLink],
    at: CivilDate,
    max_nodes: usize,
) -> Result<Toc, CorpusError> {
    let mut budget = max_nodes;
    let (sections, articles) = draft_level(view, links, at, &mut budget, 1)?;
    // sans texte en ligne, le seul refus possible du domaine est la profondeur
    Toc::build(TocDraft { sections, articles }).map_err(|_| too_deep())
}

/// L'arbre de la table dépasse la profondeur que le domaine accepte.
pub(crate) fn too_deep() -> CorpusError {
    CorpusError::Decode { detail: "table of contents deeper than the domain bound".to_owned() }
}

/// Un niveau de la table : les articles et les sections en vigueur de `links`. `int_ordre` est le
/// rang dans le fichier, ce qui garde l'ordre mêlé des articles et des sections.
fn draft_level(
    view: &View<'_>,
    links: &[StructLink],
    at: CivilDate,
    budget: &mut usize,
    depth: u16,
) -> Result<(Vec<SectionDraft>, Vec<ArticleDraft>), CorpusError> {
    // deux niveaux de marge : le domaine refuse lui-même un arbre trop profond, ce garde-fou
    // n'arrête qu'un cycle de sections
    if depth > TOC_MAX_DEPTH + 2 {
        return Err(too_deep());
    }
    let mut sections = Vec::new();
    let mut articles = Vec::new();
    for (rank, link) in links.iter().enumerate() {
        if !in_force(link.debut, link.fin, at) {
            continue;
        }
        *budget = budget.checked_sub(1).ok_or_else(|| CorpusError::TooLarge {
            detail: "table of contents above the entry bound".to_owned(),
        })?;
        let int_ordre = i64::try_from(rank).ok();
        match link.target {
            LinkTarget::Article => articles.push(ArticleDraft {
                id: Some(link.id.clone()),
                num: link.num.clone(),
                int_ordre,
                etat: link.etat.clone(),
                content_html: None,
            }),
            LinkTarget::Section => {
                let section = view.by_id(DocKind::Section, &link.id)?;
                let section = section.as_ref().and_then(|doc| doc.document.as_section());
                let (children, leaves) = match section {
                    Some(section) => {
                        draft_level(view, &section.structure, at, budget, depth.saturating_add(1))?
                    }
                    None => (Vec::new(), Vec::new()),
                };
                sections.push(SectionDraft {
                    id: Some(link.id.clone()),
                    title: link
                        .title
                        .clone()
                        .or_else(|| section.and_then(|section| section.title.clone())),
                    int_ordre,
                    etat: link.etat.clone(),
                    sections: children,
                    articles: leaves,
                });
            }
        }
    }
    Ok((sections, articles))
}

#[cfg(test)]
mod tests {
    use lf_domain::entity::Snippet;

    use super::*;
    use crate::model::{Chronicle, SectionRef, TextContext};

    fn date(year: u16, month: u8, day: u8) -> Option<CivilDate> {
        CivilDate::new(year, month, day)
    }

    fn article() -> Article {
        Article {
            id: "LEGIARTI000006284410".into(),
            num: Some(" L. 36-11 ".into()),
            etat: Some("VIGUEUR".into()),
            kind: Some("AUTONOME".into()),
            date_debut: date(2020, 1, 1),
            date_fin: date(2999, 1, 1),
            context: TextContext {
                cid: Some("LEGITEXT000006069445".into()),
                nature: Some("CODE".into()),
                title: Some("Code des postes".into()),
                sections: vec![
                    SectionRef {
                        id: "LEGISCTA000006108694".into(),
                        title: "Livre Ier".into(),
                        date_debut: None,
                        date_fin: None,
                    },
                    SectionRef {
                        id: "LEGISCTA000006108695".into(),
                        title: "<br/>".into(),
                        date_debut: None,
                        date_fin: None,
                    },
                ],
            },
            versions: Vec::new(),
            nota: "<p>Nota</p>".into(),
            content: "<p>Texte de l&#39;article</p>".into(),
            links: Vec::new(),
        }
    }

    fn version() -> TextVersion {
        TextVersion {
            id: "LEGITEXT000006069445".into(),
            nature: Some("CODE".into()),
            chronicle: Chronicle {
                cid: Some("LEGITEXT000006069445".into()),
                num: None,
                nor: Some("ECFI1524250L".into()),
                date_publi: date(1980, 3, 25),
                date_texte: date(2999, 1, 1),
                last_modified: None,
            },
            title: Some("Code".into()),
            title_full: Some("  Code des postes ".into()),
            etat: Some("VIGUEUR".into()),
            date_debut: date(2222, 2, 22),
            date_fin: date(2999, 1, 1),
            authority: None,
            ministry: None,
            visas: String::new(),
            signatories: String::new(),
            tp: String::new(),
            nota: String::new(),
            abro: String::new(),
            rect: String::new(),
        }
    }

    fn link(id: &str, direction: &str, text_nature: Option<&str>) -> Link {
        Link {
            link_type: Some("CITATION".into()),
            direction: Some(direction.into()),
            id: Some(id.into()),
            text_cid: None,
            text_nature: text_nature.map(Into::into),
            text_number: None,
            text_nor: None,
            text_signed: date(2999, 1, 1),
            title: " Décret 80-217 ".into(),
        }
    }

    #[test]
    fn an_article_keeps_its_clean_fields_and_drops_sentinels() {
        let text = article_text(&article()).unwrap();
        assert_eq!(text.article_id.as_str(), "LEGIARTI000006284410");
        assert_eq!(text.num.as_deref(), Some("L. 36-11"));
        assert_eq!(text.etat, Some(LegalStatus::Vigueur));
        assert_eq!(text.code_title.as_deref(), Some("Code des postes"));
        assert_eq!(text.parent.as_ref().map(|p| p.id.as_str()), Some("LEGITEXT000006069445"));
        assert_eq!(text.path, ["Livre Ier"], "a blank section title is dropped");
        assert_eq!((text.date_debut, text.date_fin), (date(2020, 1, 1), None));
        assert_eq!(text.text.as_str(), "Texte de l'article");
        assert_eq!(text.nota.as_ref().map(LongText::as_str), Some("Nota"));
    }

    #[test]
    fn an_article_without_parent_nota_or_valid_id_is_handled() {
        let mut bare = article();
        bare.context = TextContext::default();
        bare.nota = "  ".into();
        bare.etat = None;
        let text = article_text(&bare).unwrap();
        assert!(text.parent.is_none() && text.nota.is_none() && text.etat.is_none());
        assert!(text.path.is_empty() && text.code_title.is_none());

        bare.id = "LEGIARTI1".into();
        assert_eq!(article_text(&bare).err(), Some(unreadable_entry()));
    }

    #[test]
    fn an_oversized_article_text_is_too_large() {
        let mut huge = article();
        huge.content = "a ".repeat(8_100_000);
        let too_large = || CorpusError::TooLarge {
            detail: "text above the 16 million character bound".to_owned(),
        };
        assert_eq!(article_text(&huge).err(), Some(too_large()));
        let mut huge_nota = article();
        huge_nota.nota = "a ".repeat(8_100_000);
        assert_eq!(article_text(&huge_nota).err(), Some(too_large()));
    }

    #[test]
    fn a_text_gives_its_body_hit_and_listing() {
        let text = version();
        let body = text_body(&text, Toc::default());
        assert_eq!(body.title.as_deref(), Some("Code des postes"));
        assert_eq!(body.nature.as_deref(), Some("CODE"));
        assert_eq!(body.nor.as_ref().map(Nor::as_str), Some("ECFI1524250L"));
        assert_eq!(body.cid.as_ref().map(DocumentId::as_str), Some("LEGITEXT000006069445"));
        assert_eq!(
            (body.date_parution, body.date_debut_version, body.date_fin_version),
            (date(1980, 3, 25), None, None)
        );

        let hit = text_hit(Fond::Code, &text).unwrap();
        assert_eq!(hit.id.as_str(), "LEGITEXT000006069445");
        assert_eq!((hit.title.as_deref(), hit.date), (Some("Code des postes"), None));
        assert_eq!(hit.etat, Some(LegalStatus::Vigueur));

        let listing = code_listing(&text).unwrap();
        assert_eq!(listing.title, "Code des postes");

        let mut short = version();
        short.title_full = None;
        assert_eq!(title_of(&short).as_deref(), Some("Code"));
        short.title = None;
        assert_eq!(title_of(&short), None);
        assert!(code_listing(&short).is_none(), "no title, no listing");
    }

    #[test]
    fn a_text_of_another_family_or_with_a_bad_id_gives_no_hit() {
        let mut text = version();
        assert!(text_hit(Fond::Juri, &text).is_none(), "LEGITEXT is not a JURITEXT");
        text.id = "LEGITEXT1".into();
        assert!(text_hit(Fond::Code, &text).is_none());
        assert!(code_listing(&text).is_none());
    }

    #[test]
    fn an_article_reference_carries_a_snippet() {
        let reference = article_ref(&article()).unwrap();
        assert_eq!(reference.article_id.as_str(), "LEGIARTI000006284410");
        assert_eq!(reference.num.as_deref(), Some("L. 36-11"));
        assert_eq!(reference.snippet.as_ref().map(Snippet::as_str), Some("Texte de l'article"));
        let mut broken = article();
        broken.id = "x".into();
        assert!(article_ref(&broken).is_none());
    }

    #[test]
    fn an_article_version_comes_from_an_article_link_only() {
        let mut link = StructLink {
            target: LinkTarget::Article,
            id: "LEGIARTI000006284410".into(),
            cid: None,
            num: Some("L1".into()),
            level: None,
            etat: Some("ABROGE".into()),
            debut: date(2222, 2, 22),
            fin: date(2999, 1, 1),
            title: None,
        };
        let version = article_version(&link).unwrap();
        assert_eq!(version.etat, Some(LegalStatus::Abroge));
        assert_eq!((version.date_debut, version.date_fin), (None, None));
        link.id = "bad".into();
        assert!(article_version(&link).is_none());
        link.id = "LEGISCTA000006108694".into();
        link.target = LinkTarget::Section;
        assert!(article_version(&link).is_none());
    }

    #[test]
    fn a_related_link_names_its_nature_from_the_target_id() {
        let cases = [
            ("JORFTEXT000000519770", None, RelatedNature::Jorf),
            ("JORFCONT000000000001", None, RelatedNature::JorfCont),
            ("KALITEXT000005652329", None, RelatedNature::Kali),
            ("LEGITEXT000006069445", Some(" code "), RelatedNature::Code),
            ("LEGIARTI000006284410", Some("LOI"), RelatedNature::Loda),
            ("XYZ", Some(" Circulaire "), RelatedNature::Other("Circulaire".into())),
            ("XYZ", None, RelatedNature::Other(String::new())),
        ];
        for (id, nature, expected) in cases {
            assert_eq!(related_link(&link(id, "source", nature)).nature, expected, "{id}");
        }
        let mut anonymous = link("JORFTEXT000000519770", "cible", None);
        anonymous.id = None;
        anonymous.title = "  ".into();
        let related = related_link(&anonymous);
        assert!(related.target.is_none() && related.title.is_none() && related.date.is_none());
        assert_eq!(
            related_link(&link("JORFTEXT000000519770", "source", None)).title.as_deref(),
            Some("Décret 80-217")
        );
    }

    #[test]
    fn the_window_rule_is_debut_inclusive_fin_exclusive() {
        let at = date(2026, 10, 15).unwrap();
        assert!(in_force(None, None, at));
        assert!(in_force(date(2026, 10, 15), None, at), "the start day is in force");
        assert!(!in_force(date(2026, 10, 16), None, at));
        assert!(!in_force(None, date(2026, 10, 15), at), "the end day is not");
        assert!(in_force(None, date(2026, 10, 16), at));
        assert!(!in_force(date(2999, 1, 1), date(2999, 1, 1), at), "ENTIEREMENT_MODIF");
    }

    #[test]
    fn unsupported_operations_are_settings_errors_the_model_can_read() {
        const DETAIL: &str = "get_decision is not available in the local DILA corpus, which \
                              holds the LEGI fund only (codes, laws and decrees); use the \
                              PISTE source for it";
        assert_eq!(
            unsupported("get_decision"),
            CorpusError::Config { kind: ConfigKind::Settings, detail: DETAIL.to_owned() }
        );
        assert!(DETAIL.chars().count() <= 500, "the model reads at most 500 characters");
        assert_eq!(
            not_found("/consult/getArticle"),
            CorpusError::NotFound { what: "/consult/getArticle".into() }
        );
    }
}
