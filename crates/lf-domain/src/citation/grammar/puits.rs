//! Grammaire du « hors périmètre reconnu » : les puits anti-faux-positifs (annexe B §4.14).
//!
//! Un puits est une citation que le parseur reconnaît pour ne pas la confondre avec une autre :
//! actes de l'Union européenne (`directive 85/374/CEE du Conseil du 25 juillet 1985`), CJUE,
//! CEDH, doctrine (`D. 2016, p. 1234` est le Recueil Dalloz, pas un article D.) et ECLI d'un
//! autre pays que la France. Un candidat entièrement contenu dans un puits est écarté par
//! l'arbitrage (§4.16, règle 3).
//!
//! Précisions sur le texte du spec. Un acte nommé « décision » n'est un puits qu'avec une marque
//! européenne (`(UE)`, suffixe `/UE`, `du Conseil`), car « décision n° 21/0012 » est française.
//! Le préfixe d'article d'un acte (`Article 10 de la directive …`) accepte un numéro, ses
//! tirets, un adverbe latin et une subdivision (`paragraphe 1`, `§ 2`). Les parties d'un arrêt
//! de la CEDH tiennent en 100 caractères. « Affaire C-740/22 » seul est reconnu comme une
//! référence de la CJUE (forme réelle de justicelibre). Les plages des candidats sont des
//! offsets du texte d'ORIGINE (`Normalized::orig_range`), celles des ancres des offsets de
//! `Normalized::text`.

use crate::citation::normalize::Normalized;
use crate::citation::scan::{Anchor, AnchorClass};
use crate::citation::types::{CitationRef, UnsupportedCite, UnsupportedFamily};

use super::Candidate;
use super::jorf::{Hit, Win, scan_with};

/// Longueur maximale, en caractères, des parties d'un arrêt de la CEDH.
const PARTIES_MAX: usize = 100;

/// Adverbes latins d'un numéro d'article.
const LATIN: [&str; 9] =
    ["bis", "ter", "quater", "quinquies", "sexies", "septies", "octies", "nonies", "decies"];

/// Titres de revue qui valent à eux seuls une référence de doctrine, repliés.
const JOURNALS: [&str; 5] = ["rtd civ.", "rtd com.", "gaz. pal.", "dr. soc.", "rev. crit. dip"];

/// Candidats de la famille, tirés des ancres compatibles avec ses grammaires.
pub(crate) fn scan(n: &Normalized, anchors: &[Anchor]) -> Vec<Candidate> {
    let classes = [
        AnchorClass::Sink,
        AnchorClass::Sigle,
        AnchorClass::ArticlePrefix,
        AnchorClass::ArticleKeyword,
        AnchorClass::Family,
        AnchorClass::Ecli,
    ];
    scan_with(n, anchors, &classes, &[eu_article, eu_act, cjue, cedh, doctrine, foreign_ecli])
}

/// Un puits de la famille `family`, de span `s..end`.
fn sink(s: usize, end: usize, family: UnsupportedFamily, key: Option<String>) -> Hit {
    Hit::new(s, end, CitationRef::Unsupported(UnsupportedCite { family, key }))
}

// ---- Actes de l'Union européenne ----------------------------------------------------------------

/// `art_item` : un numéro (`10`, `10-1`, `1er`), un adverbe latin, une subdivision.
fn article_item(w: &Win, p: usize) -> Option<usize> {
    let (mut q, _) = w.digits(p, 1, 4).or_else(|| w.kw(p, "1er").map(|e| (e, 1)))?;
    while let Some((e, _)) = w.cs(q, "-").and_then(|r| w.digits(r, 1, 4)) {
        q = e;
    }
    q = w.ws(q).and_then(|r| LATIN.iter().find_map(|adverb| w.kw(r, adverb))).unwrap_or(q);
    let sub = w
        .sep(q)
        .and_then(|r| {
            ["paragraphe", "par.", "§", "point", "alinea", "al."].iter().find_map(|k| w.kw(r, k))
        })
        .and_then(|r| w.digits(w.ws0(r), 1, 3));
    Some(sub.map_or(q, |(e, _)| e))
}

/// `art_kw WS art_item` : la fin de « article 6 § 1 » ou « art. 267 ».
fn article_head(w: &Win, s: usize) -> Option<usize> {
    let q = ["articles", "article", "arts", "art."].iter().find_map(|k| w.kw(s, k))?;
    article_item(w, w.ws(q)?)
}

/// `art_kw WS art_item SEP ( "de la" | "du" ) WS` : la fin du préfixe d'article d'un acte.
fn article_prefix(w: &Win, s: usize) -> Option<usize> {
    let q = w.sep(article_head(w, s)?)?;
    w.ws(w.kw(q, "de la").or_else(|| w.kw(q, "du"))?)
}

/// `"directive" | "règlement" | "décision"` : la fin du mot, et si la nature veut une marque.
fn act_kind(w: &Win, p: usize) -> Option<(usize, bool)> {
    w.kw(p, "directive")
        .or_else(|| w.kw(p, "reglement"))
        .map(|q| (q, false))
        .or_else(|| w.kw(p, "decision").map(|q| (q, true)))
}

/// `"(" ( 'UE' | 'CE' | 'CEE' | 'Euratom' ) ")"`.
fn act_marker(w: &Win, p: usize) -> Option<usize> {
    let q = w.cs(p, "(")?;
    let q = ["Euratom", "CEE", "UE", "CE"].iter().find_map(|label| w.cs(q, label))?;
    w.cs(q, ")")
}

/// `/[0-9]{2,4}\/[0-9]{1,4}(\/(CE|CEE|UE))?/ B` : la fin, le numéro tel qu'écrit, et la présence
/// du suffixe.
fn eu_number(w: &Win, p: usize) -> Option<(usize, String, bool)> {
    let (q, _) = w.digits(p, 2, 4)?;
    let (q, _) = w.digits(w.cs(q, "/")?, 1, 4)?;
    let suffix =
        w.cs(q, "/").and_then(|r| ["CEE", "CE", "UE"].iter().find_map(|label| w.cs_word(r, label)));
    let end = suffix.unwrap_or(q);
    Some((end, w.text_of(p, end), suffix.is_some()))
}

/// `[","] WS ( "du Parlement européen et du Conseil" | "du Conseil" )`.
fn institution(w: &Win, p: usize) -> Option<usize> {
    let q = w.ws(w.cs(p, ",").unwrap_or(p))?;
    w.kw(q, "du parlement europeen et du conseil").or_else(|| w.kw(q, "du conseil"))
}

/// `[","] WS DATE_LINK` : `DATE_LINK = [ ( "du" | "en date du" | "le" | "des" ) WS ] DATE`.
fn date_link(w: &Win, p: usize) -> Option<usize> {
    let q = w.ws(w.cs(p, ",").unwrap_or(p))?;
    let q = ["en date du", "du", "le", "des"]
        .iter()
        .find_map(|link| w.kw(q, link).and_then(|r| w.ws(r)))
        .unwrap_or(q);
    w.date(q).map(|d| d.end)
}

/// `eu_act` : une directive, un règlement ou une décision de l'Union, avec son article éventuel.
fn eu_act(w: &Win, s: usize) -> Option<Hit> {
    let (q, needs_mark) = act_kind(w, article_prefix(w, s).unwrap_or(s))?;
    let paren = w.ws(q).and_then(|r| act_marker(w, r));
    let q = w.ws(paren.unwrap_or(q))?;
    let (q, number, suffixed) = eu_number(w, w.num_sign(q).unwrap_or(q))?;
    let council = institution(w, q);
    let q = council.unwrap_or(q);
    let end = date_link(w, q).unwrap_or(q);
    let end = sigle_article(w, end).unwrap_or(end);
    let european = paren.is_some() || suffixed || council.is_some();
    (european || !needs_mark).then(|| sink(s, end, UnsupportedFamily::Ue, Some(number)))
}

/// `WS? "(" SIGLE ")" SEP art_kw WS art_item` : « (RGPD), art. 17 » après un acte de l'Union.
fn sigle_article(w: &Win, p: usize) -> Option<usize> {
    let q = w.cs(w.ws0(p), "(")?;
    let (q, _) = instrument_sigle(w, q)?;
    article_head(w, w.sep(w.cs(q, ")")?)?)
}

// ---- Articles du droit de l'Union et de la Convention EDH -----------------------------------------

/// Un sigle d'instrument (casse significative) : sa fin et sa famille. Jamais « CE » ni « CP ».
fn instrument_sigle(w: &Win, p: usize) -> Option<(usize, UnsupportedFamily)> {
    ["TFUE", "TUE", "CDFUE", "RGPD"]
        .iter()
        .find_map(|label| w.cs_word(p, label).map(|q| (q, UnsupportedFamily::Ue)))
        .or_else(|| w.cs_word(p, "CEDH").map(|q| (q, UnsupportedFamily::Cedh)))
}

/// Le nom d'un instrument, replié : la fin et la famille. « Convention » seul n'en est pas un.
fn instrument_name(w: &Win, p: usize) -> Option<(usize, UnsupportedFamily)> {
    let convention = w.kw(p, "convention europeenne").and_then(|q| {
        let q = w.ws(q)?;
        let q = w
            .kw(q, "de sauvegarde des droits de l'homme")
            .or_else(|| w.kw(q, "des droits de l'homme"))?;
        Some(w.ws(q).and_then(|r| w.kw(r, "et des libertes fondamentales")).unwrap_or(q))
    });
    if let Some(q) = convention {
        return Some((q, UnsupportedFamily::Cedh));
    }
    let charte = w
        .kw(p, "charte des droits fondamentaux")
        .map(|q| w.ws(q).and_then(|r| w.kw(r, "de l'union europeenne")).unwrap_or(q));
    charte
        .or_else(|| w.kw(p, "traite sur le fonctionnement de l'union europeenne"))
        .or_else(|| w.kw(p, "traite sur l'union europeenne"))
        .or_else(|| w.kw(p, "reglement general sur la protection des donnees"))
        .map(|q| (q, UnsupportedFamily::Ue))
}

/// `eu_article` : `art_kw WS art_item ( WS SIGLE | SEP ( "de la" | "du" ) WS ( SIGLE | NOM ) )`.
fn eu_article(w: &Win, s: usize) -> Option<Hit> {
    let head = article_head(w, s)?;
    let suffix = w.ws(head).and_then(|q| instrument_sigle(w, q));
    let (end, family) = suffix.or_else(|| {
        let q = w.sep(head)?;
        let q = w.ws(w.kw(q, "de la").or_else(|| w.kw(q, "du"))?)?;
        instrument_sigle(w, q).or_else(|| instrument_name(w, q))
    })?;
    Some(sink(s, end, family, None))
}

// ---- CJUE et CEDH --------------------------------------------------------------------------------

/// `/[CT]-[0-9]{1,4}\/[0-9]{2}/ B` : une référence d'affaire de l'Union.
fn eu_case(w: &Win, p: usize) -> Option<(usize, String)> {
    let q = w.cs(p, "C-").or_else(|| w.cs(p, "T-"))?;
    let (q, _) = w.digits(q, 1, 4)?;
    let (q, _) = w.digits(w.cs(q, "/")?, 2, 2)?;
    Some((q, w.text_of(p, q)))
}

/// `[ "aff." WS ] /[CT]-[0-9]{1,4}\/[0-9]{2}/` : la référence d'affaire, précédée ou non de « aff. ».
fn eu_case_marked(w: &Win, p: usize) -> Option<(usize, String)> {
    eu_case(w, w.kw(p, "aff.").and_then(|r| w.ws(r)).unwrap_or(p))
}

/// `( 'CJUE' | 'CJCE' | "Trib. UE" ) SEP DATE SEP [ parties SEP ] [ "aff." WS ] /[CT]-[0-9]{1,4}\/[0-9]{2}/`.
fn cjue_headed(w: &Win, s: usize) -> Option<(usize, String)> {
    let q =
        w.cs_word(s, "CJUE").or_else(|| w.cs_word(s, "CJCE")).or_else(|| w.kw(s, "trib. ue"))?;
    let date = w.date(w.sep(q)?)?;
    let q = w.sep(date.end)?;
    eu_case_marked(w, q).or_else(|| after_parties(w, q, eu_case_marked))
}

/// `"affaire" WS /[CT]-[0-9]{1,4}\/[0-9]{2}/` : la référence seule.
fn cjue_case_only(w: &Win, s: usize) -> Option<(usize, String)> {
    let q = w.kw(s, "affaire").or_else(|| w.kw(s, "aff."))?;
    eu_case(w, w.ws(q)?)
}

/// `cjue`.
fn cjue(w: &Win, s: usize) -> Option<Hit> {
    let (end, case) = cjue_headed(w, s).or_else(|| cjue_case_only(w, s))?;
    Some(sink(s, end, UnsupportedFamily::Cjue, Some(case)))
}

/// `[ "req." WS ] [ NUM_SIGN ] /[0-9]{3,5}\/[0-9]{2}/ B` : la fin et le numéro de requête.
fn cedh_application(w: &Win, p: usize) -> Option<(usize, String)> {
    let q = w.kw(p, "req.").or_else(|| w.kw(p, "requete")).and_then(|r| w.ws(r)).unwrap_or(p);
    let start = w.num_sign(q).unwrap_or(q);
    let (q, _) = w.digits(start, 3, 5)?;
    let (q, _) = w.digits(w.cs(q, "/")?, 2, 2)?;
    Some((q, w.text_of(start, q)))
}

/// `[ parties SEP ]` puis la référence lue par `reader` : les parties tiennent jusqu'à la première
/// virgule après laquelle une référence se lit, dans les 100 caractères. Des parties qui portent un
/// ancrage d'article (« art. », « article ») ne sont pas des parties : la citation d'article
/// qu'elles contiennent reste lue et vérifiée, le puits ne l'absorbe pas (B §4.14).
fn after_parties(
    w: &Win,
    p: usize,
    reader: fn(&Win, usize) -> Option<(usize, String)>,
) -> Option<(usize, String)> {
    let limit = (p + PARTIES_MAX).min(w.len());
    let anchor = (p..limit).find(|&j| w.word_start(j) && article_anchor(w, j)).unwrap_or(limit);
    (p..anchor.min(limit))
        .filter(|&i| w.at(i) == Some(','))
        .find_map(|i| w.sep(i).and_then(|q| reader(w, q)))
}

/// Un ancrage d'article lu en `j` : « art. », « art », « article » ou « articles ».
fn article_anchor(w: &Win, j: usize) -> bool {
    ["art.", "articles", "article"].iter().any(|label| w.kw(j, label).is_some())
}

/// `cedh` : `( 'CEDH' | "Cour EDH" ) SEP DATE SEP [ parties SEP ] [ "req." WS ] [ NUM_SIGN ] …`.
fn cedh(w: &Win, s: usize) -> Option<Hit> {
    let q = w.cs_word(s, "CEDH").or_else(|| w.kw(s, "cour edh"))?;
    let date = w.date(w.sep(q)?)?;
    let q = w.sep(date.end)?;
    let (end, application) =
        cedh_application(w, q).or_else(|| after_parties(w, q, cedh_application))?;
    Some(sink(s, end, UnsupportedFamily::Cedh, Some(application)))
}

// ---- Doctrine -----------------------------------------------------------------------------------

/// `'D.' WS YEAR4 SEP ( "p." | "somm." | "chron." | "jur." ) WS? INT` (N33).
fn dalloz(w: &Win, s: usize) -> Option<usize> {
    let (q, _) = w.year4(w.ws(w.cs(s, "D.")?)?)?;
    let q = w.sep(q)?;
    let q = ["p.", "somm.", "chron.", "jur."].iter().find_map(|label| w.kw(q, label))?;
    w.digits(w.ws0(q), 1, 9).map(|(e, _)| e)
}

/// `'JCP' [ WS ( 'G' | 'E' | 'A' | 'S' | 'N' ) ] WS YEAR4 SEP INT` (N34).
fn jcp(w: &Win, s: usize) -> Option<usize> {
    let q = w.cs_word(s, "JCP")?;
    let q = w
        .ws(q)
        .and_then(|r| ["G", "E", "A", "S", "N"].iter().find_map(|series| w.cs_word(r, series)))
        .unwrap_or(q);
    let (q, _) = w.year4(w.ws(q)?)?;
    w.digits(w.sep(q)?, 1, 9).map(|(e, _)| e)
}

/// Titre de revue seul : `RTD civ.`, `Gaz. Pal.`, `AJDA`, `Rev. crit. DIP`…
fn journal_title(w: &Win, s: usize) -> Option<usize> {
    w.cs_word(s, "AJDA")
        .or_else(|| w.cs_word(s, "RFDA"))
        .or_else(|| JOURNALS.iter().find_map(|title| w.kw(s, title)))
}

/// `doctrine`.
fn doctrine(w: &Win, s: usize) -> Option<Hit> {
    let end = dalloz(w, s).or_else(|| jcp(w, s)).or_else(|| journal_title(w, s))?;
    Some(sink(s, end, UnsupportedFamily::Doctrine, None))
}

// ---- ECLI d'un autre pays -------------------------------------------------------------------------

/// Deux lettres : le pays d'un ECLI.
fn country(w: &Win, p: usize) -> Option<usize> {
    let letter = |i: usize| w.at(i).is_some_and(|c| c.is_ascii_alphabetic());
    (letter(p) && letter(p + 1)).then_some(p + 2)
}

/// `":"`.
fn colon(w: &Win, p: usize) -> Option<usize> {
    w.cs(p, ":")
}

/// `/[A-Za-z][A-Za-z0-9]{0,6}/` : la juridiction.
fn court(w: &Win, p: usize) -> Option<usize> {
    w.at(p).filter(char::is_ascii_alphabetic)?;
    let mut q = p + 1;
    while q < p + 7 && w.at(q).is_some_and(|c| c.is_ascii_alphanumeric()) {
        q += 1;
    }
    Some(q)
}

/// `/[0-9]{4}/` : l'année.
fn ecli_year(w: &Win, p: usize) -> Option<usize> {
    w.digits(p, 4, 4).map(|(e, _)| e)
}

/// `/[A-Za-z0-9.]{1,25}/ B` : l'ordinal, sans les points qui le terminent.
fn ordinal(w: &Win, p: usize) -> Option<usize> {
    let mut q = p;
    while q < p + 25 && w.at(q).is_some_and(|c| c.is_ascii_alphanumeric() || c == '.') {
        q += 1;
    }
    while q > p && w.at(q - 1) == Some('.') {
        q -= 1;
    }
    (q > p && w.bound(q)).then_some(q)
}

/// `ecli_etranger` : un ECLI dont le pays n'est pas la France ; celui de la France est lu par
/// la grammaire des ECLI.
fn foreign_ecli(w: &Win, s: usize) -> Option<Hit> {
    let p = w.kw(s, "ecli:")?;
    let end = [country, colon, court, colon, ecli_year, colon, ordinal]
        .iter()
        .try_fold(p, |q, step| step(w, q))?;
    let french = w.text_of(p, p + 2).eq_ignore_ascii_case("FR");
    (!french).then(|| {
        sink(s, end, UnsupportedFamily::ForeignEcli, Some(w.text_of(s, end).to_uppercase()))
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::citation::grammar::jorf::testkit::{Found, find, find_pairs};
    use crate::citation::scan::AnchorClass;
    use crate::citation::types::{CitationRef, Confidence, UnsupportedCite, UnsupportedFamily};

    fn sink(input: &str, needle: &str) -> Vec<Found> {
        find(scan, input, AnchorClass::Sink, &[needle])
    }

    fn one(input: &str, needle: &str) -> Found {
        let mut all = sink(input, needle);
        assert_eq!(all.len(), 1, "{input:?} : {all:?}");
        all.remove(0)
    }

    fn unsupported(family: UnsupportedFamily, key: Option<&str>) -> CitationRef {
        CitationRef::Unsupported(UnsupportedCite { family, key: key.map(str::to_owned) })
    }

    fn assert_none(inputs: &[(&str, &str)]) {
        for &(input, needle) in inputs {
            assert_eq!(sink(input, needle), [], "{input}");
        }
    }

    // ---- Actes de l'Union européenne -----------------------------------------------------------

    #[test]
    fn n32_directive_with_institution_and_date() {
        let input = "à la lumière de la directive 85/374/CEE du Conseil du 25 juillet 1985";
        let f = one(input, "directive");
        assert_eq!(f.text, "directive 85/374/CEE du Conseil du 25 juillet 1985");
        assert_eq!(f.value, unsupported(UnsupportedFamily::Ue, Some("85/374/CEE")));
        assert_eq!((f.confidence, f.ambiguity, f.flags), (Confidence::High, None, vec![]));
    }

    #[test]
    fn regulation_with_marker_and_number_sign() {
        let f = one("Règlement (UE) 2016/679", "Règlement");
        assert_eq!(f.text, "Règlement (UE) 2016/679");
        assert_eq!(f.value, unsupported(UnsupportedFamily::Ue, Some("2016/679")));
        let g =
            one("Règlement (CE) n° 44/2001 du Conseil du 22 décembre 2000, relatif", "Règlement");
        assert_eq!(g.text, "Règlement (CE) n° 44/2001 du Conseil du 22 décembre 2000");
        assert_eq!(g.value, unsupported(UnsupportedFamily::Ue, Some("44/2001")));
        for marker in ["(CEE)", "(Euratom)"] {
            let input = format!("règlement {marker} 2016/679");
            assert_eq!(one(&input, "règlement").text, input);
        }
    }

    #[test]
    fn the_parliament_and_council_tail_is_longer_than_the_council_one() {
        let input =
            "Directive (UE) 2016/680 du Parlement européen et du Conseil du 27 avril 2016, dite";
        let f = one(input, "Directive");
        assert_eq!(
            f.text,
            "Directive (UE) 2016/680 du Parlement européen et du Conseil du 27 avril 2016"
        );
        let g = one("directive 2016/680, du Conseil, du 27 avril 2016", "directive");
        assert_eq!(g.text, "directive 2016/680, du Conseil, du 27 avril 2016");
        let h = one("règlement 2016/679 en date du 27 avril 2016", "règlement");
        assert_eq!(h.text, "règlement 2016/679 en date du 27 avril 2016");
        for tail in ["le 27 avril 2016", "des 27 avril 2016", "27 avril 2016"] {
            let input = format!("règlement 2016/679 {tail}");
            assert_eq!(one(&input, "règlement").text, input);
        }
        let comma = "règlement 2016/679, du 27 avril 2016";
        assert_eq!(one(comma, "règlement").text, comma);
        // Une date illisible n'est pas absorbée.
        assert_eq!(
            one("règlement 2016/679 du 32 avril 2016", "règlement").text,
            "règlement 2016/679"
        );
    }

    #[test]
    fn an_article_of_an_eu_act_is_one_sink() {
        let f = one("Article 10 de la directive 85/374/CEE", "directive");
        assert_eq!(f.text, "Article 10 de la directive 85/374/CEE");
        assert_eq!(f.value, unsupported(UnsupportedFamily::Ue, Some("85/374/CEE")));
        // Ancré sur le mot « article », ou sur le puits : le même candidat.
        let both = [(AnchorClass::ArticleKeyword, "Article"), (AnchorClass::Sink, "directive")];
        assert_eq!(find_pairs(scan, "Article 10 de la directive 85/374/CEE", &both).len(), 1);
        for (input, needle) in [
            ("articles 10 du règlement 2016/679", "règlement"),
            ("art. 10-1 de la directive 2016/680", "directive"),
            ("art 10 bis de la directive 2016/680", "directive"),
            ("arts 1er du règlement 2016/679", "règlement"),
            ("article 6, paragraphe 1, de la directive 2016/680", "directive"),
            ("article 6 § 2 du règlement 2016/679", "règlement"),
            ("article 6 point 2 du règlement 2016/679", "règlement"),
        ] {
            assert_eq!(one(input, needle).text, input);
        }
        // Sans l'article, ou avec un article illisible, seul l'acte est un puits.
        assert_eq!(one("l'article du règlement 2016/679", "règlement").text, "règlement 2016/679");
        assert_eq!(one("article x du règlement 2016/679", "règlement").text, "règlement 2016/679");
        assert_eq!(one("art.10 du règlement 2016/679", "règlement").text, "règlement 2016/679");
        assert_eq!(one("article 10.du règlement 2016/679", "règlement").text, "règlement 2016/679");
        assert_eq!(
            one("article 10 des règlement 2016/679", "règlement").text,
            "règlement 2016/679"
        );
    }

    #[test]
    fn a_decision_needs_an_eu_mark() {
        for input in [
            "Décision (UE) 2019/1234",
            "décision 2014/335/UE",
            "décision 2014/335 du Conseil",
            "décision 2014/335, du Parlement européen et du Conseil",
        ] {
            let f = one(input, "écision");
            assert_eq!(f.text, input, "{input}");
        }
        assert_none(&[
            ("Décision 98/12", "écision"),
            ("décision n° 21/00123 du 5 mars 2021", "écision"),
            ("décision 98/12 du 5 mars 2021", "écision"),
        ]);
    }

    #[test]
    fn malformed_eu_acts_are_not_sinks() {
        assert_none(&[
            ("directive", "directive"),
            ("directive nationale d'aménagement", "directive"),
            ("directive 5/12", "directive"),
            ("directive 85", "directive"),
            ("directive 85/37412", "directive"),
            ("directive (XX) 2016/680", "directive"),
            ("directives 85/374", "directives"),
            ("indirective 85/374/CEE", "directive"),
            ("article 10 de la directive", "directive"),
        ]);
        // La suite d'un numéro suivi d'une barre inconnue est laissée.
        assert_eq!(one("directive 85/374/CEEX", "directive").text, "directive 85/374");
    }

    // ---- Articles du droit de l'Union et de la CEDH (L1-07) -----------------------------------------

    fn art(input: &str) -> Vec<Found> {
        find(scan, input, AnchorClass::ArticleKeyword, &["rticle", "rt.", "rts"])
    }

    fn one_art(input: &str) -> Found {
        let mut all = art(input);
        assert_eq!(all.len(), 1, "{input:?} : {all:?}");
        all.remove(0)
    }

    #[test]
    fn an_article_followed_by_an_instrument_sigle_is_one_sink() {
        for (input, family) in [
            ("Art. 267 TFUE", UnsupportedFamily::Ue),
            ("art. 4 TUE", UnsupportedFamily::Ue),
            ("article 8 CEDH", UnsupportedFamily::Cedh),
            ("article 6 § 1 CEDH", UnsupportedFamily::Cedh),
            ("art. 47 CDFUE", UnsupportedFamily::Ue),
            ("article 17 RGPD", UnsupportedFamily::Ue),
        ] {
            let f = one_art(input);
            assert_eq!(f.text, input, "{input}");
            assert_eq!(f.value, unsupported(family, None), "{input}");
            assert_eq!((f.confidence, f.ambiguity, f.flags), (Confidence::High, None, vec![]));
        }
    }

    #[test]
    fn an_article_of_a_named_instrument_is_one_sink() {
        for (input, family) in [
            (
                "article 8 de la Convention européenne des droits de l'homme",
                UnsupportedFamily::Cedh,
            ),
            (
                "article 8 de la Convention européenne des droits de l'homme et des libertés fondamentales",
                UnsupportedFamily::Cedh,
            ),
            (
                "article 8 de la Convention européenne de sauvegarde des droits de l'homme et des libertés fondamentales",
                UnsupportedFamily::Cedh,
            ),
            ("article 6 § 1 de la CEDH", UnsupportedFamily::Cedh),
            ("article 6, paragraphe 1, de la CEDH", UnsupportedFamily::Cedh),
            ("article 17 du RGPD", UnsupportedFamily::Ue),
            ("art. 267 du TFUE", UnsupportedFamily::Ue),
            ("article 4 du TUE", UnsupportedFamily::Ue),
            ("article 47 de la CDFUE", UnsupportedFamily::Ue),
            ("article 47 de la Charte des droits fondamentaux", UnsupportedFamily::Ue),
            (
                "article 47 de la Charte des droits fondamentaux de l'Union européenne",
                UnsupportedFamily::Ue,
            ),
            ("article 4 du traité sur l'Union européenne", UnsupportedFamily::Ue),
            (
                "article 267 du traité sur le fonctionnement de l'Union européenne",
                UnsupportedFamily::Ue,
            ),
            (
                "article 17 du règlement général sur la protection des données",
                UnsupportedFamily::Ue,
            ),
        ] {
            let f = one_art(input);
            assert_eq!(f.text, input, "{input}");
            assert_eq!(f.value, unsupported(family, None), "{input}");
        }
        // La phrase entière : seul le groupe nominal est absorbé.
        let f = one_art("L'article 8 de la Convention européenne des droits de l'homme. Suite");
        assert_eq!(f.text, "article 8 de la Convention européenne des droits de l'homme");
    }

    #[test]
    fn an_eu_act_followed_by_its_sigle_absorbs_the_article() {
        let input = "Le règlement (UE) 2016/679 du 27 avril 2016 (RGPD), art. 17";
        let f = one(input, "règlement");
        assert_eq!(f.text, "règlement (UE) 2016/679 du 27 avril 2016 (RGPD), art. 17");
        assert_eq!(f.value, unsupported(UnsupportedFamily::Ue, Some("2016/679")));
        for tail in [" (RGPD) art. 17", " (RGPD), article 17 bis", " (RGPD), art. 6 § 1"] {
            let input = format!("règlement (UE) 2016/679 du 27 avril 2016{tail}");
            assert_eq!(one(&input, "règlement").text, input, "{input}");
        }
        // Vrai positif conservé : sans sigle ni article, l'acte garde son étendue.
        for (input, text) in [
            (
                "règlement (UE) 2016/679 du 27 avril 2016 (RGPD)",
                "règlement (UE) 2016/679 du 27 avril 2016",
            ),
            (
                "règlement (UE) 2016/679 du 27 avril 2016, art. 17",
                "règlement (UE) 2016/679 du 27 avril 2016",
            ),
            (
                "règlement (UE) 2016/679 du 27 avril 2016 (RGPD), art. x",
                "règlement (UE) 2016/679 du 27 avril 2016",
            ),
            (
                "règlement (UE) 2016/679 du 27 avril 2016 (XYZ), art. 17",
                "règlement (UE) 2016/679 du 27 avril 2016",
            ),
            (
                "règlement (UE) 2016/679 du 27 avril 2016 (RGPD, art. 17",
                "règlement (UE) 2016/679 du 27 avril 2016",
            ),
        ] {
            assert_eq!(one(input, "règlement").text, text, "{input}");
        }
    }

    #[test]
    fn french_articles_next_to_the_instruments_are_not_sinks() {
        for input in [
            "art. L. 52-8 CE",
            "article 121-3 CP",
            "article 5 de la Convention collective nationale",
            "article 5 de la Convention",
            "article 5 de la convention européenne",
            "art. 267 tfue",
            "art. 267 TFUEX",
            "art. 267 Tfue",
            "article 17 du RGPDX",
            "article 5 du traité de Rome",
            "article x de la CEDH",
            "article 5",
            "article 5.",
            "article 5 du",
            "article 5 de la",
            "article 5 de la convention européenne des droits",
            "article 5 de CEDH",
            "article 5 la CEDH",
            "article 5 du code civil",
            "article 1240 du code civil et la CEDH",
        ] {
            assert_eq!(art(input), [], "{input}");
        }
        // L'article d'une directive reste lu par la grammaire des actes.
        let f = one("article 10 de la directive 85/374/CEE", "directive");
        assert_eq!(f.value, unsupported(UnsupportedFamily::Ue, Some("85/374/CEE")));
    }

    // ---- CJUE et CEDH ----------------------------------------------------------------------------

    #[test]
    fn cjue_forms() {
        let f = one("CJUE, 14 mars 2013, aff. C-420/11", "CJUE");
        assert_eq!(f.text, "CJUE, 14 mars 2013, aff. C-420/11");
        assert_eq!(f.value, unsupported(UnsupportedFamily::Cjue, Some("C-420/11")));
        assert_eq!(one("CJCE 5 févr. 1963 C-26/62", "CJCE").text, "CJCE 5 févr. 1963 C-26/62");
        let g = one("Trib. UE, 10 juin 2020, T-123/19", "Trib");
        assert_eq!(g.text, "Trib. UE, 10 juin 2020, T-123/19");
        assert_eq!(g.value, unsupported(UnsupportedFamily::Cjue, Some("T-123/19")));
        // « Affaire C-740/22 » seul (justicelibre).
        let h = find(scan, "Affaire C-740/22", AnchorClass::Family, &["Affaire"]);
        assert_eq!(h[0].text, "Affaire C-740/22");
        assert_eq!(h[0].value, unsupported(UnsupportedFamily::Cjue, Some("C-740/22")));
        assert_eq!(find(scan, "aff. T-1/20", AnchorClass::Family, &["aff."]).len(), 1);
    }

    #[test]
    fn cjue_with_parties_between_date_and_case() {
        // L1-06 (verify_citations-15) : les parties précèdent le numéro d'affaire.
        let input = "CJUE, 6 octobre 2015, Schrems, C-362/14";
        let f = one(input, "CJUE");
        assert_eq!(f.text, input);
        assert_eq!(f.value, unsupported(UnsupportedFamily::Cjue, Some("C-362/14")));
        let long = "CJUE, 6 octobre 2015, Maximillian Schrems c. Data Protection Commissioner, aff. C-362/14";
        let g = one(long, "CJUE");
        assert_eq!(g.text, long);
        assert_eq!(g.value, unsupported(UnsupportedFamily::Cjue, Some("C-362/14")));
        let h = one("Trib. UE, 12 mars 2019, Foo c. Commission, T-123/18", "Trib");
        assert_eq!(h.text, "Trib. UE, 12 mars 2019, Foo c. Commission, T-123/18");
        assert_eq!(h.value, unsupported(UnsupportedFamily::Cjue, Some("T-123/18")));
        // Vrai positif conservé : sans partie, le puits garde sa forme et son étendue.
        assert_eq!(
            one("CJUE, 6 octobre 2015, aff. C-362/14", "CJUE").text,
            "CJUE, 6 octobre 2015, aff. C-362/14"
        );
    }

    #[test]
    fn cjue_parties_do_not_swallow_neighbours() {
        assert_none(&[
            ("CJUE, 4 juin 2024", "CJUE"),
            ("CJUE, 4 juin 2024, Schrems", "CJUE"),
            ("CJUE, 4 juin 2024, Schrems, et voir l'article 5 du code civil", "CJUE"),
            ("CJUE, 4 juin 2024, Schrems, C-362", "CJUE"),
            ("CJUE, 4 juin 2024, Schrems, X-362/14", "CJUE"),
        ]);
        // Une citation d'article dans la zone des parties n'est pas absorbée par le puits.
        assert_none(&[
            ("CJUE, 4 juin 2024, voir l'article 1240 du code civil, C-362/14", "CJUE"),
            ("CJUE, 4 juin 2024, Schrems, art. 5 du code civil, C-362/14", "CJUE"),
            ("CEDH, 4 juin 2024, voir l'article 1240 du code civil, req. 12345/20", "CEDH"),
        ]);
        // Un nom de partie qui contient « art » sans être un ancrage reste une partie.
        let martin = one("CJUE, 4 juin 2024, Martin c. Partner, C-362/14", "CJUE");
        assert_eq!(martin.value, unsupported(UnsupportedFamily::Cjue, Some("C-362/14")));
        // Les parties tiennent en 100 caractères.
        let far = format!("CJUE, 4 juin 2024, {}, C-362/14", "x".repeat(150));
        assert_none(&[(far.as_str(), "CJUE")]);
        // Le puits s'arrête au numéro d'affaire : la suite reste lue.
        let after = one("CJUE, 6 octobre 2015, Schrems, C-362/14, et l'article 5", "CJUE");
        assert_eq!(after.text, "CJUE, 6 octobre 2015, Schrems, C-362/14");
    }

    #[test]
    fn malformed_cjue_references_are_not_sinks() {
        assert_none(&[
            ("CJUE, 14 mars 2013", "CJUE"),
            ("CJUE aff. C-420/11", "CJUE"),
            ("CJUE, 14 mars 2013, aff. C-42000/11", "CJUE"),
            ("CJUE, 14 mars 2013, aff. C-420/1", "CJUE"),
            ("CJUE, 14 mars 2013, aff. X-420/11", "CJUE"),
            ("CJUE, 14 mars 2013, aff. C-420-11", "CJUE"),
            ("CJUE, 14 mars 2013, aff. C-420", "CJUE"),
            ("CJUEX, 14 mars 2013, C-420/11", "CJUEX"),
            ("CJUE,14 mars 2013", "CJUE"),
            ("CJUE-14 mars 2013, aff. C-420/11", "CJUE"),
        ]);
        let none = find(scan, "affaire C-1/2", AnchorClass::Family, &["affaire"]);
        assert_eq!(none, []);
        assert_eq!(find(scan, "affaire", AnchorClass::Family, &["affaire"]), []);
    }

    #[test]
    fn cedh_forms() {
        let input = "CEDH, 26 avr. 1979, Sunday Times c. Royaume-Uni, req. n° 6538/74";
        let f = one(input, "CEDH");
        assert_eq!(f.text, input);
        assert_eq!(f.value, unsupported(UnsupportedFamily::Cedh, Some("6538/74")));
        assert_eq!(
            one("CEDH 7 juill. 1989 req. n° 14038/88", "CEDH").text,
            "CEDH 7 juill. 1989 req. n° 14038/88"
        );
        let g = one("Cour EDH, 8 juillet 2003, 12345/99", "Cour");
        assert_eq!(g.value, unsupported(UnsupportedFamily::Cedh, Some("12345/99")));
        assert_eq!(
            one("CEDH, 8 juillet 2003, requête 12345/99", "CEDH").text,
            "CEDH, 8 juillet 2003, requête 12345/99"
        );
        assert_eq!(
            one("CEDH, 8 juillet 2003, n° 12345/99", "CEDH").text,
            "CEDH, 8 juillet 2003, n° 12345/99"
        );
        // Les parties finissent à une virgule : un numéro qu'elles contiennent sans virgule avant
        // lui n'est pas le numéro de requête (mutant de `after_parties`).
        let named = "CEDH, 26 avr. 1979, Syndicat 123/45 c. Royaume-Uni, req. n° 6538/74";
        let h = one(named, "CEDH");
        assert_eq!(h.text, named);
        assert_eq!(h.value, unsupported(UnsupportedFamily::Cedh, Some("6538/74")));
    }

    #[test]
    fn malformed_cedh_references_are_not_sinks() {
        let far = format!("CEDH, 26 avr. 1979, {}, req. n° 6538/74", "x".repeat(150));
        assert_none(&[
            ("CEDH, 26 avr. 1979", "CEDH"),
            ("CEDH, 26 avr. 1979, Sunday Times", "CEDH"),
            ("CEDH, 26 avr. 1979, req. n° 65/74", "CEDH"),
            ("CEDH, 26 avr. 1979, req. n° 6538/7", "CEDH"),
            ("CEDH, 26 avr. 1979, req. n° 653812/74", "CEDH"),
            ("CEDH 26 avr. 1979", "CEDH"),
            ("CEDH-26 avr. 1979, req. n° 6538/74", "CEDH"),
            ("CEDH, 32 avr. 1979, req. n° 6538/74", "CEDH"),
            ("CEDH, 26 avr. 1979, req. n° 6538-74", "CEDH"),
            ("CEDH, 26 avr. 1979, X, y", "CEDH"),
            ("CEDHS, 26 avr. 1979, 6538/74", "CEDHS"),
            (far.as_str(), "CEDH"),
        ]);
    }

    // ---- Doctrine ------------------------------------------------------------------------------

    #[test]
    fn n33_dalloz_is_doctrine_not_an_article() {
        let f = find(scan, "note sous D. 2016, p. 1234", AnchorClass::ArticlePrefix, &["D."]);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].text, "D. 2016, p. 1234");
        assert_eq!(f[0].value, unsupported(UnsupportedFamily::Doctrine, None));
        for part in ["somm. 12", "chron. 45", "jur. 7", "p 3"] {
            let input = format!("D. 2016, {part}");
            let all = find(scan, &input, AnchorClass::ArticlePrefix, &["D."]);
            assert_eq!(all[0].text, input);
        }
        assert_eq!(
            find(scan, "D. 2016 p. 1234", AnchorClass::ArticlePrefix, &["D."])[0].text,
            "D. 2016 p. 1234"
        );
        for input in [
            "D. 2016",
            "D. 99, p. 12",
            "article D. 2016-12",
            "D.2016, p. 12",
            "D. 2016, p. x",
            "D. 2016, x. 12",
        ] {
            let all = find(scan, input, AnchorClass::ArticlePrefix, &["D."]);
            assert_eq!(all, [], "{input}");
        }
        // Un « D. » au milieu d'un mot n'est pas Dalloz.
        assert_eq!(find(scan, "CD. 2016, p. 12", AnchorClass::ArticlePrefix, &["D."]), []);
    }

    #[test]
    fn n34_jcp_and_the_bare_journal_titles() {
        let f = one("JCP G 2016, 123", "JCP");
        assert_eq!(f.text, "JCP G 2016, 123");
        assert_eq!(f.value, unsupported(UnsupportedFamily::Doctrine, None));
        assert_eq!(one("JCP 2016, 123", "JCP").text, "JCP 2016, 123");
        assert_eq!(one("JCP E 2016 45", "JCP").text, "JCP E 2016 45");
        for (input, needle, title) in [
            ("v. RTD civ. 2016", "RTD", "RTD civ."),
            ("RTD com. 2016", "RTD", "RTD com."),
            ("Gaz. Pal. 2016", "Gaz.", "Gaz. Pal."),
            ("AJDA 2016", "AJDA", "AJDA"),
            ("RFDA 2016", "RFDA", "RFDA"),
            ("Dr. soc. 2016", "Dr.", "Dr. soc."),
            ("Rev. crit. DIP 2016", "Rev.", "Rev. crit. DIP"),
            ("rev. crit. dip 2016", "rev.", "rev. crit. dip"),
        ] {
            let f = one(input, needle);
            assert_eq!(f.text, title, "{input}");
            assert_eq!(f.value, unsupported(UnsupportedFamily::Doctrine, None), "{input}");
        }
        assert_none(&[
            ("JCP X 2016, 123", "JCP"),
            ("JCP G 2016", "JCP"),
            ("JCP G 99, 123", "JCP"),
            ("JCPG 2016, 123", "JCPG"),
            ("JCP-2016, 123", "JCP"),
            ("AJDAX 2016", "AJDAX"),
            ("RTD civil 2016", "RTD"),
        ]);
    }

    // ---- ECLI étranger -------------------------------------------------------------------------

    fn ecli(input: &str) -> Vec<Found> {
        find(scan, input, AnchorClass::Ecli, &["ECLI", "ecli"])
    }

    #[test]
    fn p159_foreign_ecli_is_a_sink_with_its_uppercase_key() {
        let f = ecli("ECLI:EU:C:2024:216");
        assert_eq!(f[0].text, "ECLI:EU:C:2024:216");
        assert_eq!(
            f[0].value,
            unsupported(UnsupportedFamily::ForeignEcli, Some("ECLI:EU:C:2024:216"))
        );
        let g = ecli("ecli:eu:c:2024:216.");
        assert_eq!(g[0].text, "ecli:eu:c:2024:216");
        assert_eq!(
            g[0].value,
            unsupported(UnsupportedFamily::ForeignEcli, Some("ECLI:EU:C:2024:216"))
        );
        let h = ecli("ECLI:DE:BGH:2020:040320BIZR123.19.0 puis");
        assert_eq!(h[0].text, "ECLI:DE:BGH:2020:040320BIZR123.19.0");
    }

    #[test]
    fn french_and_truncated_eclis_are_left_to_their_own_grammar() {
        for input in [
            "ECLI:FR:CCASS:2025:C100399",
            "ecli:fr:cc:2025:2025.1173.QPC",
            "ECLI:EU:C:2024",
            "ECLI:EU:C:20:216",
            "ECLI:E:C:2024:216",
            "ECLI:EU:1C:2024:216",
            "ECLI:EU:C:2024:",
            "ECLI:EU:C:2024:.",
            "ECLI:EU:ABCDEFGH:2024:216",
            "ECLI:EU:C:2024:ABCDEFGHIJKLMNOPQRSTUVWXYZ",
            "ECLI:1U:C:2024:216",
            "ECLI",
        ] {
            assert_eq!(ecli(input), [], "{input}");
        }
    }

    #[test]
    fn only_sink_like_anchors_are_read() {
        let input = "directive 85/374/CEE";
        assert_eq!(find(scan, input, AnchorClass::Court, &["directive"]), []);
        for class in [
            AnchorClass::Sink,
            AnchorClass::Sigle,
            AnchorClass::ArticlePrefix,
            AnchorClass::ArticleKeyword,
            AnchorClass::Family,
            AnchorClass::Ecli,
        ] {
            assert_eq!(find(scan, input, class, &["directive"]).len(), 1, "{class:?}");
        }
    }
}
