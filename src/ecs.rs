use crate::archetype::Archetype;
use crate::component::Component;
use crate::system::System;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Serialize, Deserialize)]
pub struct Ecs {
    pub components: Vec<Component>,
    pub archetypes: Vec<Archetype>,
    pub systems: Vec<System>,
}

impl Ecs {
    pub(crate) fn finish(&mut self) {
        for archetype in &mut self.archetypes {
            archetype.finish(&self.components);
        }
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

        let mut system_components = HashSet::new();
        for system in &self.systems {
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

    pub(crate) fn ensure_system_consistency(&self) -> Result<(), EcsError> {
        for system in &self.systems {
            let required_components: HashSet<_> =
                system.inputs.iter().chain(&system.outputs).collect();

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
