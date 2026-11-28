//! Correspondance des réponses de Judilibre vers le domaine (ADR-0035, décisions 4 à 6).
//!
//! Les codes deviennent des intitulés en clair (juridiction et siège, chambre, formation,
//! publication, solution) ; les dates restent telles que publiées. Depuis V-241, l'API les rend
//! déjà en clair (`resolve_references=true`) : la juridiction, nettoyée comme un champ court, se
//! lit par son code ou par son intitulé ([`Court::parse_reply`]), et un intitulé reçu est gardé
//! (le siège, s'il commence par l'intitulé de sa juridiction : `court_label`). Les balises HTML
//! ne sont retirées que des intitulés (visas, rapprochements), des extraits et du sommaire : le
//! texte de la décision reste tel quel, car les zones y pointent en unités UTF-16 (le cas
//! d'usage le met en forme après la coupe). Tout autre champ est un champ court
//! (`short_field`) : caractères invisibles retirés, blancs repliés.
//!
//! Un résultat de recherche sans identifiant valide, sans juridiction connue ou sans date est
//! écarté et compté (`dropped_hits`) ; la même lacune dans une décision est une erreur de
//! décodage, comme un identifiant rendu différent de celui demandé.

use lf_application::ports::CorpusError;
use lf_domain::judilibre::{
    CaseLawDoc, CaseLawHit, CaseLawMention, CaseLawPage, ContestedDecision, Court,
    DECISION_LIST_MAX, DecisionPart, HIT_THEMES_MAX, JudilibreId, RelatedDecision, ZoneSpan, Zones,
    chamber_label, court_label, excerpt, formation_label, publication_label, short_field,
    solution_label, strip_markup,
};

use super::{Diagnostics, Field};
use crate::dto::judilibre::{
    ContestedDto, DecisionDto, SearchDto, SpanDto, TitleDto, Word, ZonesDto,
};
use crate::request::judilibre::{DECISION, SEARCH};

/// Ce qu'un résultat de recherche et une décision ont en commun.
struct Head {
    id: JudilibreId,
    jurisdiction: Court,
    mention: CaseLawMention,
    number: Option<String>,
    ecli: Option<String>,
    solution: Option<String>,
    publication: Option<String>,
    summary: Option<String>,
}

/// Une page de recherche. Les résultats illisibles sont écartés et comptés.
#[expect(clippy::unnecessary_wraps, reason = "signature imposée par PisteCorpus::fetch_with")]
pub(crate) fn page(dto: SearchDto, diags: &mut Diagnostics) -> Result<CaseLawPage, CorpusError> {
    let mut hits = Vec::with_capacity(dto.results.len());
    for result in dto.results {
        match hit(result) {
            Some(hit) => hits.push(hit),
            None => diags.note_dropped_hit(),
        }
    }
    Ok(CaseLawPage { total: dto.total, hits })
}

/// Un résultat de recherche : premier extrait surligné non vide, trois thèmes au plus.
fn hit(dto: DecisionDto) -> Option<CaseLawHit> {
    let head = head(&dto, SEARCH).ok()?;
    let excerpt = dto.highlights.text.iter().find_map(|word| word.0.as_deref().and_then(excerpt));
    Some(CaseLawHit {
        id: head.id,
        jurisdiction: head.jurisdiction,
        mention: head.mention,
        number: head.number,
        ecli: head.ecli,
        solution: head.solution,
        publication: head.publication,
        summary: head.summary,
        excerpt,
        themes: themes(dto.themes).take(HIT_THEMES_MAX).collect(),
    })
}

/// La décision `requested`, avec son texte tel que publié et ses zones.
///
/// # Errors
///
/// [`CorpusError::Decode`] si l'identifiant, la juridiction ou la date manque ou ne se lit pas,
/// ou si l'identifiant rendu n'est pas celui demandé (compté dans `id_mismatch`).
pub(crate) fn decision(
    requested: &JudilibreId,
    dto: DecisionDto,
    diags: &mut Diagnostics,
) -> Result<CaseLawDoc, CorpusError> {
    let head = head(&dto, DECISION)?;
    if head.id != *requested {
        diags.note_id_mismatch();
        return Err(Field { endpoint: DECISION, path: "id" }.decode("not the requested decision"));
    }
    Ok(CaseLawDoc {
        id: head.id,
        jurisdiction: head.jurisdiction,
        mention: head.mention,
        number: head.number,
        ecli: head.ecli,
        solution: head.solution,
        publication: head.publication,
        themes: themes(dto.themes).take(DECISION_LIST_MAX).collect(),
        summary: head.summary,
        text: dto.text.unwrap_or_default(),
        zones: zones(dto.zones),
        visa: dto
            .visa
            .into_iter()
            .filter_map(title)
            .map(|title| strip_markup(&title))
            .filter(|visa| !visa.is_empty())
            .take(DECISION_LIST_MAX)
            .collect(),
        related: dto
            .rapprochements
            .into_iter()
            .filter_map(title)
            .map(|title| RelatedDecision::from_html(&title))
            .filter(|related| !related.title.is_empty())
            .take(DECISION_LIST_MAX)
            .collect(),
        contested: dto.contested.as_ref().and_then(contested),
        updated: trimmed(dto.update_date.as_deref()),
        partial: dto.partial,
        to_be_deleted: dto.to_be_deleted,
    })
}

/// Identifiant, juridiction, mentions et champs courts d'une décision.
fn head(dto: &DecisionDto, endpoint: &'static str) -> Result<Head, CorpusError> {
    let id = dto
        .id
        .as_deref()
        .and_then(|id| JudilibreId::parse(id).ok())
        .ok_or_else(|| Field { endpoint, path: "id" }.decode("invalid identifier"))?;
    let jurisdiction = trimmed(dto.jurisdiction.as_deref())
        .as_deref()
        .and_then(Court::parse_reply)
        .ok_or_else(|| Field { endpoint, path: "jurisdiction" }.decode("unknown jurisdiction"))?;
    let date = trimmed(dto.decision_date.as_deref())
        .ok_or_else(|| Field { endpoint, path: "decision_date" }.decode("missing date"))?;
    let mention = CaseLawMention {
        court: court_label(jurisdiction, trimmed(dto.location.as_deref()).as_deref()),
        chamber: trimmed(dto.chamber.as_deref())
            .map(|chamber| chamber_label(jurisdiction, &chamber)),
        formation: trimmed(dto.formation.as_deref()).map(|formation| formation_label(&formation)),
        date,
    };
    Ok(Head {
        id,
        jurisdiction,
        mention,
        number: trimmed(dto.number.as_deref()),
        ecli: trimmed(dto.ecli.as_deref()),
        solution: solution(dto.solution.as_deref(), dto.solution_alt.as_deref()),
        publication: publication(&dto.publication),
        summary: dto.summary.as_deref().map(strip_markup).filter(|summary| !summary.is_empty()),
    })
}

/// Champ court nettoyé ([`short_field`]), `None` s'il ne reste rien.
fn trimmed(text: Option<&str>) -> Option<String> {
    text.and_then(short_field)
}

/// Solution en clair : l'intitulé du code, sauf `other` (« Autre », son intitulé en clair), à qui
/// la solution en clair (`solution_alt`) est préférée quand elle existe.
fn solution(code: Option<&str>, alt: Option<&str>) -> Option<String> {
    let code = trimmed(code);
    let alt = trimmed(alt);
    match (code, alt) {
        (Some(code), Some(alt)) if code == "other" || code == solution_label("other") => Some(alt),
        (Some(code), _) => Some(solution_label(&code)),
        (None, alt) => alt,
    }
}

/// Niveaux de publication en clair, séparés par des virgules ; `None` sans aucun.
fn publication(codes: &[Word]) -> Option<String> {
    let labels: Vec<String> = codes
        .iter()
        .filter_map(|word| trimmed(word.0.as_deref()))
        .map(|code| publication_label(&code))
        .collect();
    (!labels.is_empty()).then(|| labels.join(", "))
}

/// Thèmes non vides, dans l'ordre reçu.
fn themes(words: Vec<Word>) -> impl Iterator<Item = String> {
    words.into_iter().filter_map(|word| trimmed(word.0.as_deref()))
}

/// Intitulé d'un visa ou d'un rapprochement.
fn title(dto: TitleDto) -> Option<String> {
    dto.title
}

/// Décision attaquée ; `None` sans juridiction.
fn contested(dto: &ContestedDto) -> Option<ContestedDecision> {
    Some(ContestedDecision {
        court: trimmed(dto.jurisdiction.as_deref())?,
        chamber: trimmed(dto.chamber.as_deref()),
        date: trimmed(dto.date.as_deref()),
        solution: trimmed(dto.solution.as_deref()),
    })
}

/// Zones de la décision ; un fragment dont une borne manque ou dépasse un `u32` est écarté.
fn zones(dto: ZonesDto) -> Zones {
    let ZonesDto { introduction, expose, moyens, motivations, dispositif, annexes } = dto;
    let parts = [
        (DecisionPart::Introduction, introduction),
        (DecisionPart::Expose, expose),
        (DecisionPart::Moyens, moyens),
        (DecisionPart::Motivations, motivations),
        (DecisionPart::Dispositif, dispositif),
        (DecisionPart::Annexes, annexes),
    ];
    Zones::of(
        parts.into_iter().flat_map(|(part, spans)| {
            spans.into_iter().filter_map(span).map(move |span| (part, span))
        }),
    )
}

/// Un fragment de zone, ses deux bornes lues.
fn span(dto: SpanDto) -> Option<ZoneSpan> {
    let start = u32::try_from(dto.start?).ok()?;
    let end = u32::try_from(dto.end?).ok()?;
    Some(ZoneSpan { start, end })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use lf_domain::judilibre::part_text;

    use super::*;
    use crate::dto::decode::decode;

    fn fixture(name: &str) -> String {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/judilibre").join(name);
        std::fs::read_to_string(path).unwrap()
    }

    fn page_of(name: &str) -> (CaseLawPage, Diagnostics) {
        let mut diags = Diagnostics::default();
        let page = page(decode(&fixture(name), SEARCH).unwrap(), &mut diags).unwrap();
        (page, diags)
    }

    fn id(raw: &str) -> JudilibreId {
        JudilibreId::parse(raw).unwrap()
    }

    fn doc_of(name: &str, requested: &str) -> Result<CaseLawDoc, CorpusError> {
        let mut diags = Diagnostics::default();
        decision(&id(requested), decode(&fixture(name), DECISION).unwrap(), &mut diags)
    }

    #[test]
    fn a_cassation_hit_carries_its_mentions_in_plain_words() {
        let (page, diags) = page_of("search-cc.json");
        assert_eq!((page.total, page.hits.len(), diags.dropped_hits), (240, 3, 0));
        let hit = &page.hits[0];
        assert_eq!(hit.id, id("5fca624b7eeb384a40cafb85"));
        assert_eq!(hit.jurisdiction, Court::Cassation);
        assert_eq!(
            hit.mention,
            CaseLawMention {
                court: "Cour de cassation".to_owned(),
                chamber: Some("Première chambre civile".to_owned()),
                formation: Some("Formation de section".to_owned()),
                date: "2019-11-20".to_owned(),
            }
        );
        assert_eq!(hit.number.as_deref(), Some("16-15.867"));
        assert_eq!(hit.ecli.as_deref(), Some("ECLI:FR:CCASS:2019:C100963"));
        assert_eq!(hit.solution.as_deref(), Some("Cassation"));
        assert_eq!(hit.publication.as_deref(), Some("Publié au Bulletin"));
        assert!(hit.summary.as_deref().unwrap().starts_with("Un époux commun en bien"));
        assert_eq!(hit.excerpt.as_deref(), Some("contrats d'assurance sur la vie que V"));
        assert_eq!(hit.themes, ["regimes matrimoniaux", "communauté entre époux", "actif"]);
    }

    #[test]
    fn an_appeal_hit_has_its_seat_and_its_plain_solution() {
        let (page, _) = page_of("search-ca.json");
        let hit = &page.hits[0];
        assert_eq!(hit.mention.court, "Cour d'appel de Paris");
        assert_eq!(hit.mention.chamber.as_deref(), Some("Pôle 3 - Chambre 1"));
        assert_eq!(hit.mention.formation, None);
        assert_eq!(
            hit.solution.as_deref(),
            Some(
                "Confirme la décision déférée dans toutes ses dispositions, à l'égard de toutes \
                 les parties au recours"
            )
        );
        assert_eq!((hit.publication.as_deref(), hit.ecli.as_deref()), (None, None));
        assert_eq!(hit.themes, ["Demande en partage, ou contestations relatives au partage"]);
        let (empty, _) = page_of("search-empty-page.json");
        assert_eq!((empty.total, empty.hits.len()), (336_114, 0));
    }

    #[test]
    fn unreadable_hits_are_dropped_and_counted() {
        let raw = r#"{"total":9,"results":[
            {"id":"nope","jurisdiction":"cc","decision_date":"2020-01-01"},
            {"id":"5fca624b7eeb384a40cafb85","jurisdiction":"ta","decision_date":"2020-01-01"},
            {"id":"5fca624b7eeb384a40cafb85","jurisdiction":"cc","decision_date":"  "},
            {"id":"5FCA624B7EEB384A40CAFB85","jurisdiction":" CC ","decision_date":" 2020-01-01 ",
             "highlights":{"text":["<em> </em>","<em>bail</em> commercial"]},
             "publication":["b"," ","r"],"themes":[" "]}]}"#;
        let mut diags = Diagnostics::default();
        let page = page(decode(raw, SEARCH).unwrap(), &mut diags).unwrap();
        assert_eq!((page.total, page.hits.len(), diags.dropped_hits), (9, 1, 3));
        let hit = &page.hits[0];
        assert_eq!(hit.id, id("5fca624b7eeb384a40cafb85"));
        assert_eq!(hit.mention.date, "2020-01-01");
        assert_eq!(hit.excerpt.as_deref(), Some("bail commercial"));
        assert_eq!(hit.publication.as_deref(), Some("Publié au Bulletin, Publié au Rapport"));
        assert_eq!(hit.themes, Vec::<String>::new());
        assert_eq!((hit.solution.as_deref(), hit.summary.as_deref()), (None, None));
    }

    #[test]
    fn solutions_prefer_plain_words_to_other() {
        assert_eq!(solution(Some("rejet"), None).as_deref(), Some("Rejet"));
        assert_eq!(solution(Some(" other "), Some(" Confirme ")).as_deref(), Some("Confirme"));
        assert_eq!(solution(Some("other"), None).as_deref(), Some("Autre"));
        assert_eq!(solution(Some("rejet"), Some("Confirme")).as_deref(), Some("Rejet"));
        assert_eq!(solution(None, Some("Irrecevabilité")).as_deref(), Some("Irrecevabilité"));
        assert_eq!(solution(Some(" "), None), None);
        // L'intitulé en clair de `other`, que rend `resolve_references`.
        assert_eq!(solution(Some("Autre"), Some("Confirme")).as_deref(), Some("Confirme"));
        assert_eq!(solution(Some("Autre"), None).as_deref(), Some("Autre"));
        assert_eq!(solution(Some("Cassation"), Some("Confirme")).as_deref(), Some("Cassation"));
    }

    #[test]
    fn a_decision_read_in_plain_words_has_the_mentions_of_its_coded_reading() {
        for (coded, resolved, requested) in [
            ("decision-cc.json", "decision-cc-resolu.json", "5fca624b7eeb384a40cafb85"),
            ("decision-ca.json", "decision-ca-resolu.json", "63c8eefddc5b777c90992fd8"),
        ] {
            let coded = doc_of(coded, requested).unwrap();
            let resolved = doc_of(resolved, requested).unwrap();
            assert_eq!(resolved.jurisdiction, coded.jurisdiction, "{requested}");
            assert_eq!(resolved.mention, coded.mention, "{requested}");
            assert_eq!(resolved.solution, coded.solution, "{requested}");
            assert_eq!(resolved.publication, coded.publication, "{requested}");
        }
        let cassation = doc_of("decision-cc-resolu.json", "5fca624b7eeb384a40cafb85").unwrap();
        assert_eq!(cassation.mention.chamber.as_deref(), Some("Première chambre civile"));
        assert_eq!(cassation.publication.as_deref(), Some("Publié au Bulletin"));
        let appeal = doc_of("decision-ca-resolu.json", "63c8eefddc5b777c90992fd8").unwrap();
        assert_eq!(appeal.jurisdiction, Court::Appeal);
        assert_eq!(appeal.mention.court, "Cour d'appel de Paris");
    }

    #[test]
    fn a_search_in_plain_words_names_the_seat_of_every_court() {
        let (page, diags) = page_of("search-resolu.json");
        assert_eq!((page.hits.len(), diags.dropped_hits), (3, 0));
        let seats: Vec<(Court, &str)> =
            page.hits.iter().map(|hit| (hit.jurisdiction, hit.mention.court.as_str())).collect();
        assert_eq!(
            seats,
            [
                (Court::Commercial, "Tribunal de commerce de Toulouse"),
                (Court::Commercial, "Tribunal des activités économiques de Paris"),
                (Court::Judicial, "Tribunal judiciaire de Nîmes"),
            ]
        );
        assert_eq!(page.hits[1].mention.chamber.as_deref(), Some("Référé mardi salle 3"));
        assert_eq!(
            page.hits[2].solution.as_deref(),
            Some("Désigne un expert ou un autre technicien")
        );
    }

    #[test]
    fn a_cassation_decision_keeps_its_text_and_zones() {
        let doc = doc_of("decision-cc.json", "5fca624b7eeb384a40cafb85").unwrap();
        assert_eq!(doc.mention.date, "2019-11-20");
        assert_eq!(doc.updated.as_deref(), Some("2024-02-28"));
        assert_eq!(doc.themes.len(), 11);
        assert_eq!(
            doc.visa,
            [
                "Article L. 132-9 du code des assurances, dans sa rédaction antérieure à celle issue \
                 de la loi n° 2007-1775 du 17 décembre 2007,.",
                "Article L. 132-21 du même code.",
                "Article 894 du code civil.",
            ]
        );
        assert_eq!(doc.related.len(), 2);
        assert_eq!(doc.related[0].id, Some(id("60794bf99ba5988459c445e1")));
        assert!(
            doc.related[0].title.starts_with("1re Civ., 22 octobre 1980, pourvoi n° 79-14.138")
        );
        assert_eq!(
            doc.contested,
            Some(ContestedDecision {
                court: "Cour d'appel de Paris".to_owned(),
                chamber: Some("E1".to_owned()),
                date: Some("2016-01-27".to_owned()),
                solution: Some(
                    "Infirme partiellement, réforme ou modifie certaines dispositions de la \
                     décision déférée"
                        .to_owned()
                ),
            })
        );
        assert!(doc.text.starts_with("CIV. 1\n\nCOUR DE CASSATION"));
        assert_eq!(
            part_text(&doc.text, &doc.zones, DecisionPart::Motivations).as_deref(),
            Some(
                "Réponse de la Cour\n\nVu l'article L. 132-9 du code des assurances :\n\n3. La \
                 donation suppose un dépouillement irrévocable.\n\n4. En se déterminant ainsi, la \
                 cour d'appel a privé sa décision de base légale."
            )
        );
        assert!(
            part_text(&doc.text, &doc.zones, DecisionPart::Dispositif)
                .unwrap()
                .starts_with("PAR CES MOTIFS, la Cour :")
        );
        assert!(!doc.partial && !doc.to_be_deleted);
    }

    #[test]
    fn an_old_appeal_decision_has_no_zones() {
        let doc = doc_of("decision-ca-sans-zones.json", "6253cd8cbd3db21cbdd93a2f").unwrap();
        assert!(doc.zones.is_empty());
        assert_eq!(
            (doc.mention.date.as_str(), doc.mention.chamber.as_deref()),
            ("0201-02-24", None)
        );
        assert_eq!(doc.mention.court, "Cour d'appel de Paris");
        assert_eq!((doc.contested, doc.visa.len(), doc.related.len()), (None, 0, 0));
        let zoned = doc_of("decision-ca.json", "63c8eefddc5b777c90992fd8").unwrap();
        assert_eq!(
            part_text(&zoned.text, &zoned.zones, DecisionPart::Dispositif).as_deref(),
            Some(
                "PAR CES MOTIFS, LA COUR,\n\nConfirme le jugement en toutes ses dispositions ;\n\n\
                 Condamne l'appelante aux dépens d'appel."
            )
        );
    }

    #[test]
    fn another_decision_than_the_requested_one_is_refused() {
        let mut diags = Diagnostics::default();
        let dto = decode(&fixture("decision-ca.json"), DECISION).unwrap();
        let error = decision(&id("5fca624b7eeb384a40cafb85"), dto, &mut diags).unwrap_err();
        assert_eq!(
            error,
            CorpusError::Decode { detail: format!("{DECISION}: id: not the requested decision") }
        );
        assert_eq!(diags.id_mismatch, 1);
        let mut diags = Diagnostics::default();
        let dto = decode(r#"{"id":"5fca624b7eeb384a40cafb85"}"#, DECISION).unwrap();
        let error = decision(&id("5fca624b7eeb384a40cafb85"), dto, &mut diags).unwrap_err();
        assert_eq!(
            error,
            CorpusError::Decode {
                detail: format!("{DECISION}: jurisdiction: unknown jurisdiction")
            }
        );
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn odd_fields_of_a_decision_are_dropped_one_by_one() {
        let raw = r#"{"id":"5fca624b7eeb384a40cafb85","jurisdiction":"tj","location":"tj75056",
            "decision_date":"2024-05-02","text":"abc","partial":true,"to_be_deleted":true,
            "zones":{"motivations":[{"start":0,"end":2},{"start":0,"end":4294967296},{"end":1},
                                     {"start":4294967296,"end":4294967297},{"start":1}],
                     "dispositif":[{"start":2,"end":4}]},
            "visa":[{"title":" "},{"title":7},{},{"title":"<b></b>"}],"rapprochements":[{"title":"<a href=\"x\"></a>"}],
            "contested":{"chamber":"1"},"update_date":" "}"#;
        let doc = doc_of_raw(raw);
        assert_eq!(doc.mention.court, "Tribunal judiciaire (tj75056)");
        assert_eq!(
            part_text(&doc.text, &doc.zones, DecisionPart::Motivations).as_deref(),
            Some("ab")
        );
        assert_eq!(
            part_text(&doc.text, &doc.zones, DecisionPart::Dispositif).as_deref(),
            Some("c")
        );
        assert_eq!(doc.visa, ["7"]);
        assert_eq!(doc.related, Vec::<RelatedDecision>::new());
        assert_eq!((doc.contested, doc.updated), (None, None));
        assert!(doc.partial && doc.to_be_deleted);
    }

    #[test]
    fn a_decision_keeps_at_most_the_bound_of_each_list() {
        let many = |item: &str| vec![item; DECISION_LIST_MAX.saturating_add(20)].join(",");
        let raw = format!(
            r#"{{"id":"5fca624b7eeb384a40cafb85","jurisdiction":"cc","decision_date":"2024-05-02",
            "themes":[{}],"visa":[{}],"rapprochements":[{}]}}"#,
            many(r#""Bail""#),
            many(r#"{"title":"Article 1"}"#),
            many(r#"{"title":"Civ. 1"}"#)
        );
        let doc = doc_of_raw(&raw);
        assert_eq!(
            (doc.themes.len(), doc.visa.len(), doc.related.len()),
            (DECISION_LIST_MAX, DECISION_LIST_MAX, DECISION_LIST_MAX)
        );
    }

    #[test]
    fn invisible_characters_are_removed_from_every_short_field() {
        // Un tag Unicode, un contrôle bidirectionnel et une espace de largeur nulle dans chaque
        // champ court ; un saut de ligne au milieu ; une juridiction en clair, avec une espace
        // insécable et une apostrophe typographique.
        let raw = r#"{"id":"5fca624b7eeb384a40cafb85","jurisdiction":"Cour\u00a0d\u2019appel\u200b",
            "location":"ca_paris\u200b",
            "decision_date":"2024-05-02\u202e","number":"21/\udb40\udc4101234","ecli":"ECLI:FR\u2066",
            "chamber":"P\u00f4le 3\n- Chambre 1\u200b","formation":"f\u202e","solution":"other",
            "solution_alt":"Infirme\u200b","themes":["Bail\u202e","\u200b"],"text":"abc",
            "contested":{"jurisdiction":"TJ\u200b Paris","chamber":"1\u202e","date":"2023\u2066",
            "solution":"Rejet\u200b"},"update_date":"2024-06-01\u200b"}"#;
        let doc = doc_of_raw(raw);
        assert_eq!(doc.mention.court, "Cour d'appel de Paris");
        assert_eq!(doc.mention.date, "2024-05-02");
        assert_eq!(doc.mention.chamber.as_deref(), Some("Pôle 3 - Chambre 1"));
        // Code nettoyé, puis lu dans la table.
        assert_eq!(doc.mention.formation.as_deref(), Some("Formation restreinte"));
        assert_eq!(doc.number.as_deref(), Some("21/01234"));
        assert_eq!(doc.ecli.as_deref(), Some("ECLI:FR"));
        assert_eq!(doc.solution.as_deref(), Some("Infirme"));
        assert_eq!(doc.themes, ["Bail"]);
        assert_eq!(
            doc.contested,
            Some(ContestedDecision {
                court: "TJ Paris".to_owned(),
                chamber: Some("1".to_owned()),
                date: Some("2023".to_owned()),
                solution: Some("Rejet".to_owned()),
            })
        );
        assert_eq!(doc.updated.as_deref(), Some("2024-06-01"));
    }

    fn doc_of_raw(raw: &str) -> CaseLawDoc {
        let mut diags = Diagnostics::default();
        decision(&id("5fca624b7eeb384a40cafb85"), decode(raw, DECISION).unwrap(), &mut diags)
            .unwrap()
    }
}
