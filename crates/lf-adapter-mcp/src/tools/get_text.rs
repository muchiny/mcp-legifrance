//! Outil `fr_get_text` (05 §5.3) : un article de code, de loi ou de convention, avec sa citation, son
//! statut, ses dates de validité et sa place dans le texte, lu en résumé, en fenêtres de texte
//! (`offset`, `next_offset`) ou en passages (`focus`). Le `#[tool]` suit le motif de `tools/mod.rs`.

use lf_application::{
    input::GetTextInput,
    ports::{AuditLog, Clock, LegalCorpus},
};
use rmcp::{Json, model::JsonObject};
use rmcp_macros::{tool, tool_router};
use tokio_util::sync::CancellationToken;

use crate::{
    decode::to_call,
    failure::ToolFailure,
    params::get_text::GetTextParams,
    schema::{input_schema, output_schema},
    server::LegifranceServer,
    views::get_text::ArticleView,
};

/// Description du descripteur, mot pour mot (05 §5.3 ; 733 caractères depuis que « e.g. » est écrit
/// « for example », qui ne compte plus pour une fin de phrase dans le contrôle de `xtask`).
const DESCRIPTION: &str = "Read one article by id: LEGIARTI (from fr_get_act or a CODE search hit) or KALIARTI (from fr_get_convention). Returns its citation (for example \"Code civil, art. 9\"), source_url, status, validity dates and parent code or text. By default (`detail`=\"summary\") a short article comes back whole and a long one is cut after about 1,200 characters; `detail`=\"full\" returns up to `max_chars` (default 4,000) from `offset`, and `next_offset` tells where to continue. `focus` returns the most relevant passages instead (default 3). Each LEGIARTI id is one version of the article: `date_debut` and `date_fin` bound its validity; without `date_fin` it has no end date, and without `date_debut` its entry into force is not set yet (see `etat`).";

#[tool_router(router = get_text_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog, K: Clock> LegifranceServer<C, A, K> {
    #[tool(
        name = "fr_get_text",
        title = "Read an article",
        description = DESCRIPTION,
        input_schema = input_schema::<GetTextParams>(),
        output_schema = output_schema::<ArticleView>(),
        annotations(
            title = "Read an article",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fr_get_text(
        &self,
        args: JsonObject,
        ct: CancellationToken,
    ) -> Result<Json<ArticleView>, ToolFailure> {
        let call = to_call::<GetTextParams, GetTextInput>(args);
        let output = self.service.get_text(call, ct.cancelled()).await.map_err(ToolFailure)?;
        Ok(Json(ArticleView::from(output)))
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::Fixture;
    use lf_application::{ports::AuditStatus, testing::PortRequest, tools::ToolName};
    use lf_domain::{
        date::CivilDate,
        entity::{ArticleText, ParentText},
        id::{ArticleId, DocumentId},
        status::LegalStatus,
        text::LongText,
    };
    use pretty_assertions::assert_eq;
    use rmcp::model::{CallToolRequestParams, CallToolResult};
    use serde_json::{Value, json};

    const ID: &str = "LEGIARTI000006419288";
    const DESCRIPTION: &str = "Read one article by id: LEGIARTI (from fr_get_act or a CODE search hit) or KALIARTI (from fr_get_convention). Returns its citation (for example \"Code civil, art. 9\"), source_url, status, validity dates and parent code or text. By default (`detail`=\"summary\") a short article comes back whole and a long one is cut after about 1,200 characters; `detail`=\"full\" returns up to `max_chars` (default 4,000) from `offset`, and `next_offset` tells where to continue. `focus` returns the most relevant passages instead (default 3). Each LEGIARTI id is one version of the article: `date_debut` and `date_fin` bound its validity; without `date_fin` it has no end date, and without `date_debut` its entry into force is not set yet (see `etat`).";
    const PRIVACY: &str = "Chacun a droit au respect de sa vie privée. Les juges peuvent, sans préjudice de la réparation du dommage subi, prescrire toutes mesures, telles que séquestre, saisie et autres, propres à empêcher ou faire cesser une atteinte à l'intimité de la vie privée : ces mesures peuvent, s'il y a urgence, être ordonnées en référé.";

    fn article(text: &str) -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(ID).unwrap(),
            num: Some("9".to_owned()),
            etat: LegalStatus::parse("VIGUEUR"),
            code_title: Some("Code civil".to_owned()),
            parent: Some(ParentText::new(
                DocumentId::parse("LEGITEXT000006070721").unwrap(),
                Some("CODE"),
            )),
            path: vec!["Livre Ier : Des personnes".to_owned()],
            full_sections_titre: None,
            date_debut: CivilDate::new(1970, 7, 19),
            date_fin: None,
            text: LongText::from_plain(text).unwrap(),
            nota: None,
        }
    }

    async fn call(fixture: &Fixture, arguments: &Value) -> CallToolResult {
        let client = fixture.connect().await;
        let mut params = CallToolRequestParams::new("fr_get_text");
        params.arguments = arguments.as_object().cloned();
        client.call_tool(params).await.unwrap()
    }

    fn text_of(result: &CallToolResult) -> &str {
        &result.content[0].as_text().unwrap().text
    }

    /// Un texte de `sentences` phrases de 74 caractères, en un seul paragraphe.
    fn prose(sentences: usize) -> String {
        "Considérant que l'article L. 1-1 du code civil prévoit une règle précise. "
            .repeat(sentences)
    }

    #[tokio::test]
    async fn the_descriptor_announces_title_description_hints_and_schemas() {
        let client = Fixture::default().connect().await;

        let tools = client.list_all_tools().await.unwrap();

        let tool = tools.iter().find(|tool| tool.name == "fr_get_text").expect("fr_get_text");
        assert_eq!(tool.title.as_deref(), Some("Read an article"));
        assert_eq!(tool.description.as_deref(), Some(DESCRIPTION));
        assert_eq!(DESCRIPTION.chars().count(), 733);
        let hints = tool.annotations.as_ref().unwrap();
        assert_eq!(hints.title.as_deref(), Some("Read an article"));
        assert_eq!(
            (
                hints.read_only_hint,
                hints.destructive_hint,
                hints.idempotent_hint,
                hints.open_world_hint
            ),
            (Some(true), Some(false), Some(true), Some(true))
        );
        assert_eq!(tool.input_schema["required"], json!(["article_id"]));
        assert_eq!(tool.input_schema["additionalProperties"], false);
        assert_eq!(tool.input_schema["properties"]["max_chars"]["maximum"], 40_000);
        let output = tool.output_schema.as_ref().unwrap();
        assert_eq!(output["type"], "object");
        assert!(output.get("$schema").is_none());
    }

    #[tokio::test]
    async fn a_call_returns_the_view_and_writes_one_audit_line() {
        let fixture = Fixture::default();
        fixture.corpus.push_get_article(Ok(article(PRIVACY)));

        let result = call(&fixture, &json!({"article_id": ID})).await;

        assert_eq!(result.is_error, Some(false));
        let view = result.structured_content.clone().unwrap();
        assert_eq!(
            view,
            json!({
                "article_id": ID,
                "num": "9",
                "etat": "VIGUEUR",
                "code_title": "Code civil",
                "parent_id": "LEGITEXT000006070721",
                "path": ["Livre Ier : Des personnes"],
                "date_debut": "1970-07-19",
                "citation": "Code civil, art. 9",
                "source_url": format!("https://www.legifrance.gouv.fr/codes/article_lc/{ID}"),
                "text": PRIVACY,
                "text_length": 322,
            })
        );
        // La même vue sert de texte (parité des clients sans `structuredContent`).
        assert_eq!(serde_json::from_str::<Value>(text_of(&result)).unwrap(), view);
        assert_eq!(
            fixture.corpus.seen_requests(),
            [PortRequest::GetArticle(ArticleId::parse(ID).unwrap())]
        );
        let records = fixture.audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool, ToolName::FR_GET_TEXT);
        assert_eq!(records[0].status, AuditStatus::Ok);
        assert_eq!(records[0].output_count_or_size, 322);
    }

    #[tokio::test]
    async fn a_kali_article_is_read_with_its_convention_as_parent() {
        let fixture = Fixture::default();
        let mut kali = article("Le salaire minimum est fixé par accord.");
        kali.article_id = ArticleId::parse("KALIARTI000005805295").unwrap();
        kali.parent =
            Some(ParentText::new(DocumentId::parse("KALITEXT000005635534").unwrap(), None));
        fixture.corpus.push_get_article(Ok(kali));

        let result = call(&fixture, &json!({"article_id": "KALIARTI000005805295"})).await;

        let view = result.structured_content.unwrap();
        assert_eq!(view["parent_id"], "KALITEXT000005635534");
        assert_eq!(
            view["source_url"],
            "https://www.legifrance.gouv.fr/conv_coll/id/KALIARTI000005805295"
        );
    }

    #[tokio::test]
    async fn sentinel_dates_omitted() {
        let fixture = Fixture::default();
        let mut sentinel = article(PRIVACY);
        // Zone des sentinelles de l'API : début « non déterminé », fin « sans date de fin ».
        sentinel.date_debut = CivilDate::new(2222, 2, 22);
        sentinel.date_fin = CivilDate::new(2999, 1, 1);
        fixture.corpus.push_get_article(Ok(sentinel));
        let mut ended = article(PRIVACY);
        ended.date_fin = CivilDate::new(2023, 9, 7);
        fixture.corpus.push_get_article(Ok(ended));

        let open = call(&fixture, &json!({"article_id": ID})).await;
        let closed = call(&fixture, &json!({"article_id": ID})).await;

        let open = open.structured_content.unwrap();
        assert!(open.get("date_debut").is_none() && open.get("date_fin").is_none(), "{open}");
        // Une vraie date reste rendue.
        let closed = closed.structured_content.unwrap();
        assert_eq!(closed["date_debut"], "1970-07-19");
        assert_eq!(closed["date_fin"], "2023-09-07");
    }

    #[tokio::test]
    async fn paged_reading_end_to_end() {
        let fixture = Fixture::default();
        let long = prose(120);
        let canonical = long.trim_end().to_owned();
        let mut rebuilt = String::new();
        let mut offset = 0;
        let mut pages = 0;
        loop {
            fixture.corpus.push_get_article(Ok(article(&long)));
            let arguments = if pages == 0 {
                json!({"article_id": ID, "detail": "full", "max_chars": 1000})
            } else {
                json!({"article_id": ID, "offset": offset, "max_chars": 1000})
            };

            let result = call(&fixture, &arguments).await;

            assert_eq!(result.is_error, Some(false));
            let view = result.structured_content.unwrap();
            assert_eq!(view["text_length"], canonical.chars().count());
            let page = view["text"].as_str().unwrap();
            assert!(page.chars().count() <= 1_000);
            rebuilt.push_str(page);
            pages += 1;
            match view.get("next_offset") {
                // `next_offset` est repassé tel quel dans `offset`, sans `detail` : `full` est déduit.
                Some(next) => offset = next.as_u64().unwrap(),
                None => break,
            }
            assert!(pages < 40, "the reading does not end");
        }

        // Les pages se partitionnent : leur concaténation est le texte entier, sans perte.
        assert_eq!(rebuilt, canonical);
        assert!(pages >= 8, "{pages}");
        let records = fixture.audit.records();
        assert_eq!(records.len(), pages);
        assert!(records.iter().all(|record| record.status == AuditStatus::Ok));
        assert!(records.iter().all(|record| record.tool == ToolName::FR_GET_TEXT));
    }

    #[tokio::test]
    async fn focus_returns_passages_with_their_offsets_over_the_wire() {
        let fixture = Fixture::default();
        let rent = "Le locataire doit payer le loyer au bailleur à la date convenue.";
        let notice = "Le préavis de départ est de trois mois, sauf en zone tendue.";
        fixture.corpus.push_get_article(Ok(article(&format!("{rent}\n{notice}"))));

        let result = call(&fixture, &json!({"article_id": ID, "focus": "préavis"})).await;

        let view = result.structured_content.unwrap();
        assert_eq!(view["passages"], json!([{"offset": rent.chars().count() + 2, "text": notice}]));
        assert!(view.get("text").is_none() && view.get("summary").is_none());
        // Le texte rendu au modèle est celui des passages : la métrique compte leurs caractères.
        assert_eq!(
            fixture.audit.records()[0].output_count_or_size,
            u64::try_from(notice.chars().count()).unwrap()
        );
    }

    #[tokio::test]
    async fn a_long_article_comes_back_as_a_lead_summary() {
        let fixture = Fixture::default();
        fixture.corpus.push_get_article(Ok(article(&prose(40))));

        let result = call(&fixture, &json!({"article_id": ID})).await;

        let view = result.structured_content.unwrap();
        assert_eq!(view["summary_source"], "lead");
        assert!(view["summary"].as_str().unwrap().chars().count() <= 1_200);
        assert!(view.get("text").is_none() && view.get("next_offset").is_none());
        assert_eq!(view["text_length"], prose(40).trim_end().chars().count());
    }

    #[tokio::test]
    async fn a_bad_argument_is_an_in_band_invalid_arg_that_is_audited_without_network() {
        let fixture = Fixture::default();

        let unknown = call(&fixture, &json!({"article_id": ID, "foo": 1})).await;
        let missing = call(&fixture, &json!({})).await;
        let mistyped = call(&fixture, &json!({"article_id": ID, "offset": "x"})).await;
        let focus = call(&fixture, &json!({"article_id": ID, "focus": "le de la"})).await;
        let whole = call(&fixture, &json!({"article_id": "LEGITEXT000006070721"})).await;

        for result in [&unknown, &missing, &mistyped, &focus, &whole] {
            assert_eq!(result.is_error, Some(true));
            assert!(result.structured_content.is_none());
        }
        assert_eq!(
            text_of(&unknown),
            "[invalid_arg] foo: unknown field `foo`, expected one of `article_id`, `detail`, \
             `offset`, `max_chars`, `focus`, `passages`"
        );
        assert_eq!(text_of(&missing), "[invalid_arg] article_id is required.");
        assert_eq!(
            text_of(&mistyped),
            "[invalid_arg] offset: invalid value: string \"x\", expected an integer"
        );
        assert_eq!(
            text_of(&focus),
            "[invalid_arg] focus contains no searchable word (only stop words or punctuation); \
             give a few meaningful words or omit focus."
        );
        assert!(text_of(&whole).starts_with(
            "[invalid_arg] article_id='LEGITEXT000006070721' must be a Legifrance LEGIARTI... or \
             KALIARTI... identifier."
        ));
        assert_eq!(fixture.corpus.calls(), 0);
        let records = fixture.audit.records();
        assert_eq!(records.len(), 5);
        assert!(records.iter().all(|record| record.status == AuditStatus::Error));
        assert!(records.iter().all(|record| record.output_count_or_size == 0));
    }

    #[tokio::test]
    async fn an_unknown_article_is_an_in_band_not_found() {
        let fixture = Fixture::default();
        fixture.corpus.push_get_article(Err(lf_application::ports::CorpusError::NotFound {
            what: "x".to_owned(),
        }));

        let result = call(&fixture, &json!({"article_id": ID})).await;

        assert_eq!(result.is_error, Some(true));
        assert_eq!(
            text_of(&result),
            format!(
                "[not_found] No article found for {ID}. Check the id, or find the article with \
                 fr_get_act or fr_search."
            )
        );
    }

    #[tokio::test]
    async fn an_offset_past_the_end_is_an_in_band_invalid_arg() {
        let fixture = Fixture::default();
        fixture.corpus.push_get_article(Ok(article(PRIVACY)));

        let result = call(&fixture, &json!({"article_id": ID, "offset": 322})).await;

        assert_eq!(result.is_error, Some(true));
        assert_eq!(
            text_of(&result),
            "[invalid_arg] offset=322 is at or past the end of the text (text_length=322); restart from \
             offset=0 or omit offset."
        );
    }

    #[tokio::test]
    async fn an_escape_rich_window_stays_under_the_hard_cap_with_an_exact_next_offset() {
        let fixture = Fixture::default();
        // Guillemets, antislashs et emoji hors BMP : les échappements JSON doublent la taille.
        let heavy = "\"\\😀 ".repeat(10_000);
        fixture.corpus.push_get_article(Ok(article(&heavy)));

        let result =
            call(&fixture, &json!({"article_id": ID, "detail": "full", "max_chars": 40_000})).await;

        assert_eq!(result.is_error, Some(false));
        let view = result.structured_content.unwrap();
        let size = view.to_string().chars().count();
        assert!(size <= 45_000, "{size}");
        let kept = view["text"].as_str().unwrap().to_owned();
        assert!(kept.chars().count() < 40_000);
        // La page suivante commence exactement où la fenêtre raccourcie s'arrête.
        assert_eq!(view["next_offset"], kept.chars().count());
        let canonical: String = heavy.trim_end().to_owned();
        assert!(canonical.starts_with(&kept));
        assert_eq!(view["text_length"], canonical.chars().count());
    }
}
