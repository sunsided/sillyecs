use std::fmt::{Display, Formatter};
use crate::Name;
use crate::archetype::{Archetype, ArchetypeId, ArchetypeRef};
use crate::component::ComponentName;
use serde::{Deserialize, Deserializer, Serialize};
use std::ops::Deref;
use std::sync::atomic::AtomicU64;

static SYSTEM_IDS: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct System {
    #[serde(skip_deserializing, default)]
    pub id: SystemId,
    pub name: SystemName,
    #[serde(default)]
    pub description: Option<String>,
    /// The order in which systems are executed when they cannot be parallelized.
    #[serde(default)]
    pub order: u32,
    pub phase: SystemPhaseRef,
    #[serde(default)]
    pub inputs: Vec<ComponentName>,
    #[serde(default)]
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

    /// The code to iterate component values. Available after a call to [`Archetype::finish`](Archetype::finish).
    #[serde(skip_deserializing, default)]
    pub component_iter_code: String,

    /// The code to untuple component values. Available after a call to [`Archetype::finish`](Archetype::finish).
    #[serde(skip_deserializing, default)]
    pub component_untuple_code: String,
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

        // Create zipped iteration code.
        let num_components = self.inputs.len() + self.outputs.len();
        debug_assert_ne!(num_components, 0);

        if num_components == 1 {
            self.component_iter_code = String::new();
            if let Some(output) = self.outputs.first() {
                self.component_iter_code = format!("{name}", name = output.field_name_plural);
                self.component_untuple_code = format!("{name}", name = output.field_name);
            } else if let Some(input) = self.inputs.first() {
                self.component_iter_code = format!("{name}", name = input.field_name_plural);
                self.component_untuple_code = format!("{name}", name = input.field_name);
            } else {
                unreachable!();
            }
        } else {
            let mut iter_stack = String::new();
            let mut untuple_stack = String::new();

            for output in self.outputs.iter().rev() {
                if iter_stack.is_empty() {
                    iter_stack = format!("{name}.iter_mut()", name = output.field_name_plural);
                    untuple_stack = format!("{name}", name = output.field_name);
                } else {
                    iter_stack = format!(
                        "{name}.iter_mut().zip({iter_stack})",
                        name = output.field_name_plural
                    );
                    untuple_stack = format!("({name}, {untuple_stack})", name = output.field_name);
                }
            }

            for input in self.inputs.iter().rev() {
                if iter_stack.is_empty() {
                    iter_stack = format!("{name}.iter()", name = input.field_name_plural);
                    untuple_stack = format!("{name}", name = input.field_name);
                } else {
                    iter_stack = format!(
                        "{name}.iter().zip({iter_stack})",
                        name = input.field_name_plural
                    );
                    untuple_stack = format!("({name}, {untuple_stack})", name = input.field_name);
                }
            }

            self.component_iter_code = iter_stack;
            self.component_untuple_code = untuple_stack;
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SystemId(pub(crate) u64);

impl Default for SystemId {
    fn default() -> Self {
        Self(SYSTEM_IDS.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SystemPhase {
    pub name: String,
}

pub type SystemPhaseRef = String;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SystemName(pub(crate) Name);

impl Display for SystemName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

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
