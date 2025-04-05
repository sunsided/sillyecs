use crate::Name;
use crate::archetype::{Archetype, ArchetypeRef};
use crate::system::System;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashSet;
use std::hash::Hash;
use std::ops::Deref;
use std::sync::atomic::AtomicU64;
use crate::state::State;

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
}

impl World {
    pub(crate) fn finish(&mut self, archetypes: &[Archetype], systems: &[System], states: &[State]) {
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
                if !used_systems.insert(system.name.clone()) {
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
