//! Fiche ou page de navigation de Service-Public telle que le port `PublicGuidance` la rend
//! (05 §5.17 ; ADR-0033, décisions 6 à 8).
//!
//! Comme une publication du BOFiP, ces valeurs n'implémentent pas
//! [`Citable`](crate::contract::Citable) : le contrat de citation exige une adresse sous
//! legifrance.gouv.fr. La citation est une méthode à part ([`GuidanceDoc::citation`]).

use crate::{date::CivilDate, entity::OfficialSummary, text::LongText};

use super::{
    audience::Audience, blocks::SheetText, id::SheetId, references::LegalReferences,
    url::ServicePublicUrl,
};

/// Nature d'une page : fiche (`F…`), dossier ou thème (`N…`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GuidanceKind {
    /// Fiche pratique.
    Sheet,
    /// Dossier : il regroupe des fiches.
    Folder,
    /// Thème : il regroupe des dossiers.
    Theme,
}

impl GuidanceKind {
    /// Nom côté utilisateur : `sheet`, `folder` ou `theme`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sheet => "sheet",
            Self::Folder => "folder",
            Self::Theme => "theme",
        }
    }
}

/// Page listée par un dossier ou un thème.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationEntry {
    /// Numéro de la page listée.
    pub id: SheetId,
    /// Fiche ou dossier.
    pub kind: GuidanceKind,
    /// Titre, sur une ligne.
    pub title: String,
    /// Intitulé du groupe qui la porte dans la page (sous-dossier d'un dossier, sous-thème d'un
    /// thème) : ces groupes n'ont pas de page à eux.
    pub group: Option<String>,
}

/// Contenu d'une page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuidanceContent {
    /// Fiche : texte rendu et textes de Légifrance cités.
    Sheet {
        /// Texte rendu et titres des chapitres.
        text: SheetText,
        /// Textes de Légifrance cités.
        references: LegalReferences,
    },
    /// Dossier : ses fiches et sous-dossiers.
    Folder(Vec<NavigationEntry>),
    /// Thème : ses dossiers.
    Theme(Vec<NavigationEntry>),
}

/// Une page de Service-Public lue dans l'archive d'un public.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuidanceDoc {
    /// Numéro.
    pub id: SheetId,
    /// Public de l'archive lue.
    pub audience: Audience,
    /// Type du flux (attribut `type`), par exemple « Fiche d'information conditionnée ».
    pub sheet_type: Option<String>,
    /// Titre, sur une ligne.
    pub title: String,
    /// Description (`dc:description`), sur une ligne.
    pub description: Option<String>,
    /// Thème de rattachement.
    pub theme: Option<String>,
    /// Date de dernière modification importante (fiches).
    pub updated: Option<CivilDate>,
    /// Adresse publique.
    pub source_url: ServicePublicUrl,
    /// Contenu.
    pub content: GuidanceContent,
}

impl GuidanceDoc {
    /// Nature de la page, lue dans son contenu.
    pub fn kind(&self) -> GuidanceKind {
        match self.content {
            GuidanceContent::Sheet { .. } => GuidanceKind::Sheet,
            GuidanceContent::Folder(_) => GuidanceKind::Folder,
            GuidanceContent::Theme(_) => GuidanceKind::Theme,
        }
    }

    /// Texte d'une fiche ; `None` pour une page de navigation.
    pub fn text(&self) -> Option<&LongText> {
        match &self.content {
            GuidanceContent::Sheet { text, .. } => Some(&text.text),
            GuidanceContent::Folder(_) | GuidanceContent::Theme(_) => None,
        }
    }

    /// « {site}, {titre} ({numéro}), mise à jour du {date} » (05 §5.17) ; sans date, sans la fin.
    ///
    /// ```
    /// use lf_domain::{
    ///     date::CivilDate,
    ///     guidance::{Audience, GuidanceContent, GuidanceDoc, ServicePublicUrl, SheetId},
    /// };
    ///
    /// let id = SheetId::parse("F1437")?;
    /// let doc = GuidanceDoc {
    ///     source_url: ServicePublicUrl::read(None, Audience::Particuliers, &id),
    ///     id,
    ///     audience: Audience::Particuliers,
    ///     sheet_type: None,
    ///     title: "Solde bancaire insaisissable (SBI)".to_owned(),
    ///     description: None,
    ///     theme: None,
    ///     updated: CivilDate::new(2026, 4, 1),
    ///     content: GuidanceContent::Folder(Vec::new()),
    /// };
    /// assert_eq!(
    ///     doc.citation(),
    ///     "Service-Public.gouv.fr, Solde bancaire insaisissable (SBI) (F1437), mise à jour du 1er avril 2026"
    /// );
    /// # Ok::<(), lf_domain::DomainError>(())
    /// ```
    pub fn citation(&self) -> String {
        let head = format!("{}, {} ({})", self.audience.site(), self.title, self.id);
        match self.updated {
            Some(date) => format!("{head}, mise à jour du {}", date.to_french()),
            None => head,
        }
    }

    /// Résumé officiel d'une fiche (05 §5.17) : sa description, puis la liste de ses chapitres
    /// (« Chapitres : Bénéficiaires ; Montant »). Vide pour une page de navigation, qui n'a pas
    /// de texte à résumer.
    pub fn official_summary(&self) -> OfficialSummary {
        let GuidanceContent::Sheet { text, .. } = &self.content else {
            return OfficialSummary::default();
        };
        let chapters = (!text.chapters.is_empty())
            .then(|| format!("Chapitres : {}", text.chapters.join(" ; ")));
        OfficialSummary {
            analyses: self.description.iter().cloned().chain(chapters).collect(),
            titrages: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guidance::{Block, HeadingLevel, Inline, legal_references, render};
    use pretty_assertions::assert_eq;

    fn heading(title: &str) -> Block {
        Block::Heading { level: HeadingLevel::Chapter, text: vec![Inline::Text(title.to_owned())] }
    }

    fn doc(audience: Audience, content: GuidanceContent) -> GuidanceDoc {
        let id = SheetId::parse("F2408").unwrap();
        GuidanceDoc {
            source_url: ServicePublicUrl::read(None, audience, &id),
            id,
            audience,
            sheet_type: Some("Fiche d'information conditionnée".to_owned()),
            title: "Smic".to_owned(),
            description: Some("Le salaire minimum interprofessionnel de croissance.".to_owned()),
            theme: Some("Travail - Formation".to_owned()),
            updated: CivilDate::new(2026, 1, 15),
            content,
        }
    }

    fn sheet(blocks: &[Block]) -> GuidanceContent {
        GuidanceContent::Sheet { text: render(blocks).unwrap(), references: legal_references([]) }
    }

    #[test]
    fn citation_names_the_site_of_the_audience_and_the_date() {
        let part = doc(Audience::Particuliers, sheet(&[]));
        assert_eq!(
            part.citation(),
            "Service-Public.gouv.fr, Smic (F2408), mise à jour du 15 janvier 2026"
        );
        let mut pro = doc(Audience::Entreprendre, sheet(&[]));
        pro.updated = None;
        assert_eq!(pro.citation(), "Entreprendre.Service-Public.gouv.fr, Smic (F2408)");
    }

    #[test]
    fn summary_is_the_description_then_the_chapters() {
        let read = doc(Audience::Particuliers, sheet(&[heading("Montant"), heading("Calcul")]));
        assert_eq!(
            read.official_summary(),
            OfficialSummary {
                analyses: vec![
                    "Le salaire minimum interprofessionnel de croissance.".to_owned(),
                    "Chapitres : Montant ; Calcul".to_owned(),
                ],
                titrages: Vec::new(),
            }
        );
        let mut bare = doc(Audience::Particuliers, sheet(&[]));
        bare.description = None;
        assert_eq!(bare.official_summary(), OfficialSummary::default());
        let folder = doc(Audience::Particuliers, GuidanceContent::Folder(Vec::new()));
        assert_eq!(folder.official_summary(), OfficialSummary::default());
    }

    #[test]
    fn kind_and_text_follow_the_content() {
        let entry = NavigationEntry {
            id: SheetId::parse("F1437").unwrap(),
            kind: GuidanceKind::Sheet,
            title: "Solde bancaire insaisissable (SBI)".to_owned(),
            group: Some("Saisie d'argent".to_owned()),
        };
        let read = doc(Audience::Particuliers, sheet(&[heading("Montant")]));
        assert_eq!(
            (read.kind(), read.text().map(LongText::as_str)),
            (GuidanceKind::Sheet, Some("Montant"))
        );
        let folder = doc(Audience::Particuliers, GuidanceContent::Folder(vec![entry.clone()]));
        assert_eq!((folder.kind(), folder.text()), (GuidanceKind::Folder, None));
        let theme = doc(Audience::Particuliers, GuidanceContent::Theme(vec![entry]));
        assert_eq!((theme.kind(), theme.text()), (GuidanceKind::Theme, None));
    }

    #[test]
    fn kinds_have_their_user_names() {
        assert_eq!(
            [GuidanceKind::Sheet, GuidanceKind::Folder, GuidanceKind::Theme]
                .map(GuidanceKind::as_str),
            ["sheet", "folder", "theme"]
        );
    }
}
