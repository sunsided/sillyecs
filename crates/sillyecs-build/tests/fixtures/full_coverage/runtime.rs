// Runtime tests for the `full_coverage` fixture. Compiled as a `#[cfg(test)]`
// module of the synthetic library crate and run with `cargo test` by
// `tests/compile_generated.rs`, so they exercise the generated code's behavior,
// not just that it compiles.

use super::*;

fn world() -> MainWorld<NoOpPhaseEvents, CommandQueue> {
    MainWorld::new(&SystemFactory, MainWorldStates::default(), CommandQueue::new())
}

fn spawn_particle(world: &mut MainWorld<NoOpPhaseEvents, CommandQueue>) -> ::sillyecs::EntityId {
    world.spawn_particle_with(
        PositionComponent::from(PositionData::default()),
        VelocityComponent::from(VelocityData::default()),
    )
}

/// Regression: dropping the last entity of an archetype that still holds others indexed past
/// the end of the shrunk entity list and panicked.
#[test]
fn despawning_the_last_entity_keeps_the_others() {
    let mut world = world();
    let first = spawn_particle(&mut world);
    let last = spawn_particle(&mut world);

    world.despawn_by_id(last).expect("despawn last entity");
    assert_eq!(world.len(), 1);

    world.despawn_by_id(first).expect("despawn remaining entity");
    assert!(world.is_empty());
}

/// Despawning from the middle moves the last entity into the hole; its location must follow so
/// it can still be despawned by ID.
#[test]
fn despawning_moves_the_last_entity_into_the_hole() {
    let mut world = world();
    let first = spawn_particle(&mut world);
    let middle = spawn_particle(&mut world);
    let last = spawn_particle(&mut world);

    world.despawn_by_id(first).expect("despawn first entity");
    world.despawn_by_id(last).expect("despawn moved entity");
    world.despawn_by_id(middle).expect("despawn remaining entity");
    assert!(world.is_empty());
}

/// Commands travel through the user's queue, which only implements the runtime
/// `CommandSender`/`CommandReceiver`, and are applied on the next phase run.
#[test]
fn queued_commands_are_applied_by_the_next_phase_run() {
    let mut world = world();
    let id = spawn_particle(&mut world);

    world
        .command(WorldCommand::DespawnEntity(id))
        .expect("queue despawn");
    world
        .command(WorldCommand::User(UserCommand::Spawn))
        .expect("queue user command");
    assert_eq!(world.len(), 1, "commands are deferred");

    world.apply_system_phases();
    assert!(world.is_empty());
}
