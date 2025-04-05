use crate::archetype::Archetype;
use crate::component::Component;
use crate::system::{System, SystemPhase, SystemPhaseRef};
use crate::system_scheduler::schedule_systems;
use crate::world::World;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use crate::state::State;

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
    /// The systems.
    pub systems: Vec<System>,
    /// The worlds.
    pub worlds: Vec<World>,
    /// The user states.
    #[serde(default)]
    pub states: Vec<State>,

    // TODO: Schedules systems should be part of the world, not the ECS
    /// The systems in scheduling order.
    #[serde(default, skip_deserializing)]
    pub scheduled_systems: HashMap<SystemPhaseRef, Vec<Vec<System>>>,
}

impl Ecs {
    pub(crate) fn finish(&mut self) -> Result<(), EcsError> {
        let cloned_archetypes = self.archetypes.clone();
        for archetype in &mut self.archetypes {
            archetype.finish(&self.components, &cloned_archetypes);
        }

        for system in &mut self.systems {
            system.finish(&self.archetypes);
        }

        for state in &mut self.states {
            state.finish(&self.systems);
        }

        for phase in &mut self.phases {
            phase.finish();
            self.any_phase_fixed |= phase.fixed;
        }

        self.scheduled_systems()?;

        for world in &mut self.worlds {
            world.finish(&self.archetypes, &self.systems, &self.states);
        }

        Ok(())
    }
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
    #[error("A cycle was detected in the system run order (run_after edges).")]
    CycleDetectedInSystemRunOrder,
    #[error("System {1} depends on undefined system {0}.")]
    MissingSystemDependency(String, String),
    #[error("A cycle was detected in the system run order (run_after edges): System {0} depends on itself.")]
    SystemDependsOnItself(String),
    #[error("System {1} requires state '{0}' which is not defined.")]
    MissingStateInSystem(String, String),
    #[error("State '{0}' is defined multiple times.")]
    StateDefinedMultipleTimes(String),
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
                return Err(EcsError::StateDefinedMultipleTimes(state.name.type_name_raw.clone()));
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
        for system in &self.systems {
            let required_components: HashSet<_> =
                system.inputs.iter().chain(&system.outputs).collect();

            // Ensure all `run_after` dependencies exist in self.systems
            for dependency in &system.run_after {
                if !self.systems.iter().any(|s| s.name == *dependency) {
                    return Err(EcsError::MissingSystemDependency(
                        dependency.type_name_raw.clone(),
                        system.name.type_name.clone(),
                    ));
                }

                if dependency == &system.name {
                    return Err(EcsError::SystemDependsOnItself(system.name.type_name.clone()));
                }
            }

            for state in &system.states {
                if !self.states.iter().any(|ecs_state| ecs_state.name.eq(&state.name)) {
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

    pub(crate) fn scheduled_systems(&mut self) -> Result<(), EcsError> {
        let mut phase_groups = HashMap::new();
        for phase in &self.phases {
            let systems_in_group: Vec<_> = self
                .systems
                .iter()
                .filter(|s| s.phase == phase.name)
                .cloned()
                .collect();
            let groups = schedule_systems(&systems_in_group)?;
            let scheduled_systems: Vec<_> = groups
                .into_iter()
                .map(|group| {
                    group
                        .iter()
                        .map(|&system| {
                            self.systems
                                .iter()
                                .find(|s| s.id == system)
                                .expect("Failed to find system")
                        })
                        .cloned()
                        .collect()
                })
                .collect();
            phase_groups.insert(phase.name.clone(), scheduled_systems);
        }

        self.scheduled_systems = phase_groups;
        Ok(())
    }
}
