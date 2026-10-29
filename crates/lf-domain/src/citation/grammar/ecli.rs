//! Grammaire des identifiants ECLI (annexe B §4.10).
//!
//! `ECLI:pays:juridiction:année:ordinal`, casse indifférente en entrée, valeur en majuscules.
//! Un ECLI français devient `CitationRef::Ecli`, un autre pays une référence hors périmètre.
//! Les interprétations françaises (ordinal de la Cour de cassation, du Conseil d'État, du Conseil
//! constitutionnel) ne servent qu'à lever des drapeaux ; l'ECLI n'est jamais fabriqué. Les
//! plages des candidats sont des offsets du texte d'ORIGINE (`Normalized::orig_range`), celles
//! des ancres des offsets de `Normalized::text`.

#![allow(
    clippy::many_single_char_names,
    reason = "lecteur à curseur : `c` (fenêtre) et `i`, `s`, `p`, `e` (positions), comme dans la grammaire de l'annexe B"
)]

use crate::citation::dict::courts::{CcType, CeFormation};
use crate::citation::normalize::Normalized;
use crate::citation::scan::{Anchor, AnchorClass};
use crate::citation::types::{CitationRef, EcliFamily, Flag, UnsupportedCite, UnsupportedFamily};
use crate::ecli::Ecli;

use super::Candidate;
use super::cassation::lex::{self, Cur, Hit};

/// Classes d'ancres où un ECLI peut commencer.
const CLASSES: [AnchorClass; 1] = [AnchorClass::Ecli];

/// Grammaires de la famille, essayées à chaque début.
const GRAMMARS: [lex::Grammar; 1] = [ecli];

/// Préfixes de chambre de l'ordinal d'un ECLI de la Cour de cassation (`fr_ccass_ord`).
const CASS_PREFIXES: [&str; 9] = ["C1", "C2", "C3", "CO", "SO", "CR", "AP", "OR", "AV"];

/// Longueur maximale de l'ordinal.
const MAX_ORDINAL: usize = 25;

/// Candidats de la famille, tirés des ancres compatibles avec ses grammaires.
pub(crate) fn scan(n: &Normalized, anchors: &[Anchor]) -> Vec<Candidate> {
    lex::scan_family(n, anchors, &CLASSES, None, &GRAMMARS)
}

/// Nombre de caractères de `i` qui satisfont `ok`, au plus `max`.
fn run(c: &Cur<'_>, i: usize, max: usize, ok: fn(char) -> bool) -> usize {
    (0..max).take_while(|&k| c.ch(i + k).is_some_and(ok)).count()
}

/// Vrai si `text` compte entre `min` et `max` chiffres ASCII, et rien d'autre.
fn digits_only(text: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit())
}

/// `AAAA.N.TYPE` : année de l'ordinal d'un ECLI du Conseil constitutionnel.
fn cc_ordinal_year(ordinal: &str) -> Option<u16> {
    let mut parts = ordinal.split('.');
    let year = parts.next().filter(|y| digits_only(y, 4, 4))?;
    parts.next().filter(|n| digits_only(n, 1, 9))?;
    CcType::parse(parts.next()?)?;
    parts.next().is_none().then(|| {
        year.bytes()
            .fold(0_u16, |acc, b| acc.saturating_mul(10).saturating_add(u16::from(b - b'0')))
    })
}

/// Drapeaux d'un ECLI du Conseil constitutionnel.
fn cc_flags(year: u16, ordinal: &str) -> Vec<Flag> {
    match cc_ordinal_year(ordinal) {
        None => vec![Flag::EcliOrdinalNonConforme(EcliFamily::Cc)],
        Some(number_year) if year < number_year => vec![Flag::EcliAnneeAnterieureNumero],
        Some(_) => Vec::new(),
    }
}

/// Drapeaux d'un ECLI du Conseil d'État : ordinal `requête.AAAAMMJJ`.
fn ce_flags(ordinal: &str) -> Vec<Flag> {
    let shaped = ordinal
        .split_once('.')
        .is_some_and(|(request, date)| digits_only(request, 1, 6) && digits_only(date, 8, 8));
    if shaped { Vec::new() } else { vec![Flag::EcliOrdinalNonConforme(EcliFamily::Ce)] }
}

/// Drapeaux d'un ECLI de la Cour de cassation : préfixe de chambre puis cinq chiffres.
fn cass_flags(ordinal: &str) -> Vec<Flag> {
    let (prefix, number) = ordinal.split_at_checked(2).unwrap_or_default();
    let shaped = ordinal.len() == 7
        && prefix.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        && digits_only(number, 5, 5);
    if !shaped {
        vec![Flag::EcliOrdinalNonConforme(EcliFamily::Cass)]
    } else if CASS_PREFIXES.contains(&prefix) {
        Vec::new()
    } else {
        vec![Flag::ChambreCassInconnue(prefix.to_owned())]
    }
}

/// Drapeaux d'un ECLI français, selon sa juridiction (en majuscules).
fn french_flags(court: &str, year: u16, ordinal: &str) -> Vec<Flag> {
    match court {
        "CC" => cc_flags(year, ordinal),
        "CCASS" => cass_flags(ordinal),
        "CE" => [vec![Flag::EcliCodeCeSeul], ce_flags(ordinal)].concat(),
        other if CeFormation::from_ecli_court(other).is_some() => ce_flags(ordinal),
        _ => Vec::new(),
    }
}

/// `ecli` : cinq composants, ordinal de 25 caractères au plus (points compris, mais jamais
/// en dernière position).
fn ecli(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let p = c.kw(s, "ecli:")?;
    (run(c, p, 2, |ch| ch.is_ascii_alphabetic()) == 2 && c.is(p + 2, ':')).then_some(())?;
    let court = p + 3;
    c.ch(court).filter(char::is_ascii_alphabetic)?;
    let court_end = court + run(c, court, 7, |ch| ch.is_ascii_alphanumeric());
    c.is(court_end, ':').then_some(())?;
    let year = court_end + 1;
    let year_end = c.digits(year, 4, 4).filter(|&e| c.is(e, ':'))?;
    let ordinal = year_end + 1;
    c.ch(ordinal).filter(char::is_ascii_alphanumeric)?;
    let raw = run(c, ordinal, MAX_ORDINAL, |ch| ch.is_ascii_alphanumeric() || ch == '.');
    let len = (1..=raw).rev().find(|&k| !c.is(ordinal + k - 1, '.'))?;
    let end = ordinal + len;
    c.bound(end).then_some(())?;
    let text = c.text(s, end).to_uppercase();
    let value = if c.text(p, p + 2).eq_ignore_ascii_case("FR") {
        CitationRef::Ecli(Ecli::parse(&text).ok()?)
    } else {
        CitationRef::Unsupported(UnsupportedCite {
            family: UnsupportedFamily::ForeignEcli,
            key: Some(text),
        })
    };
    let flags = match &value {
        CitationRef::Ecli(_) => french_flags(
            &c.text(court, court_end).to_uppercase(),
            u16::try_from(c.val(year, year_end)).unwrap_or(0),
            &c.text(ordinal, end).to_uppercase(),
        ),
        _ => Vec::new(),
    };
    Some(Hit::new(s, end, value).with_flags(flags))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::super::cassation::testing::{matched, only, run};
    use super::*;
    use crate::citation::scan::AnchorClass::Ecli as EcliA;
    use crate::citation::types::{
        CitationRef, Confidence, EcliFamily, Flag, UnsupportedCite, UnsupportedFamily,
    };
    use crate::ecli::Ecli;

    fn ecli_of(candidate: &Candidate) -> &str {
        assert!(matches!(candidate.value, CitationRef::Ecli(_)), "{:?}", candidate.value);
        let found = match &candidate.value {
            CitationRef::Ecli(e) => Some(e.as_str()),
            _ => None,
        };
        found.expect("ECLI vérifié ci-dessus")
    }

    #[test]
    fn french_eclis_p150_to_p160() {
        for (input, span, want) in [
            (
                "ECLI:FR:CCASS:2025:C100399",
                "ECLI:FR:CCASS:2025:C100399",
                "ECLI:FR:CCASS:2025:C100399",
            ),
            (
                "Rendu public le 7 novembre 2025. ECLI:FR:CC:2025:2025.1173.QPC",
                "ECLI:FR:CC:2025:2025.1173.QPC",
                "ECLI:FR:CC:2025:2025.1173.QPC",
            ),
            (
                "ECLI:FR:CECHR:2026:506507.20260529",
                "ECLI:FR:CECHR:2026:506507.20260529",
                "ECLI:FR:CECHR:2026:506507.20260529",
            ),
            (
                "ECLI:FR:CCASS:2022:OR31631",
                "ECLI:FR:CCASS:2022:OR31631",
                "ECLI:FR:CCASS:2022:OR31631",
            ),
            (
                "ECLI:FR:CEORD:2021:454852.20210727",
                "ECLI:FR:CEORD:2021:454852.20210727",
                "ECLI:FR:CEORD:2021:454852.20210727",
            ),
            (
                "<ECLI>ECLI:FR:CC:2026:2025.1180.QPC</ECLI>",
                "ECLI:FR:CC:2026:2025.1180.QPC",
                "ECLI:FR:CC:2026:2025.1180.QPC",
            ),
            (
                "ECLI:FR:CESSR:2013:355099.20130301",
                "ECLI:FR:CESSR:2013:355099.20130301",
                "ECLI:FR:CESSR:2013:355099.20130301",
            ),
            (
                "ECLI:FR:CCASS:2013:CR00710",
                "ECLI:FR:CCASS:2013:CR00710",
                "ECLI:FR:CCASS:2013:CR00710",
            ),
            (
                "ecli:fr:ccass:2025:c100399",
                "ecli:fr:ccass:2025:c100399",
                "ECLI:FR:CCASS:2025:C100399",
            ),
        ] {
            let got = only(run(scan, input, &[(EcliA, &span[..8])]));
            assert_eq!(matched(input, &got), span, "{input}");
            assert_eq!(ecli_of(&got), want, "{input}");
            assert_eq!(got.flags.list, [], "{input}");
            assert_eq!(got.confidence, Confidence::High, "{input}");
        }
    }

    #[test]
    fn foreign_ecli_p159_is_unsupported() {
        let input = "ECLI:EU:C:2024:216";
        let got = only(run(scan, input, &[(EcliA, "ECLI")]));
        assert_eq!(matched(input, &got), input);
        assert_eq!(
            got.value,
            CitationRef::Unsupported(UnsupportedCite {
                family: UnsupportedFamily::ForeignEcli,
                key: Some("ECLI:EU:C:2024:216".into()),
            })
        );
    }

    #[test]
    fn suspect_eclis_get_their_flags_n11_to_n14() {
        for (input, flags) in [
            ("ECLI:FR:CE:2024:123456.20240101", vec![Flag::EcliCodeCeSeul]),
            ("ECLI:FR:CC:2023:123.QPC", vec![Flag::EcliOrdinalNonConforme(EcliFamily::Cc)]),
            ("ECLI:FR:CC:2023:2023.12.XYZ", vec![Flag::EcliOrdinalNonConforme(EcliFamily::Cc)]),
            ("ECLI:FR:CCASS:2099:XX99999", vec![Flag::ChambreCassInconnue("XX".into())]),
            ("ECLI:FR:CCASS:2025:C10039", vec![Flag::EcliOrdinalNonConforme(EcliFamily::Cass)]),
            ("ECLI:FR:CECHR:2026:506507.2026", vec![Flag::EcliOrdinalNonConforme(EcliFamily::Ce)]),
            ("ECLI:FR:CESSR:2026:abc.20260101", vec![Flag::EcliOrdinalNonConforme(EcliFamily::Ce)]),
            ("ECLI:FR:CC:2024:2025.1173.QPC", vec![Flag::EcliAnneeAnterieureNumero]),
            // Juridiction sans interprétation française : aucun drapeau.
            ("ECLI:FR:XYZ:2020:123", vec![]),
            // N14 : l'ordinal CE est bien formé, la date n'est pas vérifiée ici.
            ("ECLI:FR:CECHR:2026:506507.20260530", vec![]),
        ] {
            let got = only(run(scan, input, &[(EcliA, "ECLI")]));
            assert_eq!(matched(input, &got), input);
            assert_eq!(got.flags.list, flags, "{input}");
            assert!(Ecli::parse(ecli_of(&got)).is_ok());
        }
    }

    #[test]
    fn sentence_punctuation_and_limits() {
        let input = "voir ECLI:FR:CCASS:2025:C100399. Puis la suite";
        let got = only(run(scan, input, &[(EcliA, "ECLI")]));
        assert_eq!(matched(input, &got), "ECLI:FR:CCASS:2025:C100399");
        let longest = format!("ECLI:FR:CC:2025:{}", "1".repeat(25));
        let got = only(run(scan, &longest, &[(EcliA, "ECLI")]));
        assert_eq!(matched(&longest, &got), longest);
        for input in [
            "ECLI:FR:CCASS:2025",
            "ECLI:FR:CCASS:25:C100399",
            "ECLI:F:CCASS:2025:C100399",
            "ECLI:FR:1CCASS:2025:C100399",
            "ECLI:FR:ABCDEFGH:2025:C100399",
            "ECLI:FR:CC:2025:.1173",
            &format!("ECLI:FR:CC:2025:{}", "1".repeat(26)),
            "ECLI:FR:CC:20250:1",
            "XECLI:FR:CC:2025:1",
        ] {
            assert_eq!(run(scan, input, &[(EcliA, "ECLI")]), [], "{input}");
        }
    }
}
