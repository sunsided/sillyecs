use crate::Name;
use crate::component::{Component, ComponentId, ComponentRef};
use core::ops::Deref;
use core::sync::atomic::AtomicU64;
use serde::{Deserialize, Deserializer, Serialize};

static ARCHETYPE_IDS: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Archetype {
    #[serde(skip_deserializing, default)]
    pub id: ArchetypeId,
    pub name: ArchetypeName,
    #[serde(default)]
    pub description: Option<String>,
    pub components: Vec<ComponentRef>,
    #[serde(default, skip_serializing)]
    pub promotions: Vec<ArchetypeRef>,

    /// The promotion information. Available after a call to [`Archetype::finish`](Archetype::finish).
    #[serde(skip_deserializing, default)]
    pub promotion_infos: Vec<PromotionInfo>,

    /// The component IDs in ascending order. Available after a call to [`Archetype::finish`](Archetype::finish).
    #[serde(skip_deserializing, default)]
    pub component_ids: Vec<ComponentId>,

    /// The number of components. Available after a call to [`Archetype::finish`](Archetype::finish).
    #[serde(skip_deserializing, default)]
    pub component_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PromotionInfo {
    pub target: ArchetypeName,
    pub components_to_pass: Vec<ComponentRef>,
    pub components_to_add: Vec<ComponentRef>,
}

pub type ArchetypeRef = ArchetypeName;

impl Archetype {
    pub(crate) fn finish(&mut self, components: &[Component], archetypes: &[Archetype]) {
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

        // Process promotions.
        assert!(self.promotion_infos.is_empty());
        for promotion in &self.promotions {
            let target = archetypes
                .iter()
                .find(|a| a.name.eq(promotion))
                .expect("Promotion target not found");
            let mut components_to_pass = Vec::new();
            for component in &self.components {
                if target.components.contains(component) {
                    components_to_pass.push(component.clone());
                }
            }

            let mut components_to_add = Vec::new();
            for component in &target.components {
                if !self.components.contains(component) {
                    components_to_add.push(component.clone());
                }
            }
            self.promotion_infos.push(PromotionInfo {
                target: target.name.clone(),
                components_to_pass,
                components_to_add,
            });
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ArchetypeId(u64);

impl Default for ArchetypeId {
    fn default() -> Self {
        Self(ARCHETYPE_IDS.fetch_add(1, core::sync::atomic::Ordering::SeqCst))
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
