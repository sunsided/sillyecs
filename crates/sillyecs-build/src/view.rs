use crate::Name;
use crate::archetype::{Archetype, ArchetypeId, ArchetypeRef};
use crate::component::{Component, ComponentId, ComponentRef};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashSet;
use std::ops::Deref;

/// A named subset of components that may be shared across multiple archetypes.
///
/// At codegen time, the view auto-resolves to every archetype whose component
/// set is a superset of the view's components. The generated world exposes
/// `get_<view>_view(EntityId)` and `get_<view>_view_mut(EntityId)` accessors
/// that perform a single entity-location lookup followed by a single archetype
/// match, then return all view components by index without further lookups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct View {
    pub name: ViewName,
    #[serde(default)]
    pub description: Option<String>,
    pub components: Vec<ComponentRef>,

    /// The component IDs in ascending order. Available after a call to
    /// [`View::finish`](View::finish).
    #[serde(skip_deserializing, default)]
    pub component_ids: Vec<ComponentId>,
    /// The number of components. Available after a call to
    /// [`View::finish`](View::finish).
    #[serde(skip_deserializing, default)]
    pub component_count: usize,

    /// The archetypes whose components form a superset of this view's
    /// components, ordered by archetype ID. Available after a call to
    /// [`View::finish`](View::finish).
    #[serde(skip_deserializing, default)]
    pub archetypes: Vec<ArchetypeRef>,
    /// The IDs of the matching archetypes in ascending order. Available after
    /// a call to [`View::finish`](View::finish).
    #[serde(skip_deserializing, default)]
    pub archetype_ids: Vec<ArchetypeId>,
    /// The number of matching archetypes. Available after a call to
    /// [`View::finish`](View::finish).
    #[serde(skip_deserializing, default)]
    pub archetype_count: usize,
}

impl View {
    pub(crate) fn finish(&mut self, components: &[Component], archetypes: &[Archetype]) {
        let required: HashSet<&ComponentRef> = self.components.iter().collect();

        let mut ids_and_refs = Vec::new();
        for archetype in archetypes {
            let archetype_components: HashSet<&ComponentRef> =
                archetype.components.iter().collect();
            if archetype_components.is_superset(&required) {
                ids_and_refs.push((archetype.id, archetype.name.clone()));
            }
        }
        ids_and_refs.sort_unstable_by_key(|entry| entry.0);

        self.archetype_count = ids_and_refs.len();
        self.archetype_ids = ids_and_refs.iter().map(|entry| entry.0).collect();
        self.archetypes = ids_and_refs.into_iter().map(|entry| entry.1).collect();

        // Validation has already ensured each component exists in the ECS components.
        let mut ids: Vec<ComponentId> = self
            .components
            .iter()
            .map(|c| {
                components
                    .iter()
                    .find(|known| known.name.type_name == c.type_name)
                    .expect("Component must exist; view consistency check should have caught this")
                    .id
            })
            .collect();
        ids.sort_unstable();
        self.component_count = ids.len();
        self.component_ids = ids;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ViewName(Name);

impl Deref for ViewName {
    type Target = Name;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ViewName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let type_name = String::deserialize(deserializer)?;
        Ok(Self(Name::new(type_name, "View")))
    }
}
