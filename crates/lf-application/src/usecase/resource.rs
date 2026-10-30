//! Cas d'usage de la lecture d'une resource MCP (`ResourceUseCase`), qui réutilise les cas d'usage
//! de lecture (v0.3, M7 ; 05 §8.2 et §8.3 ; 00 §6).
//!
//! # Grammaire des URI
//!
//! Gabarits RFC 6570 de niveau 1, correspondance inverse écrite à la main (MUST de
//! `resources.mdx:430`) :
//!
//! ```text
//! legifrance://coverage
//! legifrance://article/{article_id}
//! legifrance://decision/{decision_id}
//! legifrance://act/{text_id}
//! legifrance://code/{code}/article/{num}
//! ```
//!
//! `ResourceRef::parse` ne laisse passer qu'une URI de 512 octets au plus faite de lettres
//! ASCII, de chiffres et de `- . _ ~ * : /`, avec des échappements `%XX` valides qui donnent de
//! l'UTF-8 ; une variable décodée ne contient ni `/`, ni contrôle, et n'est ni `.` ni `..`.
//!
//! - Hors de cette grammaire (schéma étranger, traversée, encodage invalide, caractère interdit) :
//!   `invalid_arg`, donc `-32602` dans les deux ères (C-31).
//! - Dans la grammaire, mais sans gabarit correspondant (famille inconnue, mauvais nombre de
//!   segments) : `not_found`, donc `-32002` aux pairs legacy et `-32602` aux modernes.
//!
//! Ces deux refus n'ont pas de gabarit à nommer : ils ne sont pas audités (comme un outil inconnu,
//! 02 §9.2). Tout le reste l'est, sous `resource:<gabarit>`.
//!
//! # Lecture
//!
//! Une resource est une lecture complète **bornée** : au plus 40 000 caractères de texte
//! ([`crate::budget::ResponseBudget::MAX_CHARS_MAX`]) et 500 entrées de table des matières, avec
//! en dernière ligne l'outil et le `offset` pour la suite. Les identifiants sont validés comme
//! ceux des outils (`article_id`, `decision_id` et `text_id` donnent les mêmes `invalid_arg`).
//! `code` est le slug d'un titre de `list_codes()` ; `num` est lu à la date du jour à Paris
//! (`ArticleAt::Today`).

use lf_domain::{
    citation::ArticleNum,
    date::VersionDate,
    entity::ArticleText,
    text::{DispositifRule, window},
};

use crate::{
    budget::{ReadingRequest, read},
    error::AppError,
    failure::port_failure,
    input::{CoverageInput, GetActInput, GetDecisionInput, GetTextInput, ReadingInput},
    messages,
    output::{
        get_text::{GetTextOutput, NOTA_MAX_CHARS},
        resource::{ResourceKind, ResourceOutput},
    },
    ports::{ArticleAt, Clock, CorpusError, LegalCorpus},
    service::ServiceSettings,
    tools::ToolName,
    usecase::{
        complete::code_slugs, coverage::CoverageUseCase, get_act::GetActUseCase,
        get_decision::GetDecisionUseCase, get_text::GetTextUseCase,
    },
};

/// Schéma des URI de resource.
const SCHEME: &str = "legifrance://";

/// Longueur maximale d'une URI, en octets.
const MAX_URI_BYTES: usize = 512;

/// Longueur maximale du slug `code`, en caractères (le plus long titre de code fait moins de 80).
const MAX_CODE_CHARS: usize = 100;

/// Longueur maximale du `num` d'un article, en caractères (`R*641-1`, `D. 1234-5-2`).
const MAX_NUM_CHARS: usize = 40;

/// Taille d'une page de texte : le plafond de `max_chars` (05 §8.2). Éprouvé contre
/// `ResponseBudget::MAX_CHARS_MAX` par `the_page_sizes_follow_the_budget`.
const PAGE_CHARS: i64 = 40_000;

/// Entrées de table des matières d'une resource `act` : le plafond de `toc_limit` (05 §8.2).
const TOC_ENTRIES: i64 = 500;

/// Les gabarits, pour les messages d'erreur.
const TEMPLATES: &str = "legifrance://coverage, legifrance://article/{article_id}, \
                         legifrance://decision/{decision_id}, legifrance://act/{text_id} or \
                         legifrance://code/{code}/article/{num}";

/// Ce qu'une URI désigne, après la grammaire et avant la validation des identifiants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResourceRef {
    /// `legifrance://coverage`.
    Coverage,
    /// `legifrance://article/{article_id}`.
    Article(String),
    /// `legifrance://decision/{decision_id}`.
    Decision(String),
    /// `legifrance://act/{text_id}`.
    Act(String),
    /// `legifrance://code/{code}/article/{num}`.
    CodeArticle {
        /// Slug du code.
        code: String,
        /// Numéro de l'article.
        num: String,
    },
}

impl ResourceRef {
    /// Analyse `uri` selon la grammaire ci-dessus.
    ///
    /// # Errors
    ///
    /// `invalid_arg` hors grammaire, `not_found` pour une URI de la grammaire sans gabarit.
    pub(crate) fn parse(uri: &str) -> Result<Self, AppError> {
        if uri.len() > MAX_URI_BYTES || !uri.bytes().all(is_uri_byte) {
            return Err(malformed());
        }
        let rest = uri.strip_prefix(SCHEME).ok_or_else(malformed)?;
        let decoded = rest.split('/').map(percent_decode).collect::<Option<Vec<String>>>();
        let segments = decoded.filter(|segments| segments.iter().all(|s| is_plain(s)));
        let segments = segments.ok_or_else(malformed)?;
        let segments: Vec<&str> = segments.iter().map(String::as_str).collect();
        match segments.as_slice() {
            ["coverage"] => Ok(Self::Coverage),
            ["article", id] => Ok(Self::Article((*id).to_owned())),
            ["decision", id] => Ok(Self::Decision((*id).to_owned())),
            ["act", id] => Ok(Self::Act((*id).to_owned())),
            ["code", code, "article", num] => {
                Ok(Self::CodeArticle { code: (*code).to_owned(), num: (*num).to_owned() })
            }
            _ => {
                Err(AppError::not_found(format!("No resource matches this URI; use {TEMPLATES}.")))
            }
        }
    }

    /// Le gabarit de l'URI, qui nomme la ligne d'audit.
    pub(crate) const fn kind(&self) -> ResourceKind {
        match self {
            Self::Coverage => ResourceKind::Coverage,
            Self::Article(_) => ResourceKind::Article,
            Self::Decision(_) => ResourceKind::Decision,
            Self::Act(_) => ResourceKind::Act,
            Self::CodeArticle { .. } => ResourceKind::CodeArticle,
        }
    }
}

/// Vrai si `uri` est dans la grammaire des resources : bien formée, avec ou sans gabarit connu.
/// Seule une telle URI peut être recopiée dans une réponse d'erreur (`data.uri`) : une URI hors
/// grammaire est du texte libre du client, qui ne doit pas atteindre les journaux.
#[must_use]
pub fn is_resource_uri(uri: &str) -> bool {
    matches!(ResourceRef::parse(uri), Ok(_) | Err(AppError::NotFound { .. }))
}

/// `invalid_arg` d'une URI hors grammaire. Elle ne la recopie pas.
fn malformed() -> AppError {
    AppError::invalid_argument(format!("uri is not a valid resource URI; use {TEMPLATES}."))
}

/// Octet admis dans une URI : lettres et chiffres ASCII, `- . _ ~ * : /` et `%` des échappements.
fn is_uri_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'*' | b':' | b'/' | b'%')
}

/// Variable décodée sans traversée : ni `.` ni `..`, ni `/` (un `%2F` décodé), ni contrôle.
fn is_plain(segment: &str) -> bool {
    !matches!(segment, "." | "..") && !segment.chars().any(|c| c == '/' || c.is_control())
}

/// Décode les échappements `%XX` d'un segment ; `None` pour un échappement tronqué ou non
/// hexadécimal, ou une suite d'octets qui n'est pas de l'UTF-8.
fn percent_decode(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'%' {
            let hex = segment.get(index.saturating_add(1)..index.saturating_add(3))?;
            // `from_str_radix` accepte un `+` de tête : seuls les chiffres hexadécimaux passent.
            let value = u8::from_str_radix(hex, 16)
                .ok()
                .filter(|_| hex.bytes().all(|digit| digit.is_ascii_hexdigit()))?;
            out.push(value);
            index = index.saturating_add(3);
        } else {
            out.push(byte);
            index = index.saturating_add(1);
        }
    }
    String::from_utf8(out).ok()
}

/// Cas d'usage de la lecture d'une resource : il appelle les cas d'usage de lecture des outils.
pub(crate) struct ResourceUseCase<'a, C: LegalCorpus, K: Clock> {
    corpus: &'a C,
    clock: &'a K,
    settings: &'a ServiceSettings,
    /// Taille d'une page de texte, en caractères : [`PAGE_CHARS`]. Un champ, pour qu'un test
    /// éprouve le refus d'une page hors bornes.
    page_chars: i64,
}

impl<'a, C: LegalCorpus, K: Clock> ResourceUseCase<'a, C, K> {
    /// Cas d'usage sur `corpus` ; `clock` donne « aujourd'hui » à Paris et `settings` la
    /// configuration que `legifrance://coverage` décrit.
    pub(crate) fn new(corpus: &'a C, clock: &'a K, settings: &'a ServiceSettings) -> Self {
        Self { corpus, clock, settings, page_chars: PAGE_CHARS }
    }

    /// Lit la resource désignée.
    ///
    /// # Errors
    ///
    /// - `invalid_arg` pour un identifiant, un `code` ou un `num` invalide (aucun appel au port) ;
    /// - `not_found` pour un enregistrement, un code ou un article introuvable ;
    /// - les autres erreurs du port, traduites comme celles des outils.
    pub(crate) async fn execute(&self, target: ResourceRef) -> Result<ResourceOutput, AppError> {
        match target {
            ResourceRef::Coverage => {
                let output =
                    CoverageUseCase::new(self.settings, self.clock).execute(&CoverageInput);
                Ok(ResourceOutput::coverage(output))
            }
            ResourceRef::Article(id) => {
                let input =
                    GetTextInput { article_id: Some(id), reading: full_page(self.page_chars) };
                let output = GetTextUseCase::new(self.corpus).execute(input).await?;
                Ok(ResourceOutput::article(ResourceKind::Article, &output))
            }
            ResourceRef::Decision(id) => {
                let input =
                    GetDecisionInput { decision_id: Some(id), reading: full_page(self.page_chars) };
                let output = GetDecisionUseCase::new(self.corpus).execute(input).await?;
                Ok(ResourceOutput::decision(&output))
            }
            ResourceRef::Act(id) => {
                let input = GetActInput {
                    text_id: Some(id),
                    date: None,
                    toc_offset: None,
                    toc_limit: Some(TOC_ENTRIES),
                };
                let output = GetActUseCase::new(self.corpus, self.clock).execute(input).await?;
                Ok(ResourceOutput::act(&output))
            }
            ResourceRef::CodeArticle { code, num } => self.code_article(&code, &num).await,
        }
    }

    /// `legifrance://code/{code}/article/{num}` : le slug devient un `LEGITEXT` par `list_codes()`,
    /// puis l'article est cherché par son numéro à la date du jour à Paris.
    async fn code_article(&self, code: &str, num: &str) -> Result<ResourceOutput, AppError> {
        let slug = code_slug(code)?;
        let num = article_number(num)?;
        let key = article_key(&num)?;
        let codes = self
            .corpus
            .list_codes()
            .await
            .map_err(|error| port_failure(CODE_ARTICLE, "", &error))?;
        let text_id = code_slugs(&codes).get(slug.as_str()).map(|listed| listed.id.clone());
        let text_id = text_id.ok_or_else(|| {
            AppError::not_found(format!(
                "No code matches {slug}. Complete the code variable to list the available codes."
            ))
        })?;
        let today = VersionDate::try_from(self.clock.today_paris())
            .map_err(|error| messages::domain(CODE_ARTICLE, &error))?;
        // L'API attend la clé canonique (`L1234-5`, annexe C), pas la forme citée (`L. 1234-5`) :
        // chaque variante de la clé est essayée dans l'ordre (`R*641-1` puis `R641-1`).
        let mut absent = CorpusError::NotFound { what: "article".to_owned() };
        for variant in key.key_variants() {
            match self.corpus.find_article(&text_id, &variant, ArticleAt::Today(today)).await {
                Ok(article) => {
                    let output = read_article(article, self.page_chars)?;
                    return Ok(ResourceOutput::article(ResourceKind::CodeArticle, &output));
                }
                Err(error @ CorpusError::NotFound { .. }) => absent = error,
                Err(error) => return Err(article_failure(&slug, &num, &error)),
            }
        }
        Err(article_failure(&slug, &num, &absent))
    }
}

/// Outil nommé par les journaux et les messages d'une lecture par code.
const CODE_ARTICLE: ToolName = ToolName::RESOURCE_CODE_ARTICLE;

/// Lecture en texte plein, du début, de `max_chars` caractères.
fn full_page(max_chars: i64) -> ReadingInput {
    ReadingInput {
        detail: Some("full".to_owned()),
        offset: None,
        max_chars: Some(max_chars),
        focus: None,
        passages: None,
    }
}

/// `code` : un slug, lettres ASCII, chiffres et `-`, qu'on compare en minuscules.
fn code_slug(code: &str) -> Result<String, AppError> {
    if code.is_empty() {
        return Err(messages::required("code"));
    }
    let valid = code.chars().count() <= MAX_CODE_CHARS
        && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !valid {
        return Err(AppError::invalid_argument(
            "code is not a code slug (letters, digits and '-', as in code-civil); complete the \
             code variable to list the available codes.",
        ));
    }
    Ok(code.to_ascii_lowercase())
}

/// `num` : un numéro d'article (lettres ASCII, chiffres, espace, `.`, `*` et `-`), sans blanc de
/// bord.
fn article_number(num: &str) -> Result<String, AppError> {
    let num = num.trim();
    if num.is_empty() {
        return Err(messages::required("num"));
    }
    let valid = num.chars().count() <= MAX_NUM_CHARS
        && num.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '*' | '-'));
    if !valid {
        return Err(AppError::invalid_argument(
            "num is not an article number (letters, digits, spaces, '.', '*' and '-', as in L. \
             1234-5 or R*641-1).",
        ));
    }
    Ok(num.to_owned())
}

/// Le numéro d'article lu par la grammaire des citations : `invalid_arg` si ce n'est pas un numéro
/// (`9_1` est refusé plus tôt, `99999999999` dépasse ce que la clé sait porter).
fn article_key(num: &str) -> Result<ArticleNum, AppError> {
    num.parse().map_err(|_| {
        AppError::invalid_argument(
            "num is not an article number (as in L. 1234-5, R*641-1 or 1er); search with \
             fr_search fond=CODE.",
        )
    })
}

/// Erreur de `find_article`, journalisée comme les autres : l'absence de l'article n'est pas celle
/// de l'identifiant d'un outil, elle nomme le code et le numéro demandés.
fn article_failure(slug: &str, num: &str, error: &CorpusError) -> AppError {
    let mapped = port_failure(CODE_ARTICLE, num, error);
    if matches!(error, CorpusError::NotFound { .. }) {
        AppError::not_found(format!(
            "No article {num} found in {slug}. Check the number, or search with fr_search \
             fond=CODE."
        ))
    } else {
        mapped
    }
}

/// La page de texte d'un article trouvé par son numéro, comme `fr_get_text` en texte plein : le
/// texte et le `nota` sont lus une fois, puis lâchés de l'article.
fn read_article(mut article: ArticleText, page_chars: i64) -> Result<GetTextOutput, AppError> {
    let text = std::mem::take(&mut article.text);
    let nota = article
        .nota
        .take()
        .map(|nota| window(nota.as_str(), 0, NOTA_MAX_CHARS).text.trim_end().to_owned());
    let reading = ReadingRequest::parse(&full_page(page_chars))
        .and_then(|request| read(&text, None, DispositifRule::None, &request))?;
    Ok(GetTextOutput { article, nota, reading })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::DateTime;
    use lf_domain::{
        date::CivilDate,
        entity::{Act, CodeListing, Decision, OfficialSummary, TextBody},
        id::{ArticleId, DecisionId, LegiTextId},
        status::LegalStatus,
        text::LongText,
    };
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        audited::{AuditMode, AuditPolicy},
        budget::ResponseBudget,
        error::ErrorCode,
        output::resource::ResourceContent,
        ports::{ConfigKind, PisteEnvironment},
        service::ConfigurationState,
        testing::{FakeCorpus, FixedClock, PortRequest},
    };

    const ARTICLE_ID: &str = "LEGIARTI000006419288";
    const DECISION_ID: &str = "JURITEXT000037999394";
    const TEXT_ID: &str = "LEGITEXT000006070721";

    fn settings() -> ServiceSettings {
        ServiceSettings {
            policy: AuditPolicy {
                mode: AuditMode::Strict,
                deadline: Duration::from_secs(60),
                env: PisteEnvironment::Sandbox,
                server_version: "0.0.0",
            },
            configuration: ConfigurationState {
                credentials_configured: true,
                ..ConfigurationState::default()
            },
            budget: ResponseBudget,
        }
    }

    fn clock_on(today: CivilDate) -> FixedClock {
        FixedClock::new(DateTime::from_timestamp(1_790_000_000, 0).unwrap(), today)
    }

    fn clock() -> FixedClock {
        clock_on(CivilDate::new(2026, 10, 29).unwrap())
    }

    async fn read_with(
        corpus: &FakeCorpus,
        clock: &FixedClock,
        uri: &str,
    ) -> Result<ResourceOutput, AppError> {
        let settings = settings();
        let target = ResourceRef::parse(uri)?;
        ResourceUseCase::new(corpus, clock, &settings).execute(target).await
    }

    async fn read_uri(corpus: &FakeCorpus, uri: &str) -> Result<ResourceOutput, AppError> {
        read_with(corpus, &clock(), uri).await
    }

    fn markdown(output: &ResourceOutput) -> &str {
        let ResourceContent::Markdown(text) = &output.content else { return "<not Markdown>" };
        text
    }

    fn article(text: &str, nota: Option<&str>) -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(ARTICLE_ID).unwrap(),
            num: Some("9".to_owned()),
            etat: Some(LegalStatus::Vigueur),
            code_title: Some("Code civil".to_owned()),
            parent: None,
            path: Vec::new(),
            full_sections_titre: None,
            date_debut: None,
            date_fin: None,
            text: LongText::from_plain(text).unwrap(),
            nota: nota.map(|nota| LongText::from_plain(nota).unwrap()),
        }
    }

    fn listing(title: &str, id: &str) -> CodeListing {
        CodeListing {
            id: LegiTextId::parse(id).unwrap(),
            title: title.to_owned(),
            etat: Some(LegalStatus::Vigueur),
        }
    }

    fn codes() -> Vec<CodeListing> {
        vec![
            listing("Code du travail", "LEGITEXT000006072050"),
            listing("Code civil", TEXT_ID),
            listing("Code de l'environnement", "LEGITEXT000006074220"),
        ]
    }

    // ---- grammaire --------------------------------------------------------------------------

    #[test]
    fn the_five_templates_parse() {
        let expected = [
            ("legifrance://coverage", ResourceRef::Coverage, ResourceKind::Coverage),
            (
                "legifrance://article/LEGIARTI000006419288",
                ResourceRef::Article(ARTICLE_ID.to_owned()),
                ResourceKind::Article,
            ),
            (
                "legifrance://decision/JURITEXT000037999394",
                ResourceRef::Decision(DECISION_ID.to_owned()),
                ResourceKind::Decision,
            ),
            (
                "legifrance://act/LEGITEXT000006070721_08-09-2023",
                ResourceRef::Act("LEGITEXT000006070721_08-09-2023".to_owned()),
                ResourceKind::Act,
            ),
            (
                "legifrance://code/code-civil/article/9",
                ResourceRef::CodeArticle { code: "code-civil".to_owned(), num: "9".to_owned() },
                ResourceKind::CodeArticle,
            ),
        ];
        for (uri, wanted, kind) in expected {
            let parsed = ResourceRef::parse(uri).unwrap();
            assert_eq!(parsed, wanted, "{uri}");
            assert_eq!(parsed.kind(), kind, "{uri}");
        }
    }

    #[test]
    fn variables_are_percent_decoded() {
        let parsed =
            ResourceRef::parse("legifrance://code/code-civil/article/L.%201234-5").unwrap();
        assert_eq!(
            parsed,
            ResourceRef::CodeArticle { code: "code-civil".to_owned(), num: "L. 1234-5".to_owned() }
        );
        // Un échappement qui donne de l'UTF-8 valide passe la grammaire (la validation de la
        // variable est celle du cas d'usage).
        let accent = ResourceRef::parse("legifrance://article/%C3%A9").unwrap();
        assert_eq!(accent, ResourceRef::Article("é".to_owned()));
        // Un `*` brut et un `%2a` sont la même chose.
        let star = ResourceRef::parse("legifrance://code/c/article/R%2A641-1").unwrap();
        assert_eq!(
            star,
            ResourceRef::CodeArticle { code: "c".to_owned(), num: "R*641-1".to_owned() }
        );
    }

    #[test]
    fn uris_outside_the_grammar_are_invalid_arg() {
        let long = format!("legifrance://article/{}", "A".repeat(600));
        let outside = [
            // Schéma étranger, absent ou mal formé.
            "file:///etc/passwd",
            "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288",
            "LEGIFRANCE://coverage",
            "legifrance:/coverage",
            "legifrance:coverage",
            "legifrance//coverage",
            "coverage",
            "",
            // Traversée.
            "legifrance://article/..",
            "legifrance://article/.",
            "legifrance://article/%2e%2e",
            "legifrance://article/%2E",
            "legifrance://code/../article/9",
            "legifrance://code/code-civil/article/%2E%2E",
            "legifrance://article/..%2Fact%2FX",
            // Encodage invalide.
            "legifrance://article/%",
            "legifrance://article/%4",
            "legifrance://article/%zz",
            "legifrance://article/%+1",
            "legifrance://article/%ff",
            "legifrance://article/%c3",
            "legifrance://article/%00",
            "legifrance://article/%0A",
            // Caractères hors de l'ensemble admis.
            "legifrance://article/ABC DEF",
            "legifrance://article/ABC?x=1",
            "legifrance://article/ABC#frag",
            "legifrance://article/é",
            "legifrance://article/a\\b",
            "legifrance://article/a\nb",
            "legifrance://article/a\u{0}b",
            "legifrance://coverage\u{202e}",
            &long,
        ];
        for uri in outside {
            let error = ResourceRef::parse(uri).unwrap_err();
            assert_eq!(error.code(), Some(ErrorCode::InvalidArg), "{uri:?}");
            // Le message est toujours le même : il n'écrit jamais l'URI demandée.
            assert_eq!(error.message(), malformed().message(), "{uri:?}");
            assert!(
                error.message().starts_with("uri is not a valid resource URI; use "),
                "{error}"
            );
        }
    }

    #[test]
    fn uris_in_the_grammar_without_a_template_are_not_found() {
        for uri in [
            "legifrance://",
            "legifrance://unknown",
            "legifrance://unknown/x",
            "legifrance://coverage/",
            "legifrance://coverage/extra",
            "legifrance://article",
            "legifrance://article/ID/extra",
            "legifrance://code/code-civil",
            "legifrance://code/code-civil/article",
            "legifrance://code/code-civil/section/9",
            "legifrance://code/code-civil/article/9/extra",
            "legifrance://Article/LEGIARTI000006419288",
        ] {
            let error = ResourceRef::parse(uri).unwrap_err();
            assert_eq!(error.code(), Some(ErrorCode::NotFound), "{uri}");
            assert!(error.message().starts_with("No resource matches this URI; use "), "{error}");
        }
    }

    #[test]
    fn percent_decode_edge_cases() {
        assert_eq!(percent_decode("").as_deref(), Some(""));
        assert_eq!(percent_decode("a%20b%2Fc").as_deref(), Some("a b/c"));
        assert_eq!(percent_decode("%e2%82%ac").as_deref(), Some("€"));
        for bad in ["%", "a%2", "%g0", "%0g", "%+1", "%-1", "%ff", "%é1"] {
            assert_eq!(percent_decode(bad), None, "{bad}");
        }
    }

    #[test]
    fn plain_segments_have_no_traversal_slash_or_control() {
        for plain in ["", "a", "..a", "a..", "...", "é", "a b"] {
            assert!(is_plain(plain), "{plain:?}");
        }
        for not_plain in [".", "..", "a/b", "/", "a\u{0}", "a\n", "\u{7f}", "\u{85}"] {
            assert!(!is_plain(not_plain), "{not_plain:?}");
        }
    }

    #[test]
    fn only_a_uri_of_the_grammar_may_be_echoed() {
        for uri in [
            "legifrance://coverage",
            "legifrance://article/LEGIARTI000006419288",
            "legifrance://code/code-civil/article/L.%209",
            // De la grammaire, sans gabarit : « introuvable ».
            "legifrance://inconnue/x",
        ] {
            assert!(is_resource_uri(uri), "{uri}");
        }
        for uri in [
            "",
            "texte libre",
            "https://example.org/x",
            "legifrance://article/un texte",
            "legifrance://article/é",
            "legifrance://code/../article/9",
            "legifrance://article/%2F",
            &format!("legifrance://article/{}", "x".repeat(600)),
        ] {
            assert!(!is_resource_uri(uri), "{uri}");
        }
    }

    // ---- validation des variables -----------------------------------------------------------

    #[test]
    fn code_and_num_are_validated_before_any_port_call() {
        assert_eq!(code_slug("Code-Civil").unwrap(), "code-civil");
        assert_eq!(article_number("  L. 1234-5 ").unwrap(), "L. 1234-5");
        assert_eq!(article_number("R*641-1").unwrap(), "R*641-1");

        let missing = [code_slug("").unwrap_err(), article_number("").unwrap_err()];
        assert_eq!(missing[0].message(), "code is required.");
        assert_eq!(missing[1].message(), "num is required.");
        assert_eq!(article_number("   ").unwrap_err().message(), "num is required.");

        for bad in ["code civil", "code_civil", "é", "code/civil", &"a".repeat(101)] {
            let error = code_slug(bad).unwrap_err();
            assert_eq!(error.code(), Some(ErrorCode::InvalidArg), "{bad}");
            assert!(error.message().starts_with("code is not a code slug"), "{error}");
            assert!(!error.message().contains(bad), "{error}");
        }
        assert_eq!(code_slug(&"a".repeat(100)).unwrap().len(), 100);
        for bad in ["9; DROP", "9_1", "é", "a/b", "a\tb", &"9".repeat(41)] {
            let error = article_number(bad).unwrap_err();
            assert_eq!(error.code(), Some(ErrorCode::InvalidArg), "{bad}");
            assert!(error.message().starts_with("num is not an article number"), "{error}");
        }
        assert_eq!(article_number(&"9".repeat(40)).unwrap().len(), 40);
    }

    // ---- lectures ---------------------------------------------------------------------------

    #[tokio::test]
    async fn an_article_is_read_in_full_from_the_start_up_to_40000_characters() {
        let corpus = FakeCorpus::new();
        let long = "Les mots se suivent. ".repeat(3_000);
        corpus.push_get_article(Ok(article(&long, Some("Nota de l'article."))));

        let output = read_uri(&corpus, "legifrance://article/LEGIARTI000006419288").await.unwrap();

        assert_eq!(output.kind, ResourceKind::Article);
        let text = markdown(&output);
        assert!(text.starts_with("# Code civil, art. 9\n\n- Source: "), "{text}");
        assert!(text.contains("## Nota\n\nNota de l'article."), "{text}");
        let (_, truncation) =
            text.rsplit_once("\n\nText truncated at 40,000 characters: ").unwrap();
        assert!(
            truncation.starts_with("use fr_get_text with detail=\"full\" and offset="),
            "{text}"
        );
        // Le texte lu tient dans la page de 40 000 caractères.
        let characters = text.chars().count();
        assert!(characters < 40_000 + 500, "{characters}");
        assert_eq!(corpus.calls_of("get_article"), 1);
    }

    #[tokio::test]
    async fn a_short_article_has_no_truncation_line() {
        let corpus = FakeCorpus::new();
        corpus.push_get_article(Ok(article("Chacun a droit au respect de sa vie privée.", None)));

        let output = read_uri(&corpus, "legifrance://article/LEGIARTI000006419288").await.unwrap();

        assert!(markdown(&output).ends_with("\n\nChacun a droit au respect de sa vie privée."));
    }

    #[tokio::test]
    async fn an_invalid_identifier_is_invalid_arg_without_a_port_call() {
        let corpus = FakeCorpus::new();
        for (uri, field) in [
            ("legifrance://article/not-an-id", "article_id"),
            ("legifrance://article/", "article_id"),
            ("legifrance://decision/LEGIARTI000006419288", "decision_id"),
            ("legifrance://decision/", "decision_id"),
            ("legifrance://act/JURITEXT000037999394", "text_id"),
            ("legifrance://act/", "text_id"),
        ] {
            let error = read_uri(&corpus, uri).await.unwrap_err();
            assert_eq!(error.code(), Some(ErrorCode::InvalidArg), "{uri}");
            assert!(error.message().contains(field), "{uri}: {error}");
        }
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn a_missing_record_is_not_found_with_the_tool_message() {
        let corpus = FakeCorpus::new();
        corpus.push_get_article(Err(CorpusError::NotFound { what: "/consult/getArticle".into() }));
        corpus.push_get_decision(Err(CorpusError::NotFound { what: "/consult/juri".into() }));
        corpus.push_get_act(Err(CorpusError::NotFound { what: "/consult/lawDecree".into() }));

        let article = read_uri(&corpus, "legifrance://article/LEGIARTI000006419288").await;
        let decision = read_uri(&corpus, "legifrance://decision/JURITEXT000037999394").await;
        let act = read_uri(&corpus, "legifrance://act/LEGITEXT000006070721").await;

        for (error, message) in [
            (article, "No article found for LEGIARTI000006419288."),
            (decision, "No JURI decision found for JURITEXT000037999394."),
            (act, "No LODA text found for LEGITEXT000006070721."),
        ] {
            let error = error.unwrap_err();
            assert_eq!(error.code(), Some(ErrorCode::NotFound));
            assert!(error.message().starts_with(message), "{error}");
        }
    }

    #[tokio::test]
    async fn a_decision_is_read_in_full() {
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(Decision {
            decision_id: DecisionId::parse(DECISION_ID).unwrap(),
            ecli: None,
            num: None,
            juridiction: Some("Cour de cassation".to_owned()),
            formation: None,
            solution: Some("Rejet".to_owned()),
            numero_affaire: Vec::new(),
            date_texte: CivilDate::new(2019, 6, 5),
            nature: None,
            title: Some("Cass. civ. 1re, 5 juin 2019".to_owned()),
            text: LongText::from_plain("La Cour rejette le pourvoi.").unwrap(),
            official_summary: OfficialSummary::new(["Analyse."], Vec::<String>::new()),
            cited_case_law: None,
            applied_texts: vec![],
        }));

        let output = read_uri(&corpus, "legifrance://decision/JURITEXT000037999394").await.unwrap();

        assert_eq!(output.kind, ResourceKind::Decision);
        let text = markdown(&output);
        assert!(text.starts_with("# Cass. civ. 1re, 5 juin 2019\n"), "{text}");
        assert!(text.ends_with("## Text\n\nLa Cour rejette le pourvoi."), "{text}");
    }

    #[tokio::test]
    async fn an_act_is_read_with_the_latest_version_and_up_to_500_toc_entries() {
        let corpus = FakeCorpus::new();
        corpus.push_get_act(Ok(Act {
            text_id: LegiTextId::parse(TEXT_ID).unwrap(),
            body: TextBody { title: Some("Code civil".to_owned()), ..TextBody::default() },
        }));

        let output = read_uri(&corpus, "legifrance://act/LEGITEXT000006070721").await.unwrap();

        assert_eq!(output.kind, ResourceKind::Act);
        let text = markdown(&output);
        assert!(text.contains("Version of 2026-10-29"), "{text}");
        assert!(markdown(&output).contains("No table of contents is available"));
        // L'argument de table des matières est le plafond de `toc_limit`.
        assert_eq!(corpus.calls_of("get_act"), 1);
    }

    #[tokio::test]
    async fn coverage_makes_no_port_call() {
        let corpus = FakeCorpus::new();

        let output = read_uri(&corpus, "legifrance://coverage").await.unwrap();

        assert_eq!(output.kind, ResourceKind::Coverage);
        // La couverture est mise en JSON par l'adapter : l'application rend la sortie calculée.
        let today = CivilDate::new(2026, 10, 29).unwrap();
        assert!(matches!(&output.content, ResourceContent::Coverage(c) if c.consulted_on == today));
        assert_eq!(markdown(&output), "<not Markdown>");
        assert_eq!(corpus.calls(), 0);
    }

    // ---- code/{code}/article/{num} ----------------------------------------------------------

    #[tokio::test]
    async fn a_code_article_is_resolved_through_the_code_list_at_the_paris_date() {
        let corpus = FakeCorpus::new();
        corpus.push_list_codes(Ok(codes()));
        corpus.push_find_article(Ok(article("Chacun a droit au respect de sa vie privée.", None)));

        let output =
            read_uri(&corpus, "legifrance://code/Code-Civil/article/L.%209").await.unwrap();

        assert_eq!(output.kind, ResourceKind::CodeArticle);
        assert!(markdown(&output).starts_with("# Code civil, art. 9\n"));
        let requests = corpus.seen_requests();
        let today = VersionDate::parse("2026-10-29").unwrap();
        assert_eq!(
            requests,
            [
                PortRequest::ListCodes,
                PortRequest::FindArticle(
                    LegiTextId::parse(TEXT_ID).unwrap(),
                    "L9".to_owned(),
                    ArticleAt::Today(today)
                ),
            ]
        );
    }

    #[tokio::test]
    async fn the_article_is_asked_by_its_canonical_key_and_its_variants_in_order() {
        let today = VersionDate::parse("2026-10-29").unwrap();
        let text = LegiTextId::parse(TEXT_ID).unwrap();
        let asked = |num: &str| {
            PortRequest::FindArticle(text.clone(), num.to_owned(), ArticleAt::Today(today))
        };

        // `L. 1234-5` devient `L1234-5`, la seule forme que l'API connaît.
        let corpus = FakeCorpus::new();
        corpus.push_list_codes(Ok(codes()));
        corpus.push_find_article(Ok(article("Texte.", None)));
        read_uri(&corpus, "legifrance://code/code-civil/article/L.%201234-5").await.unwrap();
        assert_eq!(corpus.seen_requests(), [PortRequest::ListCodes, asked("L1234-5")]);

        // `R*641-1` puis, faute d'article, `R641-1`.
        let corpus = FakeCorpus::new();
        corpus.push_list_codes(Ok(codes()));
        corpus.push_find_article(Err(CorpusError::NotFound { what: "/consult".to_owned() }));
        corpus.push_find_article(Ok(article("Texte.", None)));
        read_uri(&corpus, "legifrance://code/code-civil/article/R*641-1").await.unwrap();
        assert_eq!(
            corpus.seen_requests(),
            [PortRequest::ListCodes, asked("R*641-1"), asked("R641-1")]
        );

        // Toutes les variantes absentes : introuvable, avec le numéro tel que demandé.
        let corpus = FakeCorpus::new();
        corpus.push_list_codes(Ok(codes()));
        corpus.push_find_article(Err(CorpusError::NotFound { what: "/consult".to_owned() }));
        corpus.push_find_article(Err(CorpusError::NotFound { what: "/consult".to_owned() }));
        let error =
            read_uri(&corpus, "legifrance://code/code-civil/article/R*641-1").await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::NotFound));
        assert!(error.message().starts_with("No article R*641-1 found in code-civil."), "{error}");
        assert_eq!(corpus.calls_of("find_article"), 2);
    }

    #[tokio::test]
    async fn an_unknown_code_is_not_found_and_names_the_slug() {
        let corpus = FakeCorpus::new();
        corpus.push_list_codes(Ok(codes()));

        let error =
            read_uri(&corpus, "legifrance://code/code-inconnu/article/9").await.unwrap_err();

        assert_eq!(error.code(), Some(ErrorCode::NotFound));
        assert_eq!(
            error.message(),
            "No code matches code-inconnu. Complete the code variable to list the available codes."
        );
        assert_eq!(corpus.calls_of("find_article"), 0);
    }

    #[tokio::test]
    async fn an_unknown_article_number_is_not_found_and_names_the_number() {
        let corpus = FakeCorpus::new();
        corpus.push_list_codes(Ok(codes()));
        corpus.push_find_article(Err(CorpusError::NotFound { what: "/search".to_owned() }));

        let error =
            read_uri(&corpus, "legifrance://code/code-civil/article/99999").await.unwrap_err();

        assert_eq!(error.code(), Some(ErrorCode::NotFound));
        assert!(error.message().starts_with("No article 99999 found in code-civil."), "{error}");
    }

    #[tokio::test]
    async fn port_errors_of_a_code_article_keep_their_own_code() {
        let corpus = FakeCorpus::new();
        // list_codes échoue.
        corpus.push_list_codes(Err(CorpusError::Timeout));
        let error = read_uri(&corpus, "legifrance://code/code-civil/article/9").await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::UpstreamError));
        // list_codes ne trouve rien : jamais « No article found » d'un autre outil.
        corpus.push_list_codes(Err(CorpusError::NotFound { what: "/list/code".to_owned() }));
        let error = read_uri(&corpus, "legifrance://code/code-civil/article/9").await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::UpstreamError));
        // find_article échoue.
        corpus.push_list_codes(Ok(codes()));
        corpus.push_find_article(Err(CorpusError::Config {
            kind: ConfigKind::CredentialsMissing,
            detail: String::new(),
        }));
        let error = read_uri(&corpus, "legifrance://code/code-civil/article/9").await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::ConfigError));
    }

    #[tokio::test]
    async fn invalid_variables_of_a_code_article_never_reach_the_port() {
        let corpus = FakeCorpus::new();
        for uri in [
            "legifrance://code//article/9",
            "legifrance://code/code%20civil/article/9",
            "legifrance://code/code-civil/article/",
            "legifrance://code/code-civil/article/9_1",
            // Caractères admis, mais pas un numéro d'article.
            "legifrance://code/code-civil/article/99999999999",
            "legifrance://code/code-civil/article/abc",
            // Hors grammaire : refusée avant tout cas d'usage.
            "legifrance://code/../article/9",
        ] {
            let error = read_uri(&corpus, uri).await.unwrap_err();
            assert_eq!(error.code(), Some(ErrorCode::InvalidArg), "{uri}");
        }
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn a_date_outside_the_served_versions_is_invalid_arg() {
        let corpus = FakeCorpus::new();
        corpus.push_list_codes(Ok(codes()));
        let clock = clock_on(CivilDate::new(1400, 1, 1).unwrap());

        let error =
            read_with(&corpus, &clock, "legifrance://code/code-civil/article/9").await.unwrap_err();

        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(corpus.calls_of("find_article"), 0);
    }

    #[tokio::test]
    async fn a_page_size_outside_the_reading_bounds_is_refused_not_served() {
        // Au-dessous du minimum de `max_chars` : le refus de la lecture remonte tel quel.
        let corpus = FakeCorpus::new();
        corpus.push_list_codes(Ok(codes()));
        corpus.push_find_article(Ok(article("Texte.", None)));
        let (settings, clock) = (settings(), clock());
        let mut use_case = ResourceUseCase::new(&corpus, &clock, &settings);
        use_case.page_chars = 1;
        let target = ResourceRef::parse("legifrance://code/code-civil/article/9").unwrap();

        let error = use_case.execute(target).await.unwrap_err();

        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        assert!(error.message().starts_with("max_chars=1 out of range"), "{error}");
    }

    #[test]
    fn a_nota_is_capped_like_the_tool_does() {
        let long_nota = "Phrase du nota. ".repeat(200);
        let output = read_article(article("Texte.", Some(&long_nota)), PAGE_CHARS).unwrap();
        let nota = output.nota.unwrap();
        let characters = nota.chars().count();
        assert!(characters <= NOTA_MAX_CHARS, "{characters}");
        assert!(nota.ends_with('.'), "{nota}");
        assert_eq!(output.article.text, LongText::default());
        assert!(output.article.nota.is_none());
        assert_eq!(output.reading.text.as_deref(), Some("Texte."));
    }

    #[test]
    fn the_page_sizes_follow_the_budget() {
        assert_eq!(usize::try_from(PAGE_CHARS).unwrap(), ResponseBudget::MAX_CHARS_MAX);
        assert_eq!(usize::try_from(TOC_ENTRIES).unwrap(), ResponseBudget::TOC_LIMIT_MAX);
    }
}
