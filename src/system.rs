use crate::Name;
use crate::archetype::{Archetype, ArchetypeId, ArchetypeRef};
use crate::component::{ComponentName, ComponentRef};
use crate::state::StateName;
use crate::system_scheduler::{Access, Dependency, Resource};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashSet;
use std::fmt::{Display, Formatter};
use std::ops::Deref;
use std::sync::atomic::AtomicU64;

static SYSTEM_IDS: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct System {
    /// The ID of the system. Automatically generatedd.
    #[serde(skip_deserializing, default)]
    pub id: SystemId,
    /// The name of the system.
    pub name: SystemName,
    /// The optional description of the system to use as a documentation comment.
    #[serde(default)]
    pub description: Option<String>,
    /// Preferably run this system after the specified other systems.
    /// If no conflict is detected, calls may be parallelized.
    #[serde(default)]
    pub run_after: HashSet<SystemNameRef>,
    /// Whether the system requires access to entities.
    #[serde(
        default,
        rename(serialize = "needs_entities", deserialize = "entities")
    )]
    pub entities: bool,
    /// Whether the system emits commands.
    #[serde(
        default,
        rename(serialize = "emits_commands", deserialize = "commands")
    )]
    pub commands: bool,
    /// Whether the system requires access to the frame context.
    #[serde(default, rename(serialize = "needs_context", deserialize = "context"))]
    pub context: bool,
    /// Whether the system requires access to the user state (and which ones).
    #[serde(default, rename(serialize = "states", deserialize = "states"))]
    pub states: Vec<StateUse>,
    /// Whether the system requires access to components of other entities, and which ones.
    #[serde(default)]
    pub lookup: Vec<ComponentRef>,
    /// Whether the system uses a preflight phase.
    #[serde(default)]
    pub preflight: bool,
    /// Whether the system uses a postflight phase.
    #[serde(default)]
    pub postflight: bool,
    /// The phase in which to run the system.
    pub phase: SystemPhaseRef,
    /// The optional input components to the system.
    #[serde(default)]
    pub inputs: Vec<ComponentName>,
    /// The optional output components to the system.
    #[serde(default)]
    pub outputs: Vec<ComponentName>,
    /// The archetypes this system operates on. Available after a call to [`System::finish`](System::finish).
    #[serde(skip_deserializing, default)]
    pub affected_archetypes: Vec<ArchetypeRef>,
    /// The IDs of the affected archetypes in ascending order. Available after a call to [`System::finish`](System::finish).
    #[serde(skip_deserializing, default)]
    pub affected_archetype_ids: Vec<ArchetypeId>,
    /// The number of affected archetypes. Available after a call to [`System::finish`](System::finish).
    #[serde(skip_deserializing, default)]
    pub affected_archetype_count: usize,
    /// The code to iterate component values. Available after a call to [`System::finish`](System::finish).
    #[serde(skip_deserializing, default)]
    pub component_iter_code: String,
    /// The code to untuple component values. Available after a call to [`System::finish`](System::finish).
    #[serde(skip_deserializing, default)]
    pub component_untuple_code: String,
    /// The dependencies. Available after a call to [`System::finish_dependencies`](System::finish_dependencies) (e.g. via [`System::finish`](System::finish)).
    #[serde(skip)]
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct StateUse {
    /// The name of the state.
    #[serde(rename = "use")]
    pub name: StateName,
    /// Whether write access is required.
    #[serde(default)]
    pub write: bool,
}

impl System {
    pub(crate) fn finish_dependencies(&mut self) {
        self.dependencies.clear();

        // Add inputs as dependencies.
        self.dependencies
            .extend(self.inputs.iter().map(|input| Dependency {
                resource: Resource::Component(input.clone()),
                access: Access::Read,
            }));

        // Add outputs as dependencies.
        self.dependencies
            .extend(self.outputs.iter().map(|output| Dependency {
                resource: Resource::Component(output.clone()),
                access: Access::Write,
            }));

        // Add frame context and state to dependencies
        if self.context {
            self.dependencies.push(Dependency {
                resource: Resource::FrameContext,
                access: Access::Read,
            });
        }
        for state in &self.states {
            self.dependencies.push(Dependency {
                resource: Resource::UserState(state.name.clone()),
                access: if state.write {
                    Access::Write
                } else {
                    Access::Read
                },
            });
        }
    }

    pub(crate) fn finish(&mut self, archetypes: &[Archetype]) {
        self.finish_dependencies();

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
        let mut num_components = self.inputs.len() + self.outputs.len();
        if self.entities {
            num_components += 1;
        }

        debug_assert_ne!(num_components, 0);

        if num_components == 1 {
            self.component_iter_code = String::new();
            if self.entities {
                self.component_iter_code = "entities".to_string();
                self.component_untuple_code = "entity".to_string();
            } else if let Some(output) = self.outputs.first() {
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
                    untuple_stack = output.field_name.to_string();
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
                    untuple_stack = input.field_name.to_string();
                } else {
                    iter_stack = format!(
                        "{name}.iter().zip({iter_stack})",
                        name = input.field_name_plural
                    );
                    untuple_stack = format!("({name}, {untuple_stack})", name = input.field_name);
                }
            }

            if self.entities {
                if iter_stack.is_empty() {
                    iter_stack = "entities.iter()".to_string();
                    untuple_stack = "entity".to_string();
                } else {
                    iter_stack = format!("entities.iter().zip({iter_stack})",);
                    untuple_stack = format!("(entity, {untuple_stack})");
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

#[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SystemPhase {
    /// The name of the phase.
    pub name: SystemPhaseName,
    /// The optional description of the phase.
    pub description: Option<String>,
    #[serde(default, skip_serializing, rename(deserialize = "fixed"))]
    pub fixed_input: FixedTiming,
    /// Indicates that this phase is manually called and will never be executed automatically.
    #[serde(default)]
    pub manual: bool,
    /// Indicates that this phase is conditionally executed on a request.
    #[serde(default)]
    pub on_request: bool,
    /// Whether the system requires access to the user state (and which ones).
    #[serde(default, rename(serialize = "states", deserialize = "states"))]
    pub states: Vec<StateUse>,
    /// When nonzero, this phase uses a fixed timing loop with the specified time in seconds.
    #[serde(default, skip_deserializing)]
    pub fixed_secs: f32,
    /// Indicates the number of times per second that the fixed loop runs. Available after a call to [`SystemPhase::finish`](SystemPhase::finish).
    #[serde(default, skip_deserializing)]
    pub fixed_hertz: f32,
    /// Indicates whether this phase is fixed. Available after a call to [`SystemPhase::finish`](SystemPhase::finish).
    #[serde(default, skip_deserializing)]
    pub fixed: bool,
}

#[derive(Debug, Copy, Clone, PartialEq, PartialOrd, Default)]
pub enum FixedTiming {
    #[default]
    None,
    Fixed,
    FixedHertz(f32),
    FixedSecs(f32),
}

impl<'de> Deserialize<'de> for FixedTiming {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let str = String::deserialize(deserializer)?;
        let str = str.to_ascii_lowercase();
        if str.is_empty() {
            Ok(FixedTiming::None)
        } else if str == "true" {
            Ok(FixedTiming::Fixed)
        } else if let Some(number) = str.strip_suffix("hz") {
            let hertz = number
                .trim()
                .parse::<f32>()
                .map_err(serde::de::Error::custom)?;
            Ok(FixedTiming::FixedHertz(hertz))
        } else if let Some(number) = str.strip_suffix("seconds") {
            let secs = number
                .trim()
                .parse::<f32>()
                .map_err(serde::de::Error::custom)?;
            Ok(FixedTiming::FixedSecs(secs))
        } else if let Some(number) = str.strip_suffix("secs") {
            let secs = number
                .trim()
                .parse::<f32>()
                .map_err(serde::de::Error::custom)?;
            Ok(FixedTiming::FixedSecs(secs))
        } else if let Some(number) = str.strip_suffix("sec") {
            let secs = number
                .trim()
                .parse::<f32>()
                .map_err(serde::de::Error::custom)?;
            Ok(FixedTiming::FixedSecs(secs))
        } else if let Some(number) = str.strip_suffix("s") {
            let secs = number
                .trim()
                .parse::<f32>()
                .map_err(serde::de::Error::custom)?;
            Ok(FixedTiming::FixedSecs(secs))
        } else {
            Err(serde::de::Error::custom(format!(
                "Invalid fixed timing: {str}"
            )))
        }
    }
}

impl SystemPhase {
    pub(crate) fn finish(&mut self) {
        match self.fixed_input {
            FixedTiming::None => {}
            FixedTiming::Fixed => {
                self.fixed_hertz = 60.0;
                self.fixed_secs = 1.0 / 60.0;
                self.fixed = true;
            }
            FixedTiming::FixedHertz(hz) => {
                self.fixed_hertz = hz;
                self.fixed_secs = 1.0 / hz;
                self.fixed = true;
            }
            FixedTiming::FixedSecs(sec) => {
                self.fixed_secs = sec;
                self.fixed_hertz = 1.0 / sec;
                self.fixed = true;
            }
        }
    }
}

pub type SystemPhaseRef = SystemPhaseName;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SystemPhaseName(pub(crate) Name);

impl Deref for SystemPhaseName {
    type Target = Name;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SystemPhaseName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let type_name = String::deserialize(deserializer)?;
        Ok(Self(Name::new(type_name, "Phase")))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SystemName(pub(crate) Name);

pub type SystemNameRef = SystemName;

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
