use crate::Name;
use crate::component::{Component, ComponentId, ComponentRef};
use serde::{Deserialize, Deserializer, Serialize};
use std::ops::Deref;
use std::sync::atomic::AtomicU64;

static ARCHETYPE_IDS: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Serialize, Deserialize)]
pub struct Archetype {
    #[serde(skip_deserializing, default)]
    pub id: ArchetypeId,
    pub name: ArchetypeName,
    #[serde(default)]
    pub description: Option<String>,
    pub components: Vec<ComponentRef>,

    /// The component IDs in ascending order. Available after a call to [`Archetype::finish`](Archetype::finish).
    #[serde(skip_deserializing, default)]
    pub component_ids: Vec<ComponentId>,

    /// The number of components. Available after a call to [`Archetype::finish`](Archetype::finish).
    #[serde(skip_deserializing, default)]
    pub component_count: usize,
}

pub type ArchetypeRef = ArchetypeName;

impl Archetype {
    pub(crate) fn finish(&mut self, components: &[Component]) {
        let mut ids = Vec::new();
        for component in &self.components {
            let id = components
                .iter()
                .find(|c| c.name.type_name == component.type_name)
                .expect("Component not found")
                .id;
            ids.push(id);
        }
        ids.sort_unstable();
        self.component_count = ids.len();
        self.component_ids = ids;
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ArchetypeId(u64);

impl Default for ArchetypeId {
    fn default() -> Self {
        Self(ARCHETYPE_IDS.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ArchetypeName(Name);

impl Deref for ArchetypeName {
    type Target = Name;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ArchetypeName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let type_name = String::deserialize(deserializer)?;
        Ok(Self(Name::new(type_name, "Archetype")))
    }
}
