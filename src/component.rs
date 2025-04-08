use crate::Name;
use crate::archetype::{Archetype, ArchetypeId, ArchetypeRef};
use crate::system::{System, SystemId, SystemName};
use serde::{Deserialize, Deserializer, Serialize};
use std::ops::Deref;
use std::sync::atomic::AtomicU64;

static COMPONENT_IDS: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Serialize, Deserialize)]
pub struct Component {
    #[serde(skip_deserializing, default)]
    pub id: ComponentId,
    pub name: ComponentName,
    #[serde(default)]
    pub description: Option<String>,

    /// The archetypes this system operates on. Available after a call to [`Component::finish`](Component::finish).
    #[serde(skip_deserializing, default)]
    pub affected_archetypes: Vec<ArchetypeRef>,
    /// The IDs of the affected archetypes in ascending order. Available after a call to [`Component::finish`](Component::finish).
    #[serde(skip_deserializing, default)]
    pub affected_archetype_ids: Vec<ArchetypeId>,
    /// The number of affected archetypes. Available after a call to [`Component::finish`](Component::finish).
    #[serde(skip_deserializing, default)]
    pub affected_archetype_count: usize,

    /// The systems this system operates on. Available after a call to [`Component::finish`](Component::finish).
    #[serde(skip_deserializing, default)]
    pub affected_systems: Vec<SystemName>,
    /// The IDs of the affected systems in ascending order. Available after a call to [`Component::finish`](Component::finish).
    #[serde(skip_deserializing, default)]
    pub affected_system_ids: Vec<SystemId>,
    /// The number of affected systems. Available after a call to [`Component::finish`](Component::finish).
    #[serde(skip_deserializing, default)]
    pub affected_system_count: usize,
}

pub type ComponentRef = ComponentName;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ComponentId(u64);

impl Default for ComponentId {
    fn default() -> Self {
        Self(COMPONENT_IDS.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ComponentName(pub(crate) Name);

impl Deref for ComponentName {
    type Target = Name;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ComponentName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let type_name = String::deserialize(deserializer)?;
        Ok(Self(Name::new(type_name, "Component")))
    }
}

impl Component {
    pub(crate) fn finish(&mut self, archetypes: &[Archetype], systems: &[System]) {
        // Scan archetypes
        let mut ids_and_names = Vec::new();
        for archetype in archetypes {
            if archetype.components.iter().any(|c| c.eq(&self.name)) {
                ids_and_names.push((archetype.id, archetype.name.clone()));
            }
        }
        ids_and_names.sort_unstable_by_key(|entry| entry.0);

        self.affected_archetype_count = ids_and_names.len();
        self.affected_archetype_ids = ids_and_names.iter().map(|entry| entry.0).collect();
        self.affected_archetypes = ids_and_names.into_iter().map(|entry| entry.1).collect();

        // Scan systems
        let mut ids_and_names = Vec::new();
        for system in systems {
            if system.inputs.iter().any(|c| c.eq(&self.name)) {
                ids_and_names.push((system.id, system.name.clone()));
            } else if system.outputs.iter().any(|c| c.eq(&self.name)) {
                ids_and_names.push((system.id, system.name.clone()));
            }
        }
        ids_and_names.sort_unstable_by_key(|entry| entry.0);

        self.affected_system_count = ids_and_names.len();
        self.affected_system_ids = ids_and_names.iter().map(|entry| entry.0).collect();
        self.affected_systems = ids_and_names.into_iter().map(|entry| entry.1).collect();
    }
}
