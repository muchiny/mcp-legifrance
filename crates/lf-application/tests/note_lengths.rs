//! Garde globale de la longueur des notes (plan 1.1.1, section 4.4 « Après la vague 3 », R1-17) :
//! chaque variante de [`Note`], construite avec les paramètres les plus longs qu'elle puisse
//! recevoir, se rend en 200 caractères au plus (`NOTE_MAX_CHARS`, annexe B §7.10, assainissement
//! des sorties de 05 §5.9).
//!
//! Ce que ce fichier ajoute aux gardes qui existent déjà (`citation/notes.rs`, `mod tests` :
//! `notes_are_static_templates`, `every_absence_note_stays_within_200_chars_with_the_new_hint`,
//! `the_amending_note_survives_quote_gap_subdivision_and_range`, …) : elles mesurent des variantes
//! choisies à la main ; celui-ci est **exhaustif par construction**.
//!
//! - `kind_of` est un `match` sur `Note` **sans bras joker** : une variante ajoutée à `Note` ne
//!   compile plus tant qu'elle n'a pas de bras ici, donc de `Kind`.
//! - `longest` est un `match` sur `Kind` **sans bras joker** : un `Kind` sans jeu de valeurs les
//!   plus longues ne compile pas non plus.
//! - L'énumération `Kind` et sa liste `ALL_KINDS` sortent de la même macro : aucune liste à tenir à
//!   jour à la main.
//!
//! Les paramètres `&'static str` (familles, juridictions, motifs, faits, index) sont des
//! vocabulaires fermés que seul le code de production construit (`citation/resolve.rs` :
//! `fact_of`, `out_of_scope`, `not_supported`, `CouldNotConfirm { index }`, …). Ils sont recopiés
//! ici avec le plus long de chaque famille ; chaque champ reçoit en plus une valeur de 300
//! caractères, pour que la borne tienne même si un vocabulaire s'allonge. Les titres de codes sont
//! lus dans le dictionnaire (`CODES`), jamais recopiés.
//!
//! # Ce que « 200 caractères » prouve, et ce qu'il ne prouve pas
//!
//! `Note::text` finit par un rognage brut à 200 caractères (`messages::clip`) : `<= 200` est donc
//! vrai de toute note, quel que soit son gabarit, et ne garde que ce rognage. Ce qu'on veut garder
//! (R1-17), c'est que le rognage ne mange jamais le contenu utile d'une note réelle. Trois gardes
//! s'y emploient, à côté de la borne :
//!
//! - `every_realistic_note_is_whole_and_not_clipped` mesure à part les notes sans paramètre
//!   démesuré (`is_absurd` écarte `LONG_STATIC`, les valeurs de 300 caractères, la clé de 120
//!   chiffres, le titre de loi de 600 caractères) : elles finissent par la ponctuation de leur
//!   gabarit et, hors des variantes qui s'adaptent au plafond, restent strictement sous 200 ;
//! - `a_chain_keeps_whole_parts_only_...` : une chaîne ne contient que des notes entières, dans
//!   l'ordre (`join_fitting`) ;
//! - `realistic_long_notes_are_rendered_whole` épingle le texte exact de notes réalistes longues.
//!
//! Limite assumée : les vocabulaires fermés sont recopiés ci-dessus à la main ; une entrée de
//! production plus longue que la plus longue recopiée ne s'y voit pas avant d'y être ajoutée.
//!
//! Tout le fichier est sous `cfg(test)` : les aides partagent les règles de lint des tests
//! (`unwrap` et `expect` permis, `clippy.toml`).

#[cfg(test)]
mod note_lengths {
    use std::collections::BTreeSet;

    use lf_application::citation::{Field, NotCheckedReason, Note, notes::NOTE_MAX_CHARS};
    use lf_domain::{
        citation::{CODES, Sigle},
        date::{CivilDate, VersionDate},
        ecli::Ecli,
        fond::Fond,
    };

    // ---- l'exhaustivité par construction -------------------------------------------------------

    /// Fabrique `Kind` (une valeur par variante de `Note`) et `ALL_KINDS` depuis une seule liste.
    macro_rules! kinds {
        ($($name:ident),+ $(,)?) => {
            /// Une variante de `Note`, sans ses données.
            #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
            enum Kind { $($name),+ }

            /// Toutes les variantes de `Kind`.
            const ALL_KINDS: &[Kind] = &[$(Kind::$name),+];
        };
    }

    kinds!(
        NotInForce,
        NoRecord,
        NoRecordAtDate,
        Mismatch,
        Mismatches,
        ChamberNotCompared,
        HeadNotRead,
        OnlyFirstNumberChecked,
        OnlyOneNumberChecked,
        DateNotChecked,
        NativeEcli,
        Impossible,
        AmbiguousSigle,
        ReadAs,
        MissingTarget,
        NoNumber,
        NoCourt,
        NoAgreement,
        Candidates,
        OutOfScope,
        OutOfCoverage,
        NotSupported,
        NumberlessText,
        CouldNotConfirm,
        NotChecked,
        CodeNotListed,
        VersionSince,
        NotCurrentVersion,
        ArticleWithoutVersion,
        SubdivisionNotChecked,
        QuoteNotAttached,
        ArticleNotRead,
        Chain,
        RangeEndpointsOnly,
        WithRange,
        RangeEndNotFound,
        QuoteMismatch,
        QuoteTooLong,
        WithQuoteMismatch,
        NotInSandbox,
        NotInSnapshot,
        NoCitationRecognised,
        OtherCitations,
        UseSearch,
        NoDecisionReferences,
        DecisionLinkCodes,
    );

    /// La variante de `note`. Aucun bras joker : une variante de plus dans `Note` ne compile pas.
    const fn kind_of(note: &Note) -> Kind {
        match note {
            Note::NotInForce { .. } => Kind::NotInForce,
            Note::NoRecord { .. } => Kind::NoRecord,
            Note::NoRecordAtDate { .. } => Kind::NoRecordAtDate,
            Note::Mismatch { .. } => Kind::Mismatch,
            Note::Mismatches(_) => Kind::Mismatches,
            Note::ChamberNotCompared => Kind::ChamberNotCompared,
            Note::HeadNotRead => Kind::HeadNotRead,
            Note::OnlyFirstNumberChecked => Kind::OnlyFirstNumberChecked,
            Note::OnlyOneNumberChecked => Kind::OnlyOneNumberChecked,
            Note::DateNotChecked => Kind::DateNotChecked,
            Note::NativeEcli(_) => Kind::NativeEcli,
            Note::Impossible { .. } => Kind::Impossible,
            Note::AmbiguousSigle { .. } => Kind::AmbiguousSigle,
            Note::ReadAs { .. } => Kind::ReadAs,
            Note::MissingTarget { .. } => Kind::MissingTarget,
            Note::NoNumber { .. } => Kind::NoNumber,
            Note::NoCourt => Kind::NoCourt,
            Note::NoAgreement => Kind::NoAgreement,
            Note::Candidates { .. } => Kind::Candidates,
            Note::OutOfScope { .. } => Kind::OutOfScope,
            Note::OutOfCoverage { .. } => Kind::OutOfCoverage,
            Note::NotSupported { .. } => Kind::NotSupported,
            Note::NumberlessText => Kind::NumberlessText,
            Note::CouldNotConfirm { .. } => Kind::CouldNotConfirm,
            Note::NotChecked { .. } => Kind::NotChecked,
            Note::CodeNotListed => Kind::CodeNotListed,
            Note::VersionSince(_) => Kind::VersionSince,
            Note::NotCurrentVersion { .. } => Kind::NotCurrentVersion,
            Note::ArticleWithoutVersion { .. } => Kind::ArticleWithoutVersion,
            Note::SubdivisionNotChecked => Kind::SubdivisionNotChecked,
            Note::QuoteNotAttached => Kind::QuoteNotAttached,
            Note::ArticleNotRead => Kind::ArticleNotRead,
            Note::Chain(_) => Kind::Chain,
            Note::RangeEndpointsOnly => Kind::RangeEndpointsOnly,
            Note::WithRange(_) => Kind::WithRange,
            Note::RangeEndNotFound { .. } => Kind::RangeEndNotFound,
            Note::QuoteMismatch => Kind::QuoteMismatch,
            Note::QuoteTooLong => Kind::QuoteTooLong,
            Note::WithQuoteMismatch(_) => Kind::WithQuoteMismatch,
            Note::NotInSandbox { .. } => Kind::NotInSandbox,
            Note::NotInSnapshot { .. } => Kind::NotInSnapshot,
            Note::NoCitationRecognised => Kind::NoCitationRecognised,
            Note::OtherCitations { .. } => Kind::OtherCitations,
            Note::UseSearch => Kind::UseSearch,
            Note::NoDecisionReferences => Kind::NoDecisionReferences,
            Note::DecisionLinkCodes => Kind::DecisionLinkCodes,
        }
    }

    /// Les mêmes garde-fous pour les deux types de paramètres fermés que `Note` porte : un champ
    /// ou un motif de plus ne compile pas sans être mesuré ci-dessous.
    const fn field_slot(field: Field) -> usize {
        match field {
            Field::Date => 0,
            Field::Number => 1,
            Field::Chamber => 2,
            Field::Formation => 3,
            Field::Seat => 4,
            Field::Ecli => 5,
            Field::Nature => 6,
            Field::CcType => 7,
            Field::Code => 8,
            Field::ArticleNum => 9,
        }
    }

    /// Tous les champs, dans l'ordre de `field_slot` (vérifié par `every_field_and_reason_is_listed`).
    const ALL_FIELDS: [Field; 10] = [
        Field::Date,
        Field::Number,
        Field::Chamber,
        Field::Formation,
        Field::Seat,
        Field::Ecli,
        Field::Nature,
        Field::CcType,
        Field::Code,
        Field::ArticleNum,
    ];

    const fn reason_slot(reason: NotCheckedReason) -> usize {
        match reason {
            NotCheckedReason::Upstream => 0,
            NotCheckedReason::TimeBudget => 1,
        }
    }

    const ALL_REASONS: [NotCheckedReason; 2] =
        [NotCheckedReason::Upstream, NotCheckedReason::TimeBudget];

    // ---- les valeurs les plus longues ----------------------------------------------------------

    /// 300 caractères, accents compris (points de code multi-octets) : pour tout paramètre
    /// `&'static str`, bien au-delà du plus long vocabulaire connu.
    const LONG_STATIC: &str = concat!(
        "éééééééééééééééééééééééééééééééééééééééééééééééééé",
        "éééééééééééééééééééééééééééééééééééééééééééééééééé",
        "éééééééééééééééééééééééééééééééééééééééééééééééééé",
        "éééééééééééééééééééééééééééééééééééééééééééééééééé",
        "éééééééééééééééééééééééééééééééééééééééééééééééééé",
        "éééééééééééééééééééééééééééééééééééééééééééééééééé",
    );

    /// Le plus long de chaque vocabulaire fermé du moteur, plus `LONG_STATIC`.
    const FAMILIES: [&str; 5] = ["article", "identifier", "text", "code article", LONG_STATIC];
    const COURTS: [&str; 6] =
        ["Cour d'appel", "tribunal judiciaire", "lower court", "CAA", "TA", LONG_STATIC];
    const INDEXES: [&str; 9] =
        ["LODA", "JURI", "CETAT", "CONSTIT", "CNIL", "KALI", "LEGI", "ELI", LONG_STATIC];
    const SCOPES: [&str; 7] = [
        "EU law",
        "CJEU case law",
        "ECHR law and case law",
        "Legal scholarship",
        "A law report",
        "A foreign ECLI",
        LONG_STATIC,
    ];
    const WHATS: [&str; 11] = [
        "JORF",
        "amendment",
        "publication reference",
        "preamble paragraph",
        "this kind of citation",
        "ECLI of this court",
        "date out of range",
        "version date unknown",
        "an ELI this long",
        "this kind of identifier",
        LONG_STATIC,
    ];
    const FACTS: [&str; 9] = [
        "The date does not exist",
        "The year is in the future",
        "There is no such civil chamber at the Cour de cassation",
        "There is no such chamber at the Cour de cassation",
        "The ECLI ordinal does not follow the form of that court",
        "The ECLI year is earlier than the year of the number",
        "The year of the appeal number is after the decision year",
        "The citation is impossible",
        LONG_STATIC,
    ];
    /// Codes d'état d'une version qui n'est plus en vigueur (`replaced_code` de `resolve.rs`).
    const ETATS: [&str; 11] = [
        "ABROGE",
        "MODIFIE",
        "MODIFIE_MORT_NE",
        "PERIME",
        "ANNULE",
        "DISJOINT",
        "TRANSFERE",
        "REMPLACE",
        "DENONCE",
        "SUBSTITUE",
        LONG_STATIC,
    ];

    /// Un titre de loi de 600 caractères accentués : la citation d'un enregistrement de l'API, seul
    /// paramètre libre d'une note (`NotInForce`, `ArticleWithoutVersion` d'un texte non codifié).
    fn long_law_title() -> String {
        let mut title = String::from("LOI n° 2016-1321 du 7 octobre 2016 ");
        while title.chars().count() < 600 {
            title.push_str("pour une République numérique et à l'économie circulaire (1) ; ");
        }
        title
    }

    /// Libellés de texte rendus tels quels dans une note : un titre de code (le plus long du
    /// dictionnaire), un identifiant `LEGITEXT`, un titre de loi de 600 caractères.
    fn labels() -> Vec<String> {
        let mut labels: Vec<String> = CODES.iter().map(|entry| entry.title.to_owned()).collect();
        labels.push("LEGITEXT000006070721".to_owned());
        labels.push(long_law_title());
        labels
    }

    /// Clés d'article : une forme réaliste la plus longue, une démesurée.
    fn nums() -> Vec<String> {
        vec!["R*4321-123-1-2 quinquies A".to_owned(), "1".repeat(120)]
    }

    /// Valeurs canoniques (numéro de pourvoi, de délibération, d'IDCC, titre) : réaliste, démesurée.
    fn values() -> Vec<String> {
        vec!["SAN-2026-002 / 14-24.444 / 410000".to_owned(), "é".repeat(300)]
    }

    fn version(text: &str) -> VersionDate {
        VersionDate::parse(text).unwrap()
    }

    fn civil(year: u16, month: u8, day: u8) -> CivilDate {
        CivilDate::new(year, month, day).unwrap()
    }

    /// Cinq dates distinctes, le maximum rendu par `NoRecordAtDate` (`FOUND_DATES_MAX`).
    fn five_dates() -> Vec<CivilDate> {
        (1..=5).map(|day| civil(1989, 12, day)).collect()
    }

    /// Un ECLI de la longueur maximale admise (7 + 25 caractères de juridiction et d'ordinal).
    fn longest_ecli() -> Ecli {
        Ecli::parse(&format!("ECLI:FR:ABCDEF7:2025:{}", "1".repeat(25))).unwrap()
    }

    /// Les notes simples (sans note enveloppée) de chaque variante qui porte un paramètre libre
    /// ou fermé, aux valeurs les plus longues. Sert aussi de matière aux chaînes et enveloppes.
    #[expect(clippy::too_many_lines, reason = "une branche par variante de `Note`")]
    fn longest(kind: Kind) -> Vec<Note> {
        match kind {
            Kind::NotInForce => {
                let mut notes = Vec::new();
                for code in labels() {
                    for num in nums() {
                        for date_given in [false, true] {
                            notes.push(Note::NotInForce {
                                code: code.clone(),
                                num: num.clone(),
                                date: version("2026-12-31"),
                                date_given,
                            });
                        }
                    }
                }
                notes
            }
            Kind::NoRecord => FAMILIES
                .iter()
                .copied()
                .flat_map(|family| {
                    values().into_iter().flat_map(move |value| {
                        [None, Some(version("2026-12-31"))].map(|date| Note::NoRecord {
                            family,
                            value: value.clone(),
                            date,
                        })
                    })
                })
                .collect(),
            Kind::NoRecordAtDate => values()
                .into_iter()
                .flat_map(|number| {
                    [
                        (0_usize, five_dates()),
                        (usize::MAX, five_dates()),
                        (3, vec![civil(2004, 1, 19)]),
                    ]
                    .map(|(more, found)| Note::NoRecordAtDate {
                        n: usize::MAX,
                        number: number.clone(),
                        date: civil(2021, 3, 17),
                        found,
                        more,
                    })
                })
                .collect(),
            Kind::Mismatch => ALL_FIELDS
                .into_iter()
                .flat_map(|field| {
                    values().into_iter().map(move |cited| Note::Mismatch {
                        field,
                        actual: cited.clone(),
                        cited,
                    })
                })
                .collect(),
            Kind::Mismatches => {
                // Tous les champs en écart à la fois, valeurs citées et officielles démesurées.
                let all: Vec<(Field, String, String)> = ALL_FIELDS
                    .into_iter()
                    .map(|field| (field, "é".repeat(300), "é".repeat(300)))
                    .collect();
                let realistic: Vec<(Field, String, String)> = vec![
                    (Field::Date, "2026-10-03".to_owned(), "2026-10-02".to_owned()),
                    (Field::Chamber, "CIV1".to_owned(), "SOC".to_owned()),
                    (Field::Seat, "Marseille".to_owned(), "Lyon".to_owned()),
                    (Field::CcType, "QPC".to_owned(), "DC".to_owned()),
                ];
                vec![Note::Mismatches(all), Note::Mismatches(realistic)]
            }
            Kind::ChamberNotCompared => vec![Note::ChamberNotCompared],
            Kind::HeadNotRead => vec![Note::HeadNotRead],
            Kind::OnlyFirstNumberChecked => vec![Note::OnlyFirstNumberChecked],
            Kind::OnlyOneNumberChecked => vec![Note::OnlyOneNumberChecked],
            Kind::DateNotChecked => vec![Note::DateNotChecked],
            Kind::NativeEcli => vec![Note::NativeEcli(longest_ecli())],
            Kind::Impossible => {
                FACTS.iter().copied().map(|fact| Note::Impossible { fact }).collect()
            }
            Kind::AmbiguousSigle => {
                Sigle::ALL.into_iter().map(|sigle| Note::AmbiguousSigle { sigle }).collect()
            }
            Kind::ReadAs => Sigle::ALL
                .into_iter()
                .flat_map(|sigle| {
                    CODES.iter().map(move |entry| Note::ReadAs { sigle, code: entry.title })
                })
                .chain(
                    Sigle::ALL.into_iter().map(|sigle| Note::ReadAs { sigle, code: LONG_STATIC }),
                )
                .collect(),
            Kind::MissingTarget => {
                nums().into_iter().map(|num| Note::MissingTarget { num }).collect()
            }
            Kind::NoNumber => Fond::ALL
                .into_iter()
                .flat_map(|fond| {
                    [None, Some(civil(2018, 3, 28))].map(|date| Note::NoNumber { fond, date })
                })
                .collect(),
            Kind::NoCourt => vec![Note::NoCourt],
            Kind::NoAgreement => vec![Note::NoAgreement],
            Kind::Candidates => vec![Note::Candidates { n: usize::MAX }],
            Kind::OutOfScope => {
                SCOPES.iter().copied().map(|family| Note::OutOfScope { family }).collect()
            }
            Kind::OutOfCoverage => {
                COURTS.iter().copied().map(|court| Note::OutOfCoverage { court }).collect()
            }
            Kind::NotSupported => {
                WHATS.iter().copied().map(|what| Note::NotSupported { what }).collect()
            }
            Kind::NumberlessText => vec![Note::NumberlessText],
            Kind::CouldNotConfirm => {
                INDEXES.iter().copied().map(|index| Note::CouldNotConfirm { index }).collect()
            }
            Kind::NotChecked => {
                ALL_REASONS.into_iter().map(|reason| Note::NotChecked { reason }).collect()
            }
            Kind::CodeNotListed => vec![Note::CodeNotListed],
            Kind::VersionSince => vec![Note::VersionSince(civil(2016, 10, 1))],
            Kind::NotCurrentVersion => ETATS
                .iter()
                .copied()
                .flat_map(|etat| {
                    [None, Some(civil(2016, 10, 1))].map(|ended| Note::NotCurrentVersion {
                        on: version("2026-12-31"),
                        etat,
                        ended,
                    })
                })
                .collect(),
            Kind::ArticleWithoutVersion => labels()
                .into_iter()
                .flat_map(|title| {
                    nums().into_iter().map(move |num| Note::ArticleWithoutVersion {
                        title: title.clone(),
                        num,
                        date: version("2026-12-31"),
                    })
                })
                .collect(),
            Kind::SubdivisionNotChecked => vec![Note::SubdivisionNotChecked],
            Kind::QuoteNotAttached => vec![Note::QuoteNotAttached],
            Kind::ArticleNotRead => vec![Note::ArticleNotRead],
            Kind::Chain => {
                // Les notes d'une même citation, mises bout à bout dans l'ordre où le moteur les
                // rend (version lue, lecture du sigle, plage, subdivision, écarts de lecture) :
                // la chaîne la plus longue possible, puis chacune derrière une version démesurée.
                let version_warning = Note::NotCurrentVersion {
                    on: version("2026-12-31"),
                    etat: "MODIFIE_MORT_NE",
                    ended: Some(civil(2016, 10, 1)),
                };
                let tail = [
                    Note::ReadAs {
                        sigle: Sigle::Cpce,
                        code: "Code des postes et des communications électroniques",
                    },
                    Note::RangeEndpointsOnly,
                    Note::SubdivisionNotChecked,
                    Note::HeadNotRead,
                    Note::OnlyFirstNumberChecked,
                    Note::OnlyOneNumberChecked,
                    Note::ChamberNotCompared,
                    Note::DateNotChecked,
                    Note::QuoteTooLong,
                ];
                let mut chains = vec![Note::Chain(
                    std::iter::once(version_warning.clone()).chain(tail.iter().cloned()).collect(),
                )];
                chains.extend(
                    tail.iter()
                        .map(|next| Note::Chain(vec![version_warning.clone(), next.clone()])),
                );
                chains.push(Note::Chain(vec![
                    Note::ArticleWithoutVersion {
                        title: long_law_title(),
                        num: "R*4321-123-1-2 quinquies A".to_owned(),
                        date: version("2026-12-31"),
                    },
                    Note::RangeEndpointsOnly,
                    Note::SubdivisionNotChecked,
                ]));
                chains
            }
            Kind::RangeEndpointsOnly => vec![Note::RangeEndpointsOnly],
            Kind::WithRange => vec![
                Note::WithRange(Box::new(Note::ReadAs {
                    sigle: Sigle::Cpce,
                    code: "Code des procédures civiles d'exécution",
                })),
                Note::WithRange(Box::new(Note::ReadAs { sigle: Sigle::Cpce, code: LONG_STATIC })),
            ],
            Kind::RangeEndNotFound => {
                nums().into_iter().map(|num| Note::RangeEndNotFound { num }).collect()
            }
            Kind::QuoteMismatch => vec![Note::QuoteMismatch],
            Kind::QuoteTooLong => vec![Note::QuoteTooLong],
            Kind::WithQuoteMismatch => vec![
                // L'écart d'extrait en tête de chacune des notes de `verified` qu'il précède.
                Note::WithQuoteMismatch(Box::new(Note::NotCurrentVersion {
                    on: version("2026-12-31"),
                    etat: "MODIFIE_MORT_NE",
                    ended: Some(civil(2016, 10, 1)),
                })),
                Note::WithQuoteMismatch(Box::new(Note::ArticleWithoutVersion {
                    title: long_law_title(),
                    num: "R*4321-123-1-2 quinquies A".to_owned(),
                    date: version("2026-12-31"),
                })),
                Note::WithQuoteMismatch(Box::new(Note::NativeEcli(longest_ecli()))),
                Note::WithQuoteMismatch(Box::new(Note::ChamberNotCompared)),
                Note::WithQuoteMismatch(Box::new(Note::Chain(vec![
                    Note::NotCurrentVersion {
                        on: version("2026-12-31"),
                        etat: "MODIFIE_MORT_NE",
                        ended: Some(civil(2016, 10, 1)),
                    },
                    Note::ReadAs {
                        sigle: Sigle::Cpce,
                        code: "Code des postes et des communications électroniques",
                    },
                    Note::RangeEndpointsOnly,
                    Note::SubdivisionNotChecked,
                ]))),
                Note::WithQuoteMismatch(Box::new(Note::WithRange(Box::new(Note::ReadAs {
                    sigle: Sigle::Cpce,
                    code: LONG_STATIC,
                })))),
            ],
            Kind::NotInSandbox => FAMILIES
                .iter()
                .copied()
                .flat_map(|family| {
                    values().into_iter().flat_map(move |value| {
                        [None, Some(version("2026-12-31"))].into_iter().flat_map(move |date| {
                            let value = value.clone();
                            [false, true].map(move |date_given| Note::NotInSandbox {
                                family,
                                value: value.clone(),
                                date,
                                date_given,
                            })
                        })
                    })
                })
                .chain(labels().into_iter().flat_map(|label| {
                    nums().into_iter().map(move |num| Note::NotInSandbox {
                        family: "article",
                        value: format!("{label}, art. {num}"),
                        date: Some(version("2026-12-31")),
                        date_given: true,
                    })
                }))
                .collect(),
            Kind::NotInSnapshot => FAMILIES
                .iter()
                .copied()
                .flat_map(|family| {
                    values().into_iter().flat_map(move |value| {
                        [None, Some(version("2026-12-31"))].map(|date| Note::NotInSnapshot {
                            family,
                            value: value.clone(),
                            date,
                        })
                    })
                })
                .chain(labels().into_iter().flat_map(|label| {
                    nums().into_iter().map(move |num| Note::NotInSnapshot {
                        family: "article",
                        value: format!("{label}, art. {num}"),
                        date: Some(version("2026-12-31")),
                    })
                }))
                .collect(),
            Kind::NoCitationRecognised => vec![Note::NoCitationRecognised],
            Kind::OtherCitations => vec![Note::OtherCitations { count: usize::MAX }],
            Kind::UseSearch => vec![Note::UseSearch],
            Kind::NoDecisionReferences => vec![Note::NoDecisionReferences],
            Kind::DecisionLinkCodes => vec![Note::DecisionLinkCodes],
        }
    }

    // ---- les gardes ----------------------------------------------------------------------------

    /// La borne (R1-17) : toute variante de `Note`, à ses valeurs les plus longues, tient en
    /// `NOTE_MAX_CHARS` caractères (points de code, pas octets). Cette borne est celle du rognage
    /// final de `Note::text` : elle garde ce rognage, pas le contenu utile (voir plus bas
    /// `every_realistic_note_is_whole_and_not_clipped` et `a_chain_keeps_whole_parts_only_...`).
    #[test]
    fn every_note_variant_at_its_longest_stays_within_200_characters() {
        assert_eq!(NOTE_MAX_CHARS, 200, "le plafond documenté (annexe B §7.10) est de 200");
        let mut measured = 0_usize;
        let mut longest_seen = 0_usize;
        for &kind in ALL_KINDS {
            let notes = longest(kind);
            assert!(!notes.is_empty(), "{kind:?} n'a aucune valeur à mesurer");
            for note in &notes {
                assert_eq!(kind_of(note), kind, "{note:?} rangée sous le mauvais Kind");
                let text = note.text();
                let chars = text.chars().count();
                assert!(chars <= NOTE_MAX_CHARS, "{kind:?} : {chars} caractères : {text}");
                assert!(!text.is_empty(), "{kind:?} : note vide");
                longest_seen = longest_seen.max(chars);
                measured += 1;
            }
        }
        // Le plus long est bien au plafond : la mesure ne tourne pas à vide (une note démesurée
        // est rognée à 200 pile, pas sous le plafond par accident).
        assert_eq!(longest_seen, NOTE_MAX_CHARS);
        assert!(measured > 1_000, "{measured} notes mesurées");
    }

    /// Marqueurs des paramètres démesurés de `longest` : `LONG_STATIC` et les valeurs de 300 « é »,
    /// la clé de 120 chiffres, le titre de loi de 600 caractères (son motif se répète).
    fn is_absurd(note: &Note) -> bool {
        let shown = format!("{note:?}");
        ["é".repeat(20), "1".repeat(40), "(1) ; pour une".to_owned()]
            .iter()
            .any(|marker| shown.contains(marker.as_str()))
    }

    /// Les notes de `longest` dont aucun paramètre n'est démesuré : ce que le moteur peut rendre.
    fn realistic_notes() -> Vec<(Kind, Note)> {
        ALL_KINDS
            .iter()
            .flat_map(|&kind| longest(kind).into_iter().map(move |note| (kind, note)))
            .filter(|(_, note)| !is_absurd(note))
            .collect()
    }

    /// Les variantes qui adaptent leur longueur au plafond (`fit`, `join_fitting`, `quote_gap_text`) :
    /// elles peuvent remplir les 200 caractères sans rien perdre d'utile. Toutes les autres, aux
    /// valeurs réalistes, restent strictement en dessous : à 200 pile, elles seraient rognées.
    const FILLS_THE_CAP: [Kind; 7] = [
        Kind::NotInForce,
        Kind::NoRecordAtDate,
        Kind::NotInSandbox,
        Kind::NotInSnapshot,
        Kind::ArticleWithoutVersion,
        Kind::Chain,
        Kind::WithQuoteMismatch,
    ];

    /// La borne de 200 caractères est aussi un rognage de sûreté (`messages::clip`) : une note
    /// démesurée y est coupée sans un mot, si bien que « <= 200 » ne prouve rien d'une note
    /// réaliste. Ce test mesure donc à part ce que le moteur rend vraiment, sans valeur démesurée :
    /// la note est entière (elle finit par la ponctuation de son gabarit) et, hors des variantes qui
    /// s'adaptent au plafond, strictement sous 200 caractères, donc jamais rognée. Un gabarit qui
    /// s'allonge ou une valeur fermée qui dépasse le plus long connu y échoue, au lieu de passer par
    /// le rognage.
    #[test]
    fn every_realistic_note_is_whole_and_not_clipped() {
        let notes = realistic_notes();
        let kinds: BTreeSet<Kind> = notes.iter().map(|(kind, _)| *kind).collect();
        let all: BTreeSet<Kind> = ALL_KINDS.iter().copied().collect();
        assert_eq!(kinds, all, "chaque variante a au moins une note réaliste");
        assert!(notes.len() > 200, "{} notes réalistes", notes.len());
        for (kind, note) in &notes {
            let text = note.text();
            let chars = text.chars().count();
            assert!(chars <= NOTE_MAX_CHARS, "{kind:?} : {chars} caractères : {text}");
            assert!(text.ends_with('.'), "{kind:?} : note coupée ? {text}");
            if !FILLS_THE_CAP.contains(kind) {
                assert!(chars < NOTE_MAX_CHARS, "{kind:?} : {chars} caractères, rognée ? {text}");
            }
        }
    }

    /// Une chaîne de notes n'est jamais coupée au milieu d'une phrase (`join_fitting`, « mieux vaut
    /// une note de moins qu'une phrase coupée ») : son texte est celui de sa première note, suivi
    /// de certaines des suivantes, **entières** et dans l'ordre, chacune précédée d'une espace. Les
    /// chaînes sans valeur démesurée sont mesurées ; la plus longue (version lue, lecture du sigle et
    /// huit autres notes) dépasse 200 caractères mise bout à bout et en perd donc, entières.
    #[test]
    fn a_chain_keeps_whole_parts_only_and_drops_the_ones_that_do_not_fit() {
        let mut chains = 0_usize;
        let mut dropped_somewhere = false;
        for (_, note) in realistic_notes() {
            let Note::Chain(parts) = &note else { continue };
            let texts: Vec<String> = parts.iter().map(Note::text).collect();
            let text = note.text();
            let (first, others) = texts.split_first().unwrap();
            assert!(
                text.starts_with(first.as_str()),
                "la chaîne ne commence pas par {first} : {text}"
            );
            let mut rest = &text[first.len()..];
            let mut kept = 0_usize;
            for other in others {
                if let Some(after) = rest.strip_prefix(&format!(" {other}")) {
                    rest = after;
                    kept += 1;
                }
            }
            assert_eq!(rest, "", "une partie coupée en son milieu, ou hors ordre : {text}");
            assert!(text.ends_with('.'), "{text}");
            let whole = texts.join(" ");
            if whole.chars().count() > NOTE_MAX_CHARS {
                // Elle ne tient pas en entier : au moins une partie est omise, aucune n'est coupée.
                assert!(kept < others.len(), "rien d'omis alors que {whole} dépasse 200");
                assert!(text.chars().count() <= NOTE_MAX_CHARS, "{text}");
                dropped_somewhere = true;
            } else {
                assert_eq!(text, whole);
            }
            chains += 1;
        }
        assert!(chains >= 8, "{chains} chaînes mesurées");
        assert!(dropped_somewhere, "aucune chaîne ne dépasse 200 : la mesure tourne à vide");
    }

    /// Notes réalistes, textes exacts : les variantes dont la longueur dépend de plusieurs
    /// paramètres (écarts, doublons de numéro, version lue, chaînes, écart d'extrait) gardent leur
    /// contenu utile jusqu'au bout, sans rognage. Un gabarit qui s'allonge change ces textes.
    #[test]
    fn realistic_long_notes_are_rendered_whole() {
        let warning = || Note::NotCurrentVersion {
            on: version("2026-12-31"),
            etat: "MODIFIE_MORT_NE",
            ended: Some(civil(2016, 10, 1)),
        };
        let read_as = || Note::ReadAs {
            sigle: Sigle::Cpce,
            code: "Code des postes et des communications électroniques",
        };
        let expected: Vec<(Note, &str)> = vec![
            (
                Note::Mismatches(vec![
                    (Field::Date, "2026-10-03".to_owned(), "2026-10-02".to_owned()),
                    (Field::Chamber, "CIV1".to_owned(), "SOC".to_owned()),
                    (Field::Seat, "Marseille".to_owned(), "Lyon".to_owned()),
                    (Field::CcType, "QPC".to_owned(), "DC".to_owned()),
                ]),
                "Cited date 2026-10-03 differs from the official record (2026-10-02); chamber CIV1 \
                 differs (SOC); seat Marseille differs (Lyon); decision type QPC differs (DC).",
            ),
            (
                Note::NoRecordAtDate {
                    n: 12,
                    number: "SAN-2026-002 / 14-24.444 / 410000".to_owned(),
                    date: civil(2021, 3, 17),
                    found: five_dates(),
                    more: 7,
                },
                "12 records share number SAN-2026-002 / 14-24.444 / 410000, none dated 2021-03-17; \
                 dates found: 1989-12-01, 1989-12-02, 1989-12-03, 1989-12-04, 1989-12-05 and 7 \
                 more.",
            ),
            (
                warning(),
                "Version in force on 2026-12-31 (etat=MODIFIE_MORT_NE, ended 2016-10-01); omit the \
                 date to read the current text.",
            ),
            (
                Note::Chain(vec![
                    warning(),
                    read_as(),
                    Note::RangeEndpointsOnly,
                    Note::SubdivisionNotChecked,
                    Note::HeadNotRead,
                ]),
                "Version in force on 2026-12-31 (etat=MODIFIE_MORT_NE, ended 2016-10-01); omit the \
                 date to read the current text. Read 'CPCE' as Code des postes et des communications \
                 électroniques.",
            ),
            (
                Note::WithQuoteMismatch(Box::new(Note::Chain(vec![
                    warning(),
                    read_as(),
                    Note::RangeEndpointsOnly,
                ]))),
                "Quoted text not found in the official text. Version in force on 2026-12-31 \
                 (etat=MODIFIE_MORT_NE, ended 2016-10-01); omit the date to read the current text.",
            ),
        ];
        for (note, text) in expected {
            assert!(!is_absurd(&note), "{note:?}");
            assert_eq!(note.text(), text, "{note:?}");
        }
    }

    /// Chaque `Kind` est atteint : `ALL_KINDS` couvre chaque variante une fois, et
    /// `longest` produit au moins une note de chacune (le test précédent ne vérifie que ce que
    /// `longest` rend ; celui-ci, que `ALL_KINDS` n'oublie rien que `kind_of` connaisse).
    #[test]
    fn every_kind_is_listed_once_and_produced() {
        let listed: BTreeSet<Kind> = ALL_KINDS.iter().copied().collect();
        assert_eq!(listed.len(), ALL_KINDS.len(), "un Kind listé deux fois");
        let produced: BTreeSet<Kind> =
            ALL_KINDS.iter().flat_map(|&kind| longest(kind)).map(|note| kind_of(&note)).collect();
        assert_eq!(listed, produced);
    }

    /// Les listes de champs et de motifs suivent leurs `match` sans joker : un champ ajouté sans
    /// entrée dans `ALL_FIELDS` laisse un trou que cette garde voit.
    #[test]
    fn every_field_and_reason_is_listed() {
        let fields: Vec<usize> = ALL_FIELDS.into_iter().map(field_slot).collect();
        assert_eq!(fields, (0..ALL_FIELDS.len()).collect::<Vec<_>>());
        let reasons: Vec<usize> = ALL_REASONS.into_iter().map(reason_slot).collect();
        assert_eq!(reasons, (0..ALL_REASONS.len()).collect::<Vec<_>>());
    }

    /// Longueur au-delà de laquelle une clé d'article n'est plus « réaliste » : la plus longue clé
    /// réelle (« 199 terdecies-0 A bis », « R*4321-123-1-2 quinquies A ») a 26 caractères. Le
    /// parseur accepte pourtant des clés bien plus longues (« article 1-1-1-… », jusqu'à environ 200
    /// caractères) : la borne de 200 caractères tient alors encore (premier test), mais, pour
    /// `NotInForce`, seul le titre du code rétrécit (à « … »), la clé non, et passé une centaine de
    /// caractères la piste finale est coupée par la borne de sûreté.
    const REALISTIC_NUM_MAX_CHARS: usize = 40;

    fn realistic_nums() -> Vec<String> {
        nums().into_iter().filter(|num| num.chars().count() <= REALISTIC_NUM_MAX_CHARS).collect()
    }

    /// La piste donnée avec l'absence d'un article ne se perd jamais dans la coupe, quels que soient
    /// le titre (le plus long du dictionnaire, un titre de loi de 600 caractères) et une clé
    /// réaliste : seul le libellé raccourcit (`fit`, `notes.rs`). Elle est la raison d'être de la
    /// borne de 200 caractères (U01-N).
    #[test]
    fn the_hint_of_an_absent_article_survives_the_longest_label() {
        for label in labels() {
            for num in realistic_nums() {
                for date_given in [false, true] {
                    let hint = if date_given { "try another date." } else { "retry with a date." };
                    let in_force = Note::NotInForce {
                        code: label.clone(),
                        num: num.clone(),
                        date: version("2026-12-31"),
                        date_given,
                    }
                    .text();
                    assert!(in_force.ends_with(hint), "{in_force}");
                    assert!(in_force.contains(&format!("article {num} in force")), "{in_force}");
                    let sandbox = Note::NotInSandbox {
                        family: "article",
                        value: format!("{label}, art. {num}"),
                        date: Some(version("2026-12-31")),
                        date_given,
                    }
                    .text();
                    assert!(sandbox.ends_with(hint), "{sandbox}");
                    let snapshot = Note::NotInSnapshot {
                        family: "article",
                        value: format!("{label}, art. {num}"),
                        date: Some(version("2026-12-31")),
                    }
                    .text();
                    assert!(
                        snapshot.ends_with("in the local DILA snapshot, which may be out of date.")
                    );
                }
            }
        }
    }

    /// La note d'un article modificatif garde sa piste finale (`fr_get_text`) avec le plus long
    /// titre de loi, seule, et son titre rétrécit pour laisser la place à la mention de plage (U10).
    #[test]
    fn the_amending_note_keeps_its_tail_with_the_longest_title() {
        for title in labels() {
            for num in realistic_nums() {
                let text = Note::ArticleWithoutVersion {
                    title: title.clone(),
                    num,
                    date: version("2026-12-31"),
                }
                .text();
                assert!(text.ends_with("read it with fr_get_text."), "{text}");
                assert!(text.starts_with("No version in force on 2026-12-31: art. "), "{text}");
            }
        }
    }
}
