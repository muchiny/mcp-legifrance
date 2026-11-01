//! Mention d'article que le parseur n'a pas lue (revue finale I3, annexe B §7.9).
//!
//! Le filet N36 refuse en entier un article dont le numéro ne peut pas être lu (« CGI, art.
//! 885-0 V bis A », « art. 150-0 B ter I du CGI ») : aucune citation n'est rendue pour lui. Dans un
//! texte où d'autres citations sont lues, il disparaîtrait sans trace de `fr_verify_citations`.
//! [`unread_article_mentions`] le retrouve : un mot-clé d'article suivi d'un numéro, hors de toute
//! citation lue.

use crate::citation::types::Span;

/// Y a-t-il, hors de tout span de `read`, un mot-clé d'article (« art. », « article », « articles »,
/// sans égard à la casse) suivi d'un numéro : un chiffre, ou l'un des préfixes `L`, `R`, `D`, `LO`
/// (majuscules, suivis ou non d'un point) puis un chiffre ?
///
/// `read` : les spans, en octets de `input`, de toutes les citations lues (celles qui sont hors
/// périmètre comprises). Un mot-clé est lu quand lui-même ou son numéro tombe dans l'un d'eux (le
/// span d'« articles L. 104-1 et L. 104-2 » peut ne commencer qu'au numéro). Le mot-clé doit
/// commencer un mot (« particle 5 » n'en est pas un). Une
/// seule passe sur `input`, puis une recherche dichotomique par mot-clé : le coût est linéaire en
/// la taille de `input`, au logarithme du nombre de spans près.
///
/// ```
/// use lf_domain::citation::{Span, unread_article_mentions};
///
/// let text = "article 1240 du code civil ; CGI, art. 885-0 V bis A";
/// let read = [Span { start: 0, end: 26 }];
/// assert!(unread_article_mentions(text, &read));
/// assert!(!unread_article_mentions("article 1240 du code civil", &[Span { start: 0, end: 26 }]));
/// ```
#[must_use]
pub fn unread_article_mentions(input: &str, read: &[Span]) -> bool {
    let covered = merged(read);
    let mut previous_is_word = false;
    for (at, c) in input.char_indices() {
        let starts_word = !previous_is_word;
        previous_is_word = c.is_alphanumeric();
        if !starts_word || !c.is_ascii_alphabetic() {
            continue;
        }
        let rest = input.get(at..).unwrap_or_default();
        // Le span d'une citation commence parfois au numéro (« articles L. 104-1 et L. 104-2 ») :
        // le mot-clé est lu si lui-même ou son numéro tombe dans un span lu.
        let unread = number_after_keyword(rest).is_some_and(|offset| {
            !is_covered(&covered, at) && !is_covered(&covered, at.saturating_add(offset))
        });
        if unread {
            return true;
        }
    }
    false
}

/// Si `rest` commence par un mot-clé d'article entier puis un numéro, l'écart en octets entre le
/// début de `rest` et celui du numéro.
fn number_after_keyword(rest: &str) -> Option<usize> {
    let number = strip_keyword(rest)?.trim_start();
    starts_with_number(number).then(|| rest.len().saturating_sub(number.len()))
}

/// Reste de `rest` après « article », « articles » ou « art. » (casse ignorée), si `rest` en
/// commence par un, mot entier.
fn strip_keyword(rest: &str) -> Option<&str> {
    // Reste de `rest` après `word` (casse ignorée) ; `None` si `rest` n'en commence pas ainsi ou si
    // la coupe tombe au milieu d'un caractère.
    let after = |word: &str| {
        let (head, tail) = rest.split_at_checked(word.len())?;
        head.eq_ignore_ascii_case(word).then_some(tail)
    };
    if let Some(tail) = after("article") {
        let tail = tail.strip_prefix(['s', 'S']).unwrap_or(tail);
        // Suivi d'une lettre ou d'un chiffre, c'est un autre mot (« articlé », « article5 »).
        return tail.chars().next().is_none_or(|c| !c.is_alphanumeric()).then_some(tail);
    }
    if let Some(tail) = after("art") {
        return tail.strip_prefix('.');
    }
    None
}

/// `text` commence-t-il par un numéro d'article : chiffre, ou préfixe `L`/`R`/`D`/`LO` (point et
/// blancs facultatifs) puis chiffre ?
fn starts_with_number(text: &str) -> bool {
    let after_prefix = text
        .strip_prefix("LO")
        .or_else(|| text.strip_prefix(['L', 'R', 'D']))
        .map(|tail| tail.strip_prefix('.').unwrap_or(tail).trim_start());
    [Some(text), after_prefix]
        .into_iter()
        .flatten()
        .any(|candidate| candidate.chars().next().is_some_and(|c| c.is_ascii_digit()))
}

/// Intervalles de `read` triés et fusionnés. Un span vide (ou inversé) reste un intervalle qui ne
/// couvre aucun octet : [`is_covered`] exige `start <= at < end`.
fn merged(read: &[Span]) -> Vec<(usize, usize)> {
    let mut spans: Vec<(usize, usize)> = read.iter().map(|s| (s.start, s.end)).collect();
    spans.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for (start, end) in spans {
        match out.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => out.push((start, end)),
        }
    }
    out
}

/// L'octet `at` est-il dans un intervalle de `covered` ?
fn is_covered(covered: &[(usize, usize)], at: usize) -> bool {
    let after = covered.partition_point(|&(start, _)| start <= at);
    after.checked_sub(1).and_then(|i| covered.get(i)).is_some_and(|&(_, end)| at < end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::citation::{ParseOptions, parse_with};

    /// `unread_article_mentions` sur les spans que le parseur lit vraiment.
    fn unread(text: &str) -> bool {
        let spans: Vec<Span> =
            parse_with(text, &ParseOptions::default()).into_iter().map(|cite| cite.span).collect();
        unread_article_mentions(text, &spans)
    }

    /// Revue finale I3 : les deux formes refusées en entier par N36 se retrouvent, dans un texte
    /// qui contient aussi une citation lue.
    #[test]
    fn an_article_refused_by_the_parser_is_found_next_to_a_read_one() {
        for refused in ["CGI, art. 885-0 V bis A", "art. 150-0 B ter I du CGI"] {
            let text = format!("L'article 1240 du code civil ; {refused}.");
            let read = parse_with(&text, &ParseOptions::default());
            assert!(
                read.iter().all(|cite| !text[cite.span.start..cite.span.end].contains("885")),
                "le parseur refuse bien {refused} : {read:?}"
            );
            assert!(!read.is_empty(), "article 1240 est lu : {text}");
            assert!(unread(&text), "{text}");
            // Le jumeau sans l'article refusé n'a rien d'illisible.
            assert!(!unread("L'article 1240 du code civil."), "{text}");
        }
        // Entre deux citations lues, la mention refusée n'est pas couverte : les spans ne sont
        // fusionnés que s'ils se touchent.
        let between = "L'article 1240 du code civil ; CGI, art. 885-0 V bis A ; l'article 1241 du code civil.";
        assert!(unread(between), "{between}");
    }

    /// Un span vide ne couvre rien, même posé sur le mot-clé.
    #[test]
    fn an_empty_span_covers_nothing() {
        let text = "CGI, art. 885-0 V bis A";
        let at = text.find("art.").unwrap();
        assert!(unread_article_mentions(text, &[Span { start: at, end: at }]));
        assert!(!unread_article_mentions(text, &[Span { start: at, end: text.len() }]));
    }

    #[test]
    fn a_read_citation_is_not_an_unread_mention() {
        for text in [
            "article 1240 du code civil",
            "Art. L. 1234-9 du code du travail et articles 1240 à 1244 du code civil",
            "l'article LO 123 du code électoral",
            "ARTICLE R. 741-7 du code de justice administrative",
            "pas d'article ici, ni d'art de vivre, ni particle 5, ni article premier",
            "",
        ] {
            assert!(!unread(text), "{text}");
        }
    }

    /// Le mot-clé, puis le numéro : chaque forme admise, et chaque forme refusée.
    #[test]
    fn the_keyword_and_the_number_forms() {
        let none: [Span; 0] = [];
        for yes in [
            "article 5",
            "articles 5",
            "ARTICLE 5",
            "art. 5",
            "Art.5",
            "article\u{a0}5",
            "article L. 1234-9",
            "article L1234-9",
            "article R 12",
            "article D. 12",
            "article LO. 12",
            "article LO12",
            "x ; art. 12",
        ] {
            assert!(unread_article_mentions(yes, &none), "{yes}");
        }
        for no in [
            "article",
            "article ",
            "article premier",
            "article l. 5",
            "article L. x",
            "article L.",
            "article5",
            "articlé 5",
            "articles5",
            "art 5",
            "art",
            "particle 5",
            "smart. 5",
            "5 article",
            "arti",
        ] {
            assert!(!unread_article_mentions(no, &none), "{no}");
        }
    }

    #[test]
    fn a_keyword_inside_a_read_span_is_read_and_one_outside_is_not() {
        let text = "article 5 puis article 6";
        let first = Span { start: 0, end: 9 };
        let second = Span { start: 15, end: 24 };
        assert!(unread_article_mentions(text, &[first]));
        assert!(unread_article_mentions(text, &[second]));
        assert!(!unread_article_mentions(text, &[second, first]));
        // Spans vides, chevauchants et imbriqués : fusionnés sans effet de bord.
        let empty = Span { start: 3, end: 3 };
        let wide = Span { start: 0, end: 24 };
        let inner = Span { start: 2, end: 4 };
        assert!(!unread_article_mentions(text, &[empty, inner, wide]));
        assert!(!unread_article_mentions(
            text,
            &[Span { start: 0, end: 12 }, Span { start: 10, end: 24 }]
        ));
        assert!(unread_article_mentions(text, &[empty]));
        // La fin d'un span est exclue : « article 6 » commence là où finit le span précédent.
        let glued = "article 5 article 6";
        assert!(unread_article_mentions(glued, &[Span { start: 0, end: 10 }]));
        assert!(!unread_article_mentions(glued, &[Span { start: 0, end: 11 }]));
    }

    #[test]
    fn a_keyword_whose_number_is_inside_a_read_span_is_read() {
        let text = "articles 56 et 57 du code";
        // Le span commence au numéro, pas au mot-clé.
        assert!(!unread_article_mentions(text, &[Span { start: 9, end: 25 }]));
        // Un span qui couvre le mot-clé seul suffit aussi ; un span plus loin ne couvre ni l'un ni l'autre.
        assert!(!unread_article_mentions(text, &[Span { start: 0, end: 8 }]));
        assert!(unread_article_mentions(text, &[Span { start: 12, end: 25 }]));
    }

    #[test]
    fn a_hostile_text_stays_linear() {
        let text = "article ".repeat(50_000);
        assert!(!unread_article_mentions(&text, &[]));
        let numbered = "article 1 ".repeat(50_000);
        let spans: Vec<Span> =
            (0..50_000).map(|i| Span { start: i * 10, end: i * 10 + 9 }).collect();
        assert!(!unread_article_mentions(&numbered, &spans));
        assert!(unread_article_mentions(&numbered, &spans[1..]));
    }
}
