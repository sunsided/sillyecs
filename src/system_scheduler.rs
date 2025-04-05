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
use crate::ecs::EcsError;
use crate::state::StateNameRef;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum Access {
    Read,
    Write,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Dependency {
    pub resource: Resource,
    pub access: Access,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum Resource {
    /// The system accesses a component.
    Component(ComponentName),
    /// The system accesses the frame context.
    FrameContext,
    /// The system accesses user state.
    UserState(StateNameRef)
}

/// Schedules systems into parallelizable batches using resource dependencies and forced `run_after` ordering.
///
/// Forced `run_after` edges (from a system referenced in run_after to the current system) are added first.
/// Then resource–based candidate edges (writer → reader) are collected.
/// For each unordered pair of systems that share conflicting candidate edges (i.e. edges in both directions),
/// the conflict is resolved by favoring one candidate if possible; otherwise, a cycle is detected and we panic.
pub fn schedule_systems(systems: &[System]) -> Result<Vec<Vec<SystemId>>, EcsError> {
    // The final dependency graph.
    let mut graph: HashMap<SystemId, Vec<SystemId>> = HashMap::new();
    // in_degree tracks the number of incoming edges per system.
    let mut in_degree: HashMap<SystemId, usize> = HashMap::new();
    // Convenience lookup.
    let mut systems_by_id: HashMap<SystemId, &System> = HashMap::new();

    // Listing order mapping.
    let id_to_index: HashMap<SystemId, usize> = systems
        .iter()
        .enumerate()
        .map(|(i, sys)| (sys.id, i))
        .collect();

    for sys in systems {
        systems_by_id.insert(sys.id, sys);
        in_degree.insert(sys.id, 0);
    }

    // --- Step 1: Add Forced run_after Edges ---
    // For each system, add an edge from every system it must run after.
    for sys in systems {
        for run_after_name in &sys.run_after {
            // Find the system by name.
            let pred = systems.iter().find(|s| s.name.eq(run_after_name))
                .expect(&format!("Failed to find system {name} specified in run_after", name = run_after_name.type_name_raw));
            // Add forced edge: pred -> sys.
            graph.entry(pred.id).or_default().push(sys.id);
            *in_degree.entry(sys.id).or_default() += 1;
        }
    }

    // --- Step 2: Collect Resource–based Candidate Edges ---
    // We'll collect candidates in a map keyed by an unordered pair (min(id), max(id)).
    // The value is a tuple (set of candidate directions: true means first->second, false means second->first).
    type EdgeDir = bool; // true means (a -> b) where a is min(id)
    let mut candidate_edges: HashMap<(SystemId, SystemId), HashSet<EdgeDir>> = HashMap::new();

    // Build readers and writers maps.
    let mut readers: HashMap<Resource, HashSet<SystemId>> = HashMap::new();
    let mut writers: HashMap<Resource, HashSet<SystemId>> = HashMap::new();
    for sys in systems {
        for dep in &sys.dependencies {
            match dep.access {
                Access::Read => {
                    readers.entry(dep.resource.clone()).or_default().insert(sys.id);
                }
                Access::Write => {
                    writers.entry(dep.resource.clone()).or_default().insert(sys.id);
                }
            }
        }
    }

    // For each resource, for each writer, add candidate edges to each reader,
    // except if a forced run_after edge exists between them (in either direction).
    // For each resource, for each writer, add candidate edges to each reader,
    // except if a forced run_after edge exists between them (in either direction).
    for (resource, writer_ids) in &writers {
        let mut affected = HashSet::new();
        if let Some(r) = readers.get(resource) {
            affected.extend(r);
        }
        if let Some(w) = writers.get(resource) {
            affected.extend(w);
        }
        for &writer in writer_ids {
            for &reader in &affected {
                if writer == reader {
                    continue;
                }
                let writer_sys = systems_by_id.get(&writer).unwrap();
                let reader_sys = systems_by_id.get(&reader).unwrap();
                // If either system forces the other, skip the resource candidate edge.
                let forced = writer_sys.run_after.iter().any(|name| name.eq(&reader_sys.name))
                    || reader_sys.run_after.iter().any(|name| name.eq(&writer_sys.name));
                if forced {
                    continue;
                }
                // Determine an ordering key (min, max) and candidate direction.
                let (a, b, direction) = if id_to_index[&writer] < id_to_index[&reader] {
                    (writer, reader, true) // candidate edge: writer (a) -> reader (b)
                } else {
                    (reader, writer, false) // candidate edge becomes b -> a
                };
                candidate_edges.entry((a, b)).or_default().insert(direction);
            }
        }
    }

    // --- Step 3: Resolve Candidate Edge Conflicts ---
    // For each pair, if candidate_edges yields a single direction, add that edge.
    // If both directions are present, resolve the conflict using a tie-breaker.
    for ((a, b), dirs) in candidate_edges {
        let chosen = if dirs.len() == 1 {
            dirs.iter().next().unwrap().clone()
        } else {
            // Conflict: both directions were proposed.
            // Tie-breaker: favor the edge that puts the system with a non-empty run_after list later.
            let sys_a = systems_by_id.get(&a).unwrap();
            let sys_b = systems_by_id.get(&b).unwrap();
            // If one of them has forced ordering (non-empty run_after), choose that ordering.
            if !sys_a.run_after.is_empty() && sys_b.run_after.is_empty() {
                false // choose b->a, so that a (with run_after) comes later.
            } else if sys_a.run_after.is_empty() && !sys_b.run_after.is_empty() {
                true // choose a->b.
            } else {
                // Otherwise, fall back to listing order.
                // Favor the ordering consistent with the listing order.
                id_to_index[&a] < id_to_index[&b]
            }
        };
        // Now add the chosen edge in the graph.
        if chosen {
            // That means: a -> b.
            graph.entry(a).or_default().push(b);
            *in_degree.entry(b).or_default() += 1;
        } else {
            // Means: b -> a.
            graph.entry(b).or_default().push(a);
            *in_degree.entry(a).or_default() += 1;
        }
    }

    // --- Step 4: Topological Sort ---
    let mut ready: Vec<SystemId> = in_degree
        .iter()
        .filter_map(|(&id, &deg)| if deg == 0 { Some(id) } else { None })
        .collect();
    let mut scheduled: Vec<Vec<SystemId>> = Vec::new();
    let mut visited = HashSet::new();

    while !ready.is_empty() {
        // Sort ready queue by listing order.
        ready.sort_by_key(|id| id_to_index[id]);
        let mut batch = Vec::new();
        let mut used_writes = HashSet::new();
        let mut i = 0;
        while i < ready.len() {
            let candidate = ready[i];
            let sys = systems_by_id.get(&candidate).unwrap();
            // Check for conflicts within the batch: systems writing the same resource can't run in parallel.
            let conflict = sys.dependencies.iter().any(|dep| {
                matches!(dep.access, Access::Write) && used_writes.contains(&dep.resource)
            });
            if !conflict {
                batch.push(candidate);
                for dep in &sys.dependencies {
                    if let Access::Write = dep.access {
                        used_writes.insert(dep.resource.clone());
                    }
                }
                ready.remove(i);
            } else {
                i += 1;
            }
        }
        if batch.is_empty() {
            return Err(EcsError::CycleDetectedInSystemRunOrder);
        }
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
        scheduled.push(batch);
    }
    if visited.len() != systems.len() {
        return Err(EcsError::CycleDetectedInSystemRunOrder);
    }
    Ok(scheduled)
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

    fn create_system(id: u64, name: &str, inputs: Vec<&str>, outputs: Vec<&str>, prefer_after: Vec<&str>) -> System {
        let mut system = System {
            id: SystemId(id),
            name: sysname(name),
            run_after: prefer_after.into_iter().map(sysname).collect(),
            context: false,
            states: vec![],
            entities: false,
            commands: false,
            inputs: inputs.into_iter().map(compname).collect(),
            outputs: outputs.into_iter().map(compname).collect(),
            phase: phasename("default"),
            affected_archetype_count: 0,
            affected_archetype_ids: Default::default(),
            affected_archetypes: Default::default(),
            component_iter_code: String::new(),
            component_untuple_code: String::new(),
            description: None,
            dependencies: Default::default()
        };
        system.finish_dependencies();
        system
    }

    #[test]
    fn no_preference_creates_three_groups() {
        // Systems are free to run in any order that creates the least amount of groups while
        // attempting to resolve read-write ordering as much as possible.
        let systems = vec![
            create_system(1, "Producer", vec!["x"], vec![], vec![]),
            create_system(2, "Consumer", vec!["y"], vec![], vec![]),
            create_system(3, "Transformer", vec!["x"], vec!["y"], vec![]),
            create_system(4, "Backflow", vec!["y"], vec!["x"], vec![]), // creates a cycle
        ];

        let sorted = schedule_systems(&systems).unwrap();

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
            (0, "Transformer"), // reads x, writes y
            // Second group
            (1, "Consumer"),    // reads y
            (1, "Backflow"),    // reads y, writes x
            // Third group
            (2, "Producer")     // reads x
        ]);
    }

    #[test]
    fn preference_forces_two_groups() {
        let systems = vec![
            create_system(1, "Producer", vec!["x"], vec![], vec![]),
            create_system(2, "Transformer", vec!["x"], vec!["y"], vec!["Consumer"]),
            create_system(3, "Consumer", vec!["y"], vec![], vec![]),
            create_system(4, "Backflow", vec!["y"], vec!["x"], vec![]), // creates a cycle
        ];

        let sorted = schedule_systems(&systems).unwrap();

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
            (0, "Consumer"),   // reads y
            (0, "Backflow"),   // reads y, writes x
            // Second group
            (1, "Producer"),   // reads x
            (1, "Transformer") // reads x, writes y, forced to run after Consumer
        ]);
    }
}
