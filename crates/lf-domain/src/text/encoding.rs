//! Réparation du mojibake latin-1.
//!
//! Règle nouvelle, absente de la référence, qui ne répare rien (00 §8 et 04 §10.2,
//! motif de `docs/plan/annexes/donnees/scripts/dto_repair_check.py` l.16 ; six des
//! seize fixtures de la référence portent ce mojibake, 04 §10.1). Tout ou rien : la
//! réparation porte sur le texte entier (l'adapter l'applique au corps de réponse,
//! avant serde) et n'est acceptée que si elle rend de l'UTF-8 strict sans aucun
//! motif résiduel. Elle précède toute normalisation des espaces : la séquence de
//! `à` est `Ã` suivi d'une NBSP (04 §10.4), que `clean` replierait. Fonctions
//! pures, sans E/S ni journal : l'adapter tire ses compteurs du résultat
//! (04 §10.5).

use std::borrow::Cow;

/// Vrai si `s` contient un contrôle C1 (U+0080 à U+009F) n'importe où, ou une paire
/// adjacente tête UTF-8 (U+00C2 à U+00F4) puis continuation (U+0080 à U+00BF)
/// (04 §10.2, étape 1).
///
/// Détection seule : l'adapter la garde quand la réparation est coupée (04 §10.5).
/// Faux positif connu : une lettre accentuée suivie d'une NBSP
/// (`consult_juri_constit.json:68-69`).
pub fn has_latin1_mojibake(s: &str) -> bool {
    let c1 = s.chars().any(|c| matches!(c, '\u{80}'..='\u{9f}'));
    let pair = s.chars().zip(s.chars().skip(1)).any(|(head, cont)| {
        matches!(head, '\u{c2}'..='\u{f4}') && matches!(cont, '\u{80}'..='\u{bf}')
    });
    c1 || pair
}

/// Répare le texte lu en latin-1 au lieu d'UTF-8.
///
/// Sans motif, rend `Cow::Borrowed(s)`. Sinon ré-encode en latin-1, puis relit en
/// UTF-8. `Cow::Owned` si et seulement si la réparation est acceptée ; tout refus
/// rend `Cow::Borrowed(s)`. Refus : un caractère au-delà de U+00FF (le texte n'a
/// pas été lu en latin-1), des octets qui ne forment pas de l'UTF-8 strict, ou un
/// résultat qui porte encore un motif. Idempotente.
pub fn repair_latin1_mojibake(s: &str) -> Cow<'_, str> {
    if !has_latin1_mojibake(s) {
        return Cow::Borrowed(s);
    }
    let Ok(bytes) = s.chars().map(u8::try_from).collect::<Result<Vec<u8>, _>>() else {
        return Cow::Borrowed(s);
    };
    match String::from_utf8(bytes) {
        Ok(repaired) if !has_latin1_mojibake(&repaired) => Cow::Owned(repaired),
        _ => Cow::Borrowed(s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::clean;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    /// Texte rendu, et `true` si `Cow::Owned` (réparation acceptée).
    fn outcome(s: &str) -> (String, bool) {
        let repaired = repair_latin1_mojibake(s);
        let owned = matches!(repaired, Cow::Owned(_));
        (repaired.into_owned(), owned)
    }

    /// Lecture en latin-1 des octets UTF-8 de `s` : le mojibake à réparer.
    fn misdecode(s: &str) -> String {
        s.bytes().map(char::from).collect()
    }

    #[test]
    fn detection_table() {
        let cases = [
            ("", false),
            ("abc", false),
            // `é` sain suivi d'ASCII : tête sans continuation.
            ("\u{e9}t\u{e9}", false),
            ("R\u{c3}\u{a9}publique", true),
            // Contrôles C1, bornes et milieu.
            ("\u{80}", true),
            ("a\u{9f}", true),
            ("\u{85}", true),
            // DEL n'est pas un C1, et U+007F n'est pas une tête.
            ("\u{7f}\u{a0}", false),
            // Bornes de la tête (U+00C2 à U+00F4) et de la continuation (U+0080 à U+00BF).
            ("\u{c2}\u{a0}", true),
            ("\u{c1}\u{a0}", false),
            ("\u{f4}\u{bf}", true),
            ("\u{f5}\u{a0}", false),
            ("\u{c3}\u{c0}", false),
            ("\u{c3}\u{7f}", false),
            // Ordre inversé : continuation, puis tête.
            ("\u{a9}\u{c3}", false),
            // Faux positif réel : `é` suivi d'une NBSP (consult_juri_constit.json:68-69).
            ("constitutionnalit\u{e9}\u{a0};", true),
            // Forme CP1252 de `’` (V-73) : hors du motif latin-1.
            ("\u{e2}\u{20ac}\u{2122}", false),
        ];
        for (s, expected) in cases {
            assert_eq!((s, has_latin1_mojibake(s)), (s, expected));
        }
    }

    #[test]
    fn text_without_pattern_is_borrowed() {
        // Étape 2 : aucun motif, aucune réparation.
        for input in ["", "abc", "\u{e9}t\u{e9}", "constitutionnalit\u{e9} ;"] {
            assert_eq!(outcome(input), (input.to_owned(), false));
        }
    }

    #[test]
    fn repairs_fixture_excerpts() {
        let cases = [
            // consult_loda.json:7 (`title`).
            (
                "LOI n\u{c2}\u{b0} 2016-1321 du 7 octobre 2016 pour une R\u{c3}\u{a9}publique num\u{c3}\u{a9}rique (1)",
                "LOI n\u{b0} 2016-1321 du 7 octobre 2016 pour une R\u{e9}publique num\u{e9}rique (1)",
            ),
            // consult_juri.json:69 (`texte`) : le C1 U+0088 est la continuation de `È`.
            (
                "LA COUR DE CASSATION, PREMI\u{c3}\u{88}RE CHAMBRE CIVILE",
                "LA COUR DE CASSATION, PREMI\u{c8}RE CHAMBRE CIVILE",
            ),
            // search_juri.json:291 (clé de facette).
            ("Juridictions du premier degr\u{c3}\u{a9}", "Juridictions du premier degr\u{e9}"),
            // consult_article.json:8 (`texte`) : la NBSP est la continuation de `à`.
            ("propres \u{c3}\u{a0} emp\u{c3}\u{aa}cher", "propres \u{e0} emp\u{ea}cher"),
        ];
        for (input, expected) in cases {
            assert_eq!(outcome(input), (expected.to_owned(), true));
        }
    }

    #[test]
    fn repairs_synthetic_sequences() {
        let cases = [
            // Forme latin-1 de `’` (trois octets).
            ("\u{e2}\u{80}\u{99}", "\u{2019}"),
            // Quatre octets (tête F0).
            ("\u{f0}\u{9f}\u{98}\u{80}", "\u{1f600}"),
            // NBSP en mojibake.
            ("\u{c2}\u{a0}", "\u{a0}"),
            // U+00FF, dernier caractère latin-1.
            ("\u{c3}\u{bf}", "\u{ff}"),
        ];
        for (input, expected) in cases {
            assert_eq!(outcome(input), (expected.to_owned(), true));
        }
    }

    #[test]
    fn repairs_whole_body_and_keeps_json_structure() {
        // La structure JSON est ASCII, identique en latin-1 et en UTF-8 (04 §10.2) ;
        // la clé de facette (search_juri.json:291) est réparée comme la valeur.
        let body = "{\"title\":\"R\u{c3}\u{a9}publique\",\"facets\":{\"Juridictions du premier degr\u{c3}\u{a9}\":620}}";
        let repaired = "{\"title\":\"R\u{e9}publique\",\"facets\":{\"Juridictions du premier degr\u{e9}\":620}}";
        assert_eq!(outcome(body), (repaired.to_owned(), true));
    }

    #[test]
    fn does_not_touch_text_with_chars_above_ff() {
        // Étape 3 : un caractère au-delà de U+00FF (`’`, `œ`, U+0100) prouve que le
        // texte n'a pas été lu en latin-1, même si le reste porte un motif.
        for input in [
            "R\u{c3}\u{a9}publique \u{2019}",
            "\u{c5}\u{93}uvre \u{c3}\u{a9}\u{153}",
            "\u{c3}\u{a9}\u{100}",
        ] {
            assert_eq!(outcome(input), (input.to_owned(), false));
        }
    }

    #[test]
    fn refuses_invalid_utf8() {
        // Étape 5 : les octets latin-1 ne forment pas de l'UTF-8 strict.
        for input in [
            // Faux positif de détection (consult_juri_constit.json:68-69) : `E9 A0`.
            "constitutionnalit\u{e9}\u{a0};",
            // Après F4, la continuation doit être dans 80-8F.
            "\u{f4}\u{a0}",
            // C1 isolé : continuation orpheline.
            "a\u{80}b",
            // Séquence tronquée.
            "\u{c3}\u{a9}\u{c3}",
        ] {
            assert_eq!(outcome(input), (input.to_owned(), false));
        }
    }

    #[test]
    fn refuses_residual_pattern() {
        // Étape 6 : le résultat porte encore un motif, C1 compris.
        for input in [
            // Double mojibake de `é` : le résultat `Ã©` est encore un motif.
            "\u{c3}\u{83}\u{c2}\u{a9}",
            // Le résultat, `é` suivi de U+0085, contient un C1.
            "\u{c3}\u{a9}\u{c2}\u{85}",
        ] {
            assert_eq!(outcome(input), (input.to_owned(), false));
        }
    }

    #[test]
    fn accent_before_nbsp_is_a_known_false_refusal() {
        // Typographie française saine (`é` et NBSP devant « ; »,
        // consult_juri_constit.json:68-69) passée en mojibake : la réparation
        // rendrait l'original, qui porte lui-même un motif, donc l'étape 6 la
        // refuse. Limite de la règle de 00 §8, gardée telle quelle (V-72, V-73).
        let original = "constitutionnalit\u{e9}\u{a0};";
        let corrupted = "constitutionnalit\u{c3}\u{a9}\u{c2}\u{a0};";
        assert_eq!(misdecode(original), corrupted);
        assert_eq!(outcome(corrupted), (corrupted.to_owned(), false));
    }

    #[test]
    fn nbsp_normalization_before_repair_is_a_bug() {
        // consult_article.json:8 : la séquence de `à` est `Ã` suivi d'une NBSP (04 §10.4).
        let field = "propres \u{c3}\u{a0} emp\u{c3}\u{aa}cher";
        assert_eq!(outcome(field), ("propres \u{e0} emp\u{ea}cher".to_owned(), true));
        // NBSP normalisée d'abord : `C3 20` est invalide, et tout le champ reste en
        // mojibake (refus de l'étape 5), `ê` compris.
        let normalized = field.replace('\u{a0}', " ");
        assert_eq!(normalized, "propres \u{c3}  emp\u{c3}\u{aa}cher");
        assert!(has_latin1_mojibake(&normalized));
        assert_eq!(outcome(&normalized), (normalized.clone(), false));
    }

    #[test]
    fn clean_folds_whitespace_after_repair() {
        // Ordre du pipeline (03 §7.1) : réparer, puis `clean`, qui replie la NBSP
        // rendue par la réparation.
        let field = "art.\u{c2}\u{a0}9 : propres \u{c3}\u{a0} emp\u{c3}\u{aa}cher";
        let repaired = repair_latin1_mojibake(field);
        assert_eq!(repaired, "art.\u{a0}9 : propres \u{e0} emp\u{ea}cher");
        assert_eq!(clean(&repaired), "art. 9 : propres \u{e0} emp\u{ea}cher");
        // Ordre inverse : `clean` replie la NBSP des paires, le mojibake reste.
        assert_eq!(
            repair_latin1_mojibake(&clean(field)),
            "art.\u{c2} 9 : propres \u{c3} emp\u{c3}\u{aa}cher"
        );
    }

    /// Chaînes qui mêlent tout `char`, les caractères U+0080 à U+00FF (C1
    /// compris, que `any::<String>()`, soit `\PC*`, ne tire jamais), `é` et
    /// l'ASCII.
    fn mixed() -> impl Strategy<Value = String> {
        proptest::collection::vec(
            prop_oneof![
                any::<char>(),
                (0x80_u8..=0xff).prop_map(char::from),
                Just('\u{e9}'),
                Just('a'),
            ],
            0..48,
        )
        .prop_map(String::from_iter)
    }

    /// `mixed()`, ou sa lecture en latin-1 : un mojibake souvent réparable.
    fn texts() -> impl Strategy<Value = String> {
        prop_oneof![mixed(), mixed().prop_map(|s| misdecode(&s))]
    }

    proptest! {
        // 06 §2.1 : la lecture en latin-1 des octets UTF-8 de `s` est réparée,
        // sauf si `s` porte lui-même un motif (refus de l'étape 6) ; un caractère
        // au-delà de U+00FF bloque toute réparation (étape 3).
        #[test]
        fn repair_mojibake_inverts_latin1_misdecoding(
            s in mixed(),
            high in any::<char>().prop_filter("au-delà de U+00FF", |c| *c > '\u{ff}'),
        ) {
            let corrupted = misdecode(&s);
            let expected = if has_latin1_mojibake(&s) { corrupted.clone() } else { s.clone() };
            prop_assert_eq!(repair_latin1_mojibake(&corrupted), expected);
            let blocked = format!("{corrupted}{high}");
            prop_assert_eq!(outcome(&blocked), (blocked.clone(), false));
        }

        // 04 §10.3 : un second passage est l'identité (`Cow::Borrowed`).
        #[test]
        fn idempotent(s in texts()) {
            let once = repair_latin1_mojibake(&s).into_owned();
            prop_assert_eq!(outcome(&once), (once.clone(), false));
        }

        // 04 §10.6 : `repair(s)` vaut `s`, ou bien son encodage UTF-8 est
        // l'encodage latin-1 de `s` et il ne contient plus de motif ; `Cow::Owned`
        // si et seulement si le texte change.
        #[test]
        fn repair_is_identity_or_latin1_faithful(s in texts()) {
            let (repaired, owned) = outcome(&s);
            prop_assert_eq!(owned, repaired != s);
            if owned {
                let latin1: Vec<u8> = s.chars().map(|c| u8::try_from(c).unwrap()).collect();
                prop_assert_eq!(repaired.as_bytes(), latin1.as_slice());
                prop_assert!(!has_latin1_mojibake(&repaired));
            }
        }

        #[test]
        fn ascii_is_untouched(s in "[ -~\t\n]{0,64}") {
            prop_assert!(!has_latin1_mojibake(&s));
            prop_assert_eq!(outcome(&s), (s.clone(), false));
        }
    }
}
