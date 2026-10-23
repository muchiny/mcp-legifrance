//! Règles propres à l'article de `/consult/getArticle` (04 §9.1 a, b, c, e, f, g, h ; §9.2 ;
//! 03 §4.3).
//!
//! Ce fichier ne porte que des règles pures, chacune testée seule : le titre de texte préféré
//! ([`preferred_title`]), le texte parent d'après son `cid` puis son `id` ([`parent_text`]), le
//! chemin de sections ([`section_path`]) et le nota ([`nota_of`]). Aucune valeur du corps ne sort
//! d'ici : un identifiant refusé devient un [`CorpusError::Decode`] qui ne nomme que l'endpoint
//! et le chemin du champ.

use lf_application::ports::CorpusError;
use lf_domain::entity::ParentText;
use lf_domain::id::DocumentId;
use lf_domain::text::{LongText, clean_field};

use super::body::non_blank;
use super::endpoint::GET_ARTICLE;
use super::rules::long_text;
use super::{Diagnostics, Field};
use crate::dto::article::{ContextEntryDto, TextTitleDto};

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
/// ni `id` non blanc : `Ok(None)`.
///
/// # Errors
///
/// [`CorpusError::Decode`] (`<endpoint>: article.textTitles[].cid: invalid identifier`, ou `.id`
/// si seul `id` est présent) si l'identifiant retenu n'est pas lisible.
fn parent_text(title: &TextTitleDto) -> Result<Option<ParentText>, CorpusError> {
    let candidates = [(PARENT_CID, title.cid.as_deref()), (PARENT_ID, title.id.as_deref())];
    let Some((field, raw)) =
        candidates.into_iter().find_map(|(field, raw)| non_blank(raw).map(|raw| (field, raw)))
    else {
        return Ok(None);
    };
    let id = DocumentId::parse(raw).map_err(|error| field.domain(&error))?;
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

#[cfg(test)]
mod tests {
    use lf_domain::entity::ParentNature;

    use super::*;

    /// `Some(valeur.to_owned())` : une macro et non une fonction, que `clippy::unnecessary_wraps`
    /// refuserait.
    macro_rules! text {
        ($value:expr) => {
            Some($value.to_owned())
        };
    }

    /// Champ `article.texte`, pour les tests de compteur.
    const TEXTE: Field = Field { endpoint: GET_ARTICLE, path: "article.texte" };

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

    /// Erreur de décodage d'un champ de l'article.
    fn decode_error(path: &str) -> CorpusError {
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
        for (title, expected) in cases {
            assert_eq!(parent_text(&title), Ok(expected));
        }
    }

    #[test]
    fn parent_text_rejects_an_unreadable_identifier() {
        assert_eq!(
            parent_text(&ided(Some("FOO"), None, None)),
            Err(decode_error("article.textTitles[].cid"))
        );
        assert_eq!(
            parent_text(&ided(Some("FOO"), Some("LEGITEXT000006070721"), None)),
            Err(decode_error("article.textTitles[].cid"))
        );
        assert_eq!(
            parent_text(&ided(None, Some("FOO"), None)),
            Err(decode_error("article.textTitles[].id"))
        );
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
}
