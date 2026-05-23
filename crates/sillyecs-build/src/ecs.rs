use crate::archetype::{Archetype, ArchetypeId};
use crate::component::{Component, ComponentId};
use crate::state::State;
use crate::system::{System, SystemId, SystemPhase};
use crate::view::View;
use crate::world::{World, WorldId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Serialize, Deserialize)]
pub struct Ecs {
    /// The components.
    pub components: Vec<Component>,
    /// The archetypes.
    pub archetypes: Vec<Archetype>,
    /// The system phases.
    pub phases: Vec<SystemPhase>,
    /// Indicates whether any phase has fixed-time steps.
    #[serde(default, skip_deserializing)]
    pub any_phase_fixed: bool,
    /// Indicates whether any phase os conditional.
    #[serde(default, skip_deserializing)]
    pub any_phase_on_request: bool,
    /// The systems.
    pub systems: Vec<System>,
    /// The worlds.
    pub worlds: Vec<World>,
    /// The user states.
    #[serde(default)]
    pub states: Vec<State>,
    /// Named component views shared across archetypes.
    #[serde(default)]
    pub views: Vec<View>,
    /// Allow the generation of unsafe code.
    #[serde(default)]
    pub allow_unsafe: bool,
}

impl Ecs {
    pub(crate) fn finish(&mut self) -> Result<(), EcsError> {
        self.assign_ids()?;

        let cloned_archetypes = self.archetypes.clone();
        for archetype in &mut self.archetypes {
            archetype.finish(&self.components, &cloned_archetypes);
        }

        for system in &mut self.systems {
            system.finish(&self.archetypes);
        }

        for component in &mut self.components {
            component.finish(&self.archetypes, &self.systems);
        }

        for view in &mut self.views {
            view.finish(&self.components, &self.archetypes);
        }

        for state in &mut self.states {
            state.finish(&self.systems);
        }

        for phase in &mut self.phases {
            phase.finish();
            self.any_phase_fixed |= phase.fixed;
            self.any_phase_on_request |= phase.on_request;
        }

        for world in &mut self.worlds {
            world.finish(
                &self.archetypes,
                &self.systems,
                &self.states,
                &self.phases,
                &self.views,
            )?;
        }

        Ok(())
    }

    /// Assigns deterministic, per-`Ecs` IDs to components, archetypes, systems, and worlds in
    /// their order of declaration. IDs start at `1` so they remain valid for the
    /// `NonZeroU64`-backed constants the templates emit, and they are a pure function of the
    /// input YAML (no global process-wide counters).
    ///
    /// `ComponentId`, `ArchetypeId`, and `SystemId` are emitted as `#[repr(u32)]` enum
    /// discriminants in generated code, so the count for each kind must fit in `u32`. The check
    /// is done here so a too-large input fails fast with a clear error instead of producing
    /// invalid Rust that fails to compile with a confusing out-of-range discriminant message.
    fn assign_ids(&mut self) -> Result<(), EcsError> {
        check_u32_capacity("components", self.components.len())?;
        check_u32_capacity("archetypes", self.archetypes.len())?;
        check_u32_capacity("systems", self.systems.len())?;

        for (index, component) in self.components.iter_mut().enumerate() {
            component.id = ComponentId(index as u64 + 1);
        }
        for (index, archetype) in self.archetypes.iter_mut().enumerate() {
            archetype.id = ArchetypeId(index as u64 + 1);
        }
        for (index, system) in self.systems.iter_mut().enumerate() {
            system.id = SystemId(index as u64 + 1);
        }
        for (index, world) in self.worlds.iter_mut().enumerate() {
            world.id = WorldId(index as u64 + 1);
        }

        Ok(())
    }
}

fn check_u32_capacity(kind: &'static str, count: usize) -> Result<(), EcsError> {
    if count > u32::MAX as usize {
        return Err(EcsError::TooManyIds { kind, count });
    }
    Ok(())
}

#[derive(thiserror::Error, Debug)]
pub enum EcsError {
    #[error("Component '{0}' is defined more than once.")]
    DuplicateComponentDefinition(String),
    #[error("Component '{0}' in archetype '{1}' is not defined in the ECS components.")]
    MissingComponentInArchetype(String, String),
    #[error("Component '{0}' in archetype '{1}' is referenced more than once.")]
    DuplicateComponentInArchetype(String, String),
    #[error("Component '{0}' in system '{1}' is not defined in the ECS components.")]
    MissingComponentInSystem(String, String),
    #[error("Component '{0}' in system '{1}' is referenced more than once.")]
    DuplicateComponentInSystem(String, String),
    #[error("Duplicate archetype '{0}' and '{1}'")]
    DuplicateArchetype(String, String),
    #[error("System '{0}' is defined more than once.")]
    DuplicateSystem(String),
    #[error("Failed to process template: {0}")]
    TemplateError(#[from] minijinja::Error),
    #[error("System {0} requires components not covered by any archetype.")]
    NoMatchingArchetypeForSystem(String),
    #[error("Promotion of archetype '{0}' to itself is not allowed.")]
    PromotionToSelf(String),
    #[error("System {1} uses undefined phase '{0}'.")]
    MissingPhase(String, String),
    #[error("World {0} uses no archetypes.")]
    WorldWithoutArchetypes(String),
    #[error("World {1} uses undefined archetype {0}.")]
    MissingArchetypeInWorld(String, String),
    #[error("A cycle was detected in the system run order: {}.", .0.join(" -> "))]
    CycleDetectedBetweenSystems(Vec<String>),
    #[error("A cycle was detected in the system run order (run_after edges).")]
    CycleDetectedInSystemRunOrder,
    #[error("System {1} depends on undefined system {0}.")]
    MissingSystemDependency(String, String),
    #[error(
        "System {system} (phase '{system_phase}') has a run_after dependency on system {dependency} in phase '{dependency_phase}'. Cross-phase run_after edges have no effect; inter-phase ordering is enforced by phase order itself. Remove the dependency or move both systems into the same phase."
    )]
    CrossPhaseRunAfter {
        system: String,
        system_phase: String,
        dependency: String,
        dependency_phase: String,
    },
    #[error(
        "A cycle was detected in the system run order (run_after edges): System {0} depends on itself."
    )]
    SystemDependsOnItself(String),
    #[error("System {1} requires state '{0}' which is not defined.")]
    MissingStateInSystem(String, String),
    #[error("State '{0}' is defined multiple times.")]
    StateDefinedMultipleTimes(String),
    #[error(
        "Too many {kind}: {count} declared, but generated `#[repr(u32)]` IDs only support up to {max}.",
        max = u32::MAX
    )]
    TooManyIds { kind: &'static str, count: usize },
    #[error("View '{0}' is defined more than once.")]
    DuplicateView(String),
    #[error("Component '{0}' in view '{1}' is not defined in the ECS components.")]
    MissingComponentInView(String, String),
    #[error("Component '{0}' in view '{1}' is referenced more than once.")]
    DuplicateComponentInView(String, String),
    #[error("View '{0}' requires components not covered by any archetype.")]
    NoMatchingArchetypeForView(String),
    #[error("View '{0}' has no components.")]
    ViewWithoutComponents(String),
}

impl Ecs {
    pub(crate) fn ensure_distinct_archetype_components(&self) -> Result<(), EcsError> {
        let mut archetype_component_sets: HashMap<String, String> = HashMap::new();
        for archetype in &self.archetypes {
            let mut component_set = archetype
                .components
                .iter()
                .map(|c| c.type_name.clone())
                .collect::<Vec<_>>();
            component_set.sort_unstable();
            let component_set = component_set.join("+").to_ascii_lowercase();
            if let Some(duplicate) = archetype_component_sets.get(&component_set) {
                return Err(EcsError::DuplicateArchetype(
                    archetype.name.type_name.clone(),
                    duplicate.clone(),
                ));
            }
            archetype_component_sets
                .insert(component_set.clone(), archetype.name.type_name.clone());

            if archetype.promotions.contains(&archetype.name) {
                return Err(EcsError::PromotionToSelf(archetype.name.type_name.clone()));
            }
        }
        Ok(())
    }

    /// Ensure that all states are valid.
    pub(crate) fn ensure_state_consistency(&self) -> Result<(), EcsError> {
        let mut set = HashSet::new();
        for state in &self.states {
            if !set.insert(state.name.clone()) {
                return Err(EcsError::StateDefinedMultipleTimes(
                    state.name.type_name_raw.clone(),
                ));
            }
        }
        Ok(())
    }

    /// Ensure that all components used by archetypes are defined in the components vector of the ECS.
    pub(crate) fn ensure_component_consistency(&self) -> Result<(), EcsError> {
        let mut defined_components = HashSet::new();
        for component in &self.components {
            if !defined_components.insert(&component.name) {
                return Err(EcsError::DuplicateComponentDefinition(
                    component.name.type_name.clone(),
                ));
            }
        }

        for archetype in &self.archetypes {
            let mut archetype_components = HashSet::new();
            for component_ref in &archetype.components {
                if !archetype_components.insert(component_ref) {
                    return Err(EcsError::DuplicateComponentInArchetype(
                        component_ref.type_name.clone(),
                        archetype.name.type_name.clone(),
                    ));
                }

                if !defined_components.contains(component_ref) {
                    return Err(EcsError::MissingComponentInArchetype(
                        component_ref.type_name.clone(),
                        archetype.name.type_name.clone(),
                    ));
                }
            }
        }

        for system in &self.systems {
            let mut system_components = HashSet::new();

            // Validate system inputs
            for component_ref in &system.inputs {
                if !system_components.insert(component_ref) {
                    return Err(EcsError::DuplicateComponentInSystem(
                        component_ref.type_name.clone(),
                        system.name.type_name.clone(),
                    ));
                }

                if !defined_components.contains(component_ref) {
                    return Err(EcsError::MissingComponentInSystem(
                        component_ref.type_name.clone(),
                        system.name.type_name.clone(),
                    ));
                }
            }

            // Validate system outputs
            for component_ref in &system.outputs {
                if !system_components.insert(component_ref) {
                    return Err(EcsError::DuplicateComponentInSystem(
                        component_ref.type_name.clone(),
                        system.name.type_name.clone(),
                    ));
                }

                if !defined_components.contains(component_ref) {
                    return Err(EcsError::MissingComponentInSystem(
                        component_ref.type_name.clone(),
                        system.name.type_name.clone(),
                    ));
                }
            }
        }

        Ok(())
    }

    pub(crate) fn ensure_view_consistency(&self) -> Result<(), EcsError> {
        let defined_components: HashSet<_> = self.components.iter().map(|c| &c.name).collect();

        let mut seen_view_names = HashSet::new();
        for view in &self.views {
            if !seen_view_names.insert(&view.name) {
                return Err(EcsError::DuplicateView(view.name.type_name_raw.clone()));
            }

            if view.components.is_empty() {
                return Err(EcsError::ViewWithoutComponents(
                    view.name.type_name_raw.clone(),
                ));
            }

            let mut seen_components = HashSet::new();
            for component_ref in &view.components {
                if !seen_components.insert(component_ref) {
                    return Err(EcsError::DuplicateComponentInView(
                        component_ref.type_name.clone(),
                        view.name.type_name_raw.clone(),
                    ));
                }

                if !defined_components.contains(component_ref) {
                    return Err(EcsError::MissingComponentInView(
                        component_ref.type_name.clone(),
                        view.name.type_name_raw.clone(),
                    ));
                }
            }

            let required: HashSet<_> = view.components.iter().collect();
            if !self.archetypes.iter().any(|archetype| {
                archetype
                    .components
                    .iter()
                    .collect::<HashSet<_>>()
                    .is_superset(&required)
            }) {
                return Err(EcsError::NoMatchingArchetypeForView(
                    view.name.type_name_raw.clone(),
                ));
            }
        }

        Ok(())
    }

    pub(crate) fn ensure_world_consistency(&mut self) -> Result<(), EcsError> {
        for world in &mut self.worlds {
            if world.archetypes_refs.is_empty() {
                return Err(EcsError::WorldWithoutArchetypes(
                    world.name.type_name_raw.clone(),
                ));
            }
            for archetype in &world.archetypes_refs {
                if !self.archetypes.iter().any(|a| a.name.eq(&archetype)) {
                    return Err(EcsError::MissingArchetypeInWorld(
                        archetype.type_name_raw.clone(),
                        world.name.type_name_raw.clone(),
                    ));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn ensure_system_consistency(&mut self) -> Result<(), EcsError> {
        // Reject duplicate system names up front. The scheduler relies on names being unique to
        // make its name-based tie-break total (and the `system_phases` HashMap below would
        // otherwise silently collapse duplicates onto the last phase declared).
        let mut seen_names = HashSet::new();
        for system in &self.systems {
            if !seen_names.insert(&system.name) {
                return Err(EcsError::DuplicateSystem(system.name.type_name_raw.clone()));
            }
        }

        let system_phases: HashMap<_, _> =
            self.systems.iter().map(|s| (&s.name, &s.phase)).collect();

        for system in &self.systems {
            let required_components: HashSet<_> =
                system.inputs.iter().chain(&system.outputs).collect();

            // Ensure all `run_after` dependencies exist in self.systems
            for dependency in &system.run_after {
                let Some(dep_phase) = system_phases.get(dependency) else {
                    return Err(EcsError::MissingSystemDependency(
                        dependency.type_name_raw.clone(),
                        system.name.type_name.clone(),
                    ));
                };

                if dependency == &system.name {
                    return Err(EcsError::SystemDependsOnItself(
                        system.name.type_name.clone(),
                    ));
                }

                if *dep_phase != &system.phase {
                    return Err(EcsError::CrossPhaseRunAfter {
                        system: system.name.type_name.clone(),
                        system_phase: system.phase.type_name_raw.clone(),
                        dependency: dependency.type_name_raw.clone(),
                        dependency_phase: dep_phase.type_name_raw.clone(),
                    });
                }
            }

            for state in &system.states {
                if !self
                    .states
                    .iter()
                    .any(|ecs_state| ecs_state.name.eq(&state.name))
                {
                    return Err(EcsError::MissingStateInSystem(
                        state.name.type_name_raw.clone(),
                        system.name.type_name.clone(),
                    ));
                }
            }

            if !self.phases.iter().any(|phase| phase.name.eq(&system.phase)) {
                return Err(EcsError::MissingPhase(
                    system.phase.type_name_raw.clone(),
                    system.name.type_name.clone(),
                ));
            }

            if !self.archetypes.iter().any(|archetype| {
                archetype
                    .components
                    .iter()
                    .collect::<HashSet<_>>()
                    .is_superset(&required_components)
            }) {
                return Err(EcsError::NoMatchingArchetypeForSystem(
                    system.name.type_name.clone(),
                ));
            }
        }
        Ok(())
    }
}
