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
use crate::ecs::EcsError;
use crate::state::StateNameRef;
use crate::system::{System, SystemId};
use std::collections::{HashMap, HashSet, VecDeque};

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
    UserState(StateNameRef),
}

/// Schedules systems into parallelizable batches using resource dependencies and forced `run_after` ordering.
///
/// Forced `run_after` edges (from a system referenced in run_after to the current system) are added first.
/// Then resource–based candidate edges (writer → reader or writer → writer) are collected.
/// For each unordered pair of systems that share conflicting candidate edges (i.e. edges in both directions),
/// the conflict is resolved by honoring any forced ordering (direct or transitive); if neither applies, a tie-break by ID is used.
pub fn schedule_systems(systems: &[System]) -> Result<Vec<Vec<SystemId>>, EcsError> {
    let n = systems.len();

    // map names ↔ ids
    let id_by_name = systems
        .iter()
        .map(|sys| (sys.name.clone(), sys.id))
        .collect::<HashMap<_, _>>();
    let name_by_id = systems
        .iter()
        .map(|sys| (sys.id, sys.name.clone()))
        .collect::<HashMap<_, _>>();

    // Build initial adjacency for forced run_after edges
    let mut graph: HashMap<SystemId, HashSet<SystemId>> = HashMap::new();
    let mut forced_edges: HashSet<(SystemId, SystemId)> = HashSet::new();
    for sys in systems {
        graph.entry(sys.id).or_default();
        for pred in &sys.run_after {
            let p = id_by_name[&pred];
            graph.entry(p).or_default().insert(sys.id);
            forced_edges.insert((p, sys.id));
        }
    }

    // Build forced adjacency for reachability
    let mut forced_adj: HashMap<SystemId, Vec<SystemId>> = HashMap::new();
    for &(u, v) in &forced_edges {
        forced_adj.entry(u).or_default().push(v);
    }

    // Helper: check transitive forced reachability u →* v
    fn forced_reachable(
        adj: &HashMap<SystemId, Vec<SystemId>>,
        start: SystemId,
        target: SystemId,
    ) -> bool {
        let mut stack = vec![start];
        let mut seen = HashSet::new();
        while let Some(u) = stack.pop() {
            if u == target {
                return true;
            }
            if !seen.insert(u) {
                continue;
            }
            if let Some(neis) = adj.get(&u) {
                for &w in neis {
                    stack.push(w);
                }
            }
        }
        false
    }

    // Add resource-based edges: any writer → (reader or writer) of same resource
    for a in systems {
        for b in systems {
            if a.id == b.id {
                continue;
            }
            if a.dependencies.iter().any(|da| {
                da.access == Access::Write
                    && b.dependencies.iter().any(|db| db.resource == da.resource)
            }) {
                graph.entry(a.id).or_default().insert(b.id);
            }
        }
    }

    // Resolve bidirectional conflicts
    for a in systems {
        for b in systems {
            if a.id >= b.id {
                continue;
            }
            let has_ab = graph.get(&a.id).map_or(false, |s| s.contains(&b.id));
            let has_ba = graph.get(&b.id).map_or(false, |s| s.contains(&a.id));
            if has_ab && has_ba {
                // honor any forced (direct or transitive)
                let reach_ab = forced_reachable(&forced_adj, a.id, b.id);
                let reach_ba = forced_reachable(&forced_adj, b.id, a.id);
                if reach_ab && !reach_ba {
                    // a should precede b: remove b→a
                    graph.get_mut(&b.id).unwrap().remove(&a.id);
                    continue;
                }
                if reach_ba && !reach_ab {
                    graph.get_mut(&a.id).unwrap().remove(&b.id);
                    continue;
                }
                // no clear forced preference: tie-break by ID
                if a.id < b.id {
                    graph.get_mut(&a.id).unwrap().remove(&b.id);
                } else {
                    graph.get_mut(&b.id).unwrap().remove(&a.id);
                }
            }
        }
    }

    // Break remaining cycles by removing one edge per cycle, tie-breaking by highest SystemId
    // Helper to find a cycle and return its edges
    fn find_cycle(
        graph: &HashMap<SystemId, HashSet<SystemId>>,
    ) -> Option<Vec<(SystemId, SystemId)>> {
        fn dfs(
            u: SystemId,
            graph: &HashMap<SystemId, HashSet<SystemId>>,
            visited: &mut HashSet<SystemId>,
            stack: &mut Vec<SystemId>,
            stack_set: &mut HashSet<SystemId>,
        ) -> Option<Vec<(SystemId, SystemId)>> {
            visited.insert(u);
            stack.push(u);
            stack_set.insert(u);
            if let Some(neis) = graph.get(&u) {
                for &v in neis {
                    if !visited.contains(&v) {
                        if let Some(cycle) = dfs(v, graph, visited, stack, stack_set) {
                            return Some(cycle);
                        }
                    } else if stack_set.contains(&v) {
                        // Found a cycle: collect edges from v to u
                        let mut cycle = Vec::new();
                        let mut started = false;
                        let mut prev = u;
                        for &node in stack.iter() {
                            if node == v {
                                started = true;
                                prev = v;
                                continue;
                            }
                            if started {
                                cycle.push((prev, node));
                                prev = node;
                            }
                        }
                        // close the cycle
                        cycle.push((u, v));
                        return Some(cycle);
                    }
                }
            }
            stack.pop();
            stack_set.remove(&u);
            None
        }

        let mut visited = HashSet::new();
        let mut stack = Vec::new();
        let mut stack_set = HashSet::new();
        for &u in graph.keys() {
            if !visited.contains(&u) {
                if let Some(cycle) = dfs(u, graph, &mut visited, &mut stack, &mut stack_set) {
                    return Some(cycle);
                }
            }
        }
        None
    }

    // Remove one edge per cycle, choosing the edge with highest source ID
    while let Some(cycle_edges) = find_cycle(&graph) {
        if let Some(&(rem_u, rem_v)) = cycle_edges.iter().max_by_key(|&&(u, _)| u) {
            graph.get_mut(&rem_u).unwrap().remove(&rem_v);
        }
    }

    // Compute in-degrees
    let mut in_deg: HashMap<SystemId, usize> = systems.iter().map(|s| (s.id, 0)).collect();
    for (&_u, succs) in &graph {
        for &v in succs {
            *in_deg.get_mut(&v).unwrap() += 1;
        }
    }

    // Kahn’s algorithm, layered
    let mut layers: Vec<Vec<SystemId>> = Vec::new();
    let mut queue: VecDeque<SystemId> = in_deg
        .iter()
        .filter_map(|(&id, &d)| if d == 0 { Some(id) } else { None })
        .collect();
    let mut visited = 0;

    while !queue.is_empty() {
        let mut next = VecDeque::new();
        let mut layer = Vec::new();

        while let Some(u) = queue.pop_front() {
            layer.push(u);
            visited += 1;
            for &v in graph.get(&u).unwrap_or(&HashSet::new()) {
                let d = in_deg.get_mut(&v).unwrap();
                *d -= 1;
                if *d == 0 {
                    next.push_back(v);
                }
            }
        }

        layer.sort();
        layers.push(layer);
        queue = next;
    }

    if visited != n {
        // detect any remaining cycle edge
        for (&u, succs) in &graph {
            for &v in succs {
                if *in_deg.get(&u).unwrap() > 0 && *in_deg.get(&v).unwrap() > 0 {
                    let nu = &name_by_id[&u].type_name_raw;
                    let nv = &name_by_id[&v].type_name_raw;
                    return Err(EcsError::CycleDetectedBetweenSystems(
                        nu.clone(),
                        nv.clone(),
                    ));
                }
            }
        }
        return Err(EcsError::CycleDetectedInSystemRunOrder);
    }

    Ok(layers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Name;
    use crate::component::ComponentName;
    use crate::system::{System, SystemId, SystemName, SystemPhaseName, SystemPhaseRef};

    fn sysname(name: &str) -> SystemName {
        SystemName(Name::new(name.to_string(), "System"))
    }

    fn compname(name: &str) -> ComponentName {
        ComponentName(Name::new(name.to_string(), "Component"))
    }

    fn phasename(name: &str) -> SystemPhaseRef {
        SystemPhaseName(Name::new(name.to_string(), "Phase"))
    }

    fn create_system(
        id: u64,
        name: &str,
        inputs: Vec<&str>,
        outputs: Vec<&str>,
        prefer_after: Vec<&str>,
    ) -> System {
        let mut system = System {
            id: SystemId(id),
            name: sysname(name),
            run_after: prefer_after.into_iter().map(sysname).collect(),
            context: false,
            states: vec![],
            lookup: vec![],
            preflight: false,
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
            dependencies: Default::default(),
            postflight: false,
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

        assert_eq!(
            ordered,
            vec![
                // First group
                (0, "Backflow"), // reads y, writes x
                // Second group
                (1, "Producer"),    // reads x
                (1, "Transformer"), // reads x, writes y
                // Third group
                (2, "Consumer"), // reads y
            ]
        );
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

        assert_eq!(
            ordered,
            vec![
                // First group
                (0, "Consumer"), // reads y
                (0, "Backflow"), // reads y, writes x
                // Second group
                (1, "Producer"),    // reads x
                (1, "Transformer")  // reads x, writes y, forced to run after Consumer
            ]
        );
    }
}
