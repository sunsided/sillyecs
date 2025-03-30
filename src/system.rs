use crate::Name;
use crate::archetype::{Archetype, ArchetypeId, ArchetypeName, ArchetypeRef};
use crate::component::{Component, ComponentId, ComponentName};
use serde::{Deserialize, Deserializer, Serialize};
use std::ops::Deref;
use std::sync::atomic::AtomicU64;

static SYSTEM_IDS: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Serialize, Deserialize)]
pub struct System {
    #[serde(skip_deserializing, default)]
    pub id: SystemId,
    pub name: SystemName,
    #[serde(default)]
    pub description: Option<String>,
    pub inputs: Vec<ComponentName>,
    pub outputs: Vec<ComponentName>,

    /// The archetypes this system operates on. Available after a call to [`Archetype::finish`](Archetype::finish).
    #[serde(skip_deserializing, default)]
    pub affected_archetypes: Vec<ArchetypeRef>,

    /// The IDs of the affected archetypes in ascending order. Available after a call to [`Archetype::finish`](Archetype::finish).
    #[serde(skip_deserializing, default)]
    pub affected_archetype_ids: Vec<ArchetypeId>,

    /// The number of affected archetypes. Available after a call to [`Archetype::finish`](Archetype::finish).
    #[serde(skip_deserializing, default)]
    pub affected_archetype_count: usize,
}

impl System {
    pub(crate) fn finish(&mut self, archetypes: &[Archetype]) {
        let mut ids_and_names = Vec::new();
        'archetype: for archetype in archetypes {
            // All inputs must exist in the component.
            for input in &self.inputs {
                if !archetype.components.contains(input) {
                    continue 'archetype;
                }
            }

            // All outputs must exist in the component.
            for output in &self.outputs {
                if !archetype.components.contains(output) {
                    continue 'archetype;
                }
            }

            let id = archetype.id;
            ids_and_names.push((id, archetype.name.clone()));
        }
        ids_and_names.sort_unstable_by_key(|entry| entry.0);

        self.affected_archetype_count = ids_and_names.len();
        self.affected_archetype_ids = ids_and_names.iter().map(|entry| entry.0).collect();
        self.affected_archetypes = ids_and_names.into_iter().map(|entry| entry.1).collect();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SystemId(u64);

impl Default for SystemId {
    fn default() -> Self {
        Self(SYSTEM_IDS.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SystemName(Name);

impl Deref for SystemName {
    type Target = Name;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SystemName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let type_name = String::deserialize(deserializer)?;
        Ok(Self(Name::new(type_name, "System")))
    }
}
