//! Grammaire du bloc de constitutionnalité (annexe B §4.4) : articles de la Constitution de
//! 1958, de la Déclaration de 1789, du Préambule de 1946 et de la Charte de l'environnement.
//!
//! Les plages des candidats sont des offsets du texte d'ORIGINE (`Normalized::orig_range`),
//! celles des ancres des offsets de `Normalized::text`.
//!
//! - Chaque article est un `TextArticle` de nature `CONSTITUTION`, `DDHC`, `PREAMBULE_1946` ou
//!   `CHARTE_ENVIRONNEMENT`. La date du texte est implicite quand elle est fixe (1958-10-04,
//!   1789-08-26, 1946-10-27) ; la Charte n'en porte pas.
//! - Un alinéa du Préambule n'a pas de numéro d'article : `ArticleNum::special = SansNumero` et
//!   `sub = [alinea N]` (P085).
//! - « Son article 16 » n'a pas de texte : `TextArticle { text: None }` avec l'ambiguïté
//!   `texte_manquant`. L'arbitrage le rattache au dernier texte cité (§4.15) ou le laisse tel
//!   quel.

use crate::citation::normalize::Normalized;
use crate::citation::scan::{Anchor, AnchorClass};
use crate::citation::types::{
    Ambiguity, ArticleNum, CitationRef, Confidence, SpecialNum, Subdivision, TextCite, TextNature,
};
use crate::date::CivilDate;

use super::Candidate;
use super::codes::{self, ArtList, Hit, Item, Ordinal, Tail, abbr, boundary, ordinal, word, ws};

/// Un texte du bloc de constitutionnalité.
fn block_cite(nature: TextNature, date: Option<CivilDate>) -> TextCite {
    TextCite {
        nature,
        number: None,
        date,
        date_raw: None,
        title_hint: None,
        anaphora: None,
        appellation: None,
        emitter: None,
    }
}

/// `[ WS ( dated | undated ) ]` : fin de la date écrite après le titre, s'il y en a une.
fn optional_date(text: &str, at: usize, forms: &[&str]) -> usize {
    ws(text, at).and_then(|pos| forms.iter().find_map(|form| word(text, pos, form))).unwrap_or(at)
}

/// `constitution` : « Constitution [du 4 octobre 1958 | de 1958] » ou « Const. ».
fn constitution(text: &str, at: usize) -> Option<(TextCite, usize)> {
    let end = word(text, at, "constitution")
        .map(|end| optional_date(text, end, &["du 4 octobre 1958", "de 1958"]))
        .or_else(|| abbr(text, at, "const"))?;
    Some((block_cite(TextNature::Constitution, CivilDate::new(1958, 10, 4)), end))
}

/// `ddhc` : le titre complet, « Déclaration de 1789 » ou le sigle `DDHC`.
fn ddhc(text: &str, at: usize) -> Option<(TextCite, usize)> {
    let end = word(text, at, "declaration des droits de l'homme et du citoyen")
        .map(|end| optional_date(text, end, &["du 26 aout 1789", "de 1789"]))
        .or_else(|| word(text, at, "declaration de 1789"))
        .or_else(|| {
            let end = at + "DDHC".len();
            let sigle = text.get(at..).unwrap_or_default().starts_with("DDHC");
            sigle.then_some(end).filter(|&end| boundary(text, end))
        })?;
    Some((block_cite(TextNature::Ddhc, CivilDate::new(1789, 8, 26)), end))
}

/// `preambule46` : « Préambule de la Constitution du 27 octobre 1946 » ou « de 1946 ».
fn preambule(text: &str, at: usize) -> Option<(TextCite, usize)> {
    let end = [
        "preambule de la constitution du 27 octobre 1946",
        "preambule de la constitution de 1946",
        "preambule de 1946",
    ]
    .iter()
    .find_map(|form| word(text, at, form))?;
    Some((block_cite(TextNature::Preambule1946, CivilDate::new(1946, 10, 27)), end))
}

/// `charte_env` : « Charte de l'environnement [de 2004] ».
fn charte(text: &str, at: usize) -> Option<(TextCite, usize)> {
    let end = word(text, at, "charte de l'environnement")
        .map(|end| optional_date(text, end, &["de 2004"]))?;
    Some((block_cite(TextNature::CharteEnvironnement, None), end))
}

/// `const_block` : l'un des quatre textes.
pub(super) fn const_block(text: &str, at: usize) -> Option<(TextCite, usize)> {
    constitution(text, at)
        .or_else(|| ddhc(text, at))
        .or_else(|| preambule(text, at))
        .or_else(|| charte(text, at))
}

/// Un article qui n'a pas de numéro : l'alinéa d'un préambule.
fn unnumbered() -> ArticleNum {
    ArticleNum {
        prefix: None,
        special: Some(SpecialNum::SansNumero),
        base: Vec::new(),
        latin: None,
        letters: None,
    }
}

/// `[sub_lead] art_kw art_list link const_block` et `art_kw art_list WS 'DDHC'`.
fn article_of_block(text: &str, start: usize) -> Option<Hit> {
    let (lead, pos) = codes::sub_lead(text, start).unwrap_or_else(|| (Vec::new(), start));
    let list = codes::art_list(text, codes::art_kw(text, pos)?)?;
    let after = ws(text, list.end)?;
    let (cite, end) =
        codes::link(text, after, true)
            .and_then(|target| const_block(text, target))
            .or_else(|| ddhc(text, after).filter(|(_, end)| *end - after == "DDHC".len()))?;
    let tail = Tail {
        span_end: end,
        group_end: end,
        confidence: Confidence::High,
        ambiguity: None,
        flags: Vec::new(),
        version: None,
    };
    let make =
        |article, _| CitationRef::TextArticle { text: Some(cite.clone()), article, at_date: None };
    Some(Hit { end, found: codes::article_founds(start, &lead, &list, &tail, &make) })
}

/// `( ORD_WORD | INT "e" ) WS "alinéa" WS "du" WS preambule46` (P085).
fn preambule_paragraph(text: &str, start: usize) -> Option<Hit> {
    let (rank, end) = ordinal(text, start)?;
    let end = word(text, ws(text, end)?, "alinea")?;
    let (cite, end) = preambule(text, ws(text, word(text, ws(text, end)?, "du")?)?)?;
    let alinea = match rank {
        Ordinal::Nth(n) => Subdivision::Alinea(n),
        Ordinal::Last => Subdivision::DernierAlinea,
    };
    let item = Item {
        num: unnumbered(),
        range_to: None,
        span: start..end,
        subs: vec![alinea],
        version: None,
        end,
    };
    let list = ArtList { items: vec![item], end };
    let tail = Tail {
        span_end: end,
        group_end: end,
        confidence: Confidence::High,
        ambiguity: None,
        flags: Vec::new(),
        version: None,
    };
    let make =
        |article, _| CitationRef::TextArticle { text: Some(cite.clone()), article, at_date: None };
    Some(Hit { end, found: codes::article_founds(start, &[], &list, &tail, &make) })
}

/// `anaphore_txt` : « Son article 16 », sans texte nommé.
fn son_article(text: &str, start: usize) -> Option<Hit> {
    let after = ws(text, word(text, start, "son")?)?;
    let item = codes::art_item(text, ws(text, word(text, after, "article")?)?)?;
    let end = item.end;
    let list = ArtList { items: vec![item], end };
    let tail = Tail {
        span_end: end,
        group_end: end,
        confidence: Confidence::High,
        ambiguity: Some(Ambiguity::TexteManquant),
        flags: Vec::new(),
        version: None,
    };
    let make = |article, _| CitationRef::TextArticle { text: None, article, at_date: None };
    Some(Hit { end, found: codes::article_founds(start, &[], &list, &tail, &make) })
}

fn block_article_at(text: &str, start: usize) -> Option<Hit> {
    article_of_block(text, start)
        .or_else(|| preambule_paragraph(text, start))
        .or_else(|| son_article(text, start))
}

/// Candidats de la famille, tirés des ancres compatibles avec ses grammaires.
pub(crate) fn scan(n: &Normalized, anchors: &[Anchor]) -> Vec<Candidate> {
    codes::drive(
        n,
        anchors,
        &[AnchorClass::ArticleKeyword, AnchorClass::Family, AnchorClass::Sigle],
        block_article_at,
    )
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use pretty_assertions::assert_eq;

    use super::super::codes::test_support::{Row, anchors, check_rows, foreign, norm, tail};
    use super::*;
    use crate::citation::scan::AnchorClass::{self, ArticleKeyword};

    const ART: (AnchorClass, &str) = (ArticleKeyword, "article");
    const ART_DOT: (AnchorClass, &str) = (ArticleKeyword, "art.");
    const ALINEA: (AnchorClass, &str) = (ArticleKeyword, "alinéa");
    const SON: (AnchorClass, &str) = (ArticleKeyword, "son");
    const ONZIEME: (AnchorClass, &str) = (ArticleKeyword, "onzième");

    fn cite_label(t: &TextCite) -> String {
        format!("{} {}", t.nature.code(), t.date.map_or_else(|| "-".to_owned(), |d| d.to_string()))
    }

    fn show(c: &Candidate) -> String {
        let CitationRef::TextArticle { text, article, at_date } = &c.value else {
            return format!("autre : {:?}", c.value);
        };
        let key = article.num.num_key();
        let mut s = format!("Art {}", if key.is_empty() { "-" } else { &key });
        if !article.sub.is_empty() {
            let _ = write!(s, " sub={:?}", article.sub);
        }
        assert_eq!(*at_date, None, "pas de date de version dans le bloc");
        s + &format!(" of {}", text.as_ref().map_or_else(|| "none".to_owned(), cite_label))
            + &tail(c)
    }

    fn check(rows: &[Row<'_>]) {
        check_rows(scan, show, rows);
    }

    #[test]
    fn articles_of_the_constitutional_block_p080_to_p086() {
        check(&[
            (
                "dans les conditions prévues à l\u{2019}article 61-1 de la Constitution, d\u{2019}une question prioritaire",
                &[ART],
                &[("article 61-1 de la Constitution", "Art 61-1 of CONSTITUTION 1958-10-04")],
            ),
            (
                "article 54 de la Constitution",
                &[ART],
                &[("article 54 de la Constitution", "Art 54 of CONSTITUTION 1958-10-04")],
            ),
            (
                "il résulte de la combinaison des articles 6, 8, 9 et 16 de la Déclaration des droits de l\u{2019}homme et du citoyen de 1789 que",
                &[ART],
                &[
                    ("6", "Art 6 of DDHC 1789-08-26 grp"),
                    ("8", "Art 8 of DDHC 1789-08-26 grp"),
                    ("9", "Art 9 of DDHC 1789-08-26 grp"),
                    ("16", "Art 16 of DDHC 1789-08-26 grp"),
                ],
            ),
            (
                "En second lieu, selon l\u{2019}article 6 de la Déclaration de 1789, la loi est",
                &[ART],
                &[("article 6 de la Déclaration de 1789", "Art 6 of DDHC 1789-08-26")],
            ),
            ("art. 16 DDHC", &[ART_DOT], &[("art. 16 DDHC", "Art 16 of DDHC 1789-08-26")]),
            (
                "le onzième alinéa du Préambule de la Constitution du 27 octobre 1946",
                &[ALINEA],
                &[(
                    "onzième alinéa du Préambule de la Constitution du 27 octobre 1946",
                    "Art - sub=[Alinea(11)] of PREAMBULE_1946 1946-10-27",
                )],
            ),
            (
                "l'article 5 de la Charte de l'environnement",
                &[ART],
                &[("article 5 de la Charte de l'environnement", "Art 5 of CHARTE_ENVIRONNEMENT -")],
            ),
        ]);
    }

    #[test]
    fn written_dates_and_other_forms_of_the_block_titles() {
        check(&[
            (
                "article 61-1 de la Const.",
                &[ART],
                &[("article 61-1 de la Const.", "Art 61-1 of CONSTITUTION 1958-10-04")],
            ),
            (
                "article 6 de la Constitution du 4 octobre 1958",
                &[ART],
                &[(
                    "article 6 de la Constitution du 4 octobre 1958",
                    "Art 6 of CONSTITUTION 1958-10-04",
                )],
            ),
            (
                "article 6 de la Constitution de 1958",
                &[ART],
                &[("article 6 de la Constitution de 1958", "Art 6 of CONSTITUTION 1958-10-04")],
            ),
            (
                "article 6 de la Déclaration des droits de l'homme et du citoyen du 26 août 1789",
                &[ART],
                &[(
                    "article 6 de la Déclaration des droits de l'homme et du citoyen du 26 août 1789",
                    "Art 6 of DDHC 1789-08-26",
                )],
            ),
            (
                "article 3 du Préambule de la Constitution de 1946",
                &[ART],
                &[(
                    "article 3 du Préambule de la Constitution de 1946",
                    "Art 3 of PREAMBULE_1946 1946-10-27",
                )],
            ),
            (
                "article 3 du Préambule de 1946",
                &[ART],
                &[("article 3 du Préambule de 1946", "Art 3 of PREAMBULE_1946 1946-10-27")],
            ),
            (
                "article 1er de la Charte de l'environnement de 2004",
                &[ART],
                &[(
                    "article 1er de la Charte de l'environnement de 2004",
                    "Art 1er of CHARTE_ENVIRONNEMENT -",
                )],
            ),
            (
                "le 3° de l'article 61-1 de la Constitution",
                &[ART],
                &[(
                    "3° de l'article 61-1 de la Constitution",
                    "Art 61-1 sub=[Item(3)] of CONSTITUTION 1958-10-04",
                )],
            ),
            (
                "le dernier alinéa du Préambule de 1946",
                &[ALINEA],
                &[(
                    "dernier alinéa du Préambule de 1946",
                    "Art - sub=[DernierAlinea] of PREAMBULE_1946 1946-10-27",
                )],
            ),
            (
                "le 11e alinéa du Préambule de la Constitution de 1946",
                &[ALINEA],
                &[(
                    "11e alinéa du Préambule de la Constitution de 1946",
                    "Art - sub=[Alinea(11)] of PREAMBULE_1946 1946-10-27",
                )],
            ),
        ]);
    }

    #[test]
    fn other_families_are_reported_as_such() {
        assert!(show(&foreign()).starts_with("autre : Unsupported"));
    }

    #[test]
    fn his_article_has_no_text() {
        check(&[
            (
                "Son article 16 dispose que",
                &[ART],
                &[("Son article 16", "Art 16 of none amb=texte_manquant")],
            ),
            (
                "Cette loi et son article 3, alinéa 2",
                &[ART],
                &[("son article 3, alinéa 2", "Art 3 sub=[Alinea(2)] of none amb=texte_manquant")],
            ),
        ]);
    }

    #[test]
    fn other_texts_are_not_the_block() {
        check(&[
            ("article 6 de la Constitutionnelle", &[ART], &[]),
            ("article 6 du Conseil constitutionnel", &[ART], &[]),
            ("article 6 de la Déclaration universelle", &[ART], &[]),
            ("article 6 Déclaration de 1789", &[ART], &[]),
            ("article 6 DDHCX", &[ART], &[]),
            ("article de la Constitution", &[ART], &[]),
            ("alinéa du Préambule de 1946", &[ALINEA], &[]),
            ("le onzième alinéa du Préambule de 1958", &[ALINEA], &[]),
            ("le onzième alinéa de la Constitution", &[ALINEA], &[]),
            ("le onzième alinéa du", &[ALINEA], &[]),
            ("son article", &[ART], &[]),
            ("son article x", &[ART], &[]),
            ("Son", &[ART], &[]),
            ("Son", &[SON], &[]),
            ("son rôle", &[SON], &[]),
            ("onzième", &[ONZIEME], &[]),
            ("onzième alinéa", &[ONZIEME], &[]),
            ("article 5", &[ART], &[]),
            ("", &[ART], &[]),
        ]);
        let n = norm("article 54 de la Constitution");
        assert_eq!(scan(&n, &[]), Vec::new());
        assert_eq!(scan(&n, &anchors(&n, &[(AnchorClass::Court, "article")])), Vec::new());
    }

    #[test]
    fn block_titles_alone() {
        let end = |text: &str| const_block(text, 0).map(|(cite, end)| (cite.nature.code(), end));
        assert_eq!(end("Constitution."), Some(("CONSTITUTION", 12)));
        assert_eq!(end("Const. de"), Some(("CONSTITUTION", 6)));
        assert_eq!(
            end("Constitution du 4 octobre 1958 x"),
            Some(("CONSTITUTION", "Constitution du 4 octobre 1958".len()))
        );
        assert_eq!(end("DDHC"), Some(("DDHC", 4)));
        assert_eq!(end("DDHC."), Some(("DDHC", 4)));
        assert_eq!(end("Déclaration de 1789"), Some(("DDHC", "Déclaration de 1789".len())));
        assert_eq!(end("Charte de l'environnement"), Some(("CHARTE_ENVIRONNEMENT", 25)));
        assert_eq!(end("Préambule de 1946"), Some(("PREAMBULE_1946", "Préambule de 1946".len())));
        for not_a_block in
            ["Constitutionnel", "Conseil", "ddhc", "DDHCX", "Charte", "Préambule", ""]
        {
            assert!(const_block(not_a_block, 0).is_none(), "{not_a_block}");
        }
    }
}
