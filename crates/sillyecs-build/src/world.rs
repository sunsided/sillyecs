use crate::Name;
use crate::archetype::{Archetype, ArchetypeRef};
use crate::component::ComponentRef;
use crate::ecs::EcsError;
use crate::state::State;
use crate::system::{System, SystemPhase, SystemPhaseRef};
use crate::system_scheduler::schedule_systems;
use crate::view::View;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ops::Deref;

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
    /// Views whose matching archetypes are all present in this world. Each entry is the view
    /// restricted to the world's own archetypes.
    #[serde(default, skip_deserializing)]
    pub views: Vec<View>,

    /// The systems in scheduling order (based on this world's systems). Ordered by phase name so
    /// that codegen output is deterministic between runs.
    #[serde(default, skip_deserializing)]
    pub scheduled_systems: BTreeMap<SystemPhaseRef, Vec<Vec<System>>>,
    /// The components used in this world (based on this world's archetypes). Ordered by component
    /// and archetype name so that codegen output is deterministic between runs.
    #[serde(default, skip_deserializing)]
    pub components: BTreeMap<ComponentRef, BTreeSet<ArchetypeRef>>,
}

impl World {
    pub(crate) fn finish(
        &mut self,
        archetypes: &[Archetype],
        systems: &[System],
        states: &[State],
        phases: &[SystemPhase],
        views: &[View],
    ) -> Result<(), EcsError> {
        let mut used_systems = HashSet::new();
        let mut used_states = HashSet::new();

        for archetype in archetypes {
            if !self.archetypes_refs.iter().any(|a| a.eq(&archetype.name)) {
                continue;
            }

            for component in &archetype.components {
                self.components
                    .entry(component.clone())
                    .and_modify(|set| {
                        set.insert(archetype.name.clone());
                    })
                    .or_insert(BTreeSet::from([archetype.name.clone()]));
            }

            self.archetypes.push(archetype.clone());
            for system in systems
                .iter()
                .filter(|s| s.affected_archetype_ids.contains(&archetype.id))
            {
                if used_systems.insert(system.name.clone()) {
                    self.systems.push(system.clone());
                }

                for state in system.states.iter() {
                    if used_states.insert(state.name.clone()) {
                        let state = states
                            .iter()
                            .find(|s| s.name.eq(&state.name))
                            .cloned()
                            .expect("Failed to find state that is known to exist");

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

        let world_archetypes: HashSet<&ArchetypeRef> = self.archetypes_refs.iter().collect();
        for view in views {
            let mut narrowed = view.clone();
            let mut kept_archetypes = Vec::new();
            let mut kept_ids = Vec::new();
            for (archetype_name, archetype_id) in view
                .archetypes
                .iter()
                .zip(view.archetype_ids.iter().copied())
            {
                if world_archetypes.contains(archetype_name) {
                    kept_archetypes.push(archetype_name.clone());
                    kept_ids.push(archetype_id);
                }
            }

            if kept_archetypes.is_empty() {
                continue;
            }

            narrowed.archetype_count = kept_archetypes.len();
            narrowed.archetypes = kept_archetypes;
            narrowed.archetype_ids = kept_ids;
            self.views.push(narrowed);
        }

        Ok(())
    }

    pub(crate) fn scheduled_systems(&mut self, phases: &[SystemPhase]) -> Result<(), EcsError> {
        let mut phase_groups = BTreeMap::new();
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

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct WorldId(pub(crate) u64);

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
