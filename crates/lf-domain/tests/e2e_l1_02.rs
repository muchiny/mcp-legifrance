//! L1-02 : code orphelin aspiré par l'article suivant (E2E verify_citations-6, gap3-2).
//! Garde de bout en bout ; les tests de couverture vivent dans `src/citation/`.
use lf_domain::citation::{CitationRef, CodeRef, Subdivision, parse};

/// Pour chaque citation lue : son texte, le code et la clé d'article.
fn read(input: &str) -> Vec<(String, String, String)> {
    parse(input)
        .iter()
        .filter_map(|c| {
            let raw = input.get(c.span.start..c.span.end)?.to_owned();
            match &c.value {
                CitationRef::CodeArticle { code, article, .. } => Some((
                    raw,
                    match code {
                        CodeRef::Known(key) => format!("{key:?}"),
                        CodeRef::Missing => "Missing".to_owned(),
                        other => format!("{other:?}"),
                    },
                    article.num.num_key(),
                )),
                _ => None,
            }
        })
        .collect()
}

fn pairs(input: &str) -> Vec<(String, String)> {
    read(input).into_iter().map(|(_, code, key)| (code, key)).collect()
}

fn p(code: &str, key: &str) -> (String, String) {
    (code.to_owned(), key.to_owned())
}

#[test]
fn an_ordinal_alinea_keeps_its_code_and_the_next_article_keeps_its_own() {
    assert_eq!(
        pairs("Voir l'art. 9, al. 1er, C. pr. civ. ; art. 9 C. civ."),
        [p("CodeDeProcedureCivile", "9"), p("CodeCivil", "9")]
    );
    assert_eq!(
        pairs("Voir l'art. 9, al. 1er, C. civ. ; art. 9 C. pr. civ. et art. 1240 C. civ."),
        [p("CodeCivil", "9"), p("CodeDeProcedureCivile", "9"), p("CodeCivil", "1240")]
    );
}

#[test]
fn the_third_e2e_text_gives_six_citations_with_their_own_codes() {
    let input = "Les articles 1240 et 1241 du code civil ; art. 1240, al. 1er, C. civ. ; \
                 art. 1240 s. C. civ. ; article L. 1234-9, alinéa 2 du code du travail ; \
                 art. 1240 C. pén.";
    assert_eq!(
        pairs(input),
        [
            p("CodeCivil", "1240"),
            p("CodeCivil", "1241"),
            p("CodeCivil", "1240"),
            p("CodeCivil", "1240"),
            p("CodeDuTravail", "L1234-9"),
            p("CodePenal", "1240"),
        ]
    );
}

#[test]
fn a_code_is_never_a_false_verified_on_the_neighbour() {
    assert_eq!(
        pairs("art. 9, alinéa 1er du code civil ; art. 1240 du code pénal"),
        [p("CodeCivil", "9"), p("CodePenal", "1240")]
    );
}

#[test]
fn et_seq_keeps_the_code() {
    assert_eq!(pairs("art. 1240 et s. du code civil"), [p("CodeCivil", "1240")]);
    assert_eq!(
        pairs("art. 1103 et 1104 du code civil, art. 1217 s. C. civ."),
        [p("CodeCivil", "1103"), p("CodeCivil", "1104"), p("CodeCivil", "1217")]
    );
    assert_eq!(pairs("article 1240 et suivants du code civil"), [p("CodeCivil", "1240")]);
}

#[test]
fn the_alinea_is_an_alinea_not_a_truncated_raw() {
    let input = "art. 9, al. 1er, C. pr. civ.";
    assert_eq!(read(input)[0].0, input);
    let subs: Vec<Vec<Subdivision>> = parse("art. 9, al. 1er, C. civ.")
        .into_iter()
        .filter_map(|c| match c.value {
            CitationRef::CodeArticle { article, .. } => Some(article.sub),
            _ => None,
        })
        .collect();
    assert_eq!(subs, [vec![Subdivision::Alinea(1)]]);
}
