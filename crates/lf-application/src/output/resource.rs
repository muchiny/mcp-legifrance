//! Sortie de la lecture d'une resource MCP (v0.3, M7, 05 §8.2 et §8.3).
//!
//! Une lecture rend du Markdown, sauf `legifrance://coverage` dont le contenu est la vue JSON de
//! `fr_coverage` : l'application ne sérialise pas de JSON (02 §3.2), l'adapter MCP le fait et la
//! sortie lui remet la couverture calculée.
//!
//! Le Markdown est rendu ici, parce que la métrique d'audit est le nombre de caractères renvoyés
//! (00 §7) : l'application doit connaître le texte final. Les titres de section et les libellés
//! sont en anglais (Q14) ; le texte de la loi ou de la décision est celui de Légifrance.

use std::fmt::Write as _;

use lf_domain::{contract::Citable, entity::OfficialSummary};

use crate::{
    audited::AuditMetric,
    budget::ResponseBudget,
    output::{
        coverage::CoverageOutput, get_act::GetActOutput, get_act::TocLine,
        get_decision::GetDecisionOutput, get_text::GetTextOutput,
    },
    tools::ToolName,
};

/// Type MIME d'une resource Markdown.
const MARKDOWN: &str = "text/markdown";

/// Type MIME de la resource de couverture.
const JSON: &str = "application/json";

/// Le gabarit (ou la resource) lu : il nomme la ligne d'audit et le type MIME.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    /// `legifrance://article/{article_id}`.
    Article,
    /// `legifrance://decision/{decision_id}`.
    Decision,
    /// `legifrance://act/{text_id}`.
    Act,
    /// `legifrance://code/{code}/article/{num}`.
    CodeArticle,
    /// `legifrance://coverage`.
    Coverage,
}

impl ResourceKind {
    /// Les cinq gabarits, dans l'ordre de 05 §8.2.
    pub const ALL: [Self; 5] =
        [Self::Coverage, Self::Article, Self::Decision, Self::Act, Self::CodeArticle];

    /// Nom de la ligne d'audit : `resource:<nom du gabarit>` (00 §7).
    #[must_use]
    pub const fn audit_name(self) -> ToolName {
        match self {
            Self::Article => ToolName::RESOURCE_ARTICLE,
            Self::Decision => ToolName::RESOURCE_DECISION,
            Self::Act => ToolName::RESOURCE_ACT,
            Self::CodeArticle => ToolName::RESOURCE_CODE_ARTICLE,
            Self::Coverage => ToolName::RESOURCE_COVERAGE,
        }
    }

    /// Type MIME du contenu.
    #[must_use]
    pub const fn mime_type(self) -> &'static str {
        match self {
            Self::Coverage => JSON,
            Self::Article | Self::Decision | Self::Act | Self::CodeArticle => MARKDOWN,
        }
    }
}

/// Contenu lu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceContent {
    /// Le Markdown rendu (05 §8.3).
    Markdown(String),
    /// La couverture, que l'adapter MCP met en JSON (la vue de `fr_coverage`).
    Coverage(CoverageOutput),
}

/// Ce que `ResourceUseCase` rend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceOutput {
    /// Gabarit lu.
    pub kind: ResourceKind,
    /// Contenu lu.
    pub content: ResourceContent,
}

/// Métrique d'audit (00 §7) : les caractères renvoyés, soit ceux du Markdown. La couverture est
/// mise en JSON par l'adapter, que l'application ne peut pas mesurer : elle compte les lacunes,
/// comme `fr_coverage`.
impl AuditMetric for ResourceOutput {
    fn output_count_or_size(&self) -> u64 {
        match &self.content {
            ResourceContent::Markdown(text) => {
                u64::try_from(text.chars().count()).unwrap_or(u64::MAX)
            }
            ResourceContent::Coverage(coverage) => coverage.output_count_or_size(),
        }
    }
}

impl ResourceOutput {
    /// La couverture, pour `legifrance://coverage`.
    pub(crate) fn coverage(output: CoverageOutput) -> Self {
        Self { kind: ResourceKind::Coverage, content: ResourceContent::Coverage(output) }
    }

    /// Un article (`kind` : [`ResourceKind::Article`] ou [`ResourceKind::CodeArticle`]), en
    /// Markdown (05 §8.3). La lecture est celle de `fr_get_text` en texte plein, du début.
    pub(crate) fn article(kind: ResourceKind, output: &GetTextOutput) -> Self {
        let article = &output.article;
        let contract = article.contract();
        let mut md = header(contract.citation.as_str(), contract.source_url.as_str());
        let mut id_line = format!("- Id: {}", article.article_id);
        if let Some(etat) = &article.etat {
            let _ = write!(id_line, " · Status: {}", etat.as_api_code());
        }
        if let Some(start) = article.date_debut {
            let _ = write!(id_line, " · In force since {start}");
        }
        if let Some(end) = article.date_fin {
            let _ = write!(id_line, " · Until {end}");
        }
        md.push_str(&id_line);
        md.push('\n');
        if !article.path.is_empty() {
            let _ = writeln!(md, "- Location: {}", article.path.join(" > "));
        }
        md.push('\n');
        body(&mut md, output.reading.text.as_deref(), output.reading.note.as_deref());
        if let Some(nota) = &output.nota {
            let _ = write!(md, "\n\n## Nota\n\n{nota}");
        }
        truncation(&mut md, output.reading.next_offset, "fr_get_text");
        Self { kind, content: ResourceContent::Markdown(md) }
    }

    /// Une décision, en Markdown (05 §8.3) : métadonnées, sommaire officiel s'il existe, texte.
    pub(crate) fn decision(output: &GetDecisionOutput) -> Self {
        let decision = &output.decision;
        let contract = decision.contract();
        let mut md = header(contract.citation.as_str(), contract.source_url.as_str());
        let _ = writeln!(md, "- Id: {}", decision.decision_id);
        if let Some(ecli) = &contract.ecli {
            let _ = writeln!(md, "- ECLI: {}", ecli.as_str());
        }
        let court = [decision.juridiction.as_deref(), decision.formation.as_deref()];
        let court: Vec<&str> = court.into_iter().flatten().collect();
        if !court.is_empty() {
            let _ = writeln!(md, "- Court: {}", court.join(", "));
        }
        if let Some(date) = decision.date_texte {
            let _ = writeln!(md, "- Date: {date}");
        }
        if let Some(solution) = &decision.solution {
            let _ = writeln!(md, "- Solution: {solution}");
        }
        if !decision.numero_affaire.is_empty() {
            let _ = writeln!(md, "- Case numbers: {}", decision.numero_affaire.join(", "));
        }
        md.push('\n');
        official_summary(&mut md, &decision.official_summary);
        md.push_str("## Text\n\n");
        body(&mut md, output.reading.text.as_deref(), output.reading.note.as_deref());
        truncation(&mut md, output.reading.next_offset, "fr_get_decision");
        Self { kind: ResourceKind::Decision, content: ResourceContent::Markdown(md) }
    }

    /// Un texte consolidé, en Markdown (05 §8.3) : métadonnées puis table des matières imbriquée.
    pub(crate) fn act(output: &GetActOutput) -> Self {
        let act = &output.act;
        let contract = act.contract();
        let body = &act.body;
        let mut md = header(contract.citation.as_str(), contract.source_url.as_str());
        let mut id_line = format!("- Id: {}", act.text_id);
        if let Some(etat) = &body.etat {
            let _ = write!(id_line, " · Status: {}", etat.as_api_code());
        }
        let _ = write!(id_line, " · Version of {}", output.date);
        md.push_str(&id_line);
        md.push('\n');
        if let Some(nature) = &body.nature {
            let _ = writeln!(md, "- Nature: {nature}");
        }
        if let Some(reference) = &body.jo_reference {
            let _ = writeln!(md, "- Official journal: {reference}");
        }
        md.push_str("\n## Table of contents\n\n");
        if output.toc.is_empty() {
            md.push_str("No table of contents is available from Légifrance for this text.");
        }
        for line in &output.toc {
            toc_line(&mut md, line);
        }
        if let Some(next) = output.next_toc_offset {
            let shown = output.toc.len();
            let _ = write!(
                md,
                "\nTable of contents truncated at {shown} entries: use fr_get_act with \
                 toc_offset={next}."
            );
        }
        Self { kind: ResourceKind::Act, content: ResourceContent::Markdown(md) }
    }
}

/// Titre et ligne `Source` ; la ligne suivante est celle de l'identifiant.
fn header(citation: &str, source_url: &str) -> String {
    format!("# {citation}\n\n- Source: {source_url}\n")
}

/// Le texte lu, ou la remarque quand il n'y en a pas (texte vide).
fn body(md: &mut String, text: Option<&str>, note: Option<&str>) {
    md.push_str(text.or(note).unwrap_or_default());
}

/// La dernière ligne d'une lecture coupée : l'outil de lecture et le `offset` exact où reprendre
/// (la fenêtre finit en fin de phrase ou de mot, donc avant le plafond lui-même).
fn truncation(md: &mut String, next_offset: Option<u32>, tool: &str) {
    if let Some(next) = next_offset {
        let _ = write!(
            md,
            "\n\nText truncated at {} characters: use {tool} with detail=\"full\" and \
             offset={next}.",
            group_thousands(ResponseBudget::MAX_CHARS_MAX)
        );
    }
}

/// Un nombre avec une virgule tous les trois chiffres (`40,000`).
fn group_thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// La section `## Official summary` : les analyses en paragraphes, les titrages en liste.
fn official_summary(md: &mut String, summary: &OfficialSummary) {
    if summary.analyses.is_empty() && summary.titrages.is_empty() {
        return;
    }
    md.push_str("## Official summary\n\n");
    for analysis in &summary.analyses {
        let _ = write!(md, "{analysis}\n\n");
    }
    for keywords in &summary.titrages {
        let _ = writeln!(md, "- {keywords}");
    }
    if !summary.titrages.is_empty() {
        md.push('\n');
    }
}

/// Une entrée de table des matières : une liste imbriquée de deux espaces par niveau.
fn toc_line(md: &mut String, line: &TocLine) {
    match line {
        TocLine::Section { depth, title } => {
            let _ = writeln!(md, "{}- {title}", indent(*depth));
        }
        TocLine::Article { depth, article_id, num, etat: _ } => {
            let label = num.as_deref().map_or_else(String::new, |num| format!("Art. {num} "));
            let _ = writeln!(md, "{}- {label}({article_id})", indent(*depth));
        }
    }
}

/// Retrait d'une entrée de profondeur `depth` (1 au premier niveau).
fn indent(depth: u16) -> String {
    "  ".repeat(usize::from(depth.saturating_sub(1)))
}

#[cfg(test)]
mod tests {
    use lf_domain::{
        coverage::{ConfigFacts, coverage},
        date::{CivilDate, VersionDate},
        ecli::Ecli,
        entity::{Act, ArticleText, Decision, TextBody},
        fond::Fond,
        id::{ArticleId, DecisionId, LegiTextId},
        status::LegalStatus,
        text::LongText,
    };
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        audited::AuditMode, budget::Reading, output::coverage::CoverageConfiguration,
        ports::PisteEnvironment,
    };

    const ARTICLE_ID: &str = "LEGIARTI000006419288";
    const DECISION_ID: &str = "JURITEXT000037999394";
    const TEXT_ID: &str = "LEGITEXT000006070721";

    fn markdown(output: &ResourceOutput) -> &str {
        let ResourceContent::Markdown(text) = &output.content else { return "<not Markdown>" };
        text
    }

    fn reading(text: Option<&str>) -> Reading {
        Reading {
            summary: None,
            text: text.map(str::to_owned),
            text_length: 9_999,
            next_offset: None,
            passages: None,
            note: None,
        }
    }

    fn article() -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(ARTICLE_ID).unwrap(),
            num: Some("9".to_owned()),
            etat: Some(LegalStatus::Vigueur),
            code_title: Some("Code civil".to_owned()),
            parent: None,
            path: vec![
                "Livre Ier : Des personnes".to_owned(),
                "Titre Ier : Des droits civils".to_owned(),
            ],
            full_sections_titre: None,
            date_debut: CivilDate::new(1970, 7, 19),
            date_fin: None,
            text: LongText::default(),
            nota: None,
        }
    }

    fn article_output(reading: Reading) -> GetTextOutput {
        GetTextOutput { article: article(), nota: None, reading }
    }

    // ---- ResourceKind -----------------------------------------------------------------------

    #[test]
    fn kinds_name_their_audit_line_and_mime_type() {
        let expected = [
            (ResourceKind::Coverage, "resource:coverage", "application/json"),
            (ResourceKind::Article, "resource:article", "text/markdown"),
            (ResourceKind::Decision, "resource:decision", "text/markdown"),
            (ResourceKind::Act, "resource:act", "text/markdown"),
            (ResourceKind::CodeArticle, "resource:code_article", "text/markdown"),
        ];
        assert_eq!(expected.map(|(kind, _, _)| kind), ResourceKind::ALL);
        for (kind, name, mime) in expected {
            assert_eq!(kind.audit_name().as_str(), name);
            assert_eq!(kind.mime_type(), mime);
            // Une lecture de resource n'est jamais un outil du catalogue.
            assert!(!ToolName::ALL.contains(&kind.audit_name()));
        }
    }

    // ---- articles ---------------------------------------------------------------------------

    #[test]
    fn an_article_renders_the_documented_markdown() {
        let text = "Chacun a droit au respect de sa vie privée.";
        let output =
            ResourceOutput::article(ResourceKind::Article, &article_output(reading(Some(text))));

        assert_eq!(output.kind, ResourceKind::Article);
        assert_eq!(
            markdown(&output),
            "# Code civil, art. 9\n\
             \n\
             - Source: https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288\n\
             - Id: LEGIARTI000006419288 · Status: VIGUEUR · In force since 1970-07-19\n\
             - Location: Livre Ier : Des personnes > Titre Ier : Des droits civils\n\
             \n\
             Chacun a droit au respect de sa vie privée."
        );
        // Les caractères renvoyés : ceux du Markdown entier.
        assert_eq!(
            output.output_count_or_size(),
            u64::try_from(markdown(&output).chars().count()).unwrap()
        );
    }

    #[test]
    fn an_article_without_optional_fields_keeps_the_bare_lines() {
        let mut bare = article_output(reading(Some("Texte.")));
        bare.article.etat = None;
        bare.article.date_debut = None;
        bare.article.path = Vec::new();

        let output = ResourceOutput::article(ResourceKind::CodeArticle, &bare);

        assert_eq!(output.kind, ResourceKind::CodeArticle);
        assert_eq!(
            markdown(&output),
            "# Code civil, art. 9\n\
             \n\
             - Source: https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288\n\
             - Id: LEGIARTI000006419288\n\
             \n\
             Texte."
        );
    }

    #[test]
    fn an_ended_article_says_until_when_and_a_nota_follows_the_text() {
        let mut ended = article_output(reading(Some("Texte.")));
        ended.article.date_fin = CivilDate::new(2020, 1, 1);
        ended.nota = Some("Les dispositions entrent en vigueur le 1er janvier.".to_owned());

        let text = markdown(&ResourceOutput::article(ResourceKind::Article, &ended)).to_owned();

        assert!(text.contains("· In force since 1970-07-19 · Until 2020-01-01\n"), "{text}");
        assert!(
            text.ends_with(
                "Texte.\n\n## Nota\n\nLes dispositions entrent en vigueur le 1er janvier."
            ),
            "{text}"
        );
    }

    #[test]
    fn a_truncated_article_ends_with_the_exact_resume_offset() {
        let mut cut = article_output(reading(Some("Début du texte")));
        cut.reading.next_offset = Some(39_876);

        let text = markdown(&ResourceOutput::article(ResourceKind::Article, &cut)).to_owned();

        assert!(
            text.ends_with(
                "Début du texte\n\nText truncated at 40,000 characters: use fr_get_text with \
                 detail=\"full\" and offset=39876."
            ),
            "{text}"
        );
    }

    #[test]
    fn an_empty_article_gives_the_reading_note_instead_of_a_text() {
        let mut empty = article_output(reading(None));
        empty.reading.note =
            Some("No text is available from Légifrance for this record.".to_owned());
        let text = markdown(&ResourceOutput::article(ResourceKind::Article, &empty)).to_owned();
        assert!(
            text.ends_with("\n\nNo text is available from Légifrance for this record."),
            "{text}"
        );

        // Ni texte ni remarque : les métadonnées seules.
        let nothing = article_output(reading(None));
        let text = markdown(&ResourceOutput::article(ResourceKind::Article, &nothing)).to_owned();
        assert!(text.ends_with("Titre Ier : Des droits civils\n\n"), "{text}");
    }

    // ---- décisions --------------------------------------------------------------------------

    fn decision() -> Decision {
        Decision {
            decision_id: DecisionId::parse(DECISION_ID).unwrap(),
            ecli: Some(Ecli::parse("ECLI:FR:CCASS:2019:C100460").unwrap()),
            num: Some("2025-1173".to_owned()),
            juridiction: Some("Cour de cassation".to_owned()),
            formation: Some("Première chambre civile".to_owned()),
            solution: Some("Rejet".to_owned()),
            numero_affaire: vec!["24-13.470".to_owned(), "24-13.471".to_owned()],
            date_texte: CivilDate::new(2019, 6, 5),
            nature: None,
            title: Some(
                "Cour de cassation, civile, Chambre civile 1, 5 juin 2019, 18-12.345".to_owned(),
            ),
            text: LongText::default(),
            official_summary: OfficialSummary::new(
                ["La Cour rejette le pourvoi."],
                ["CONTRATS ET OBLIGATIONS - Bonne foi"],
            ),
            cited_case_law: None,
            applied_texts: vec![],
        }
    }

    #[test]
    fn a_decision_renders_metadata_summary_and_text() {
        let output = ResourceOutput::decision(&GetDecisionOutput {
            decision: decision(),
            reading: reading(Some("La Cour rejette le pourvoi.")),
        });

        assert_eq!(output.kind, ResourceKind::Decision);
        assert_eq!(
            markdown(&output),
            "# Cour de cassation, civile, Chambre civile 1, 5 juin 2019, 18-12.345\n\
             \n\
             - Source: https://www.legifrance.gouv.fr/juri/id/JURITEXT000037999394\n\
             - Id: JURITEXT000037999394\n\
             - ECLI: ECLI:FR:CCASS:2019:C100460\n\
             - Court: Cour de cassation, Première chambre civile\n\
             - Date: 2019-06-05\n\
             - Solution: Rejet\n\
             - Case numbers: 24-13.470, 24-13.471\n\
             \n\
             ## Official summary\n\
             \n\
             La Cour rejette le pourvoi.\n\
             \n\
             - CONTRATS ET OBLIGATIONS - Bonne foi\n\
             \n\
             ## Text\n\
             \n\
             La Cour rejette le pourvoi."
        );
    }

    #[test]
    fn a_decision_without_optional_fields_keeps_only_the_text_section() {
        let mut bare = decision();
        bare.ecli = None;
        bare.juridiction = None;
        bare.formation = Some("Formation seule".to_owned());
        bare.solution = None;
        bare.numero_affaire = Vec::new();
        bare.date_texte = None;
        bare.official_summary = OfficialSummary::default();
        let mut cut = reading(Some("Texte"));
        cut.next_offset = Some(100);

        let output = ResourceOutput::decision(&GetDecisionOutput { decision: bare, reading: cut });
        let text = markdown(&output);

        assert!(text.contains("- Court: Formation seule\n"), "{text}");
        for absent in ["ECLI", "Date:", "Solution", "Case numbers", "Official summary"] {
            assert!(!text.contains(absent), "{absent}: {text}");
        }
        assert!(
            text.ends_with(
                "## Text\n\nTexte\n\nText truncated at 40,000 characters: use fr_get_decision with \
                 detail=\"full\" and offset=100."
            ),
            "{text}"
        );
    }

    #[test]
    fn an_official_summary_with_only_one_kind_of_entry_renders_it_alone() {
        let mut only_keywords = decision();
        only_keywords.official_summary = OfficialSummary::new(Vec::<String>::new(), ["MOT-CLÉ"]);
        let text = markdown(&ResourceOutput::decision(&GetDecisionOutput {
            decision: only_keywords,
            reading: reading(Some("T")),
        }))
        .to_owned();
        assert!(text.contains("## Official summary\n\n- MOT-CLÉ\n\n## Text"), "{text}");

        let mut only_analyses = decision();
        only_analyses.official_summary = OfficialSummary::new(["Analyse."], Vec::<String>::new());
        let text = markdown(&ResourceOutput::decision(&GetDecisionOutput {
            decision: only_analyses,
            reading: reading(Some("T")),
        }))
        .to_owned();
        assert!(text.contains("## Official summary\n\nAnalyse.\n\n## Text"), "{text}");
    }

    // ---- textes consolidés ------------------------------------------------------------------

    fn act_output(toc: Vec<TocLine>, next_toc_offset: Option<u32>) -> GetActOutput {
        GetActOutput {
            act: Act {
                text_id: LegiTextId::parse(TEXT_ID).unwrap(),
                body: TextBody {
                    title: Some("Code civil".to_owned()),
                    nature: Some("CODE".to_owned()),
                    etat: Some(LegalStatus::Vigueur),
                    jo_reference: Some("JORF n°0235 du 8 octobre 2016".to_owned()),
                    ..TextBody::default()
                },
            },
            date: VersionDate::parse("2026-10-29").unwrap(),
            toc_total: u32::try_from(toc.len()).unwrap(),
            toc,
            next_toc_offset,
            note: None,
        }
    }

    #[test]
    fn an_act_renders_metadata_and_a_nested_table_of_contents() {
        let toc = vec![
            TocLine::Section { depth: 1, title: "Titre Ier".to_owned() },
            TocLine::Section { depth: 2, title: "Chapitre Ier".to_owned() },
            TocLine::Article {
                depth: 3,
                article_id: ArticleId::parse(ARTICLE_ID).unwrap(),
                num: Some("1".to_owned()),
                etat: None,
            },
            TocLine::Article {
                depth: 1,
                article_id: ArticleId::parse("LEGIARTI000006419289").unwrap(),
                num: None,
                etat: Some(LegalStatus::Abroge),
            },
        ];

        let output = ResourceOutput::act(&act_output(toc, None));

        assert_eq!(output.kind, ResourceKind::Act);
        assert_eq!(
            markdown(&output),
            "# Code civil\n\
             \n\
             - Source: https://www.legifrance.gouv.fr/codes/texte_lc/LEGITEXT000006070721\n\
             - Id: LEGITEXT000006070721 · Status: VIGUEUR · Version of 2026-10-29\n\
             - Nature: CODE\n\
             - Official journal: JORF n°0235 du 8 octobre 2016\n\
             \n\
             ## Table of contents\n\
             \n\
             - Titre Ier\n\
             \x20 - Chapitre Ier\n\
             \x20   - Art. 1 (LEGIARTI000006419288)\n\
             - (LEGIARTI000006419289)\n"
        );
    }

    #[test]
    fn a_cut_table_of_contents_says_where_to_resume() {
        let mut act = act_output(
            vec![TocLine::Section { depth: 1, title: "Titre Ier".to_owned() }],
            Some(500),
        );
        act.act.body.etat = None;
        act.act.body.nature = None;
        act.act.body.jo_reference = None;
        act.toc_total = 900;

        let text = markdown(&ResourceOutput::act(&act)).to_owned();

        assert!(text.contains("- Id: LEGITEXT000006070721 · Version of 2026-10-29\n"), "{text}");
        assert!(!text.contains("Nature"), "{text}");
        assert!(
            text.ends_with(
                "- Titre Ier\n\nTable of contents truncated at 1 entries: use fr_get_act with \
                 toc_offset=500."
            ),
            "{text}"
        );
    }

    #[test]
    fn an_act_without_table_of_contents_says_so() {
        let text = markdown(&ResourceOutput::act(&act_output(Vec::new(), None))).to_owned();
        assert!(
            text.ends_with(
                "## Table of contents\n\nNo table of contents is available from Légifrance for \
                 this text."
            ),
            "{text}"
        );
    }

    // ---- couverture et métrique -------------------------------------------------------------

    #[test]
    fn coverage_keeps_its_computed_output_and_counts_gaps() {
        let facts = ConfigFacts {
            credentials_configured: true,
            custom_host: false,
            has_problems: false,
            local_snapshot: false,
        };
        let coverage_output = CoverageOutput {
            coverage: coverage(PisteEnvironment::Sandbox, &facts, &Fond::ALL),
            environment: PisteEnvironment::Sandbox,
            configuration: CoverageConfiguration {
                credentials_configured: true,
                audit_mode: AuditMode::Strict,
                problems: Vec::new(),
                local_snapshot: false,
            },
            consulted_on: CivilDate::new(2026, 10, 29).unwrap(),
        };

        let output = ResourceOutput::coverage(coverage_output.clone());

        assert_eq!(output.kind, ResourceKind::Coverage);
        assert_eq!(output.content, ResourceContent::Coverage(coverage_output));
        assert_eq!(output.output_count_or_size(), 10);
        // Une couverture n'est pas du Markdown : l'adapter la met en JSON.
        assert_eq!(markdown(&output), "<not Markdown>");
    }

    #[test]
    fn group_thousands_writes_commas() {
        assert_eq!(group_thousands(0), "0");
        assert_eq!(group_thousands(999), "999");
        assert_eq!(group_thousands(1_000), "1,000");
        assert_eq!(group_thousands(40_000), "40,000");
        assert_eq!(group_thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn the_truncation_line_follows_the_hard_cap_of_a_reading() {
        // « 40,000 » est `max_chars` maximal d'une lecture : s'il change, le texte suit.
        assert_eq!(group_thousands(ResponseBudget::MAX_CHARS_MAX), "40,000");
    }
}
