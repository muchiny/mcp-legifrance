//! Article seul (`fr_get_text`, `citations.py:248-276`).

use crate::{
    contract::{Citable, CitationContract, article_citation_for},
    date::CivilDate,
    id::{ArticleId, DocumentId},
    status::LegalStatus,
    text::LongText,
    url::{ResourceKind, legifrance_url},
};

/// Famille du texte parent d'un article, qui décide de la règle de citation (03 §5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParentNature {
    /// Code : « Code civil, art. 9 ».
    Code,
    /// Texte non codifié (loi, ordonnance, décret, arrêté) : l'appel de note final du titre
    /// est retiré de la citation (Q12).
    Text,
    /// Convention collective (`KALITEXT`).
    Kali,
    /// Nature absente ou non reconnue : traitée comme un code (parité).
    Unknown,
}

/// Texte parent d'un article : identifiant (`cid`) et nature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentText {
    /// Identifiant du texte parent.
    pub id: DocumentId,
    /// Famille du texte parent.
    pub nature: ParentNature,
}

impl ParentText {
    /// Texte parent d'identifiant `id` et de nature brute `nature_raw` (`textTitles[].nature`).
    ///
    /// Un identifiant `KALITEXT` donne [`ParentNature::Kali`]. Sinon le libellé, sans égard à la
    /// casse, `_` et `-` valant espace : `CODE` donne [`ParentNature::Code`] ; `LOI`,
    /// `LOI ORGANIQUE`, `LOI CONSTITUTIONNELLE`, `ORDONNANCE`, `DECRET`, `DECRET LOI` et
    /// `ARRETE` donnent [`ParentNature::Text`] ; le reste, ou l'absence de libellé, donne
    /// [`ParentNature::Unknown`]. Les valeurs réelles de `textTitles[].nature` pour un article
    /// de loi sont **NON VÉRIFIÉES** (V-142).
    pub fn new(id: DocumentId, nature_raw: Option<&str>) -> Self {
        let nature = parent_nature(&id, nature_raw);
        Self { id, nature }
    }
}

/// Famille du parent : le préfixe `KALITEXT` l'emporte ; sinon le libellé brut, replié (casse
/// ASCII, `_` et `-` vaut espace, blancs en une seule espace), est cherché dans la table :
/// `CODE` donne [`ParentNature::Code`] ; `LOI`, `LOI ORGANIQUE`, `LOI CONSTITUTIONNELLE`,
/// `ORDONNANCE`, `DECRET`, `DECRET LOI` et `ARRETE` donnent [`ParentNature::Text`] ; tout le
/// reste, y compris l'absence de libellé, donne [`ParentNature::Unknown`]. Table **NON
/// VÉRIFIÉE** contre l'API réelle (V-142).
fn parent_nature(id: &DocumentId, raw: Option<&str>) -> ParentNature {
    if matches!(id, DocumentId::KaliText(_)) {
        return ParentNature::Kali;
    }
    match raw.map(fold_label).as_deref() {
        Some("CODE") => ParentNature::Code,
        Some(
            "LOI"
            | "LOI ORGANIQUE"
            | "LOI CONSTITUTIONNELLE"
            | "ORDONNANCE"
            | "DECRET"
            | "DECRET LOI"
            | "ARRETE",
        ) => ParentNature::Text,
        Some(_) | None => ParentNature::Unknown,
    }
}

/// Libellé de nature replié : majuscules ASCII, `_` et `-` en espaces, blancs repliés.
fn fold_label(raw: &str) -> String {
    let spaced: String = raw
        .to_ascii_uppercase()
        .chars()
        .map(|c| if c == '_' || c == '-' { ' ' } else { c })
        .collect();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Texte d'un article de code, de loi ou de convention collective.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArticleText {
    /// Identifiant de l'article.
    pub article_id: ArticleId,
    /// Numéro affiché.
    pub num: Option<String>,
    /// État juridique.
    pub etat: Option<LegalStatus>,
    /// Titre du texte parent (`citations.py:236-245`, `_code_title` : entrée `textTitles`
    /// de nature `CODE` en priorité, sinon la première).
    pub code_title: Option<String>,
    /// Texte parent (`cid` et nature de la même entrée de `textTitles`) ; décide de la règle
    /// de citation.
    pub parent: Option<ParentText>,
    /// Chemin des sections, du plus haut au plus bas (`context.titresTM[].titre`).
    pub path: Vec<String>,
    /// Chemin des sections tel que rendu par l'API, conservé pour la parité.
    pub full_sections_titre: Option<String>,
    /// Début de vigueur ; `None` : début non déterminé.
    pub date_debut: Option<CivilDate>,
    /// Fin de vigueur ; `None` : sans date de fin.
    pub date_fin: Option<CivilDate>,
    /// Texte de l'article.
    pub text: LongText,
    /// Nota de l'article ; `None` : absent ou vide.
    pub nota: Option<LongText>,
}

impl Citable for ArticleText {
    /// Citation selon la nature du parent (03 §5.4) ; l'URL ne dépend que du type
    /// d'identifiant : `code_article` pour un article `LEGIARTI`, `conv_coll` pour un article
    /// `KALIARTI`. `loda_article` n'est pas employé tant que V-140 et V-142 ne sont pas levées.
    fn contract(&self) -> CitationContract {
        let kind = match self.article_id {
            ArticleId::Legi(_) => ResourceKind::CodeArticle,
            ArticleId::Kali(_) => ResourceKind::Kali,
        };
        let nature = self.parent.as_ref().map_or(ParentNature::Unknown, |parent| parent.nature);
        let id = self.article_id.as_str();
        CitationContract::from_url(
            legifrance_url(kind, id),
            article_citation_for(nature, self.code_title.as_deref(), self.num.as_deref())
                .as_deref(),
            id,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn article(id: &str) -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(id).unwrap(),
            num: Some("9".into()),
            etat: None,
            code_title: Some("Code civil".into()),
            parent: None,
            path: vec![],
            full_sections_titre: None,
            date_debut: None,
            date_fin: None,
            text: LongText::default(),
            nota: None,
        }
    }

    fn parent(id: &str, nature: Option<&str>) -> ParentText {
        ParentText::new(DocumentId::parse(id).unwrap(), nature)
    }

    #[test]
    fn legi_article_cites_code_and_number() {
        let citation = article("LEGIARTI000006419288").contract();
        assert_eq!(
            (citation.citation.as_str(), citation.source_url.as_str()),
            (
                "Code civil, art. 9",
                "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288"
            )
        );
    }

    #[test]
    fn kali_article_uses_conv_coll_url() {
        // test_kali_article_url_uses_conv_coll de la référence.
        let citation = article("KALIARTI000005805295").contract();
        assert_eq!(
            citation.source_url.as_str(),
            "https://www.legifrance.gouv.fr/conv_coll/id/KALIARTI000005805295"
        );
    }

    #[test]
    fn parent_nature_per_prefix_and_label() {
        let legi = "LEGITEXT000006070721";
        let table = [
            (legi, Some("CODE"), ParentNature::Code),
            (legi, Some("code"), ParentNature::Code),
            (legi, Some("  Code "), ParentNature::Code),
            (legi, Some("Loi"), ParentNature::Text),
            (legi, Some("LOI ORGANIQUE"), ParentNature::Text),
            (legi, Some("LOI_CONSTITUTIONNELLE"), ParentNature::Text),
            (legi, Some("DECRET-LOI"), ParentNature::Text),
            (legi, Some("decret  loi"), ParentNature::Text),
            (legi, Some("DECRET"), ParentNature::Text),
            (legi, Some("ORDONNANCE"), ParentNature::Text),
            (legi, Some("ARRETE"), ParentNature::Text),
            (legi, Some("???"), ParentNature::Unknown),
            (legi, Some(""), ParentNature::Unknown),
            (legi, Some("LOIS"), ParentNature::Unknown),
            (legi, None, ParentNature::Unknown),
            // Le préfixe KALITEXT l'emporte sur le libellé, présent ou non.
            ("KALITEXT000005635534", None, ParentNature::Kali),
            ("KALITEXT000005635534", Some("CODE"), ParentNature::Kali),
            // Un autre préfixe ne décide rien : seul le libellé compte.
            ("JORFTEXT000032921889", Some("LOI"), ParentNature::Text),
        ];
        for (id, raw, expected) in table {
            let parent = parent(id, raw);
            assert_eq!(parent.nature, expected, "{id} {raw:?}");
            assert_eq!(parent.id.as_str(), id);
        }
    }

    #[test]
    fn fold_label_normalizes_case_separators_and_blanks() {
        assert_eq!(fold_label("  Décret_loi--x 	 y "), "DéCRET LOI X Y");
        assert_eq!(fold_label(""), "");
    }

    #[test]
    fn law_article_cites_title_without_note_call() {
        let mut law = article("LEGIARTI000033219575");
        law.num = Some("1".into());
        law.code_title =
            Some("LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique (1)".into());
        law.parent = Some(parent("JORFTEXT000033202746", Some("LOI")));
        let citation = law.contract();
        assert_eq!(
            (citation.citation.as_str(), citation.source_url.as_str()),
            (
                "LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique, art. 1",
                "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000033219575"
            )
        );
        // Même titre, parent de nature code : l'appel de note reste (parité).
        law.parent = Some(parent("LEGITEXT000006070721", Some("CODE")));
        assert_eq!(
            law.contract().citation.as_str(),
            "LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique (1), art. 1"
        );
    }

    #[test]
    fn unknown_parent_nature_falls_back_to_codes_url() {
        let mut unknown = article("LEGIARTI000006419288");
        unknown.code_title = Some("Texte (1)".into());
        for parent in [None, Some(parent("LEGITEXT000006070721", Some("???")))] {
            unknown.parent = parent;
            let citation = unknown.contract();
            assert_eq!(
                (citation.citation.as_str(), citation.source_url.as_str()),
                (
                    "Texte (1), art. 9",
                    "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288"
                )
            );
        }
    }

    #[test]
    fn article_url_depends_on_id_type_not_parent() {
        for nature in [Some("CODE"), Some("LOI"), None] {
            let mut legi = article("LEGIARTI000006419288");
            legi.parent = Some(parent("LEGITEXT000006070721", nature));
            assert_eq!(
                legi.contract().source_url.as_str(),
                "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288"
            );
            let mut kali = article("KALIARTI000005805295");
            kali.parent = Some(parent("KALITEXT000005635534", nature));
            assert_eq!(
                kali.contract().source_url.as_str(),
                "https://www.legifrance.gouv.fr/conv_coll/id/KALIARTI000005805295"
            );
        }
    }

    #[test]
    fn article_text_carries_path_nota_and_parent() {
        let mut full = article("LEGIARTI000006419288");
        full.etat = LegalStatus::parse("VIGUEUR");
        full.parent = Some(parent("LEGITEXT000006070721", Some("CODE")));
        full.path =
            vec!["Livre Ier : Des personnes".into(), "Titre Ier : Des droits civils".into()];
        full.nota = Some(LongText::default());
        assert_eq!(
            (&full.parent, full.path.len(), full.nota.is_some(), full.etat.is_some()),
            (
                &Some(ParentText {
                    id: DocumentId::parse("LEGITEXT000006070721").unwrap(),
                    nature: ParentNature::Code
                }),
                2,
                true,
                true
            )
        );
        assert_eq!(full.path[1], "Titre Ier : Des droits civils");
    }
}
