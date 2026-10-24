//! System scheduling module that manages parallel execution of ECS systems.
//!
//! This module provides functionality to analyze system dependencies and group them into
//! parallelizable batches for efficient execution. It handles:
//!
//! - Component read/write dependencies between systems
//! - Explicit ordering requirements
//! - Resource conflict resolution
//! - Parallel batch scheduling
//! - Rejection of contradictory `run_after` ordering
//!
//! The main entry point is the [`schedule_systems`] function which takes a slice of systems
//! and returns an ordered list of system batches that can be executed in parallel while
//! respecting all dependencies and constraints.
//!
//! # Guarantees
//!
//! - Every `run_after` edge is honored. A `run_after` cycle is a user contradiction and is
//!   rejected with [`EcsError::CycleDetectedBetweenSystems`].
//! - Two systems that conflict on a resource (at least one of them writes it) never share a
//!   parallel group; one of them always runs strictly before the other.
//! - The schedule depends only on system names and their declared accesses, not on the order
//!   in which systems are declared in YAML.
//!
//! # Conflict direction
//!
//! The dependency graph starts out as the `run_after` edges and only ever grows; no edge is
//! removed again, so it stays acyclic and every ordering committed to it survives. Each
//! conflicting pair is then ordered in its preferred direction unless the graph already orders
//! it the other way, in which case the pair is left as is:
//!
//! 1. Writer-vs-reader pairs prefer writer → reader, so the reader observes this tick's data.
//!    Pairs are visited by (writer name, reader name), so in a pure data-dependency cycle the
//!    edge out of the lexicographically *greatest* writer is the one that cannot be honored.
//!    Being overruled by a `run_after` chain is the user's intent and stays silent; being
//!    overruled by other data dependencies emits a `cargo:warning`.
//! 2. Writer-vs-writer pairs have no natural direction and prefer the alphabetically-earlier
//!    name as predecessor. They are visited in name-pair order.
//!
//! After Kahn's algorithm produces a layer, the layer is also sorted by name, so the sequential
//! call order *within* a parallel group is independent of YAML declaration order.
//!
//! Renaming a system can still re-order it, but re-ordering systems in the YAML file will not.
//! System names are guaranteed unique by `Ecs::ensure_system_consistency`, so the comparison is
//! total.

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

    // Sort DFS start nodes so cycle discovery (and therefore which edge the cycle-break loop
    // removes / which path the diagnostic reports) is independent of `HashMap` iteration order.
    let mut starts: Vec<SystemId> = graph.keys().copied().collect();
    starts.sort_by_key(|id| id.0);

    for start in starts {
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
/// Forced `run_after` edges are added first; a cycle among them is an error. Each conflicting
/// pair of systems is then ordered in its preferred direction, unless the graph already orders
/// it the other way. Edges are never removed, so the graph stays acyclic and every conflicting
/// pair ends up ordered. See the module-level docs for the rationale.
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
    for sys in systems {
        graph.entry(sys.id).or_default();
        for pred in &sys.run_after {
            let p = id_by_name[&pred];
            graph.entry(p).or_default().insert(sys.id);
        }
    }

    // A run_after cycle is a contradiction in the user's ordering; nothing can satisfy it.
    if let Some(cycle_edges) = find_cycle(&graph) {
        return Err(EcsError::CycleDetectedBetweenSystems(cycle_path(
            &cycle_edges,
            &name_by_id,
        )));
    }
    let forced = graph.clone();

    // Helper: is `target` reachable from `start` in `graph`?
    fn reachable(
        graph: &HashMap<SystemId, HashSet<SystemId>>,
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
            if let Some(neis) = graph.get(&u) {
                for &w in neis {
                    stack.push(w);
                }
            }
        }
        false
    }

    // Classify resource conflicts per unordered pair.
    // - Writer-vs-reader (only one side writes a shared resource): prefer writer → reader.
    // - Writer-vs-writer (both write a shared resource): prefer the name-earlier predecessor.
    let mut write_read: Vec<(SystemId, SystemId)> = Vec::new();
    let mut write_write: Vec<(SystemId, SystemId)> = Vec::new();
    for a in systems {
        for b in systems {
            if a.id >= b.id {
                continue;
            }
            let a_writes_shared = a.dependencies.iter().any(|da| {
                da.access == Access::Write
                    && b.dependencies.iter().any(|db| db.resource == da.resource)
            });
            let b_writes_shared = b.dependencies.iter().any(|db| {
                db.access == Access::Write
                    && a.dependencies.iter().any(|da| da.resource == db.resource)
            });
            match (a_writes_shared, b_writes_shared) {
                (false, false) => {}
                (true, false) => write_read.push((a.id, b.id)),
                (false, true) => write_read.push((b.id, a.id)),
                (true, true) => {
                    let a_name = &name_by_id[&a.id].type_name_raw;
                    let b_name = &name_by_id[&b.id].type_name_raw;
                    if a_name < b_name {
                        write_write.push((a.id, b.id));
                    } else {
                        write_write.push((b.id, a.id));
                    }
                }
            }
        }
    }

    // Visit pairs in name order (not `SystemId` order), so YAML reordering cannot change which
    // preference gets overruled when the preferences form a cycle.
    let name_key = |&(u, v): &(SystemId, SystemId)| {
        (
            name_by_id[&u].type_name_raw.as_str(),
            name_by_id[&v].type_name_raw.as_str(),
        )
    };
    write_read.sort_by_key(name_key);
    write_write.sort_by_key(name_key);

    // Commit each pair in its preferred direction unless the graph already orders it the other
    // way. Edges are never removed, so the graph stays acyclic and every pair stays ordered.
    for (pred, succ) in write_read {
        if !reachable(&graph, succ, pred) {
            graph.entry(pred).or_default().insert(succ);
        } else if !reachable(&forced, succ, pred) {
            println!(
                "cargo:warning=sillyecs scheduler: data-dependency cycle; {} runs before {} \
                 and reads data {} writes in the previous tick",
                name_by_id[&succ].type_name_raw,
                name_by_id[&pred].type_name_raw,
                name_by_id[&pred].type_name_raw,
            );
        }
    }
    for (pred, succ) in write_write {
        if !reachable(&graph, succ, pred) {
            graph.entry(pred).or_default().insert(succ);
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

    // Run_after cycles were rejected up front and no committed edge closes a cycle.
    assert_eq!(
        visited, n,
        "scheduler graph must be acyclic by construction"
    );

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

    /// Three-system resource cycle (no bidirectional pair). The writer → reader preference out of
    /// the lexicographically-*greatest* writer is the one that cannot be honored. The old ID-based
    /// rule would overrule the highest-`SystemId` writer instead; the IDs are chosen so the two
    /// rules pick different edges and therefore different schedules.
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

    /// Regression for sillyecs scheduler vs. user `run_after`: every system writes a shared
    /// resource (so each pair is in conflict), and a `run_after` chain pins the order. The
    /// alphabetically-earliest system (`DynamicLight`) `run_after`s `Render`, so the name-based
    /// tie-break with `FrameGlobals` (`DynamicLight < FrameGlobals`, also `FrameGlobals < Render`)
    /// must not put `DynamicLight` in front of `Render`. A tie-break the graph already orders the
    /// other way is skipped, so the user's forced chain is preserved end-to-end.
    #[test]
    fn cycle_aware_tiebreak_preserves_forced_chain_against_name_order() {
        // All systems write to the same resource `g`. Run-after chain:
        //   RenderBackground -> RenderSky -> Render -> DynamicLight
        // FrameGlobals has no `run_after`. Alphabetical order is:
        //   DynamicLight < FrameGlobals < Render < RenderBackground < RenderSky
        // The naive name tie-break would put `DynamicLight` before `FrameGlobals` and
        // `FrameGlobals` before `Render`, but `Render -> DynamicLight` is forced, which would
        // require dropping a user-specified `run_after` to break the resulting cycle.
        let systems = vec![
            create_system(1, "RenderBackground", vec![], vec!["g"], vec![]),
            create_system(2, "RenderSky", vec![], vec!["g"], vec!["RenderBackground"]),
            create_system(3, "Render", vec![], vec!["g"], vec!["RenderSky"]),
            create_system(4, "DynamicLight", vec![], vec!["g"], vec!["Render"]),
            create_system(5, "FrameGlobals", vec![], vec!["g"], vec![]),
        ];

        let sorted = schedule_systems(&systems).unwrap();

        let mut ordered: Vec<(usize, &str)> = vec![];
        for (group_idx, group) in sorted.iter().enumerate() {
            for sys_id in group {
                let sys = systems.iter().find(|s| s.id == *sys_id).unwrap();
                ordered.push((group_idx, &sys.name.type_name_raw));
            }
        }

        // Every system is in its own layer (they all write `g`) and the forced chain holds.
        let position = |name: &str| ordered.iter().position(|(_, n)| *n == name).unwrap();
        assert!(
            position("RenderBackground") < position("RenderSky"),
            "forced edge RenderBackground -> RenderSky must be honored: {:?}",
            ordered
        );
        assert!(
            position("RenderSky") < position("Render"),
            "forced edge RenderSky -> Render must be honored: {:?}",
            ordered
        );
        assert!(
            position("Render") < position("DynamicLight"),
            "forced edge Render -> DynamicLight must be honored: {:?}",
            ordered
        );
        // FrameGlobals shares a write conflict with every other system, so it cannot share a
        // layer with any of them.
        let frame_layer = ordered
            .iter()
            .find(|(_, n)| *n == "FrameGlobals")
            .unwrap()
            .0;
        for (layer, name) in &ordered {
            if *name == "FrameGlobals" {
                continue;
            }
            assert_ne!(
                *layer, frame_layer,
                "FrameGlobals must not share a layer with {} (both write `g`)",
                name
            );
        }
    }

    /// Systems per layer, by name.
    fn layer_names(systems: &[System], layers: &[Vec<SystemId>]) -> Vec<Vec<String>> {
        layers
            .iter()
            .map(|layer| {
                layer
                    .iter()
                    .map(|id| {
                        let sys = systems.iter().find(|s| s.id == *id).unwrap();
                        sys.name.type_name_raw.clone()
                    })
                    .collect()
            })
            .collect()
    }

    /// Asserts that no two systems sharing a layer conflict on a resource.
    fn assert_no_conflict_within_layers(systems: &[System], layers: &[Vec<SystemId>]) {
        for layer in layers {
            for a in layer {
                for b in layer {
                    let a = systems.iter().find(|s| s.id == *a).unwrap();
                    let b = systems.iter().find(|s| s.id == *b).unwrap();
                    if a.id == b.id {
                        continue;
                    }
                    let conflict = a.dependencies.iter().any(|da| {
                        da.access == Access::Write
                            && b.dependencies.iter().any(|db| db.resource == da.resource)
                    });
                    assert!(
                        !conflict,
                        "{} and {} conflict but share a layer: {:?}",
                        a.name.type_name_raw,
                        b.name.type_name_raw,
                        layer_names(systems, layers)
                    );
                }
            }
        }
    }

    /// Regression: `run_after` edges and data dependencies formed several cycles here. The old
    /// cycle-break step dropped `Foxtrot -> Delta` (Foxtrot writes `b`, Delta reads it) in one
    /// cycle and the reverse path in another, so the two landed in the same parallel group.
    #[test]
    fn conflicting_systems_never_share_a_layer() {
        let systems = vec![
            create_system(0, "Echo", vec!["a"], vec![], vec![]),
            create_system(1, "Foxtrot", vec![], vec!["b"], vec!["Echo"]),
            create_system(2, "Charlie", vec!["b"], vec![], vec![]),
            create_system(3, "Alpha", vec![], vec!["a"], vec!["Charlie"]),
            create_system(4, "Delta", vec!["b"], vec!["c"], vec!["Echo", "Alpha"]),
            create_system(5, "Bravo", vec!["b", "c"], vec!["a", "d"], vec![]),
        ];

        let layers = schedule_systems(&systems).unwrap();
        assert_no_conflict_within_layers(&systems, &layers);
    }

    /// Name, inputs, outputs and `run_after` of a system under test.
    type Spec<'a> = (&'a str, Vec<&'a str>, Vec<&'a str>, Vec<&'a str>);

    /// Regression: the old cycle-break step walked the graph in `SystemId` order, so with several
    /// cycles the dropped edges, and therefore the schedule, depended on YAML declaration order.
    #[test]
    fn schedule_is_independent_of_declaration_order() {
        let specs: Vec<Spec> = vec![
            ("Echo", vec!["a", "c", "d"], vec!["b"], vec![]),
            ("Bravo", vec![], vec!["d"], vec![]),
            ("Alpha", vec!["c", "d"], vec!["a"], vec![]),
            ("Delta", vec!["a"], vec!["c"], vec!["Alpha"]),
            ("Charlie", vec![], vec!["d"], vec!["Echo"]),
            ("Foxtrot", vec!["d"], vec![], vec![]),
        ];
        let build = |specs: &[Spec]| -> Vec<System> {
            specs
                .iter()
                .enumerate()
                .map(|(id, (name, inputs, outputs, run_after))| {
                    create_system(
                        id as u64,
                        name,
                        inputs.clone(),
                        outputs.clone(),
                        run_after.clone(),
                    )
                })
                .collect()
        };

        let forward = build(&specs);
        let mut reversed_specs = specs.clone();
        reversed_specs.reverse();
        let reversed = build(&reversed_specs);

        let forward_layers = schedule_systems(&forward).unwrap();
        let reversed_layers = schedule_systems(&reversed).unwrap();
        assert_eq!(
            layer_names(&forward, &forward_layers),
            layer_names(&reversed, &reversed_layers),
        );
        assert_no_conflict_within_layers(&forward, &forward_layers);
    }

    /// A `run_after` cycle cannot be satisfied by any schedule and must be rejected instead of
    /// silently dropping one of the user's edges.
    #[test]
    fn contradictory_run_after_is_rejected() {
        let systems = vec![
            create_system(1, "Alpha", vec![], vec!["a"], vec!["Gamma"]),
            create_system(2, "Beta", vec!["a"], vec![], vec!["Alpha"]),
            create_system(3, "Gamma", vec![], vec![], vec!["Beta"]),
        ];

        match schedule_systems(&systems) {
            Err(EcsError::CycleDetectedBetweenSystems(path)) => {
                assert_eq!(path.len(), 4, "closed walk over three systems: {path:?}");
                assert_eq!(path.first(), path.last());
            }
            other => panic!("expected CycleDetectedBetweenSystems, got {other:?}"),
        }
    }

    /// Randomized check of the scheduler guarantees on small systems with consistent `run_after`
    /// edges: conflicting systems never share a layer, every `run_after` edge is honored, and the
    /// schedule does not depend on declaration order.
    #[test]
    fn randomized_schedules_uphold_guarantees() {
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut rnd = move |m: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % m as u64) as usize
        };
        let components = ["a", "b", "c", "d"];
        let names = ["Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot"];

        for _ in 0..2_000 {
            let n = 2 + rnd(5);
            // Index `i` may only run after indices `< i`, which keeps `run_after` acyclic. Names
            // are shuffled so that this does not coincide with name order.
            let mut name_of: Vec<&str> = names[..n].to_vec();
            for k in (1..n).rev() {
                name_of.swap(k, rnd(k + 1));
            }
            let mut specs = vec![];
            for i in 0..n {
                let (mut inputs, mut outputs) = (vec![], vec![]);
                for c in components {
                    match rnd(4) {
                        0 => inputs.push(c),
                        1 => outputs.push(c),
                        _ => {}
                    }
                }
                let run_after: Vec<&str> =
                    (0..i).filter(|_| rnd(5) == 0).map(|j| name_of[j]).collect();
                specs.push((name_of[i], inputs, outputs, run_after));
            }
            let build = |order: &[usize]| -> Vec<System> {
                order
                    .iter()
                    .enumerate()
                    .map(|(id, &k)| {
                        let (name, inputs, outputs, run_after) = &specs[k];
                        create_system(
                            id as u64,
                            name,
                            inputs.clone(),
                            outputs.clone(),
                            run_after.clone(),
                        )
                    })
                    .collect()
            };

            let order: Vec<usize> = (0..n).collect();
            let mut shuffled = order.clone();
            for k in (1..n).rev() {
                shuffled.swap(k, rnd(k + 1));
            }

            let systems = build(&order);
            let layers = schedule_systems(&systems).unwrap();
            assert_no_conflict_within_layers(&systems, &layers);

            let layer_of = |id: SystemId| layers.iter().position(|l| l.contains(&id)).unwrap();
            for sys in &systems {
                for pred in &sys.run_after {
                    let pred = systems.iter().find(|s| &s.name == pred).unwrap();
                    assert!(
                        layer_of(pred.id) < layer_of(sys.id),
                        "{} must run after {}: {:?}",
                        sys.name.type_name_raw,
                        pred.name.type_name_raw,
                        layer_names(&systems, &layers)
                    );
                }
            }

            let shuffled_systems = build(&shuffled);
            let shuffled_layers = schedule_systems(&shuffled_systems).unwrap();
            assert_eq!(
                layer_names(&systems, &layers),
                layer_names(&shuffled_systems, &shuffled_layers),
            );
        }
    }
}
