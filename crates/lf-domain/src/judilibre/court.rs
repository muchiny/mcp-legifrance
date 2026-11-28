//! Juridictions de Judilibre (ADR-0035, décisions 3, 4 et 11).
//!
//! Taxonomie lue le 2026-11-27 (`/taxonomy?id=jurisdiction`, levée de V-183) : `cc`, `ca`, `tj`,
//! `tcom`, `cph`. Aucune juridiction administrative.

use std::fmt;

/// Une juridiction de Judilibre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Court {
    /// Cour de cassation (`cc`).
    Cassation,
    /// Cours d'appel (`ca`).
    Appeal,
    /// Tribunaux judiciaires (`tj`).
    Judicial,
    /// Tribunaux de commerce (`tcom`).
    Commercial,
    /// Conseils de prud'hommes (`cph`).
    Labour,
}

impl Court {
    /// Les cinq juridictions, dans l'ordre de la taxonomie.
    pub const ALL: [Self; 5] =
        [Self::Cassation, Self::Appeal, Self::Judicial, Self::Commercial, Self::Labour];

    /// Codes de l'API, dans l'ordre de [`Court::ALL`] : les valeurs admises de `courts`.
    pub const CODES: [&'static str; 5] = ["cc", "ca", "tj", "tcom", "cph"];

    /// Code de l'API (`cc`, `ca`…), envoyé en `jurisdiction`.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Cassation => "cc",
            Self::Appeal => "ca",
            Self::Judicial => "tj",
            Self::Commercial => "tcom",
            Self::Labour => "cph",
        }
    }

    /// Intitulé de la juridiction quand son siège est inconnu : « Cour de cassation »,
    /// « Cour d'appel »…
    #[must_use]
    pub const fn generic_label(self) -> &'static str {
        match self {
            Self::Cassation => "Cour de cassation",
            Self::Appeal => "Cour d'appel",
            Self::Judicial => "Tribunal judiciaire",
            Self::Commercial => "Tribunal de commerce",
            Self::Labour => "Conseil de prud'hommes",
        }
    }

    /// Juridiction d'un code de l'API, insensible à la casse et aux espaces de bord.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        Self::ALL.into_iter().find(|court| court.code().eq_ignore_ascii_case(raw))
    }

    /// Juridiction lue dans une réponse de l'API : son code (`cc`), ou son intitulé générique
    /// (« Cour de cassation »), que l'API rend avec `resolve_references=true` (V-241). L'intitulé
    /// est comparé sans la casse ni les espaces de bord, l'apostrophe typographique valant
    /// l'apostrophe droite. Les arguments d'un outil ne passent que par [`Court::parse`].
    #[must_use]
    pub fn parse_reply(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        Self::parse(raw).or_else(|| {
            Self::ALL.into_iter().find(|court| fold(raw).eq(fold(court.generic_label())))
        })
    }

    /// Vrai si `seat`, siège reçu en clair, nomme un siège de cette juridiction : il commence par
    /// son intitulé générique, suivi d'une espace ou de rien (« Tribunal judiciaire de Nîmes »),
    /// ou, pour un tribunal de commerce, par « Tribunal des activités économiques » (« … de
    /// Lyon », « … du Havre », relevés de V-241). Casse et apostrophe comme dans
    /// [`Court::parse_reply`].
    pub(super) fn names_a_seat(self, seat: &str) -> bool {
        let renamed = (self == Self::Commercial).then_some(ECONOMIC_ACTIVITIES_COURT);
        std::iter::once(self.generic_label())
            .chain(renamed)
            .any(|head| starts_with_words(seat, head))
    }

    /// Rang dans [`Court::ALL`], qui sert de bit dans un [`CourtSet`].
    const fn bit(self) -> u8 {
        match self {
            Self::Cassation => 0b0_0001,
            Self::Appeal => 0b0_0010,
            Self::Judicial => 0b0_0100,
            Self::Commercial => 0b0_1000,
            Self::Labour => 0b1_0000,
        }
    }
}

/// Nom que portent certains tribunaux de commerce (`tcom`) dans les sièges que rend l'API.
const ECONOMIC_ACTIVITIES_COURT: &str = "Tribunal des activités économiques";

/// Intitulé ramené à une forme comparable, caractère par caractère : minuscules, apostrophe
/// typographique (U+2019) remplacée par l'apostrophe droite.
fn fold(label: &str) -> impl Iterator<Item = char> + '_ {
    label.chars().map(|c| if c == '\u{2019}' { '\'' } else { c }).flat_map(char::to_lowercase)
}

/// Vrai si `text` commence par `head` (comparés par [`fold`]), suivi d'une espace ou de rien.
fn starts_with_words(text: &str, head: &str) -> bool {
    let mut text = fold(text);
    fold(head).all(|c| text.next() == Some(c)) && matches!(text.next(), None | Some(' '))
}

impl fmt::Display for Court {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// Ensemble non vide de juridictions, sans doublon, toujours parcouru dans l'ordre de
/// [`Court::ALL`]. Le défaut est l'ensemble des cinq : l'API, à qui on n'envoie aucune
/// juridiction, ne cherche que la Cour de cassation (ADR-0035, décision 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourtSet(u8);

impl CourtSet {
    /// Les cinq juridictions.
    pub const ALL: Self = Self(0b1_1111);

    /// Ensemble des juridictions données ; `None` si la liste est vide. Un doublon ne compte
    /// qu'une fois : l'appelant qui veut le refuser le détecte avec [`CourtSet::insert`].
    #[must_use]
    pub fn of(courts: impl IntoIterator<Item = Court>) -> Option<Self> {
        let bits = courts.into_iter().fold(0, |bits, court| bits | court.bit());
        (bits != 0).then_some(Self(bits))
    }

    /// Ajoute `court` ; faux si elle y était déjà.
    pub fn insert(&mut self, court: Court) -> bool {
        let fresh = !self.contains(court);
        self.0 |= court.bit();
        fresh
    }

    /// Vrai si `court` en fait partie.
    #[must_use]
    pub const fn contains(self, court: Court) -> bool {
        self.0 & court.bit() != 0
    }

    /// Les juridictions, dans l'ordre de [`Court::ALL`].
    pub fn iter(self) -> impl Iterator<Item = Court> {
        Court::ALL.into_iter().filter(move |court| self.contains(*court))
    }
}

impl Default for CourtSet {
    fn default() -> Self {
        Self::ALL
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn codes_follow_the_taxonomy() {
        assert_eq!(Court::ALL.map(Court::code), Court::CODES);
        for court in Court::ALL {
            assert_eq!(Court::parse(court.code()), Some(court));
            assert_eq!(Court::parse(&format!(" {} ", court.code().to_uppercase())), Some(court));
            assert_eq!(court.to_string(), court.code());
        }
        for raw in ["", "ce", "cass", "c c", "tc", "prud"] {
            assert_eq!(Court::parse(raw), None);
        }
    }

    #[test]
    fn a_reply_names_its_court_by_code_or_by_label() {
        for court in Court::ALL {
            assert_eq!(Court::parse_reply(court.code()), Some(court));
            assert_eq!(Court::parse_reply(court.generic_label()), Some(court));
            assert_eq!(Court::parse_reply(&court.generic_label().to_uppercase()), Some(court));
            // Un intitulé n'est jamais un argument d'outil.
            assert_eq!(Court::parse(court.generic_label()), None);
        }
        assert_eq!(Court::parse_reply(" Cour d’appel "), Some(Court::Appeal));
        assert_eq!(Court::parse_reply("Conseil de prud’hommes"), Some(Court::Labour));
        for raw in ["", "Cour", "Cour d'appel de Paris", "Tribunal des activités économiques"] {
            assert_eq!(Court::parse_reply(raw), None, "{raw}");
        }
    }

    #[test]
    fn generic_labels_name_each_court() {
        assert_eq!(
            Court::ALL.map(Court::generic_label),
            [
                "Cour de cassation",
                "Cour d'appel",
                "Tribunal judiciaire",
                "Tribunal de commerce",
                "Conseil de prud'hommes"
            ]
        );
    }

    #[test]
    fn empty_court_set_is_impossible() {
        assert_eq!(CourtSet::of([]), None);
        let set = CourtSet::of([Court::Labour, Court::Cassation, Court::Labour]).unwrap();
        assert_eq!(set.iter().collect::<Vec<_>>(), [Court::Cassation, Court::Labour]);
        assert_eq!(CourtSet::default(), CourtSet::ALL);
        assert_eq!(CourtSet::ALL.iter().collect::<Vec<_>>(), Court::ALL);
    }

    #[test]
    fn insert_reports_a_duplicate() {
        let mut set = CourtSet::of([Court::Appeal]).unwrap();
        assert!(set.insert(Court::Judicial));
        assert!(!set.insert(Court::Appeal));
        assert!(!set.insert(Court::Judicial));
        assert_eq!(set.iter().collect::<Vec<_>>(), [Court::Appeal, Court::Judicial]);
        for court in Court::ALL {
            let single = CourtSet::of([court]).unwrap();
            assert_eq!(
                Court::ALL.map(|other| single.contains(other)),
                Court::ALL.map(|other| other == court)
            );
        }
    }
}
