use crate::Name;
use crate::archetype::{Archetype, ArchetypeId, ArchetypeRef};
use crate::component::{ComponentName, ComponentRef};
use crate::state::StateName;
use crate::system_scheduler::{Access, Dependency, Resource};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashSet;
use std::fmt::{Display, Formatter};
use std::ops::Deref;

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccessType {
    None,
    #[default]
    Read,
    Write,
}

/// How a system iterates the entities of its affected archetypes.
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SystemIteration {
    /// Iterate every entity of every affected archetype.
    #[default]
    All,
    /// Iterate only the dirty entities tracked by each affected archetype. Archetypes touched
    /// by a `Dirty` system are automatically marked as dirty-tracking during ECS finalization,
    /// and the dirty set is cleared at the end of each phase that contains a dirty system.
    Dirty,
}

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
    /// How the system iterates its affected archetypes' entities.
    #[serde(default)]
    pub iteration: SystemIteration,
    /// Convenience flag mirroring `iteration == SystemIteration::Dirty`. Computed during
    /// deserialization-time defaulting; exposed to templates so they can branch without
    /// learning the enum representation.
    #[serde(default, skip_deserializing)]
    pub iteration_dirty: bool,
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
    /// The dirty-variant iteration code (prepends `dirty_indices` to the zip chain). Available
    /// after a call to [`System::finish`](System::finish), populated only when
    /// [`iteration`](Self::iteration) is [`SystemIteration::Dirty`].
    #[serde(skip_deserializing, default)]
    pub component_iter_code_dirty: String,
    /// The dirty-variant untuple code matching [`component_iter_code_dirty`](Self::component_iter_code_dirty).
    #[serde(skip_deserializing, default)]
    pub component_untuple_code_dirty: String,
    /// The dependencies. Available after a call to [`System::finish_dependencies`](System::finish_dependencies) (e.g. via [`System::finish`](System::finish)).
    #[serde(skip)]
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct StateUse {
    /// The name of the state.
    #[serde(rename = "use")]
    pub name: StateName,
    /// How the default access level.
    #[serde(default)]
    pub default: AccessType,
    /// How the readiness check accesses the state.
    #[serde(default)]
    pub check: Option<AccessType>,
    /// How the phase begin hook accesses the state.
    #[serde(default)]
    pub begin_phase: Option<AccessType>,
    /// How the preflight accesses the state.
    #[serde(default)]
    pub preflight: Option<AccessType>,
    /// How the system accesses the state.
    #[serde(default)]
    pub system: Option<AccessType>,
    /// How the postflight accesses the state.
    #[serde(default)]
    pub postflight: Option<AccessType>,
    /// How the phase end hook accesses the state.
    #[serde(default)]
    pub end_phase: Option<AccessType>,
}

impl AccessType {
    pub const fn is_write(&self) -> bool {
        matches!(self, Self::Write)
    }
}

impl StateUse {
    pub fn any_write(&self) -> bool {
        self.check.unwrap_or(self.default).is_write()
            || self.begin_phase.unwrap_or(self.default).is_write()
            || self.preflight.unwrap_or(self.default).is_write()
            || self.system.unwrap_or(self.default).is_write()
            || self.postflight.unwrap_or(self.default).is_write()
            || self.end_phase.unwrap_or(self.default).is_write()
    }

    /// Fills every unset lifecycle access hook with [`Self::default`] so templates can rely on
    /// concrete `none`/`read`/`write` values instead of falling through on null.
    pub fn apply_defaults(&mut self) {
        set_default_state(&mut self.check, self.default);
        set_default_state(&mut self.begin_phase, self.default);
        set_default_state(&mut self.preflight, self.default);
        set_default_state(&mut self.system, self.default);
        set_default_state(&mut self.postflight, self.default);
        set_default_state(&mut self.end_phase, self.default);
    }
}

fn set_default_state(state: &mut Option<AccessType>, default: AccessType) {
    if state.is_none() {
        *state = Some(default);
    }
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
                access: if state.any_write() {
                    Access::Write
                } else {
                    Access::Read
                },
            });
        }
    }

    fn apply_state_defaults(&mut self) {
        for state in &mut self.states {
            state.apply_defaults();
        }
    }

    pub(crate) fn finish(&mut self, archetypes: &[Archetype]) {
        // Set dependencies after default states
        self.apply_state_defaults();
        self.finish_dependencies();
        self.iteration_dirty = matches!(self.iteration, SystemIteration::Dirty);

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
            // Multi-component case: emit a `.zip(...)`-chained iterator with a
            // trailing `.map(...)` that flattens the right-nested tuple into a
            // flat `(a, b, c, ...)`. The destructuring pattern in the template
            // is then a flat tuple, which is easier to read and gives `rustc`
            // a uniform shape regardless of arity. This keeps the runtime
            // crate dependency-free: no `izip!` macro is needed, the
            // expansion that `itertools::izip!` would produce is materialized
            // here at codegen time as a plain expression.
            //
            // The argument order (entity, inputs..., outputs...) must match
            // the order in which the surrounding templates feed the bindings
            // to `apply_single` / `apply_many`.
            let mut iters: Vec<String> = Vec::with_capacity(num_components);
            let mut names: Vec<String> = Vec::with_capacity(num_components);

            if self.entities {
                iters.push("entities.iter()".to_string());
                names.push("entity".to_string());
            }
            for input in &self.inputs {
                iters.push(format!("{name}.iter()", name = input.field_name_plural));
                names.push(input.field_name.to_string());
            }
            for output in &self.outputs {
                iters.push(format!(
                    "{name}.iter_mut()",
                    name = output.field_name_plural
                ));
                names.push(output.field_name.to_string());
            }

            // Build the zip chain: `iters[0].zip(iters[1]).zip(iters[2])...`.
            let mut iter_expr = iters[0].clone();
            for next in &iters[1..] {
                iter_expr = format!("{iter_expr}.zip({next})");
            }

            // For N >= 3, the chained `.zip(...)` yields a right-nested tuple
            // `((a, b), c)` etc. Add a `.map(...)` that destructures the
            // nesting back into a flat tuple. For N == 2 the zip output is
            // already a flat 2-tuple, so we skip the map to keep the emitted
            // code minimal.
            if iters.len() >= 3 {
                // Closure input pattern: walk from the innermost zip outward
                // so the pattern matches the right-nested zip output.
                // Example for 4 iters: `(((a, b), c), d)`.
                let mut closure_pat = format!("({}, {})", names[0], names[1]);
                for name in &names[2..] {
                    closure_pat = format!("({closure_pat}, {name})");
                }
                let flat_tuple = format!("({})", names.join(", "));
                iter_expr = format!("{iter_expr}.map(|{closure_pat}| {flat_tuple})");
            }

            self.component_iter_code = iter_expr;
            self.component_untuple_code = format!("({})", names.join(", "));
        }

        if self.iteration_dirty {
            // Build the dirty-variant iter expression. The zip chain always starts with
            // `dirty_indices.iter()` so each archetype-step receives its own dirty index slice
            // alongside the entity / component slices. Same flat-tuple shape as `apply_all`
            // emission above so `apply_all_dirty` can use a single destructuring pattern.
            let mut iters: Vec<String> = vec!["dirty_indices.iter()".to_string()];
            let mut names: Vec<String> = vec!["dirty".to_string()];

            if self.entities {
                iters.push("entities.iter()".to_string());
                names.push("entity".to_string());
            }
            for input in &self.inputs {
                iters.push(format!("{name}.iter()", name = input.field_name_plural));
                names.push(input.field_name.to_string());
            }
            for output in &self.outputs {
                iters.push(format!(
                    "{name}.iter_mut()",
                    name = output.field_name_plural
                ));
                names.push(output.field_name.to_string());
            }

            debug_assert!(iters.len() >= 2);

            let mut iter_expr = iters[0].clone();
            for next in &iters[1..] {
                iter_expr = format!("{iter_expr}.zip({next})");
            }

            if iters.len() >= 3 {
                let mut closure_pat = format!("({}, {})", names[0], names[1]);
                for name in &names[2..] {
                    closure_pat = format!("({closure_pat}, {name})");
                }
                let flat_tuple = format!("({})", names.join(", "));
                iter_expr = format!("{iter_expr}.map(|{closure_pat}| {flat_tuple})");
            }

            self.component_iter_code_dirty = iter_expr;
            self.component_untuple_code_dirty = format!("({})", names.join(", "));
        }
    }
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SystemId(pub(crate) u64);

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

        for state in &mut self.states {
            state.apply_defaults();
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
