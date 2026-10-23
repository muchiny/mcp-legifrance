//! Convention collective consolidée de `/consult/kaliText` vers `Convention` (04 §9.1 a, b, c,
//! e, g ; §9.2).
//!
//! Même règles d'identité que [`act`](super::act) : un objet sans identité est un `NotFound`,
//! l'identifiant renvoyé est gardé même s'il diffère de celui demandé (compté), absent ou blanc
//! c'est l'identifiant demandé (compté aussi). Le suffixe de version d'un `KALITEXT` est retiré
//! et compté (`upstream_suffix_stripped`) ; il ne donne pas de début de version. Le reste est le
//! [`TextBody`] commun, complété par le seul premier conteneur : IDCC, titre et identifiant du
//! conteneur, puis le bulletin officiel (`originePubli`, à défaut `numeroBo`). Un IDCC illisible
//! est omis sans diagnostic.
//!
//! [`TextBody`]: lf_domain::entity::TextBody

use std::mem;

use lf_application::ports::CorpusError;
use lf_domain::contract::Idcc;
use lf_domain::entity::Convention;
use lf_domain::id::KaliTextId;
use lf_domain::text::clean_field;

use super::body::{non_blank, text_body};
use super::endpoint::KALI_TEXT;
use super::rules::{require, resolve_id};
use super::{Diagnostics, Field};
use crate::dto::consult_text::ConsultTextDto;

/// Champ `id` de la réponse.
const ID: Field = Field { endpoint: KALI_TEXT, path: "id" };

/// Convention collective de `dto`, demandée sous `requested` ; ses anomalies sont comptées dans
/// `diags`.
///
/// # Errors
///
/// [`CorpusError::NotFound`] (`/consult/kaliText`) si l'objet n'a ni `id`, ni `cid`, ni titre
/// non blanc ; `diags` n'est alors pas touché. [`CorpusError::Decode`]
/// (`/consult/kaliText: id: invalid identifier`) si l'identifiant renvoyé n'est pas un
/// `KALITEXT` lisible, puis toute erreur de `text_body`.
pub(crate) fn convention(
    dto: ConsultTextDto,
    requested: &KaliTextId,
    diags: &mut Diagnostics,
) -> Result<Convention, CorpusError> {
    let mut dto = require(Some(dto), KALI_TEXT)?;
    let text_id = resolve_id(requested, dto.id.as_deref(), ID, diags)?;
    let containers = mem::take(&mut dto.conteneurs);
    let numero_bo = dto.numero_bo.take();
    let origine_publi = dto.origine_publi.take();
    let body = text_body(dto, KALI_TEXT, None, diags)?;
    let first = containers.into_iter().next().unwrap_or_default();
    Ok(Convention {
        text_id,
        body,
        idcc: first.num.as_deref().and_then(Idcc::parse),
        container_title: clean_field(first.titre.as_deref()),
        container_id: container_id_of(first.cid.as_deref()),
        bulletin: bulletin_of(origine_publi.as_deref(), numero_bo.as_deref()),
    })
}

/// Bulletin officiel : `originePubli` nettoyé, à défaut `numeroBo` nettoyé.
fn bulletin_of(origine: Option<&str>, numero_bo: Option<&str>) -> Option<String> {
    clean_field(origine).or_else(|| clean_field(numero_bo))
}

/// Identifiant du conteneur, rogné ; `None` s'il est absent ou blanc.
fn container_id_of(cid: Option<&str>) -> Option<String> {
    non_blank(cid).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use lf_domain::contract::{Citable, Nor};
    use lf_domain::date::CivilDate;
    use lf_domain::entity::{TocArticle, TocNode};
    use lf_domain::id::DocumentId;
    use lf_domain::status::LegalStatus;

    use super::*;
    use crate::dto::consult_text::ConteneurDto;
    use crate::dto::de::RawDate;
    use crate::map::fixtures::decode_as;

    /// `Some(valeur.to_owned())` : une macro et non une fonction, que `clippy::unnecessary_wraps`
    /// refuserait.
    macro_rules! text {
        ($value:expr) => {
            Some($value.to_owned())
        };
    }

    /// Identifiant demandé dans la plupart des tests.
    const REQUESTED: &str = "KALITEXT000049878950";

    /// Identifiant `KALITEXT` valide.
    fn kali(raw: &str) -> KaliTextId {
        KaliTextId::parse(raw).unwrap()
    }

    /// Texte d'identifiant `id`, sans autre champ.
    fn with_id(id: &str) -> ConsultTextDto {
        ConsultTextDto { id: text!(id), ..ConsultTextDto::default() }
    }

    /// Texte dont les conteneurs sont `containers`, sous l'identifiant demandé.
    fn with_containers(containers: Vec<ConteneurDto>) -> ConsultTextDto {
        ConsultTextDto { conteneurs: containers, ..with_id(REQUESTED) }
    }

    /// `convention` de `dto` demandée en `requested`, et les diagnostics qu'elle laisse.
    fn convention_of(
        dto: ConsultTextDto,
        requested: &str,
    ) -> (Result<Convention, CorpusError>, Diagnostics) {
        let mut diags = Diagnostics::default();
        let convention = convention(dto, &kali(requested), &mut diags);
        (convention, diags)
    }

    /// Premier article de la table des matières, en profondeur d'abord.
    fn first_article(nodes: &[TocNode]) -> Option<&TocArticle> {
        nodes.iter().find_map(|node| match node {
            TocNode::Article(article) => Some(article),
            TocNode::Section(section) => first_article(&section.children),
        })
    }

    #[test]
    fn convention_fixture_maps() {
        let title = "Annexe IX - Stipulations spécifiques applicables aux techniciens du \
                     spectacle et aux artistes interprètes, engagés pour la production de \
                     phonogrammes et vidéogrammes musicaux";
        let (convention, diags) =
            convention_of(decode_as::<ConsultTextDto>("consult_kali"), REQUESTED);
        let convention = convention.unwrap();
        assert_eq!(convention.text_id, kali(REQUESTED));

        let body = &convention.body;
        assert_eq!(body.cid.as_ref().map(DocumentId::as_str), Some("KALITEXT000049878950"));
        assert_eq!(body.nor.as_ref().map(Nor::as_str), Some("ASET2450455M"));
        assert_eq!(body.title.as_deref(), Some(title));
        assert_eq!(body.nature, None);
        assert_eq!(body.etat, Some(LegalStatus::VigueurEtendue));
        assert_eq!(body.date_parution, None);
        assert_eq!(body.num_parution, None);
        assert_eq!(body.date_debut_version, None);
        assert_eq!(body.date_fin_version, None);
        assert_eq!(body.jo_reference, None);
        assert_eq!(body.aliases, Vec::<String>::new());
        assert_eq!((body.toc.len(), body.toc.dropped()), (2, 0));
        assert!(first_article(body.toc.roots()).unwrap().inline_text.is_some());

        assert_eq!(convention.idcc.map(Idcc::get), Some(2121));
        assert_eq!(
            convention.container_title.as_deref(),
            Some("Convention collective nationale de l'édition du 14 janvier 2000")
        );
        assert_eq!(convention.container_id.as_deref(), Some("KALICONT000005635096"));
        assert_eq!(convention.bulletin.as_deref(), Some("BO n°2024-20"));
        assert_eq!(diags, Diagnostics::default());

        let contract = convention.contract();
        assert_eq!(
            contract.source_url.as_str(),
            "https://www.legifrance.gouv.fr/conv_coll/id/KALITEXT000049878950"
        );
        assert_eq!(contract.citation.as_str(), title);
    }

    #[test]
    fn convention_reads_only_the_first_container() {
        let first = ConteneurDto { cid: text!("KALICONT000000000001"), ..ConteneurDto::default() };
        let second = ConteneurDto {
            cid: text!("KALICONT000000000002"),
            num: text!("2121"),
            titre: text!("Autre"),
        };
        let (convention, _) = convention_of(with_containers(vec![first, second]), REQUESTED);
        let convention = convention.unwrap();
        assert_eq!(convention.idcc, None);
        assert_eq!(convention.container_title, None);
        assert_eq!(convention.container_id.as_deref(), Some("KALICONT000000000001"));

        let (convention, _) = convention_of(with_containers(Vec::new()), REQUESTED);
        let convention = convention.unwrap();
        assert_eq!(
            (convention.idcc, convention.container_title, convention.container_id),
            (None, None, None)
        );
    }

    #[test]
    fn convention_invalid_idcc_is_none_without_diagnostic() {
        for (raw, expected) in
            [("ABC", None), ("0", None), ("10000", None), ("0000000000002121", Some(2121))]
        {
            let container = ConteneurDto { num: text!(raw), ..ConteneurDto::default() };
            let (convention, diags) = convention_of(with_containers(vec![container]), REQUESTED);
            assert_eq!(convention.unwrap().idcc.map(Idcc::get), expected, "{raw}");
            assert_eq!(diags, Diagnostics::default(), "{raw}");
        }
    }

    #[test]
    fn container_id_is_trimmed_and_never_blank() {
        assert_eq!(
            container_id_of(Some(" KALICONT000005635096 ")).as_deref(),
            Some("KALICONT000005635096")
        );
        assert_eq!(container_id_of(Some("  ")), None);
        assert_eq!(container_id_of(None), None);
    }

    #[test]
    fn bulletin_falls_back_to_numero_bo() {
        let cases = [
            (Some("BO n°2024-20"), Some("2024-20"), Some("BO n°2024-20")),
            (None, Some("2024-20"), Some("2024-20")),
            (Some("<br>"), Some("2024-20"), Some("2024-20")),
            (Some(" "), None, None),
            (None, None, None),
        ];
        for (origine, numero_bo, expected) in cases {
            assert_eq!(bulletin_of(origine, numero_bo).as_deref(), expected, "{origine:?}");
        }
    }

    #[test]
    fn convention_id_rules() {
        let (convention, diags) = convention_of(
            ConsultTextDto { title: text!("Convention"), ..ConsultTextDto::default() },
            REQUESTED,
        );
        assert_eq!(convention.unwrap().text_id, kali(REQUESTED));
        assert_eq!(diags, Diagnostics { id_mismatch: 1, ..Diagnostics::default() });

        let (convention, diags) = convention_of(with_id("KALITEXT000000000001"), REQUESTED);
        assert_eq!(convention.unwrap().text_id, kali("KALITEXT000000000001"));
        assert_eq!(diags, Diagnostics { id_mismatch: 1, ..Diagnostics::default() });

        let (convention, diags) =
            convention_of(with_id("KALITEXT000049878950_08-09-2023"), REQUESTED);
        assert_eq!(convention.unwrap().text_id, kali(REQUESTED));
        assert_eq!(diags, Diagnostics { upstream_suffix_stripped: 1, ..Diagnostics::default() });

        for raw in ["LEGITEXT000033205014", "KALITEXT000049878950_99-99-9999"] {
            let (convention, diags) = convention_of(with_id(raw), REQUESTED);
            let error = convention.unwrap_err();
            assert_eq!(
                error,
                CorpusError::Decode { detail: "/consult/kaliText: id: invalid identifier".into() },
                "{raw}"
            );
            assert!(!format!("{error:?} {error}").contains(raw), "{raw}");
            assert_eq!(diags, Diagnostics::default(), "{raw}");
        }

        let (convention, diags) = convention_of(ConsultTextDto::default(), REQUESTED);
        assert_eq!(convention, Err(CorpusError::NotFound { what: "/consult/kaliText".into() }));
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn convention_body_error_is_propagated() {
        let dto = ConsultTextDto { cid: text!("pas-un-cid"), ..with_id(REQUESTED) };
        assert_eq!(
            convention_of(dto, REQUESTED).0,
            Err(CorpusError::Decode {
                detail: "/consult/kaliText: cid: invalid identifier".into()
            })
        );
    }

    #[test]
    fn convention_status_and_dates_follow_the_shared_rules() {
        let dto = ConsultTextDto {
            etat: text!("ABROGE"),
            juris_state: text!("VIGUEUR_ETEN"),
            date_debut_version: Some(RawDate::Text("2024-01-01".to_owned())),
            ..with_id(REQUESTED)
        };
        let body = convention_of(dto, REQUESTED).0.unwrap().body;
        assert_eq!(body.etat, Some(LegalStatus::Abroge));
        assert_eq!(body.date_debut_version, CivilDate::new(2024, 1, 1));
    }
}
