//! Règles propres à l'article de `/consult/getArticle` (04 §9.1 a, b, c, e, f, g, h ; §9.2 ;
//! 03 §4.3).
//!
//! [`article`] assemble l'[`ArticleText`] d'une réponse, sans décision locale, à partir de règles
//! pures chacune testée seule : le titre de texte préféré ([`preferred_title`]), le texte parent
//! d'après son `cid` puis son `id` ([`parent_text`]), le chemin de sections ([`section_path`]) et
//! le nota ([`nota_of`]). Aucune valeur du corps ne sort d'ici : un identifiant refusé devient un
//! [`CorpusError::Decode`] qui ne nomme que l'endpoint et le chemin du champ.

use lf_application::ports::CorpusError;
use lf_domain::entity::{ArticleText, ParentText};
use lf_domain::id::{ArticleId, DocumentId};
use lf_domain::status::LegalStatus;
use lf_domain::text::{LongText, clean_field};

use super::body::non_blank;
use super::dates::civil;
use super::endpoint::GET_ARTICLE;
use super::rules::{long_text, parse_counted, require, resolve_id};
use super::{Diagnostics, Field};
use crate::dto::article::{ArticleDto, ContextEntryDto, GetArticleResponseDto, TextTitleDto};

/// Champ `id` de l'article.
const ID: Field = Field { endpoint: GET_ARTICLE, path: "article.id" };
/// Champ `dateDebut` de l'article.
const DATE_DEBUT: Field = Field { endpoint: GET_ARTICLE, path: "article.dateDebut" };
/// Champ `dateFin` de l'article.
const DATE_FIN: Field = Field { endpoint: GET_ARTICLE, path: "article.dateFin" };
/// Champ `texte` de l'article.
const TEXTE: Field = Field { endpoint: GET_ARTICLE, path: "article.texte" };
/// Champ `cid` d'un titre de texte parent.
const PARENT_CID: Field = Field { endpoint: GET_ARTICLE, path: "article.textTitles[].cid" };
/// Champ `id` d'un titre de texte parent.
const PARENT_ID: Field = Field { endpoint: GET_ARTICLE, path: "article.textTitles[].id" };
/// Champ `nota` de l'article.
const NOTA: Field = Field { endpoint: GET_ARTICLE, path: "article.nota" };

/// Vrai si `nature`, rognée et sans égard à la casse, est `CODE`.
fn is_code(nature: Option<&str>) -> bool {
    nature.is_some_and(|nature| nature.trim().eq_ignore_ascii_case("CODE"))
}

/// Titre de texte retenu : le premier de nature `CODE`, sinon le premier ; `None` si la liste
/// est vide.
fn preferred_title(titles: &[TextTitleDto]) -> Option<&TextTitleDto> {
    titles.iter().find(|title| is_code(title.nature.as_deref())).or_else(|| titles.first())
}

/// Texte parent de `title` : identifiant lu dans `cid`, à défaut dans `id` (le premier des deux
/// qui n'est pas blanc), nature brute de `title`.
///
/// Un `cid` présent et valide l'emporte sur un `id` illisible, qui n'est alors pas lu. Ni `cid`
/// ni `id` non blanc : `Ok(None)`. L'identifiant retenu est lu par `parse_counted` (règle c) : un
/// suffixe de version retiré, hors `LEGITEXT`, est compté dans `diags`.
///
/// # Errors
///
/// [`CorpusError::Decode`] (`<endpoint>: article.textTitles[].cid: invalid identifier`, ou `.id`
/// si seul `id` est présent) si l'identifiant retenu n'est pas lisible.
fn parent_text(
    title: &TextTitleDto,
    diags: &mut Diagnostics,
) -> Result<Option<ParentText>, CorpusError> {
    let candidates = [(PARENT_CID, title.cid.as_deref()), (PARENT_ID, title.id.as_deref())];
    let Some((field, raw)) =
        candidates.into_iter().find_map(|(field, raw)| non_blank(raw).map(|raw| (field, raw)))
    else {
        return Ok(None);
    };
    let id = parse_counted::<DocumentId>(raw, diags).map_err(|error| field.domain(&error))?;
    Ok(Some(ParentText::new(id, title.nature.as_deref())))
}

/// Titres des sections traversées, nettoyés par `clean_field`, dans l'ordre reçu ; les titres
/// absents ou vides sont omis.
fn section_path(entries: &[ContextEntryDto]) -> Vec<String> {
    entries.iter().filter_map(|entry| clean_field(entry.titre.as_deref())).collect()
}

/// Nota de l'article : le meilleur texte de `html` et de `plain` (règle f) ; `None` s'il est
/// vide. Les invisibles retirés sont comptés dans `diags`, même quand le nota est omis.
///
/// # Errors
///
/// [`CorpusError::Decode`] (`/consult/getArticle: article.nota: text too large`) si le nota
/// dépasse la taille permise.
fn nota_of(
    html: Option<&str>,
    plain: Option<&str>,
    diags: &mut Diagnostics,
) -> Result<Option<LongText>, CorpusError> {
    let nota = long_text(html, plain, NOTA, diags)?;
    Ok((!nota.is_empty()).then_some(nota))
}

/// Article de la réponse `dto` de `/consult/getArticle`, demandé sous `requested` (04 §9.1, §9.2) ;
/// ses anomalies sont comptées dans `diags`.
///
/// Ordre des erreurs, la première l'emporte : l'identité de l'objet, l'identifiant renvoyé, le
/// `cid` du texte parent choisi, `dateDebut`, `dateFin`, le texte, le nota. Le titre et le parent
/// viennent de la même entrée de `textTitles`.
///
/// # Errors
///
/// [`CorpusError::NotFound`] (`/consult/getArticle`) si l'article est absent ou sans `id` ni
/// `cid` ; `diags` n'est alors pas touché. [`CorpusError::Decode`] du premier champ refusé :
/// `article.id` ou `article.textTitles[].cid` (`invalid identifier`), `article.dateDebut` ou
/// `article.dateFin` (`unreadable date`), `article.texte` ou `article.nota` (`text too large`).
pub(crate) fn article(
    dto: GetArticleResponseDto,
    requested: &ArticleId,
    diags: &mut Diagnostics,
) -> Result<ArticleText, CorpusError> {
    let ArticleDto {
        id,
        num,
        etat,
        texte,
        texte_html,
        nota,
        nota_html,
        date_debut,
        date_fin,
        full_sections_titre,
        context,
        text_titles,
        ..
    } = require(dto.article, GET_ARTICLE)?;
    let article_id = resolve_id(requested, id.as_deref(), ID, diags)?;
    let chosen = preferred_title(&text_titles);
    let parent = chosen.map(|title| parent_text(title, diags)).transpose()?.flatten();
    let date_debut = civil(date_debut.as_ref(), DATE_DEBUT, diags)?;
    let date_fin = civil(date_fin.as_ref(), DATE_FIN, diags)?;
    let text = long_text(texte_html.as_deref(), texte.as_deref(), TEXTE, diags)?;
    let nota = nota_of(nota_html.as_deref(), nota.as_deref(), diags)?;
    Ok(ArticleText {
        article_id,
        num: clean_field(num.as_deref()),
        etat: etat.as_deref().and_then(LegalStatus::parse),
        code_title: chosen.and_then(|title| clean_field(title.titre.as_deref())),
        parent,
        path: section_path(&context.titres_tm),
        full_sections_titre: clean_field(full_sections_titre.as_deref()),
        date_debut,
        date_fin,
        text,
        nota,
    })
}

#[cfg(test)]
mod tests {
    use lf_domain::contract::Citable;
    use lf_domain::entity::ParentNature;
    use lf_domain::status::LegalStatus;

    use super::*;
    use crate::dto::article::GetArticleResponseDto;
    use crate::dto::decode::decode;
    use crate::map::fixtures::{d, decode_as, text};

    /// Titre de texte de `titre`, `nature` et sans identifiant.
    fn titled(titre: &str, nature: Option<&str>) -> TextTitleDto {
        TextTitleDto {
            titre: text!(titre),
            nature: nature.map(str::to_owned),
            ..TextTitleDto::default()
        }
    }

    /// Titre de texte portant `cid`, `id` et `nature`.
    fn ided(cid: Option<&str>, id: Option<&str>, nature: Option<&str>) -> TextTitleDto {
        TextTitleDto {
            cid: cid.map(str::to_owned),
            id: id.map(str::to_owned),
            nature: nature.map(str::to_owned),
            ..TextTitleDto::default()
        }
    }

    /// Titre du titre de texte retenu parmi `titles`.
    fn picked(titles: &[TextTitleDto]) -> Option<&str> {
        preferred_title(titles).and_then(|title| title.titre.as_deref())
    }

    /// Identifiant `LEGITEXT` du Code civil.
    fn civil_code() -> DocumentId {
        DocumentId::parse("LEGITEXT000006070721").unwrap()
    }

    /// Erreur de décodage d'un identifiant illisible au champ `path` de l'article.
    fn invalid_id(path: &str) -> CorpusError {
        CorpusError::Decode { detail: format!("/consult/getArticle: {path}: invalid identifier") }
    }

    /// Entrée de contexte de titre `titre`.
    fn entry(titre: Option<&str>) -> ContextEntryDto {
        ContextEntryDto { titre: titre.map(str::to_owned), ..ContextEntryDto::default() }
    }

    #[test]
    fn code_nature_matching() {
        for nature in ["CODE", "code", " Code "] {
            assert!(is_code(Some(nature)), "{nature:?}");
        }
        for nature in [Some("CODES"), Some(""), Some("LOI"), None] {
            assert!(!is_code(nature), "{nature:?}");
        }
    }

    #[test]
    fn preferred_title_picks_the_first_code_entry() {
        assert!(preferred_title(&[]).is_none());
        assert_eq!(picked(&[titled("A", Some("LOI")), titled("B", Some("CODE"))]), Some("B"));
        assert_eq!(picked(&[titled("A", Some("code")), titled("B", Some("CODE"))]), Some("A"));
        assert_eq!(picked(&[titled("A", Some("LOI")), titled("B", Some("DECRET"))]), Some("A"));
        assert_eq!(picked(&[titled("A", None)]), Some("A"));
    }

    #[test]
    fn parent_text_rules() {
        let code = Some(ParentText { id: civil_code(), nature: ParentNature::Code });
        let cases = [
            (ided(Some("LEGITEXT000006070721"), None, Some("CODE")), code.clone()),
            (ided(Some("LEGITEXT000006070721"), Some("garbage"), Some("CODE")), code.clone()),
            (ided(None, Some("LEGITEXT000006070721"), Some("CODE")), code.clone()),
            (ided(Some("  "), Some("LEGITEXT000006070721"), Some("CODE")), code),
            (
                ided(Some("KALITEXT000005635534"), None, None),
                Some(ParentText {
                    id: DocumentId::parse("KALITEXT000005635534").unwrap(),
                    nature: ParentNature::Kali,
                }),
            ),
            (
                ided(Some("JORFTEXT000033202746"), None, Some("LOI")),
                Some(ParentText {
                    id: DocumentId::parse("JORFTEXT000033202746").unwrap(),
                    nature: ParentNature::Text,
                }),
            ),
            (ided(None, None, Some("CODE")), None),
            (ided(Some(" "), Some(""), None), None),
        ];
        let mut diags = Diagnostics::default();
        for (title, expected) in cases {
            assert_eq!(parent_text(&title, &mut diags), Ok(expected));
        }
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn parent_text_rejects_an_unreadable_identifier() {
        let mut diags = Diagnostics::default();
        assert_eq!(
            parent_text(&ided(Some("FOO"), None, None), &mut diags),
            Err(invalid_id("article.textTitles[].cid"))
        );
        assert_eq!(
            parent_text(&ided(Some("FOO"), Some("LEGITEXT000006070721"), None), &mut diags),
            Err(invalid_id("article.textTitles[].cid"))
        );
        assert_eq!(
            parent_text(&ided(None, Some("FOO"), None), &mut diags),
            Err(invalid_id("article.textTitles[].id"))
        );
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn section_path_keeps_cleaned_titles_in_order() {
        let entries = [
            entry(Some("Livre <mark>Ier</mark>")),
            entry(Some("   ")),
            entry(None),
            entry(Some("Titre II")),
        ];
        assert_eq!(section_path(&entries), ["Livre Ier", "Titre II"]);
        assert_eq!(section_path(&[]), Vec::<String>::new());
    }

    #[test]
    fn nota_rules() {
        let mut diags = Diagnostics::default();
        assert_eq!(nota_of(None, None, &mut diags), Ok(None));
        assert_eq!(diags, Diagnostics::default());

        let plain = nota_of(None, Some("Nota de l'article"), &mut diags).unwrap().unwrap();
        assert_eq!(plain.as_str(), "Nota de l'article");

        let html = nota_of(
            Some("<p>Alinéa un.</p><p>Alinéa deux.</p>"),
            Some("Alinéa un. Alinéa deux."),
            &mut diags,
        )
        .unwrap()
        .unwrap();
        assert_eq!(html.as_str(), "Alinéa un.\n\nAlinéa deux.");
        assert_eq!(diags, Diagnostics::default());

        assert_eq!(nota_of(None, Some("\u{200b}"), &mut diags), Ok(None));
        assert_eq!(diags.invisible_chars_removed, 1);
    }

    #[test]
    fn invisible_counts_accumulate_across_text_and_nota() {
        let mut diags = Diagnostics::default();
        long_text(None, Some("a\u{200b}"), TEXTE, &mut diags).unwrap();
        assert_eq!(nota_of(None, Some("\u{200b}\u{200b}"), &mut diags), Ok(None));
        assert_eq!(diags.invisible_chars_removed, 3);
    }

    #[test]
    fn nota_too_large_is_decode_without_the_text() {
        let mut diags = Diagnostics::default();
        let huge = "a".repeat(16_000_001);
        assert_eq!(
            nota_of(None, Some(&huge), &mut diags),
            Err(CorpusError::Decode {
                detail: "/consult/getArticle: article.nota: text too large".into()
            })
        );
        assert_eq!(diags, Diagnostics::default());
    }

    // ---- article() : assemblage ----

    /// Identifiant `LEGIARTI` de l'article 9 du Code civil.
    const ART_9: &str = "LEGIARTI000006419288";

    /// Titres de texte d'un article de loi, puis de code.
    const LAW_THEN_CODE: &str = r#"[
        {"nature":"LOI","titre":"Loi X","cid":"JORFTEXT000033202746"},
        {"nature":"code","titre":"Code civil","cid":"LEGITEXT000006070721"}
    ]"#;

    /// Identifiant d'article demandé.
    fn art(raw: &str) -> ArticleId {
        ArticleId::parse(raw).unwrap()
    }

    /// Erreur de décodage de détail `detail`.
    fn decode_detail(detail: &str) -> CorpusError {
        CorpusError::Decode { detail: detail.into() }
    }

    /// `article` de la réponse `body` (corps complet), demandé en `requested`.
    fn run_body(body: &str, requested: &str) -> (Result<ArticleText, CorpusError>, Diagnostics) {
        let dto = decode::<GetArticleResponseDto>(body, GET_ARTICLE).unwrap();
        let mut diags = Diagnostics::default();
        let mapped = article(dto, &art(requested), &mut diags);
        (mapped, diags)
    }

    /// `article` de la réponse `{"article": <article_json>}`, demandé en `requested`.
    fn run(article_json: &str, requested: &str) -> (Result<ArticleText, CorpusError>, Diagnostics) {
        run_body(&format!(r#"{{"article":{article_json}}}"#), requested)
    }

    /// `article` de la fixture réelle `consult_article`, demandé en `requested`.
    fn map_fixture(requested: &str) -> (Result<ArticleText, CorpusError>, Diagnostics) {
        let dto = decode_as::<GetArticleResponseDto>("consult_article");
        let mut diags = Diagnostics::default();
        let mapped = article(dto, &art(requested), &mut diags);
        (mapped, diags)
    }

    /// Article valide de texte brut `texte` et de `fields` supplémentaires (JSON, sans accolades).
    fn with_fields(fields: &str) -> String {
        format!(r#"{{"id":"{ART_9}",{fields}}}"#)
    }

    #[test]
    fn article_fixture_maps() {
        let (mapped, diags) = map_fixture(ART_9);
        let article = mapped.unwrap();
        assert_eq!(article.article_id, art(ART_9));
        assert_eq!(article.num.as_deref(), Some("9"));
        assert_eq!(article.etat, Some(LegalStatus::Vigueur));
        assert_eq!(article.code_title.as_deref(), Some("Code civil"));
        assert_eq!(
            article.parent,
            Some(ParentText { id: civil_code(), nature: ParentNature::Code })
        );
        assert_eq!(article.path, ["Livre Ier : Des personnes", "Titre Ier : Des droits civils"]);
        assert_eq!(article.full_sections_titre.as_deref(), Some("Livre Ier : Des personnes"));
        assert_eq!(article.date_debut, Some(d(1970, 7, 19)));
        assert_eq!(article.date_fin, None);
        // Le golden annonce `byte_size` 364 : ce sont les octets du mojibake (P-037).
        assert_eq!(
            article.text.as_str(),
            "Chacun a droit au respect de sa vie privée. Les juges peuvent, sans préjudice de la \
             réparation du dommage subi, prescrire toutes mesures, telles que séquestre, saisie et \
             autres, propres à empêcher ou faire cesser une atteinte à l'intimité de la vie \
             privée : ces mesures peuvent, s'il y a urgence, être ordonnées en référé."
        );
        assert_eq!(article.text.char_len(), 322);
        assert_eq!(article.text.paragraphs().count(), 1);
        assert_eq!(article.nota, None);
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn kaliarti_variant_keeps_the_returned_id_with_diagnostic() {
        let (nominal, _) = map_fixture(ART_9);
        let (mapped, diags) = map_fixture("KALIARTI000049886059");
        let article = mapped.unwrap();
        assert_eq!(article.article_id, art(ART_9));
        assert_eq!(diags, Diagnostics { id_mismatch: 1, ..Diagnostics::default() });
        assert_eq!(article, nominal.unwrap());
        // Le golden écrit `…/conv_coll/id/KALIARTI000049886059` (divergence `returned_id_kept`).
        assert_eq!(
            article.contract().source_url.as_str(),
            "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288"
        );
    }

    #[test]
    fn article_citation() {
        let contract = map_fixture(ART_9).0.unwrap().contract();
        assert_eq!(contract.citation.as_str(), "Code civil, art. 9");
        assert_eq!(
            contract.source_url.as_str(),
            "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288"
        );
    }

    #[test]
    fn article_identity_follows_the_shared_rules() {
        let not_found = CorpusError::NotFound { what: "/consult/getArticle".into() };
        let bodies = [
            r#"{"article":null}"#,
            "{}",
            r#"{"executionTime":1,"dereferenced":false}"#,
            r#"{"article":{}}"#,
            r#"{"article":{"etat":"VIGUEUR","texte":"  ","textTitles":[{"nature":"CODE"}]}}"#,
            r#"{"article":{"num":"9","texte":"t"}}"#,
        ];
        for body in bodies {
            let (mapped, diags) = run_body(body, ART_9);
            assert_eq!(mapped, Err(not_found.clone()), "{body}");
            assert_eq!(diags, Diagnostics::default(), "{body}");
        }

        let (mapped, diags) = run(&format!(r#"{{"cid":"{ART_9}"}}"#), ART_9);
        let article = mapped.unwrap();
        assert_eq!(article.article_id, art(ART_9));
        assert!(article.text.is_empty());
        assert_eq!(diags, Diagnostics { id_mismatch: 1, ..Diagnostics::default() });

        let (mapped, diags) = run(r#"{"id":"LEGIARTI000006419288_01-02-2003"}"#, ART_9);
        assert_eq!(mapped.unwrap().article_id, art(ART_9));
        assert_eq!(diags, Diagnostics { upstream_suffix_stripped: 1, ..Diagnostics::default() });

        let (mapped, diags) = run(r#"{"id":"LEGITEXT000006070721"}"#, ART_9);
        assert_eq!(
            mapped,
            Err(decode_detail("/consult/getArticle: article.id: invalid identifier"))
        );
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn article_without_html_uses_plain_text() {
        let plain = "Lorsque R<P, le taux T = 5 %.";
        let json = with_fields(&format!(r#""texte":"{plain}","texteHtml":"   ""#));
        let (mapped, diags) = run(&json, ART_9);
        let article = mapped.unwrap();
        assert_eq!(article.text.as_str(), plain);
        assert_eq!(article.text.char_len(), 29);
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn code_title_prefers_nature_code() {
        let json = with_fields(&format!(r#""textTitles":{LAW_THEN_CODE}"#));
        assert_eq!(run(&json, ART_9).0.unwrap().code_title.as_deref(), Some("Code civil"));

        let no_code = with_fields(
            r#""textTitles":[{"nature":"LOI","titre":"Loi X"},{"nature":"DECRET","titre":"Y"}]"#,
        );
        assert_eq!(run(&no_code, ART_9).0.unwrap().code_title.as_deref(), Some("Loi X"));

        for fields in [r#""textTitles":[]"#, r#""textTitles":null"#, r#""num":"9""#] {
            let article = run(&with_fields(fields), ART_9).0.unwrap();
            assert_eq!((article.code_title, article.parent), (None, None), "{fields}");
        }
    }

    #[test]
    fn parent_comes_from_the_same_text_title() {
        let json = with_fields(&format!(r#""textTitles":{LAW_THEN_CODE}"#));
        let parent = run(&json, ART_9).0.unwrap().parent;
        assert_eq!(parent, Some(ParentText { id: civil_code(), nature: ParentNature::Code }));

        let no_code = with_fields(
            r#""textTitles":[{"nature":"LOI","titre":"Loi X","cid":"JORFTEXT000033202746"}]"#,
        );
        let parent = run(&no_code, ART_9).0.unwrap().parent.unwrap();
        assert_eq!(parent.id.as_str(), "JORFTEXT000033202746");
        assert_eq!(parent.nature, ParentNature::Text);

        // L'entrée d'indice 0 n'est pas la choisie : son `cid` illisible n'est jamais lu.
        let second = with_fields(
            r#""textTitles":[{"nature":"LOI","cid":"BAR"},{"nature":"CODE","cid":"FOO"}]"#,
        );
        assert_eq!(
            run(&second, ART_9).0,
            Err(decode_detail("/consult/getArticle: article.textTitles[].cid: invalid identifier"))
        );

        let without_id = with_fields(r#""textTitles":[{"nature":"CODE","titre":"Code civil"}]"#);
        let article = run(&without_id, ART_9).0.unwrap();
        assert_eq!(article.parent, None);
        assert_eq!(article.code_title.as_deref(), Some("Code civil"));
    }

    #[test]
    fn a_suffixed_parent_id_is_stripped_and_counted_outside_legitext() {
        let cases = [
            (
                r#"{"nature":"LOI","cid":"JORFTEXT000033202746_08-10-2016"}"#,
                "JORFTEXT000033202746",
                1,
            ),
            (
                r#"{"nature":"LOI","id":"JORFTEXT000033202746_08-10-2016"}"#,
                "JORFTEXT000033202746",
                1,
            ),
            (
                r#"{"nature":"CODE","cid":"LEGITEXT000006070721_09-04-2026"}"#,
                "LEGITEXT000006070721",
                0,
            ),
            (r#"{"nature":"CODE","cid":"LEGITEXT000006070721"}"#, "LEGITEXT000006070721", 0),
        ];
        for (title, parent, stripped) in cases {
            let json = with_fields(&format!(r#""textTitles":[{title}]"#));
            let (mapped, diags) = run(&json, ART_9);
            let parent_id = mapped.unwrap().parent.map(|parent| parent.id);
            assert_eq!(parent_id.as_ref().map(DocumentId::as_str), Some(parent), "{title}");
            assert_eq!(
                diags,
                Diagnostics { upstream_suffix_stripped: stripped, ..Diagnostics::default() },
                "{title}"
            );
        }
    }

    #[test]
    fn short_fields_are_cleaned() {
        let json = with_fields(
            r#""num":" 9 <sup>bis</sup> ","fullSectionsTitre":"  Livre   Ier  ","textTitles":[{"nature":"CODE","titre":"Code <mark>civil</mark>"}]"#,
        );
        let article = run(&json, ART_9).0.unwrap();
        assert_eq!(article.num.as_deref(), Some("9 bis"));
        assert_eq!(article.full_sections_titre.as_deref(), Some("Livre Ier"));
        assert_eq!(article.code_title.as_deref(), Some("Code civil"));

        let blank = with_fields(r#""num":"   ","fullSectionsTitre":"<br>""#);
        let article = run(&blank, ART_9).0.unwrap();
        assert_eq!((article.num, article.full_sections_titre), (None, None));
    }

    #[test]
    fn status_code_is_typed() {
        let cases = [
            (r#""etat":"VIGUEUR""#, Some(LegalStatus::Vigueur)),
            (r#""etat":"abroge""#, Some(LegalStatus::Abroge)),
            (r#""etat":"ABROGE_DIFF""#, Some(LegalStatus::AbrogeDiff)),
            (r#""etat":"NOUVEAU_CODE""#, Some(LegalStatus::Other("NOUVEAU_CODE".into()))),
            (r#""etat":"  ""#, None),
            (r#""num":"9""#, None),
        ];
        for (fields, expected) in cases {
            assert_eq!(run(&with_fields(fields), ART_9).0.unwrap().etat, expected, "{fields}");
        }
    }

    #[test]
    fn dates_go_through_the_shared_converter() {
        let (mapped, diags) = run(&with_fields(r#""dateDebut":17197200000"#), ART_9);
        assert_eq!(mapped.unwrap().date_debut, Some(d(1970, 7, 19)));
        assert_eq!(diags, Diagnostics { non_midnight_timestamps: 1, ..Diagnostics::default() });

        for (fields, path) in [
            (r#""dateFin":"hier""#, "article.dateFin"),
            (r#""dateDebut":"hier""#, "article.dateDebut"),
        ] {
            assert_eq!(
                run(&with_fields(fields), ART_9).0,
                Err(decode_detail(&format!("/consult/getArticle: {path}: unreadable date")))
            );
        }

        let cases = [("2222-01-01", None), ("2221-12-31", Some(d(2221, 12, 31)))];
        for (end, expected) in cases {
            let json = with_fields(&format!(r#""dateFin":"{end}""#));
            assert_eq!(run(&json, ART_9).0.unwrap().date_fin, expected, "{end}");
        }
    }

    #[test]
    fn nota_prefers_html_then_plain() {
        let both = with_fields(
            r#""notaHtml":"<p>Alinéa un.</p><p>Alinéa deux.</p>","nota":"Alinéa un. Alinéa deux.""#,
        );
        let nota = run(&both, ART_9).0.unwrap().nota.unwrap();
        assert_eq!(nota.as_str(), "Alinéa un.\n\nAlinéa deux.");

        let plain = with_fields(r#""nota":"Nota simple""#);
        assert_eq!(run(&plain, ART_9).0.unwrap().nota.unwrap().as_str(), "Nota simple");

        let none = with_fields(r#""nota":"","notaHtml":"""#);
        assert_eq!(run(&none, ART_9).0.unwrap().nota, None);
    }

    #[test]
    fn oversized_text_or_nota_is_a_decode_error() {
        let size = usize::try_from(LongText::MAX_CHARS).unwrap() + 1;
        for field in ["texte", "nota"] {
            let json = with_fields(&format!(r#""{field}":"{}""#, "a".repeat(size)));
            let (mapped, diags) = run(&json, ART_9);
            drop(json);
            assert_eq!(
                mapped,
                Err(decode_detail(&format!(
                    "/consult/getArticle: article.{field}: text too large"
                )))
            );
            assert_eq!(diags, Diagnostics::default());
        }
    }

    #[test]
    fn kali_article_maps_with_a_kali_parent() {
        let kali = "KALIARTI000005805295";
        let json = format!(
            r#"{{"id":"{kali}","num":"3","texte":"Texte","textTitles":[{{"nature":"???","titre":"Convention X","cid":"KALITEXT000005635534"}}]}}"#
        );
        let (mapped, diags) = run(&json, kali);
        let article = mapped.unwrap();
        assert_eq!(article.article_id, art(kali));
        assert_eq!(article.parent.as_ref().map(|parent| parent.nature), Some(ParentNature::Kali));
        assert_eq!(
            article.contract().source_url.as_str(),
            "https://www.legifrance.gouv.fr/conv_coll/id/KALIARTI000005805295"
        );
        assert_eq!(diags, Diagnostics::default());
        assert_eq!(diags.id_mismatch, 0);
    }

    #[test]
    fn law_article_citation_through_the_mapping() {
        let law = "LEGIARTI000033219575";
        let json = format!(
            r#"{{"id":"{law}","num":1,"texte":"Texte","textTitles":[{{"nature":"LOI","titre":"LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique (1)","cid":"JORFTEXT000033202746"}}]}}"#
        );
        let article = run(&json, law).0.unwrap();
        assert_eq!(article.parent.as_ref().map(|parent| parent.nature), Some(ParentNature::Text));
        let contract = article.contract();
        assert_eq!(
            contract.citation.as_str(),
            "LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique, art. 1"
        );
        assert_eq!(
            contract.source_url.as_str(),
            "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000033219575"
        );
    }

    #[test]
    fn error_details_never_contain_body_values() {
        let secret = "secret-token-42";
        let cases = [
            format!(r#"{{"id":"{secret}"}}"#),
            with_fields(&format!(r#""textTitles":[{{"nature":"CODE","cid":"{secret}"}}]"#)),
            with_fields(&format!(r#""dateFin":"{secret}""#)),
        ];
        for json in cases {
            let error = run(&json, ART_9).0.unwrap_err();
            assert!(!format!("{error:?} {error}").contains(secret), "{error:?}");
        }
    }
}
