use crate::component::ComponentName;
use crate::system::{System, SystemId};
use std::collections::{HashMap, HashSet};

pub fn schedule_systems(systems: &[System]) -> Vec<&System> {
    let mut graph: HashMap<SystemId, Vec<SystemId>> = HashMap::new();
    let mut in_degree: HashMap<SystemId, usize> = HashMap::new();
    let mut systems_by_id: HashMap<SystemId, &System> = HashMap::new();

    let mut readers: HashMap<ComponentName, HashSet<SystemId>> = HashMap::new(); // component -> systems reading it
    let mut writers: HashMap<ComponentName, HashSet<SystemId>> = HashMap::new(); // component -> systems writing it

    // Index systems and gather readers/writers
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

    // Build graph edges based on write → read dependencies
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

    // Priority queue: systems with zero in-degree
    let mut ready: Vec<SystemId> = in_degree
        .iter()
        .filter_map(|(&id, &deg)| if deg == 0 { Some(id) } else { None })
        .collect();

    let mut scheduled = Vec::new();
    let mut visited = HashSet::new();

    while scheduled.len() < systems.len() {
        if ready.is_empty() {
            // Cycle detected: select lowest-order unscheduled system
            let mut remaining: Vec<_> = in_degree
                .keys()
                .filter(|&id| !visited.contains(id))
                .collect();
            remaining.sort_by_key(|id| systems_by_id[id].order);
            ready.push(*remaining[0]);
        }

        // Pick system with lowest order among ready
        ready.sort_by_key(|id| systems_by_id[id].order);
        let current = ready.remove(0);

        if visited.insert(current) {
            scheduled.push(systems_by_id[&current]);

            if let Some(dependents) = graph.get(&current) {
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
    }

    scheduled
}

#[cfg(test)]
mod tests {
    use crate::Name;
    use crate::component::ComponentName;
    use crate::system::{System, SystemId, SystemName, SystemPhaseRef};
    use super::*;

    fn sysname(name: &str) -> SystemName {
        SystemName(Name::new(name.to_string(), "System"))
    }

    fn compname(name: &str) -> ComponentName {
        ComponentName(Name::new(name.to_string(), "Component"))
    }

    fn phasename(name: &str) -> SystemPhaseRef {
        name.to_string()
    }

    #[test]
    fn it_works() {
        let systems = vec![
            System {
                id: SystemId(1),
                name: sysname("Producer"),
                order: 1,
                inputs: vec![],
                outputs: vec![compname("x")],
                phase: phasename("default"),
                affected_archetype_count: 0,
                affected_archetype_ids: Default::default(),
                affected_archetypes: Default::default(),
                component_iter_code: String::new(),
                component_untuple_code: String::new(),
                description: None
            },

            System {
                id: SystemId(3),
                name: sysname("Consumer"),
                order: 3,
                inputs: vec![compname("y")],
                outputs: vec![],
                phase: phasename("default"),
                affected_archetype_count: 0,
                affected_archetype_ids: Default::default(),
                affected_archetypes: Default::default(),
                component_iter_code: String::new(),
                component_untuple_code: String::new(),
                description: None
            },

            System {
                id: SystemId(2),
                name: sysname("Transformer"),
                order: 2,
                inputs: vec![compname("x")],
                outputs: vec![compname("y")],
                phase: phasename("default"),
                affected_archetype_count: 0,
                affected_archetype_ids: Default::default(),
                affected_archetypes: Default::default(),
                component_iter_code: String::new(),
                component_untuple_code: String::new(),
                description: None
            },


            System {
                id: SystemId(4),
                name: sysname("Backflow"),
                order: 4,
                inputs: vec![compname("y")],
                outputs: vec![compname("x")],
                phase: phasename("default"),
                affected_archetype_count: 0,
                affected_archetype_ids: Default::default(),
                affected_archetypes: Default::default(),
                component_iter_code: String::new(),
                component_untuple_code: String::new(),
                description: None
            }, // creates a cycle
        ];

        let sorted = schedule_systems(&systems);
        println!("Execution order:");
        for sys in sorted {
            println!("- {}", sys.name);
        }
    }
}
