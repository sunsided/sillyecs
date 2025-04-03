//! System scheduling module that manages parallel execution of ECS systems.
//!
//! This module provides functionality to analyze system dependencies and group them into
//! parallelizable batches for efficient execution. It handles:
//!
//! - Component read/write dependencies between systems
//! - Explicit ordering requirements
//! - Resource conflict resolution
//! - Parallel batch scheduling
//! - Cyclic dependency handling through fallback ordering
//!
//! The main entry point is the [`schedule_systems`] function which takes a slice of systems
//! and returns an ordered list of system batches that can be executed in parallel while
//! respecting all dependencies and constraints.

use crate::component::ComponentName;
use crate::system::{System, SystemId};
use std::collections::{HashMap, HashSet};

/// Schedules systems into parallelizable batches based on their component dependencies and execution order.
///
/// This function takes a slice of systems and returns a vector of system batches, where each batch contains
/// systems that can be executed in parallel. Systems are scheduled based on Kahn's algorithm for
/// topological sorting by:
///
/// 1. Component dependencies - Systems reading components written by other systems must execute after them
/// 2. Explicit ordering - Systems with lower order values execute before those with higher values
/// 3. Resource conflicts - Systems writing the same components cannot execute in parallel
///
/// # Parameters
///
/// * `systems` - A slice containing the systems to schedule
///
/// # Returns
///
/// A `Vec<Vec<SystemId>>` where each inner vector represents a batch of systems that can run in parallel.
/// The outer vector represents the sequential order in which batches must be executed.
///
/// # Algorithm
///
/// 1. Builds a dependency graph based on component read/write relationships
/// 2. Uses a modified topological sort that:
///    - Handles cycles by falling back to system order
///    - Groups independent systems into parallel batches
///    - Respects explicit ordering within batches
/// 3. Ensures systems writing the same components are in different batches
pub fn schedule_systems(systems: &[System]) -> Vec<Vec<SystemId>> {
    let mut graph: HashMap<SystemId, Vec<SystemId>> = HashMap::new();
    let mut in_degree: HashMap<SystemId, usize> = HashMap::new();
    let mut systems_by_id: HashMap<SystemId, &System> = HashMap::new();

    let mut readers: HashMap<ComponentName, HashSet<SystemId>> = HashMap::new();
    let mut writers: HashMap<ComponentName, HashSet<SystemId>> = HashMap::new();

    for sys in systems {
        systems_by_id.insert(sys.id, sys);
        in_degree.entry(sys.id).or_insert(0);

        for input in &sys.inputs {
            readers.entry(input.clone()).or_default().insert(sys.id);
        }
        for output in &sys.outputs {
            writers.entry(output.clone()).or_default().insert(sys.id);
        }
    }

    for (component, writers_of) in &writers {
        if let Some(readers_of) = readers.get(component) {
            for &writer in writers_of {
                for &reader in readers_of {
                    if writer != reader {
                        graph.entry(writer).or_default().push(reader);
                        *in_degree.entry(reader).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    let mut ready: Vec<SystemId> = in_degree
        .iter()
        .filter_map(|(&id, &deg)| if deg == 0 { Some(id) } else { None })
        .collect();

    let mut scheduled: Vec<Vec<SystemId>> = Vec::new();
    let mut visited = HashSet::new();

    while visited.len() < systems.len() {
        if ready.is_empty() {
            // Fallback: lowest-order unscheduled system
            let mut remaining: Vec<_> = in_degree
                .keys()
                .filter(|&&id| !visited.contains(&id))
                .collect();
            remaining.sort_by_key(|id| systems_by_id[id].order);
            ready.push(*remaining[0]);
        }

        let mut batch = Vec::new();
        let mut used_outputs = HashSet::new();

        ready.sort_by_key(|id| systems_by_id[id].order);
        let mut i = 0;
        while i < ready.len() {
            let candidate = ready[i];

            if visited.contains(&candidate) {
                ready.remove(i);
                continue;
            }

            let system = systems_by_id[&candidate];
            let has_conflict = system
                .outputs
                .iter()
                .any(|out| used_outputs.contains(out));

            if !has_conflict {
                batch.push(candidate);
                used_outputs.extend(system.outputs.iter().cloned());
                ready.remove(i);
            } else {
                i += 1;
            }
        }

        // Mark as visited and update graph
        for sys_id in &batch {
            visited.insert(*sys_id);

            if let Some(dependents) = graph.get(sys_id) {
                for &dep in dependents {
                    if let Some(deg) = in_degree.get_mut(&dep) {
                        *deg -= 1;
                        if *deg == 0 {
                            ready.push(dep);
                        }
                    }
                }
            }
        }

        let batch_refs = batch.into_iter().collect();
        scheduled.push(batch_refs);
    }

    scheduled
}

#[cfg(test)]
mod tests {
    use crate::Name;
    use crate::component::ComponentName;
    use crate::system::{System, SystemId, SystemName, SystemPhaseName, SystemPhaseRef};
    use super::*;

    fn sysname(name: &str) -> SystemName {
        SystemName(Name::new(name.to_string(), "System"))
    }

    fn compname(name: &str) -> ComponentName {
        ComponentName(Name::new(name.to_string(), "Component"))
    }

    fn phasename(name: &str) -> SystemPhaseRef {
        SystemPhaseName(Name::new(name.to_string(), "Phase"))
    }

    fn create_system(id: u64, name: &str, order: u32, inputs: Vec<&str>, outputs: Vec<&str>) -> System {
        System {
            id: SystemId(id),
            name: sysname(name),
            order,
            context: false,
            state: false,
            entities: false,
            inputs: inputs.into_iter().map(compname).collect(),
            outputs: outputs.into_iter().map(compname).collect(),
            phase: phasename("default"),
            affected_archetype_count: 0,
            affected_archetype_ids: Default::default(),
            affected_archetypes: Default::default(),
            component_iter_code: String::new(),
            component_untuple_code: String::new(),
            description: None
        }
    }

    #[test]
    fn order_one() {
        let systems = vec![
            create_system(1, "Producer", 1, vec!["x"], vec![]),
            // Allow the consumer to run with or after the transformer by ordering.
            create_system(3, "Consumer", 3, vec!["y"], vec![]),
            create_system(2, "Transformer", 2, vec!["x"], vec!["y"]),
            create_system(4, "Backflow", 4, vec!["y"], vec!["x"]), // creates a cycle
        ];

        let sorted = schedule_systems(&systems);

        let mut counter = 0;
        let mut ordered: Vec<(usize, &str)> = vec![];
        for group in sorted {
            for sys in group {
                let sys = systems.iter().find(|s| s.id == sys).unwrap();
                ordered.push((counter, &sys.name.type_name_raw));
            }
            counter += 1;
        }

        assert_eq!(ordered, vec![
            // First group
            (0, "Producer"),
            // Second group
            (1, "Transformer"),
            // Third group
            (2, "Consumer"),
            (2, "Backflow")
        ]);
    }

    #[test]
    fn order_two() {
        let systems = vec![
            create_system(1, "Producer", 1, vec!["x"], vec![]),
            // Force the consumer to run before the transformer by ordering.
            create_system(3, "Consumer", 2, vec!["y"], vec![]),
            create_system(2, "Transformer", 3, vec!["x"], vec!["y"]),
            create_system(4, "Backflow", 4, vec!["y"], vec!["x"]), // creates a cycle
        ];

        let sorted = schedule_systems(&systems);

        let mut counter = 0;
        let mut ordered: Vec<(usize, &str)> = vec![];
        for group in sorted {
            for sys in group {
                let sys = systems.iter().find(|s| s.id == sys).unwrap();
                ordered.push((counter, &sys.name.type_name_raw));
            }
            counter += 1;
        }

        assert_eq!(ordered, vec![
            // First group
            (0, "Producer"),
            // Second group
            (1, "Consumer"),
            // Third group
            (2, "Transformer"),
            // Fourth group
            (3, "Backflow")
        ]);
    }
}
