//! Décisions de Judilibre, telles que les outils les rendent (ADR-0035, décisions 4 à 6 et 11).
//!
//! Les champs gardent ce que la source a publié : aucune date n'est corrigée (une décision de
//! 2017 est publiée au `0201-02-24`), aucun texte n'est réécrit. Seules les balises HTML des
//! textes appliqués, des rapprochements et des extraits sont retirées.

use crate::{date::CivilDate, text::LongText};

use super::{Court, JudilibreId, Zones};

/// Longueur maximale d'un extrait de recherche, en caractères, `…` compris.
pub const EXCERPT_MAX_CHARS: usize = 300;

/// Nombre maximal de thèmes rendus pour une décision trouvée.
pub const HIT_THEMES_MAX: usize = 3;

/// Première année d'une date de décision qu'on écrit en toutes lettres dans une citation. Une
/// date plus ancienne est une erreur de la source (`0201-02-24`) : elle est citée telle que
/// publiée. Le stock de la Cour de cassation commence en 1947.
pub const CITATION_YEAR_MIN: u16 = 1800;

/// Mentions qu'exigent les CGU pour chaque décision : la juridiction et son siège, la chambre
/// ou la formation, la date de prononcé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseLawMention {
    /// Juridiction et siège, en clair : « Cour d'appel de Paris ».
    pub court: String,
    /// Chambre, en clair : « Première chambre civile », « Pôle 3 - Chambre 1 ».
    pub chamber: Option<String>,
    /// Formation de la Cour de cassation, en clair : « Formation de section ».
    pub formation: Option<String>,
    /// Date de la décision, telle que publiée (`AAAA-MM-JJ` en principe).
    pub date: String,
}

impl CaseLawMention {
    /// Citation de la décision : juridiction, chambre (ou formation), date en toutes lettres si
    /// elle est plausible, et numéro : « Cour d'appel de Paris, Pôle 3 - Chambre 1, 18 janvier
    /// 2023, n° 20/17369 ».
    #[must_use]
    pub fn citation(&self, number: Option<&str>) -> String {
        let mut parts = vec![self.court.clone()];
        if let Some(chamber) = self.chamber.as_ref().or(self.formation.as_ref()) {
            parts.push(chamber.clone());
        }
        parts.push(citation_date(&self.date));
        if let Some(number) = number {
            parts.push(format!("n° {number}"));
        }
        parts.join(", ")
    }
}

/// Date de citation : en toutes lettres pour une date valide d'année au moins
/// [`CITATION_YEAR_MIN`], sinon telle que publiée.
fn citation_date(raw: &str) -> String {
    CivilDate::parse(raw)
        .ok()
        .filter(|date| date.year() >= CITATION_YEAR_MIN)
        .map_or_else(|| raw.trim().to_owned(), CivilDate::to_french)
}

/// Une décision trouvée par `fr_search_judilibre` : jamais son texte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseLawHit {
    /// Identifiant, à lire avec `fr_get_judilibre`.
    pub id: JudilibreId,
    /// Juridiction.
    pub jurisdiction: Court,
    /// Mentions des CGU.
    pub mention: CaseLawMention,
    /// Numéro de pourvoi ou de rôle.
    pub number: Option<String>,
    /// ECLI (Cour de cassation seulement).
    pub ecli: Option<String>,
    /// Solution, en clair.
    pub solution: Option<String>,
    /// Niveau de publication, en clair.
    pub publication: Option<String>,
    /// Sommaire de la Cour de cassation.
    pub summary: Option<String>,
    /// Premier extrait surligné, sans balises, d'au plus [`EXCERPT_MAX_CHARS`] caractères.
    pub excerpt: Option<String>,
    /// Thèmes, au plus [`HIT_THEMES_MAX`].
    pub themes: Vec<String>,
}

/// Une page de résultats de `fr_search_judilibre`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseLawPage {
    /// Nombre total de décisions trouvées, toutes pages confondues.
    pub total: u64,
    /// Décisions de la page, dans l'ordre de l'API.
    pub hits: Vec<CaseLawHit>,
}

/// Une décision liée (rapprochement) : son intitulé, et son identifiant s'il est connu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedDecision {
    /// Intitulé, sans balises : « 1re Civ., 22 octobre 1980, pourvoi n° 79-14.193 ».
    pub title: String,
    /// Identifiant Judilibre, lu dans le lien de l'intitulé.
    pub id: Option<JudilibreId>,
    /// Identifiant Légifrance (`JURITEXT`), lu dans le lien de l'intitulé.
    pub legifrance_id: Option<String>,
}

impl RelatedDecision {
    /// Rapprochement d'un intitulé HTML : balises retirées, identifiant tiré du premier lien.
    #[must_use]
    pub fn from_html(title: &str) -> Self {
        let target = first_href(title);
        Self {
            title: strip_markup(title),
            id: target.and_then(|target| JudilibreId::parse(target).ok()),
            legifrance_id: target.and_then(juritext),
        }
    }
}

/// La décision attaquée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContestedDecision {
    /// Juridiction et siège, tels que publiés : « Cour d'appel de Paris ».
    pub court: String,
    /// Chambre, telle que publiée.
    pub chamber: Option<String>,
    /// Date, telle que publiée.
    pub date: Option<String>,
    /// Solution, telle que publiée.
    pub solution: Option<String>,
}

/// Une décision lue par `fr_get_judilibre`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseLawDoc {
    /// Identifiant.
    pub id: JudilibreId,
    /// Juridiction.
    pub jurisdiction: Court,
    /// Mentions des CGU.
    pub mention: CaseLawMention,
    /// Numéro de pourvoi ou de rôle.
    pub number: Option<String>,
    /// ECLI (Cour de cassation seulement).
    pub ecli: Option<String>,
    /// Solution, en clair.
    pub solution: Option<String>,
    /// Niveau de publication, en clair.
    pub publication: Option<String>,
    /// Thèmes, tous.
    pub themes: Vec<String>,
    /// Sommaire de la Cour de cassation.
    pub summary: Option<String>,
    /// Texte, tel que publié (indices des zones en unités UTF-16).
    pub text: String,
    /// Zones du texte ; vides pour une décision ancienne.
    pub zones: Zones,
    /// Textes appliqués, sans balises.
    pub visa: Vec<String>,
    /// Rapprochements.
    pub related: Vec<RelatedDecision>,
    /// Décision attaquée.
    pub contested: Option<ContestedDecision>,
    /// Date de dernière mise à jour, telle que publiée (`update_date`).
    pub updated: Option<String>,
    /// La décision n'est publiée qu'en partie.
    pub partial: bool,
    /// La décision est retirée de la base : elle se rend comme introuvable.
    pub to_be_deleted: bool,
}

/// Texte d'un fragment HTML court (intitulé, extrait), sans balises ni entités, sur une ligne.
#[must_use]
pub fn strip_markup(html: &str) -> String {
    LongText::from_html(html)
        .map(|text| text.as_str().split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

/// Extrait d'un passage surligné : balises retirées, coupé à [`EXCERPT_MAX_CHARS`] caractères,
/// `…` compris. `None` s'il ne reste rien.
#[must_use]
pub fn excerpt(highlight: &str) -> Option<String> {
    let text = strip_markup(highlight);
    if text.chars().count() <= EXCERPT_MAX_CHARS {
        return (!text.is_empty()).then_some(text);
    }
    let kept: String = text.chars().take(EXCERPT_MAX_CHARS.saturating_sub(1)).collect();
    Some(format!("{}…", kept.trim_end()))
}

/// Cible du premier lien (`href="…"` ou `href='…'`) d'un fragment HTML.
fn first_href(html: &str) -> Option<&str> {
    let start = html.find("href=")?.saturating_add("href=".len());
    let rest = html.get(start..)?;
    let quote = rest.chars().next().filter(|c| matches!(c, '"' | '\''))?;
    let rest = rest.get(quote.len_utf8()..)?;
    rest.find(quote).and_then(|end| rest.get(..end))
}

/// Identifiant `JURITEXT` + 12 chiffres contenu dans `target`, s'il y en a un.
fn juritext(target: &str) -> Option<String> {
    let start = target.find("JURITEXT")?;
    let candidate = target.get(start..start.saturating_add(20))?;
    candidate
        .bytes()
        .skip("JURITEXT".len())
        .all(|byte| byte.is_ascii_digit())
        .then(|| candidate.to_owned())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn mention(court: &str, chamber: Option<&str>, formation: Option<&str>, date: &str) -> CaseLawMention {
        CaseLawMention {
            court: court.to_owned(),
            chamber: chamber.map(str::to_owned),
            formation: formation.map(str::to_owned),
            date: date.to_owned(),
        }
    }

    #[test]
    fn citations_name_court_chamber_date_and_number() {
        let ca = mention("Cour d'appel de Paris", Some("Pôle 3 - Chambre 1"), None, "2023-01-18");
        assert_eq!(
            ca.citation(Some("20/17369")),
            "Cour d'appel de Paris, Pôle 3 - Chambre 1, 18 janvier 2023, n° 20/17369"
        );
        let cc = mention(
            "Cour de cassation",
            Some("Première chambre civile"),
            Some("Formation de section"),
            "2019-11-20",
        );
        assert_eq!(
            cc.citation(Some("16-15.867")),
            "Cour de cassation, Première chambre civile, 20 novembre 2019, n° 16-15.867"
        );
        let formation = mention("Cour de cassation", None, Some("Formation restreinte"), "2020-05-01");
        assert_eq!(formation.citation(None), "Cour de cassation, Formation restreinte, 1er mai 2020");
        let bare = mention("Tribunal judiciaire (tj75056)", None, None, "2024-02-29");
        assert_eq!(bare.citation(None), "Tribunal judiciaire (tj75056), 29 février 2024");
    }

    #[test]
    fn an_implausible_date_is_cited_as_published() {
        let old = mention("Cour d'appel de Paris", None, None, "0201-02-24");
        assert_eq!(old.citation(Some("15/15181")), "Cour d'appel de Paris, 0201-02-24, n° 15/15181");
        assert_eq!(citation_date("1799-12-31"), "1799-12-31");
        assert_eq!(citation_date("1800-01-01"), "1er janvier 1800");
        assert_eq!(citation_date(" 2019-02-30 "), "2019-02-30");
        assert_eq!(citation_date("inconnue"), "inconnue");
    }

    #[test]
    fn tags_are_stripped_text_kept() {
        assert_eq!(
            strip_markup(
                "Article L. <a href=\"https://www.legifrance.gouv.fr/x\">132-13</a> du code des \
                 assurances."
            ),
            "Article L. 132-13 du code des assurances."
        );
        assert_eq!(strip_markup("sur la <em>vie</em>\n et l&#39;usufruit"), "sur la vie et l'usufruit");
        assert_eq!(strip_markup("R<P"), "R<P");
        assert_eq!(strip_markup("  "), "");
    }

    #[test]
    fn excerpts_are_bounded() {
        assert_eq!(excerpt("<em>donation</em> indirecte").as_deref(), Some("donation indirecte"));
        assert_eq!(excerpt("<em></em>"), None);
        let long = "mot ".repeat(200);
        let cut = excerpt(&long).unwrap();
        assert_eq!(cut.chars().count(), EXCERPT_MAX_CHARS);
        assert!(cut.ends_with("mot…") || cut.ends_with("mo…") || cut.ends_with('…'));
        let exact = "é".repeat(EXCERPT_MAX_CHARS);
        assert_eq!(excerpt(&exact).as_deref(), Some(exact.as_str()));
        let over = "é".repeat(EXCERPT_MAX_CHARS + 1);
        assert_eq!(excerpt(&over).unwrap().chars().count(), EXCERPT_MAX_CHARS);
    }

    #[test]
    fn related_decisions_keep_their_judilibre_or_legifrance_id() {
        let judilibre = RelatedDecision::from_html(
            "1re Civ., 22 octobre 1980, pourvoi n° <a href=\"60794bf99ba5988459c445e1\">79-14.193</a>",
        );
        assert_eq!(judilibre.title, "1re Civ., 22 octobre 1980, pourvoi n° 79-14.193");
        assert_eq!(judilibre.id.as_ref().map(JudilibreId::as_str), Some("60794bf99ba5988459c445e1"));
        assert_eq!(judilibre.legifrance_id, None);
        let legifrance = RelatedDecision::from_html(
            "<a href='https://www.legifrance.gouv.fr/juri/id/JURITEXT000007006420'>1re Civ.</a>",
        );
        assert_eq!(legifrance.id, None);
        assert_eq!(legifrance.legifrance_id.as_deref(), Some("JURITEXT000007006420"));
        let plain = RelatedDecision::from_html("Com., 3 mai 2006");
        assert_eq!((plain.id, plain.legifrance_id), (None, None));
        for odd in ["<a href=x>y</a>", "<a href=\"unterminated>y</a>", "<a href=\"JURITEXT0000\">"] {
            let related = RelatedDecision::from_html(odd);
            assert_eq!((related.id, related.legifrance_id), (None, None), "{odd}");
        }
        assert_eq!(juritext("JURITEXT00000700642X"), None);
    }
}
