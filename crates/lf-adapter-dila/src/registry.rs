//! Registre des archives appliquées et plan de synchronisation (rapport 10 §10.2, annexe C §7.11).
//!
//! Le registre est un petit fichier texte, une ligne `état<TAB>nom` par archive :
//!
//! ```text
//! # lf-adapter-dila archive registry v1
//! applied<TAB>Freemium_legi_global_20250713-140000.tar.gz
//! applied<TAB>LEGI_20261028-212713.tar.gz
//! ```
//!
//! Il retient **le** global sur lequel le corpus est bâti (état `applied`, ou `building` tant
//! que son ingestion n'est pas finie) et les incréments appliqués par-dessus. Il est la seule
//! mémoire de la synchronisation : le répertoire d'archives peut perdre le global (1,1 Go) une
//! fois celui-ci appliqué, sans que le corpus soit à refaire.
//!
//! [`Registry::plan`] décide quoi appliquer, dans quel ordre, et quand tout reconstruire :
//!
//! - **premier bâti** : le registre est vide, on part du plus récent global du répertoire ;
//! - **nouveau global** : un global plus récent que celui du registre remplace tout (rapport 10
//!   §10.2 : « un nouveau nom de global déclenche la reconstruction ») ;
//! - **global interrompu** : le registre le dit `building`, le magasin est dans un état
//!   indéterminé, on rejoue le global puis tous ses incréments ;
//! - **incrément en retard** : un incrément jamais appliqué est plus ancien qu'un incrément déjà
//!   appliqué. Les suppressions ne commutent pas avec les ajouts, on rejoue donc tout dans
//!   l'ordre au lieu de deviner.
//!
//! Sinon, le plan est la liste des incréments du répertoire postérieurs au global et pas encore
//! appliqués, du plus ancien au plus récent.

use std::{collections::BTreeSet, fs, io, path::Path};

use crate::{
    archive::{ArchiveKind, ArchiveName},
    error::IngestError,
};

const HEADER: &str = "# lf-adapter-dila archive registry v1";

/// État d'une archive dans le registre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveStatus {
    /// L'archive a été ingérée jusqu'au bout.
    Applied,
    /// Ingestion en cours ou interrompue : le magasin n'est pas fiable. Réservé au global, dont
    /// l'ingestion vide le magasin d'abord.
    Building,
}

impl ArchiveStatus {
    fn word(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Building => "building",
        }
    }
}

/// Pourquoi le magasin est refait depuis un global.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebuildReason {
    /// Aucune archive appliquée : premier bâti.
    FirstBuild,
    /// Un global plus récent que celui du registre est arrivé.
    NewGlobal {
        /// Le global qu'il remplace.
        previous: ArchiveName,
    },
    /// L'ingestion du global du registre n'a pas été menée à son terme.
    InterruptedGlobal,
    /// Un incrément jamais appliqué est antérieur à un incrément déjà appliqué.
    LateIncrement {
        /// L'incrément arrivé trop tard.
        late: ArchiveName,
    },
}

/// Ce que la synchronisation doit faire, dans l'ordre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Raison de la reconstruction ; `None` si les archives s'appliquent par-dessus le magasin.
    pub rebuild: Option<RebuildReason>,
    /// Archives à ingérer, dans l'ordre : le global d'abord en cas de reconstruction, puis les
    /// incréments du plus ancien au plus récent.
    pub steps: Vec<ArchiveName>,
}

impl Plan {
    /// Vrai si rien n'est à faire.
    pub fn is_up_to_date(&self) -> bool {
        self.steps.is_empty()
    }
}

/// Registre des archives appliquées.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Registry {
    global: Option<(ArchiveName, ArchiveStatus)>,
    increments: BTreeSet<ArchiveName>,
}

impl Registry {
    /// Registre vide.
    pub fn new() -> Self {
        Self::default()
    }

    /// Lit le registre ; un fichier absent est un registre vide.
    ///
    /// # Errors
    ///
    /// [`IngestError::Io`] si le fichier existe sans être lisible, [`IngestError::Registry`] si
    /// une de ses lignes est invalide.
    pub fn load(path: &Path) -> Result<Self, IngestError> {
        match fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::new()),
            Err(source) => Err(IngestError::Io { action: "read the archive registry", source }),
        }
    }

    /// Écrit le registre, de façon atomique (fichier voisin puis renommage) : un arrêt en cours
    /// d'écriture laisse l'ancien registre intact.
    ///
    /// # Errors
    ///
    /// [`IngestError::Io`] si l'écriture ou le renommage échoue.
    pub fn save(&self, path: &Path) -> Result<(), IngestError> {
        let mut temporary = path.as_os_str().to_owned();
        temporary.push(".tmp");
        let io_error = |source| IngestError::Io { action: "write the archive registry", source };
        fs::write(&temporary, self.render()).map_err(io_error)?;
        fs::rename(&temporary, path).map_err(io_error)
    }

    /// Lit le texte d'un registre.
    ///
    /// # Errors
    ///
    /// [`IngestError::Registry`] avec le numéro de la première ligne invalide : état inconnu,
    /// nom qui n'est pas une archive LEGI, `building` sur un incrément, deuxième global.
    pub fn parse(text: &str) -> Result<Self, IngestError> {
        let mut registry = Self::new();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let invalid = IngestError::Registry { line: index + 1 };
            let Some((status, file_name)) = line.split_once('\t') else {
                return Err(invalid);
            };
            let status = match status {
                "applied" => ArchiveStatus::Applied,
                "building" => ArchiveStatus::Building,
                _ => return Err(invalid),
            };
            let Some(name) = ArchiveName::parse(file_name.trim()) else {
                return Err(invalid);
            };
            match (name.kind(), status) {
                (ArchiveKind::Global, _) if registry.global.is_none() => {
                    registry.global = Some((name, status));
                }
                (ArchiveKind::Increment, ArchiveStatus::Applied) => {
                    registry.increments.insert(name);
                }
                _ => return Err(invalid),
            }
        }
        Ok(registry)
    }

    /// Le texte du registre.
    pub fn render(&self) -> String {
        let globals = self.global.iter().map(|(name, status)| (name, *status));
        let increments = self.increments.iter().map(|name| (name, ArchiveStatus::Applied));
        let lines =
            globals.chain(increments).map(|(name, status)| format!("{}\t{name}\n", status.word()));
        std::iter::once(format!("{HEADER}\n")).chain(lines).collect()
    }

    /// Le global du registre et son état.
    pub fn global(&self) -> Option<(&ArchiveName, ArchiveStatus)> {
        self.global.as_ref().map(|(name, status)| (name, *status))
    }

    /// Le dernier incrément appliqué.
    pub fn last_increment(&self) -> Option<&ArchiveName> {
        self.increments.last()
    }

    /// Nombre d'incréments appliqués.
    pub fn increment_count(&self) -> usize {
        self.increments.len()
    }

    /// Vrai si l'incrément est déjà appliqué.
    pub fn has_applied(&self, name: &ArchiveName) -> bool {
        self.increments.contains(name)
    }

    /// Commence l'ingestion d'un global : tout incrément appliqué est oublié, le global est
    /// `building`. À enregistrer **avant** de vider le magasin : un arrêt entre les deux laisse
    /// un registre qui demande la reconstruction, jamais un magasin vide déclaré appliqué.
    pub fn begin_global(&mut self, name: ArchiveName) {
        self.increments.clear();
        self.global = Some((name, ArchiveStatus::Building));
    }

    /// Déclare le global `building` appliqué ; sans effet s'il n'y en a pas.
    pub fn finish_global(&mut self) {
        if let Some((_, status)) = &mut self.global {
            *status = ArchiveStatus::Applied;
        }
    }

    /// Déclare un incrément appliqué.
    pub fn record_increment(&mut self, name: ArchiveName) {
        self.increments.insert(name);
    }

    /// Décide quoi appliquer parmi les archives `available` du répertoire (voir la
    /// documentation du module). Les doublons et l'ordre de `available` sont sans effet.
    ///
    /// # Errors
    ///
    /// [`IngestError::NoGlobal`] s'il faut bâtir le corpus sans qu'aucun global ne soit connu ;
    /// [`IngestError::MissingArchive`] s'il faut rejouer le global du registre et qu'il n'est
    /// plus dans le répertoire.
    pub fn plan(&self, available: &[ArchiveName]) -> Result<Plan, IngestError> {
        let newest_global = available.iter().filter(|n| n.kind() == ArchiveKind::Global).max();
        let (base, rebuild) = match (&self.global, newest_global) {
            (None, None) => return Err(IngestError::NoGlobal),
            (None, Some(newest)) => (Some(newest), Some(RebuildReason::FirstBuild)),
            (Some((recorded, _)), Some(newest)) if newest.stamp() > recorded.stamp() => {
                (Some(newest), Some(RebuildReason::NewGlobal { previous: recorded.clone() }))
            }
            (Some((recorded, ArchiveStatus::Building)), _) => {
                (Self::find_global(available, recorded), Some(RebuildReason::InterruptedGlobal))
            }
            (Some((recorded, ArchiveStatus::Applied)), _) => (Some(recorded), None),
        };
        let Some(base) = base else {
            let name = self.global.as_ref().map(|(n, _)| n.file_name()).unwrap_or_default();
            return Err(IngestError::MissingArchive { name: name.to_owned() });
        };

        let mut later: Vec<&ArchiveName> = available
            .iter()
            .filter(|n| n.kind() == ArchiveKind::Increment && n.stamp() > base.stamp())
            .collect();
        later.sort();
        later.dedup();

        if rebuild.is_some() {
            return Ok(Plan { rebuild, steps: Self::from_base(base, &later) });
        }

        let newest_applied = self.increments.last().map(ArchiveName::stamp);
        let todo: Vec<&ArchiveName> =
            later.iter().copied().filter(|n| !self.has_applied(n)).collect();
        let late = todo
            .iter()
            .find(|n| newest_applied.is_some_and(|applied| n.stamp() < applied))
            .map(|n| (*n).clone());
        let Some(late) = late else {
            return Ok(Plan { rebuild: None, steps: todo.into_iter().cloned().collect() });
        };
        // un incrément en retard : le global doit être rejoué, donc encore présent
        let global = Self::find_global(available, base)
            .ok_or_else(|| IngestError::MissingArchive { name: base.file_name().to_owned() })?;
        Ok(Plan {
            rebuild: Some(RebuildReason::LateIncrement { late }),
            steps: Self::from_base(global, &later),
        })
    }

    /// Le global de `available` qui porte l'horodatage de `wanted`.
    fn find_global<'a>(
        available: &'a [ArchiveName],
        wanted: &ArchiveName,
    ) -> Option<&'a ArchiveName> {
        available.iter().find(|n| n.kind() == ArchiveKind::Global && n.stamp() == wanted.stamp())
    }

    fn from_base(base: &ArchiveName, later: &[&ArchiveName]) -> Vec<ArchiveName> {
        std::iter::once(base).chain(later.iter().copied()).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GLOBAL_2025: &str = "Freemium_legi_global_20250713-140000.tar.gz";
    const GLOBAL_2026: &str = "Freemium_legi_global_20261001-140000.tar.gz";

    fn name(file_name: &str) -> ArchiveName {
        ArchiveName::parse(file_name).unwrap()
    }

    fn incr(day: u32) -> ArchiveName {
        name(&format!("LEGI_202610{day:02}-212713.tar.gz"))
    }

    fn files(names: &[&ArchiveName]) -> Vec<String> {
        names.iter().map(|n| n.file_name().to_owned()).collect()
    }

    fn steps(plan: &Plan) -> Vec<String> {
        plan.steps.iter().map(|n| n.file_name().to_owned()).collect()
    }

    /// Registre bâti sur le global 2025, incréments `applied` donnés.
    fn built_on_2025(applied: &[u32]) -> Registry {
        let mut registry = Registry::new();
        registry.begin_global(name(GLOBAL_2025));
        registry.finish_global();
        for day in applied {
            registry.record_increment(incr(*day));
        }
        registry
    }

    #[test]
    fn an_empty_registry_has_nothing_recorded() {
        let registry = Registry::new();
        assert_eq!(registry.global(), None);
        assert_eq!(registry.last_increment(), None);
        assert_eq!(registry.increment_count(), 0);
        assert!(!registry.has_applied(&incr(1)));
    }

    #[test]
    fn the_registry_round_trips_through_its_text_form() {
        let mut registry = built_on_2025(&[28, 3, 1]);
        assert_eq!(registry.global(), Some((&name(GLOBAL_2025), ArchiveStatus::Applied)));
        assert_eq!(registry.last_increment(), Some(&incr(28)));
        assert_eq!(registry.increment_count(), 3);
        let text = registry.render();
        assert_eq!(
            text,
            "# lf-adapter-dila archive registry v1\n\
             applied\tFreemium_legi_global_20250713-140000.tar.gz\n\
             applied\tLEGI_20261001-212713.tar.gz\n\
             applied\tLEGI_20261003-212713.tar.gz\n\
             applied\tLEGI_20261028-212713.tar.gz\n"
        );
        assert_eq!(Registry::parse(&text).unwrap(), registry);

        registry.begin_global(name(GLOBAL_2026));
        assert_eq!(registry.increment_count(), 0, "a new global forgets the old increments");
        assert!(registry.render().contains("building\tFreemium_legi_global_20261001"));
        assert_eq!(Registry::parse(&registry.render()).unwrap(), registry);
        registry.finish_global();
        assert_eq!(registry.global().unwrap().1, ArchiveStatus::Applied);
    }

    #[test]
    fn finishing_without_a_global_changes_nothing() {
        let mut registry = Registry::new();
        registry.finish_global();
        assert_eq!(registry, Registry::new());
    }

    #[test]
    fn parsing_tolerates_comments_blank_lines_crlf_and_padding() {
        let text = "\n# note\r\napplied\tFreemium_legi_global_20250713-140000.tar.gz\r\n\n  applied\tLEGI_20261001-212713.tar.gz  \n";
        let registry = Registry::parse(text).unwrap();
        assert_eq!(registry.global().unwrap().0.file_name(), GLOBAL_2025);
        assert!(registry.has_applied(&incr(1)));
    }

    #[test]
    fn parsing_names_the_first_invalid_line() {
        let cases = [
            ("applied Freemium_legi_global_20250713-140000.tar.gz\n", 1),
            ("# h\nunknown\tFreemium_legi_global_20250713-140000.tar.gz\n", 2),
            ("applied\tnotes.txt\n", 1),
            ("applied\tKALI_20261001-212713.tar.gz\n", 1),
            ("building\tLEGI_20261001-212713.tar.gz\n", 1),
            (
                "applied\tFreemium_legi_global_20250713-140000.tar.gz\n\napplied\tFreemium_legi_global_20261001-140000.tar.gz\n",
                3,
            ),
        ];
        for (text, line) in cases {
            let error = Registry::parse(text).unwrap_err();
            let expected = format!("archive registry line {line} is invalid");
            assert_eq!(error.to_string(), expected, "{text:?}");
        }
    }

    #[test]
    fn load_and_save_go_through_a_file_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.tsv");
        assert_eq!(Registry::load(&path).unwrap(), Registry::new(), "absent file = empty registry");
        let registry = built_on_2025(&[1, 2]);
        registry.save(&path).unwrap();
        assert_eq!(Registry::load(&path).unwrap(), registry);
        assert!(
            !dir.path().join("registry.tsv.tmp").exists(),
            "the temporary file is renamed away"
        );
        built_on_2025(&[1, 2, 3]).save(&path).unwrap();
        assert_eq!(Registry::load(&path).unwrap().increment_count(), 3);
    }

    #[test]
    fn load_and_save_report_io_failures() {
        let dir = tempfile::tempdir().unwrap();
        let error = Registry::load(dir.path()).unwrap_err();
        assert!(error.to_string().starts_with("cannot read the archive registry: "), "{error}");
        std::fs::write(dir.path().join("bad.tsv"), "applied\tnope\n").unwrap();
        let error = Registry::load(&dir.path().join("bad.tsv")).unwrap_err();
        assert_eq!(error.to_string(), "archive registry line 1 is invalid");

        let missing_parent = dir.path().join("absent").join("registry.tsv");
        let error = Registry::new().save(&missing_parent).unwrap_err();
        assert!(error.to_string().starts_with("cannot write the archive registry: "), "{error}");
        // le renommage échoue quand la cible est un dossier non vide
        let target = dir.path().join("occupied");
        std::fs::create_dir_all(target.join("child")).unwrap();
        let error = Registry::new().save(&target).unwrap_err();
        assert!(error.to_string().starts_with("cannot write the archive registry: "), "{error}");
    }

    #[test]
    fn the_first_build_starts_from_the_newest_global_and_the_increments_after_it() {
        let ancient = name("LEGI_20250101-000000.tar.gz");
        let available = [incr(1), name(GLOBAL_2025), incr(2), ancient, name(GLOBAL_2026), incr(3)];
        let plan = Registry::new().plan(&available).unwrap();
        assert_eq!(plan.rebuild, Some(RebuildReason::FirstBuild));
        assert_eq!(
            steps(&plan),
            files(&[&name(GLOBAL_2026), &incr(1), &incr(2), &incr(3)]),
            "the older global and the increment older than the new one are left out"
        );
    }

    #[test]
    fn a_registry_and_a_directory_without_global_cannot_build_anything() {
        let expected = "no global archive is available to build the corpus from";
        let error = Registry::new().plan(&[incr(1), incr(2)]).unwrap_err();
        assert_eq!(error.to_string(), expected);
        let error = Registry::new().plan(&[]).unwrap_err();
        assert_eq!(error.to_string(), expected);
    }

    #[test]
    fn up_to_date_means_an_empty_plan_without_rebuild() {
        let registry = built_on_2025(&[1, 2]);
        let plan = registry.plan(&[name(GLOBAL_2025), incr(1), incr(2)]).unwrap();
        assert!(plan.is_up_to_date());
        assert_eq!(plan.rebuild, None);
        // le global n'a plus besoin d'être sur le disque une fois appliqué
        let plan = registry.plan(&[incr(1), incr(2)]).unwrap();
        assert!(plan.is_up_to_date());
    }

    #[test]
    fn new_increments_are_applied_oldest_first_on_top_of_the_store() {
        let registry = built_on_2025(&[1, 2]);
        let available = [incr(5), incr(1), incr(3), incr(2), incr(3), name(GLOBAL_2025)];
        let plan = registry.plan(&available).unwrap();
        assert_eq!(plan.rebuild, None);
        assert_eq!(steps(&plan), files(&[&incr(3), &incr(5)]));
        assert!(!plan.is_up_to_date());
    }

    #[test]
    fn increments_older_than_the_global_are_ignored() {
        let registry = built_on_2025(&[]);
        let before = name("LEGI_20250601-000000.tar.gz");
        let plan = registry.plan(&[before, incr(1)]).unwrap();
        assert_eq!(steps(&plan), files(&[&incr(1)]));
    }

    #[test]
    fn rebuild_plans_replay_the_new_global_then_every_increment_after_it() {
        let registry = built_on_2025(&[1, 2, 3]);
        let available = [name(GLOBAL_2025), name(GLOBAL_2026), incr(1), incr(2), incr(4), incr(30)];
        let plan = registry.plan(&available).unwrap();
        assert_eq!(plan.rebuild, Some(RebuildReason::NewGlobal { previous: name(GLOBAL_2025) }));
        // incr(1) (20261001-212713) est postérieur à GLOBAL_2026 (20261001-140000)
        assert_eq!(
            steps(&plan),
            files(&[&name(GLOBAL_2026), &incr(1), &incr(2), &incr(4), &incr(30)])
        );
    }

    #[test]
    fn an_older_global_in_the_directory_never_triggers_a_rebuild() {
        let registry = built_on_2025(&[1]);
        let stale = name("Freemium_legi_global_20240101-140000.tar.gz");
        let plan = registry.plan(&[stale, name(GLOBAL_2025), incr(2)]).unwrap();
        assert_eq!(plan.rebuild, None);
        assert_eq!(steps(&plan), files(&[&incr(2)]));
    }

    #[test]
    fn an_interrupted_global_is_replayed_with_all_its_increments() {
        let mut registry = built_on_2025(&[]);
        registry.begin_global(name(GLOBAL_2025));
        let plan = registry.plan(&[incr(2), name(GLOBAL_2025), incr(1)]).unwrap();
        assert_eq!(plan.rebuild, Some(RebuildReason::InterruptedGlobal));
        assert_eq!(steps(&plan), files(&[&name(GLOBAL_2025), &incr(1), &incr(2)]));
    }

    #[test]
    fn an_interrupted_global_that_left_the_directory_cannot_be_replayed() {
        let mut registry = Registry::new();
        registry.begin_global(name(GLOBAL_2025));
        let missing =
            format!("archive {GLOBAL_2025} is needed but missing from the archive directory");
        let error = registry.plan(&[incr(1)]).unwrap_err();
        assert_eq!(error.to_string(), missing);
        // seul un global de même horodatage convient
        let error =
            registry.plan(&[name("Freemium_legi_global_20240101-140000.tar.gz")]).unwrap_err();
        assert_eq!(error.to_string(), missing);
    }

    #[test]
    fn a_new_global_supersedes_an_interrupted_one() {
        let mut registry = Registry::new();
        registry.begin_global(name(GLOBAL_2025));
        let plan = registry.plan(&[name(GLOBAL_2026)]).unwrap();
        assert_eq!(plan.rebuild, Some(RebuildReason::NewGlobal { previous: name(GLOBAL_2025) }));
        assert_eq!(steps(&plan), files(&[&name(GLOBAL_2026)]));
    }

    #[test]
    fn a_late_increment_forces_an_ordered_replay() {
        // incr(2) et incr(5) sont appliqués ; incr(3) arrive après coup : ses suppressions ne
        // commutent pas avec le contenu d'incr(5)
        let registry = built_on_2025(&[2, 5]);
        let available = [incr(2), incr(3), incr(5), incr(6), name(GLOBAL_2025)];
        let plan = registry.plan(&available).unwrap();
        assert_eq!(plan.rebuild, Some(RebuildReason::LateIncrement { late: incr(3) }));
        assert_eq!(
            steps(&plan),
            files(&[&name(GLOBAL_2025), &incr(2), &incr(3), &incr(5), &incr(6)])
        );
    }

    #[test]
    fn a_late_increment_needs_the_global_on_disk() {
        let registry = built_on_2025(&[2, 5]);
        let error = registry.plan(&[incr(2), incr(3), incr(5)]).unwrap_err();
        let missing =
            format!("archive {GLOBAL_2025} is needed but missing from the archive directory");
        assert_eq!(error.to_string(), missing);
    }
}
