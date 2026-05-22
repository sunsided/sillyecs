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
//!
//! # Conflict tie-break direction
//!
//! When two systems have a bidirectional resource conflict that is not resolved by any
//! `run_after` edge (direct or transitive), the scheduler removes one of the two candidate
//! edges so that one system can run before the other. The system whose name compares
//! lexicographically *less* is chosen to run first: i.e. the alphabetically-earlier name
//! becomes the predecessor and the alphabetically-later name becomes the successor.
//!
//! The same comparison breaks any cycle that survives bidirectional-conflict resolution: the
//! edge whose source system has the lexicographically *greatest* name is dropped, so the
//! alphabetically-latest system in the cycle loses its outgoing edge.
//!
//! After Kahn's algorithm produces a layer, the layer is also sorted by name, so the sequential
//! call order *within* a parallel group is independent of YAML declaration order.
//!
//! Tie-breaking by name (rather than by `SystemId`) makes scheduling independent of the order
//! in which systems are declared in YAML. Renaming a system can still re-order it, but
//! re-ordering systems in the YAML file will not. System names are guaranteed unique by
//! `Ecs::ensure_system_consistency`, so the comparison is total.

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

/// Finds a cycle in `graph` and returns its edges in traversal order, or `None` if the graph is
/// acyclic. Implemented as an iterative tri-color DFS over an explicit work stack so deep system
/// graphs cannot overflow the thread stack.
fn find_cycle(graph: &HashMap<SystemId, HashSet<SystemId>>) -> Option<Vec<(SystemId, SystemId)>> {
    // Colors: 0 = White (unseen), 1 = Gray (on current DFS path), 2 = Black (done).
    let mut color: HashMap<SystemId, u8> = HashMap::with_capacity(graph.len());
    let mut parent: HashMap<SystemId, SystemId> = HashMap::new();

    let neighbors_of = |u: SystemId| -> std::vec::IntoIter<SystemId> {
        let mut v: Vec<SystemId> = graph
            .get(&u)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default();
        // Sort for deterministic cycle discovery (graph stores `HashSet`).
        v.sort_by_key(|id| id.0);
        v.into_iter()
    };

    for &start in graph.keys() {
        if color.get(&start).copied().unwrap_or(0) != 0 {
            continue;
        }
        color.insert(start, 1);
        let mut work: Vec<(SystemId, std::vec::IntoIter<SystemId>)> =
            vec![(start, neighbors_of(start))];

        while let Some((u, mut it)) = work.pop() {
            match it.next() {
                Some(v) => {
                    // Resume `u` after exploring `v`.
                    work.push((u, it));
                    let c = color.get(&v).copied().unwrap_or(0);
                    if c == 0 {
                        parent.insert(v, u);
                        color.insert(v, 1);
                        work.push((v, neighbors_of(v)));
                    } else if c == 1 {
                        // Back-edge u -> v closes a cycle. Walk parents from `u` back to `v`.
                        let mut cycle = vec![(u, v)];
                        let mut cur = u;
                        while cur != v {
                            let p = *parent
                                .get(&cur)
                                .expect("gray node must have a parent on the DFS path");
                            cycle.push((p, cur));
                            cur = p;
                        }
                        cycle.reverse();
                        return Some(cycle);
                    }
                }
                None => {
                    color.insert(u, 2);
                }
            }
        }
    }
    None
}

/// Builds the cycle path (`[n0, n1, ..., n_{k-1}, n0]`) from cycle edges produced by
/// [`find_cycle`].
fn cycle_path(
    cycle_edges: &[(SystemId, SystemId)],
    name_by_id: &HashMap<SystemId, crate::system::SystemName>,
) -> Vec<String> {
    let mut path: Vec<String> = cycle_edges
        .iter()
        .map(|&(u, _)| name_by_id[&u].type_name_raw.clone())
        .collect();
    if let Some(first) = cycle_edges.first() {
        path.push(name_by_id[&first.0].type_name_raw.clone());
    }
    path
}

/// Schedules systems into parallelizable batches using resource dependencies and forced `run_after` ordering.
///
/// Forced `run_after` edges (from a system referenced in run_after to the current system) are added first.
/// Then resource–based candidate edges (writer → reader or writer → writer) are collected.
/// For each unordered pair of systems that share conflicting candidate edges (i.e. edges in both directions),
/// the conflict is resolved by honoring any forced ordering (direct or transitive); if neither applies, the
/// system with the lexicographically-earlier name is chosen as the predecessor. Any cycle that remains is
/// broken by removing the outgoing edge of the system whose name compares greatest. See the module-level
/// docs for the rationale.
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
                // No clear forced preference: tie-break by system name. The system with the
                // lexicographically-earlier name runs first, so we drop the edge that would make
                // it the successor. System names are guaranteed unique by
                // `Ecs::ensure_system_consistency`, so this comparison is total.
                if a.name.type_name_raw < b.name.type_name_raw {
                    // a precedes b: keep a→b, drop b→a
                    graph.get_mut(&b.id).unwrap().remove(&a.id);
                } else {
                    // b precedes a: keep b→a, drop a→b
                    graph.get_mut(&a.id).unwrap().remove(&b.id);
                }
            }
        }
    }

    // (Cycle detection lives at module scope; see `find_cycle`.)

    // Remove one edge per cycle, choosing the edge whose source system has the
    // lexicographically-greatest name. Tie-breaking by name (rather than by SystemId) keeps
    // scheduling independent of YAML declaration order.
    while let Some(cycle_edges) = find_cycle(&graph) {
        if let Some(&(rem_u, rem_v)) = cycle_edges
            .iter()
            .max_by_key(|&&(u, _)| &name_by_id[&u].type_name_raw)
        {
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

        // Sort within-layer by system name (not `SystemId`) so the sequential call order inside
        // a parallel group is also independent of YAML declaration order.
        layer.sort_by(|x, y| {
            name_by_id[x]
                .type_name_raw
                .cmp(&name_by_id[y].type_name_raw)
        });
        layers.push(layer);
        queue = next;
    }

    if visited != n {
        // Re-run cycle detection on the residual graph to surface the full path of the cycle
        // (rather than two arbitrary endpoints) for diagnostics.
        let residual: HashMap<SystemId, HashSet<SystemId>> = graph
            .iter()
            .filter(|(u, _)| in_deg.get(u).copied().unwrap_or(0) > 0)
            .map(|(&u, succs)| {
                let kept: HashSet<SystemId> = succs
                    .iter()
                    .copied()
                    .filter(|v| in_deg.get(v).copied().unwrap_or(0) > 0)
                    .collect();
                (u, kept)
            })
            .collect();
        if let Some(cycle_edges) = find_cycle(&residual) {
            return Err(EcsError::CycleDetectedBetweenSystems(cycle_path(
                &cycle_edges,
                &name_by_id,
            )));
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
                // First group (name-sorted: Backflow < Consumer)
                (0, "Backflow"), // reads y, writes x
                (0, "Consumer"), // reads y
                // Second group (name-sorted: Producer < Transformer)
                (1, "Producer"),    // reads x
                (1, "Transformer")  // reads x, writes y, forced to run after Consumer
            ]
        );
    }

    /// Bidirectional resource conflict between two systems whose name order *disagrees* with
    /// `SystemId` order. The old ID-based tie-break would let the higher-`SystemId` system run
    /// first; the name-based tie-break makes the alphabetically-earlier name run first.
    #[test]
    fn bidirectional_tiebreak_uses_name_not_id() {
        let systems = vec![
            // id=1, but lexicographically *latest* name -> must NOT run first
            create_system(1, "ZuluWriter", vec!["b"], vec!["a"], vec![]),
            // id=2, but lexicographically *earliest* name -> must run first
            create_system(2, "AlphaWriter", vec!["a"], vec!["b"], vec![]),
        ];

        let sorted = schedule_systems(&systems).unwrap();

        let mut ordered: Vec<(usize, &str)> = vec![];
        for (group_idx, group) in sorted.iter().enumerate() {
            for sys_id in group {
                let sys = systems.iter().find(|s| s.id == *sys_id).unwrap();
                ordered.push((group_idx, &sys.name.type_name_raw));
            }
        }

        // Under the old ID rule this would be `[(0, "ZuluWriter"), (1, "AlphaWriter")]`.
        assert_eq!(
            ordered,
            vec![(0, "AlphaWriter"), (1, "ZuluWriter")],
            "alphabetically-earlier name must run first regardless of SystemId",
        );
    }

    /// Three-system resource cycle (no bidirectional pair, so resolution skips to the cycle-break
    /// step). The cycle-break rule drops the outgoing edge of the lexicographically-*greatest*
    /// name. The old ID-based rule would drop the highest-`SystemId` source instead; the IDs are
    /// chosen so the two rules pick different edges and therefore different schedules.
    #[test]
    fn cycle_break_uses_greatest_name_not_highest_id() {
        // Cycle: Zulu(1) -> Alpha(2) -> Beta(3) -> Zulu(1).
        //
        // Old rule: drop Beta -> Zulu (Beta has highest id). Schedule: Zulu, Alpha, Beta.
        // New rule: drop Zulu -> Alpha (Zulu has greatest name). Schedule: Alpha, Beta, Zulu.
        let systems = vec![
            create_system(1, "Zulu", vec!["c"], vec!["a"], vec![]),
            create_system(2, "Alpha", vec!["a"], vec!["b"], vec![]),
            create_system(3, "Beta", vec!["b"], vec!["c"], vec![]),
        ];

        let sorted = schedule_systems(&systems).unwrap();

        let mut ordered: Vec<(usize, &str)> = vec![];
        for (group_idx, group) in sorted.iter().enumerate() {
            for sys_id in group {
                let sys = systems.iter().find(|s| s.id == *sys_id).unwrap();
                ordered.push((group_idx, &sys.name.type_name_raw));
            }
        }

        assert_eq!(
            ordered,
            vec![(0, "Alpha"), (1, "Beta"), (2, "Zulu")],
            "cycle break must drop the edge from the lexicographically-greatest source",
        );
    }

    /// `find_cycle` must terminate without overflowing the thread stack on graphs whose longest
    /// path is far deeper than the recursive implementation could tolerate. A linear chain of
    /// 50_000 nodes with a single back-edge at the end is enough to overflow the default
    /// 8 MiB stack with the previous recursive DFS.
    #[test]
    fn find_cycle_handles_deep_graphs_without_stack_overflow() {
        const N: u64 = 50_000;
        let mut graph: HashMap<SystemId, HashSet<SystemId>> = HashMap::new();
        for i in 0..N {
            graph
                .entry(SystemId(i))
                .or_default()
                .insert(SystemId(i + 1));
        }
        graph.entry(SystemId(N)).or_default(); // sink, no outgoing edges
        assert!(find_cycle(&graph).is_none(), "linear chain must be acyclic");

        // Close the chain into a cycle and confirm it is detected without recursion.
        graph.get_mut(&SystemId(N)).unwrap().insert(SystemId(0));
        let cycle = find_cycle(&graph).expect("back-edge must produce a cycle");
        assert_eq!(
            cycle.len() as u64,
            N + 1,
            "cycle should include every edge in the chain plus the back-edge",
        );
    }

    /// `cycle_path` should render the cycle as a closed walk `[n0, ..., n_{k-1}, n0]`, and the
    /// resulting `EcsError::CycleDetectedBetweenSystems` should format it as an arrow-separated
    /// path. The previous error variant only named two endpoints.
    #[test]
    fn cycle_path_renders_full_path_in_error_message() {
        let names = [
            (SystemId(1), sysname("Alpha")),
            (SystemId(2), sysname("Beta")),
            (SystemId(3), sysname("Gamma")),
        ];
        let name_by_id: HashMap<SystemId, _> = names.into_iter().collect();

        // Triangle: Alpha -> Beta -> Gamma -> Alpha.
        let cycle_edges = vec![
            (SystemId(1), SystemId(2)),
            (SystemId(2), SystemId(3)),
            (SystemId(3), SystemId(1)),
        ];

        let path = cycle_path(&cycle_edges, &name_by_id);
        assert_eq!(
            path,
            vec![
                "Alpha".to_string(),
                "Beta".to_string(),
                "Gamma".to_string(),
                "Alpha".to_string(),
            ],
            "cycle path must list every node in order and close back to the start",
        );

        let err = EcsError::CycleDetectedBetweenSystems(path);
        assert_eq!(
            err.to_string(),
            "A cycle was detected in the system run order: Alpha -> Beta -> Gamma -> Alpha.",
        );
    }
}
