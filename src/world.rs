use crate::Name;
use crate::archetype::{Archetype, ArchetypeRef};
use crate::system::{System, SystemPhase, SystemPhaseRef};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::ops::Deref;
use std::sync::atomic::AtomicU64;
use crate::ecs::EcsError;
use crate::state::State;
use crate::system_scheduler::schedule_systems;

static WORLD_IDS: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Serialize, Deserialize)]
pub struct World {
    #[serde(skip_deserializing, default)]
    pub id: WorldId,
    pub name: WorldName,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(skip_serializing, rename(deserialize = "archetypes"))]
    pub archetypes_refs: Vec<ArchetypeRef>,
    #[serde(skip_deserializing)]
    pub archetypes: Vec<Archetype>,
    #[serde(skip_deserializing)]
    pub systems: Vec<System>,
    #[serde(skip_deserializing)]
    pub states: Vec<State>,

    /// The systems in scheduling order (based on this world's systems).
    #[serde(default, skip_deserializing)]
    pub scheduled_systems: HashMap<SystemPhaseRef, Vec<Vec<System>>>,
}

impl World {
    pub(crate) fn finish(&mut self, archetypes: &[Archetype], systems: &[System], states: &[State], phases: &[SystemPhase]) -> Result<(), EcsError> {
        let mut used_systems = HashSet::new();
        let mut used_states = HashSet::new();
        for archetype in archetypes {
            if !self.archetypes_refs.iter().any(|a| a.eq(&archetype.name)) {
                continue;
            }

            self.archetypes.push(archetype.clone());
            if let Some(system) = systems
                .iter()
                .find(|s| s.affected_archetype_ids.contains(&archetype.id))
            {
                if used_systems.insert(system.name.clone()) {
                    self.systems.push(system.clone());
                }

                for state in system.states.iter() {
                    if used_states.insert(state.name.clone()) {
                        let state = states.iter().find(|s| s.name.eq(&state.name)).cloned().expect("Failed to find state that is known to exist");

                        assert!(
                            !self.states.iter().any(|s| s.name.eq(&state.name)),
                            "State '{}' is already in the world",
                            state.name.type_name_raw
                        );
                        self.states.push(state.clone());
                    }
                }
            }
        }

        self.scheduled_systems(phases)?;
        if !self.systems.is_empty() {
            debug_assert_ne!(
                self.scheduled_systems.len(),
                0,
                "Some systems should be scheduled"
            );
        }

        Ok(())
    }

    pub(crate) fn scheduled_systems(&mut self, phases: &[SystemPhase]) -> Result<(), EcsError> {
        let mut phase_groups = HashMap::new();
        for phase in phases {
            let systems_in_group: Vec<_> = self
                .systems
                .iter()
                .filter(|s| s.phase == phase.name)
                .cloned()
                .collect();
            let groups = schedule_systems(&systems_in_group)?;
            let scheduled_systems: Vec<_> = groups
                .into_iter()
                .map(|group| {
                    group
                        .iter()
                        .map(|&system| {
                            self.systems
                                .iter()
                                .find(|s| s.id == system)
                                .expect("Failed to find system")
                        })
                        .cloned()
                        .collect()
                })
                .collect();
            phase_groups.insert(phase.name.clone(), scheduled_systems);
        }

        self.scheduled_systems = phase_groups;
        Ok(())
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct WorldId(u64);

impl Default for WorldId {
    fn default() -> Self {
        Self(WORLD_IDS.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct WorldName(pub(crate) Name);

impl Deref for WorldName {
    type Target = Name;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de> Deserialize<'de> for WorldName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let type_name = String::deserialize(deserializer)?;
        Ok(Self(Name::new(type_name, "World")))
    }
}
