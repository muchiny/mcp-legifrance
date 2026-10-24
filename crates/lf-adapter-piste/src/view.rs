//! Projection JSON des entités du domaine, pour les tests de contrat (04 §10.3, §13.3 ; 06 §2.3).
//!
//! [`view`] met une entité produite par le mapping ([`Mapped`]) sous la forme d'une valeur JSON
//! qui porte tous ses champs, et eux seuls : c'est la projection unique que lisent les tests,
//! dont `no_mojibake_after_mapping` (D-35). Chaque entité est déstructurée sans `..` : un champ
//! ajouté au domaine casse la compilation de ce fichier, jusqu'à ce qu'il soit projeté. Les clés
//! sont celles du domaine, en `snake_case` ; les identifiants, les dates (`AAAA-MM-JJ`) et le
//! numéro de convention collective sont rendus par leur `Display`, un `Option` absent par `null`,
//! une énumération par son code. Un texte long ([`LongText`]) devient
//! `{"text", "paragraphs", "char_len", "invisible_removed"}`, où `paragraphs` liste, pour chaque
//! paragraphe, `[position du premier caractère, nombre de caractères]` (le texte n'est pas
//! répété). La table des matières est un arbre `{"section": {…}}` / `{"article": {…}}` pris sur
//! `roots()`. Le contrat de citation est `{"citation", "source_url", "ecli"}`.
//!
//! [`leaf_strings`] liste les chaînes d'une valeur JSON avec leur chemin, et [`mojibake_paths`]
//! celles qui portent encore un motif de mojibake. Ce module n'existe que sous `cfg(test)`.

use std::fmt::Display;

use lf_domain::contract::{Citable, CitationContract};
use lf_domain::entity::{
    Act, ArticleText, CodeArticleRef, CompanyAgreement, Convention, Decision, Deliberation,
    OfficialSummary, ParentText, SearchHit, SearchPage, TextBody, Toc, TocArticle, TocNode,
    TocSection,
};
use lf_domain::status::LegalStatus;
use lf_domain::text::LongText;
use lf_domain::text::encoding::has_latin1_mojibake;
use serde_json::{Map, Value, json};

use crate::map::fixtures::Mapped;

/// Valeur rendue par son `Display` (identifiant, date, numéro de convention, ECLI…).
fn shown(value: &impl Display) -> Value {
    Value::String(value.to_string())
}

/// `Option` rendu par le `Display` de sa valeur, `null` si absente.
fn opt_shown(value: Option<&impl Display>) -> Value {
    value.map_or(Value::Null, shown)
}

/// Statut juridique rendu par son code d'API, `null` si absent.
fn status(value: Option<&LegalStatus>) -> Value {
    value.map_or(Value::Null, |etat| json!(etat.as_api_code()))
}

/// Texte long : texte canonique, position et longueur de chaque paragraphe, longueur totale et
/// nombre d'invisibles retirés.
fn long_text(text: &LongText) -> Value {
    let paragraphs: Vec<Value> = text
        .paragraphs()
        .map(|(start, paragraph)| json!([start, paragraph.chars().count()]))
        .collect();
    json!({
        "text": text.as_str(),
        "paragraphs": paragraphs,
        "char_len": text.char_len(),
        "invisible_removed": text.invisible_removed(),
    })
}

/// Texte long facultatif, `null` si absent.
fn opt_long_text(text: Option<&LongText>) -> Value {
    text.map_or(Value::Null, long_text)
}

/// Contrat de citation : citation, URL de la source et ECLI natif.
fn contract(contract: &CitationContract) -> Value {
    // L'ELI est toujours absent (décision C6) : `every_normalized_fixture_has_contract` le vérifie.
    let CitationContract { citation, source_url, ecli, eli: _ } = contract;
    json!({
        "citation": citation.as_str(),
        "source_url": source_url.as_str(),
        "ecli": opt_shown(ecli.as_ref()),
    })
}

/// Objet JSON formé de couples clé, valeur.
fn fields(pairs: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Object(
        pairs.into_iter().map(|(key, value)| (key.to_owned(), value)).collect::<Map<_, _>>(),
    )
}

/// Champs communs d'une loi, d'un décret ou d'une convention collective.
fn body_pairs(body: &TextBody) -> Vec<(&'static str, Value)> {
    let TextBody {
        cid,
        nor,
        title,
        nature,
        etat,
        date_parution,
        num_parution,
        date_debut_version,
        date_fin_version,
        jo_reference,
        aliases,
        toc,
    } = body;
    vec![
        ("cid", opt_shown(cid.as_ref())),
        ("nor", opt_shown(nor.as_ref())),
        ("title", json!(title)),
        ("nature", json!(nature)),
        ("etat", status(etat.as_ref())),
        ("date_parution", opt_shown(date_parution.as_ref())),
        ("num_parution", json!(num_parution)),
        ("date_debut_version", opt_shown(date_debut_version.as_ref())),
        ("date_fin_version", opt_shown(date_fin_version.as_ref())),
        ("jo_reference", json!(jo_reference)),
        ("aliases", json!(aliases)),
        ("toc", toc_tree(toc)),
        ("toc_dropped", json!(toc.dropped())),
    ]
}

/// Table des matières : l'arbre des racines.
fn toc_tree(toc: &Toc) -> Value {
    Value::Array(toc.roots().iter().map(toc_node).collect())
}

/// Nœud de la table des matières : `{"section": {…}}` ou `{"article": {…}}`.
fn toc_node(node: &TocNode) -> Value {
    match node {
        TocNode::Section(section) => {
            let TocSection { id, title, etat, children } = section;
            json!({"section": {
                "id": opt_shown(id.as_ref()),
                "title": title,
                "etat": status(etat.as_ref()),
                "children": children.iter().map(toc_node).collect::<Vec<_>>(),
            }})
        }
        TocNode::Article(article) => {
            let TocArticle { article_id, num, etat, inline_text } = article;
            json!({"article": {
                "article_id": shown(article_id),
                "num": num,
                "etat": status(etat.as_ref()),
                "inline_text": opt_long_text(inline_text.as_ref()),
            }})
        }
    }
}

/// Loi, décret ou code : l'identifiant, les champs communs et le contrat.
fn act_view(act: &Act) -> Value {
    let Act { text_id, body } = act;
    fields(
        [("text_id", shown(text_id))]
            .into_iter()
            .chain(body_pairs(body))
            .chain([("contract", contract(&act.contract()))]),
    )
}

/// Convention collective : l'identifiant, les champs communs, ceux de la convention et le contrat.
fn convention_view(convention_text: &Convention) -> Value {
    let Convention { text_id, body, idcc, container_title, container_id, bulletin } =
        convention_text;
    fields([("text_id", shown(text_id))].into_iter().chain(body_pairs(body)).chain([
        ("idcc", opt_shown(idcc.as_ref())),
        ("container_title", json!(container_title)),
        ("container_id", json!(container_id)),
        ("bulletin", json!(bulletin)),
        ("contract", contract(&convention_text.contract())),
    ]))
}

/// Texte parent d'un article : identifiant et nature.
fn parent_view(parent: &ParentText) -> Value {
    let ParentText { id, nature } = parent;
    json!({"id": shown(id), "nature": format!("{nature:?}").to_lowercase()})
}

/// Article de code, de loi ou de convention.
fn article_view(article: &ArticleText) -> Value {
    let ArticleText {
        article_id,
        num,
        etat,
        code_title,
        parent,
        path,
        full_sections_titre,
        date_debut,
        date_fin,
        text,
        nota,
    } = article;
    json!({
        "article_id": shown(article_id),
        "num": num,
        "etat": status(etat.as_ref()),
        "code_title": code_title,
        "parent": parent.as_ref().map_or(Value::Null, parent_view),
        "path": path,
        "full_sections_titre": full_sections_titre,
        "date_debut": opt_shown(date_debut.as_ref()),
        "date_fin": opt_shown(date_fin.as_ref()),
        "text": long_text(text),
        "nota": opt_long_text(nota.as_ref()),
        "contract": contract(&article.contract()),
    })
}

/// Sommaire officiel d'une décision.
fn summary_view(summary: &OfficialSummary) -> Value {
    let OfficialSummary { analyses, titrages } = summary;
    json!({"analyses": analyses, "titrages": titrages})
}

/// Décision de justice.
fn decision_view(decision: &Decision) -> Value {
    let Decision {
        decision_id,
        ecli,
        num,
        juridiction,
        formation,
        solution,
        numero_affaire,
        date_texte,
        nature,
        title,
        text,
        official_summary,
    } = decision;
    json!({
        "decision_id": shown(decision_id),
        "ecli": opt_shown(ecli.as_ref()),
        "num": num,
        "juridiction": juridiction,
        "formation": formation,
        "solution": solution,
        "numero_affaire": numero_affaire,
        "date_texte": opt_shown(date_texte.as_ref()),
        "nature": nature,
        "title": title,
        "text": long_text(text),
        "official_summary": summary_view(official_summary),
        "contract": contract(&decision.contract()),
    })
}

/// Délibération de la CNIL.
fn deliberation_view(deliberation: &Deliberation) -> Value {
    let Deliberation {
        deliberation_id,
        num,
        nature,
        nature_delib,
        etat,
        date_texte,
        date_publication,
        title,
        title_long,
        text,
    } = deliberation;
    json!({
        "deliberation_id": shown(deliberation_id),
        "num": num,
        "nature": nature,
        "nature_delib": nature_delib,
        "etat": status(etat.as_ref()),
        "date_texte": opt_shown(date_texte.as_ref()),
        "date_publication": opt_shown(date_publication.as_ref()),
        "title": title,
        "title_long": title_long,
        "text": long_text(text),
        "contract": contract(&deliberation.contract()),
    })
}

/// Accord d'entreprise (métadonnées seules).
fn agreement_view(agreement: &CompanyAgreement) -> Value {
    let CompanyAgreement {
        agreement_id,
        nature,
        title,
        raison_sociale,
        siret,
        idcc,
        code_ape,
        secteur,
        themes,
        syndicats,
        date_texte,
        date_effet,
        date_depot,
    } = agreement;
    json!({
        "agreement_id": shown(agreement_id),
        "nature": nature,
        "title": title,
        "raison_sociale": raison_sociale,
        "siret": siret,
        "idcc": opt_shown(idcc.as_ref()),
        "code_ape": code_ape,
        "secteur": secteur,
        "themes": themes,
        "syndicats": syndicats,
        "date_texte": opt_shown(date_texte.as_ref()),
        "date_effet": opt_shown(date_effet.as_ref()),
        "date_depot": opt_shown(date_depot.as_ref()),
        "contract": contract(&agreement.contract()),
    })
}

/// Article d'un code cité par un résultat de recherche.
fn code_article_view(article: &CodeArticleRef) -> Value {
    let CodeArticleRef { article_id, num, snippet } = article;
    json!({
        "article_id": shown(article_id),
        "num": num,
        "snippet": snippet.as_ref().map_or(Value::Null, |text| json!(text.as_str())),
    })
}

/// Résultat d'une recherche.
fn hit_view(hit: &SearchHit) -> Value {
    let SearchHit {
        fond,
        id,
        version_start,
        cid,
        title,
        date,
        nature,
        etat,
        snippet,
        idcc,
        raison_sociale,
        articles,
    } = hit;
    json!({
        "fond": fond.name(),
        "id": shown(id),
        "version_start": opt_shown(version_start.as_ref()),
        "cid": opt_shown(cid.as_ref()),
        "title": title,
        "date": opt_shown(date.as_ref()),
        "nature": nature,
        "etat": status(etat.as_ref()),
        "snippet": snippet.as_ref().map_or(Value::Null, |text| json!(text.as_str())),
        "idcc": opt_shown(idcc.as_ref()),
        "raison_sociale": raison_sociale,
        "articles": articles.iter().map(code_article_view).collect::<Vec<_>>(),
        "contract": contract(&hit.contract()),
    })
}

/// Page de résultats d'une recherche.
fn page_view(page: &SearchPage) -> Value {
    let SearchPage { fond, query, total, total_articles, hits, dropped, pagination } = page;
    json!({
        "fond": fond.name(),
        "query": query,
        "total": total,
        "total_articles": total_articles,
        "hits": hits.iter().map(hit_view).collect::<Vec<_>>(),
        "dropped": dropped,
        "pagination": pagination.api_name(),
    })
}

/// Projection JSON de l'entité `mapped`, avec tous ses champs (voir la documentation du module).
pub(crate) fn view(mapped: &Mapped) -> Value {
    match mapped {
        Mapped::Search(page) => page_view(page),
        Mapped::Act(act) => act_view(act),
        Mapped::Convention(convention) => convention_view(convention),
        Mapped::Article(article) => article_view(article),
        Mapped::Decision(decision) => decision_view(decision),
        Mapped::Deliberation(deliberation) => deliberation_view(deliberation),
        Mapped::Agreement(agreement) => agreement_view(agreement),
    }
}

/// Chaînes de `value` avec leur chemin, dans l'ordre du document : `text.text`,
/// `hits[0].title`, `toc[3].article.num`. Les clés ne sont pas des chaînes à contrôler : seules
/// les valeurs sont rendues.
pub(crate) fn leaf_strings(value: &Value) -> Vec<(String, &str)> {
    let mut leaves = Vec::new();
    collect_leaves(value, String::new(), &mut leaves);
    leaves
}

/// Ajoute à `leaves` les chaînes de `value`, dont le chemin est `path`.
fn collect_leaves<'a>(value: &'a Value, path: String, leaves: &mut Vec<(String, &'a str)>) {
    match value {
        Value::String(text) => leaves.push((path, text)),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_leaves(item, format!("{path}[{index}]"), leaves);
            }
        }
        Value::Object(entries) => {
            for (key, item) in entries {
                let child = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
                collect_leaves(item, child, leaves);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// Chemins des chaînes de l'entité qui portent un motif de mojibake latin-1 (04 §10.2, étape 1).
pub(crate) fn mojibake_paths(mapped: &Mapped) -> Vec<String> {
    leaf_strings(&view(mapped))
        .into_iter()
        .filter(|(_, text)| has_latin1_mojibake(text))
        .map(|(path, _)| path)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Diagnostics;
    use crate::map::fixtures::{ALL, Kind, decoded, fixture, map_body, map_fixture};

    /// Fixtures dont le corps reçu porte du mojibake réparable (04 §10.1).
    const REPAIRED: [&str; 6] = [
        "consult_article",
        "consult_juri",
        "consult_loda",
        "search_code",
        "search_juri",
        "search_loda",
    ];

    /// Mappe `fixture` depuis son corps non réparé.
    fn map_unrepaired(name: &str) -> Mapped {
        let fixture = fixture(name);
        map_body(
            fixture.kind,
            &decoded(fixture, false).text,
            fixture.requested,
            &mut Diagnostics::default(),
        )
        .expect(name)
    }

    /// D-35 : aucune chaîne d'une entité mappée ne porte de motif de mojibake, pour les seize
    /// fixtures (corps réparé) et la variante `KALIARTI` ; chaque projection a des chaînes.
    #[test]
    fn no_mojibake_after_mapping() {
        for fixture in &ALL {
            let mapped = map_fixture(fixture, &mut Diagnostics::default()).expect(fixture.name);
            assert_eq!(mojibake_paths(&mapped), Vec::<String>::new(), "{}", fixture.name);
            assert!(!leaf_strings(&view(&mapped)).is_empty(), "{}", fixture.name);
        }
        let text = decoded(fixture("consult_article"), true).text;
        let variant =
            map_body(Kind::GetArticle, &text, "KALIARTI000049886059", &mut Diagnostics::default())
                .expect("variant maps");
        assert_eq!(mojibake_paths(&variant), Vec::<String>::new());
        assert!(!leaf_strings(&view(&variant)).is_empty(), "variant");
    }

    /// Le contrôle n'est pas vide de sens : sans réparation, les six fixtures en mojibake
    /// ressortent, et l'article seulement par son texte.
    #[test]
    fn no_mojibake_check_detects_unrepaired_fixtures() {
        for name in REPAIRED {
            assert!(!mojibake_paths(&map_unrepaired(name)).is_empty(), "{name}");
        }
        assert_eq!(mojibake_paths(&map_unrepaired("consult_article")), ["text.text"]);
    }

    /// 04 §10.2, étape 3 : le faux positif de la détection (une lettre accentuée suivie d'une
    /// NBSP) est gardé tel quel par le décodage, puis neutralisé par la normalisation du mapping.
    #[test]
    fn constit_nbsp_false_positive_is_neutralised_by_mapping() {
        let constit = fixture("consult_juri_constit");
        let body = decoded(constit, true);
        assert_eq!((body.detected, body.repaired), (true, false));
        let raw: Value = serde_json::from_str(&body.text).expect("fixture is JSON");
        let mut raw_paths: Vec<String> = leaf_strings(&raw)
            .into_iter()
            .filter(|(_, text)| has_latin1_mojibake(text))
            .map(|(path, _)| path)
            .collect();
        raw_paths.sort_unstable();
        assert_eq!(raw_paths, ["text.texte", "text.texteHtml"]);
        let mapped = map_fixture(constit, &mut Diagnostics::default()).expect("constit maps");
        assert_eq!(mojibake_paths(&mapped), Vec::<String>::new());
    }

    /// La projection d'un accord porte tous ses champs, sous les clés du domaine.
    #[test]
    fn view_lists_every_acco_field() {
        let mapped =
            map_fixture(fixture("consult_acco"), &mut Diagnostics::default()).expect("acco maps");
        let projected = view(&mapped);
        let mut keys: Vec<&str> =
            projected.as_object().expect("object").keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut expected = [
            "agreement_id",
            "nature",
            "title",
            "raison_sociale",
            "siret",
            "idcc",
            "code_ape",
            "secteur",
            "themes",
            "syndicats",
            "date_texte",
            "date_effet",
            "date_depot",
            "contract",
        ];
        expected.sort_unstable();
        assert_eq!(keys, expected);
        assert_eq!(projected["idcc"], "275");
        assert_eq!(projected["date_effet"], "2025-12-20");
        assert_eq!(projected["themes"], json!(["Télétravail"]));
    }

    /// 04 §13.3, P-038 : la projection `view` de chacune des seize fixtures, puis de la variante
    /// `KALIARTI` de l'article, est épinglée par un snapshot `insta` (fichiers sous `snapshots/`).
    /// Un snapshot neuf ou différent fait échouer le test ; hors CI, `insta` écrit un `.snap.new`
    /// à relire (`INSTA_UPDATE=auto` vaut `no` quand `CI` est posé).
    #[test]
    fn mapping_snapshots() {
        for fixture in &ALL {
            let mapped = map_fixture(fixture, &mut Diagnostics::default()).expect(fixture.name);
            insta::assert_json_snapshot!(fixture.name, view(&mapped));
        }
        let text = decoded(fixture("consult_article"), true).text;
        let mapped =
            map_body(Kind::GetArticle, &text, "KALIARTI000049886059", &mut Diagnostics::default())
                .expect("variant maps");
        // aucun crochet dans un nom de fichier : le nom de la variante est écrit en clair
        insta::assert_json_snapshot!("consult_article_kaliarti000049886059", view(&mapped));
    }
}
