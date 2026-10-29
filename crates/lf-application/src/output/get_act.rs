//! Sortie de `fr_get_act` (v0.1) : `GetActOutput` et sa métrique d'audit.
//!
//! [`TocLine`] est aussi la ligne de table des matières de `fr_get_convention` : les deux outils
//! rendent la même table (05 §5.0.4).

use lf_domain::{date::VersionDate, entity::Act, id::ArticleId, status::LegalStatus};

use crate::audited::AuditMetric;

/// Une entrée de la table des matières rendue, en préordre : une section titrée ou un article.
/// Une section sans titre n'est pas une entrée (le domaine l'écarte à l'aplatissement) ; ses
/// enfants gardent leur profondeur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TocLine {
    /// Section titrée.
    Section {
        /// Profondeur, 1 au premier niveau.
        depth: u16,
        /// Titre nettoyé, non vide.
        title: String,
    },
    /// Article, lisible par `fr_get_text`.
    Article {
        /// Profondeur, 1 au premier niveau.
        depth: u16,
        /// Identifiant de l'article.
        article_id: ArticleId,
        /// Numéro affiché.
        num: Option<String>,
        /// État de l'article ; la vue l'omet quand il égale celui du texte.
        etat: Option<LegalStatus>,
    },
}

/// Ce que `fr_get_act` rend à l'adapter : les métadonnées du texte, la page de table des matières
/// et la note éventuelle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetActOutput {
    /// Le texte lu. Sa table des matières est vidée : la page rendue est dans `toc`.
    pub act: Act,
    /// Version servie : `date`, sinon le suffixe de `text_id`, sinon aujourd'hui à Paris.
    pub date: VersionDate,
    /// Entrées de la page, après raccourcissement sous la cible du budget.
    pub toc: Vec<TocLine>,
    /// Nombre d'entrées de la table entière (`toc_total`).
    pub toc_total: u32,
    /// Début de la page suivante (`next_toc_offset`), absent en fin de table.
    pub next_toc_offset: Option<u32>,
    /// Remarque sur la réponse : le texte est un code entier (05 §5.2).
    pub note: Option<&'static str>,
}

impl AuditMetric for GetActOutput {
    /// Entrées de table des matières renvoyées (05 §5.0.1).
    fn output_count_or_size(&self) -> u64 {
        u64::try_from(self.toc.len()).unwrap_or(u64::MAX)
    }
}
