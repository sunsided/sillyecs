use sillyecs_build::{EcsCode, EcsError};
use std::io::BufReader;

#[test]
fn it_works() {
    let file = include_str!("ecs.yaml");
    let reader = BufReader::new(file.as_bytes());
    EcsCode::generate(reader).expect("Failed to build ECS");
}

/// The `Update` phase in `ecs.yaml` has `on_request: true`, so the world template
/// must emit the conditional-phase helpers (`request_update_phase`,
/// `ConditionalPhaseFlags`, `set_update_requested`, `is_update_requested`) and
/// document them. Regression for issue #32: the doc strings on those helpers were
/// either missing or wrong (the struct doc said "Spawns an entity into the world.").
#[test]
fn on_request_phase_emits_documented_helpers() {
    let file = include_str!("ecs.yaml");
    let reader = BufReader::new(file.as_bytes());
    let code = EcsCode::generate(reader).expect("Failed to build ECS");

    assert!(code.world.contains("struct ConditionalPhaseFlags"));
    assert!(code.world.contains("fn request_update_phase"));
    assert!(code.world.contains("fn set_update_requested"));
    assert!(code.world.contains("fn is_update_requested"));

    // The `Spawn` impls for archetypes legitimately carry "Spawns an entity into the world."
    // The previous ConditionalPhaseFlags doc was a copy of that, immediately above
    // `struct ConditionalPhaseFlags`. Check the new struct doc replaced it there.
    let flags_block_idx = code
        .world
        .find("struct ConditionalPhaseFlags")
        .expect("ConditionalPhaseFlags struct missing");
    let preceding = &code.world[..flags_block_idx];
    let doc_start = preceding
        .rfind("///")
        .expect("ConditionalPhaseFlags struct has no doc comment");
    assert!(
        !preceding[doc_start..].contains("Spawns an entity into the world."),
        "stale ConditionalPhaseFlags doc comment leaked into generated output"
    );
    assert!(
        code.world.contains("Single-consumer request flags"),
        "ConditionalPhaseFlags doc block missing from generated output"
    );
    assert!(
        code.world.contains("Requests execution of"),
        "request_X_phase doc block missing from generated output"
    );
}

/// Regression for the codegen bug where a `SystemPhase` `states:` entry that omits any of the
/// per-lifecycle access hooks caused the templates to emit
/// `todo!("Invalid state use in ECS construction"),` in parameter lists, producing invalid Rust.
/// `SystemPhase::finish` now fills missing hooks from `default`, mirroring `System`.
#[test]
fn phase_state_with_partial_hooks_does_not_emit_invalid_rust() {
    const YAML: &str = r#"
states:
  - name: Renderer
components:
  - name: Position
archetypes:
  - name: Particle
    components: [Position]
worlds:
  - name: Main
    archetypes: [Particle]
phases:
  - name: Render
    manual: true
    states:
      - use: Renderer
        default: write   # all per-hook accesses default to write; none are spelled out
systems:
  - name: Move
    phase: Render
    outputs: [Position]
"#;

    let reader = BufReader::new(YAML.as_bytes());
    let code = EcsCode::generate(reader).expect("Failed to build ECS");

    for (name, snippet) in [
        ("components", &code.components),
        ("archetypes", &code.archetypes),
        ("systems", &code.systems),
        ("world", &code.world),
    ] {
        assert!(
            !snippet.contains("Invalid state use in ECS construction"),
            "{name} output contained the unreachable `todo!` arm, which means a phase-state \
             hook was left unset at codegen time"
        );
    }
}

/// Regression for issue #25: previously, `ArchetypeId`, `SystemId`, `WorldId`, and `ComponentId`
/// were assigned by process-wide `AtomicU64` counters in the build crate, which made successive
/// `EcsCode::generate` calls in the same process emit *different* numeric IDs for the same input
/// YAML. IDs are now assigned per-`Ecs` from the deserialization order, so two generations from
/// the same YAML must produce byte-identical output.
#[test]
fn ids_are_deterministic_across_generate_calls() {
    const YAML: &str = r#"
components:
  - name: Position
  - name: Velocity
archetypes:
  - name: Particle
    components: [Position, Velocity]
  - name: Static
    components: [Position]
worlds:
  - name: Main
    archetypes: [Particle, Static]
  - name: Secondary
    archetypes: [Particle]
phases:
  - name: Update
systems:
  - name: Move
    phase: Update
    inputs: [Velocity]
    outputs: [Position]
  - name: Settle
    phase: Update
    inputs: [Position]
"#;

    let first = EcsCode::generate(BufReader::new(YAML.as_bytes())).expect("first generate");
    let second = EcsCode::generate(BufReader::new(YAML.as_bytes())).expect("second generate");

    assert_eq!(
        first.components, second.components,
        "component IDs / generated component module drifted between generate() calls"
    );
    assert_eq!(
        first.archetypes, second.archetypes,
        "archetype IDs / generated archetype module drifted between generate() calls"
    );
    assert_eq!(
        first.systems, second.systems,
        "system IDs / generated system module drifted between generate() calls"
    );
    assert_eq!(
        first.world, second.world,
        "world IDs / generated world module drifted between generate() calls"
    );
}

/// Regression for issue #27: per-tick `Box::new(&self.archetypes)` heap allocation was emitted in
/// preflight/postflight call sites of systems with `lookup:` entries. The trait method now takes
/// `&dyn XComponentLookup` directly and the call sites pass `&self.archetypes` without boxing.
#[test]
fn lookup_methods_take_bare_reference_not_box() {
    const YAML: &str = r#"
components:
  - name: Position
  - name: Velocity
archetypes:
  - name: Particle
    components: [Position, Velocity]
worlds:
  - name: Main
    archetypes: [Particle]
phases:
  - name: Update
systems:
  - name: Move
    phase: Update
    inputs: [Velocity]
    outputs: [Position]
    lookup: [Position]
    preflight: true
    postflight: true
"#;

    let reader = BufReader::new(YAML.as_bytes());
    let code = EcsCode::generate(reader).expect("Failed to build ECS");

    assert!(
        !code.systems.contains("Box<&dyn"),
        "trait method must not wrap lookup reference in Box"
    );
    assert!(
        !code.world.contains("Box::new(&self.archetypes)"),
        "preflight/postflight call sites must not allocate a Box around &self.archetypes"
    );
    assert!(
        code.systems.contains("&dyn MoveComponentLookup"),
        "trait method should accept &dyn MoveComponentLookup directly"
    );
    assert!(
        code.world.contains("&self.archetypes,"),
        "preflight/postflight call sites should pass &self.archetypes directly"
    );
}

/// Regression for issue #28: a `run_after` edge that points at a system in a different phase
/// used to pass validation silently and then be dropped by the per-phase scheduler. It must be
/// rejected at build time so the misconfiguration is visible to the user.
#[test]
fn cross_phase_run_after_is_rejected() {
    const YAML: &str = r#"
components:
  - name: Position
archetypes:
  - name: Particle
    components: [Position]
worlds:
  - name: Main
    archetypes: [Particle]
phases:
  - name: Update
  - name: Render
    manual: true
systems:
  - name: Tick
    phase: Update
    outputs: [Position]
  - name: Draw
    phase: Render
    run_after: [Tick]
    inputs: [Position]
"#;

    let reader = BufReader::new(YAML.as_bytes());
    let err = match EcsCode::generate(reader) {
        Ok(_) => panic!("cross-phase run_after must fail"),
        Err(e) => e,
    };
    match err {
        EcsError::CrossPhaseRunAfter {
            system,
            system_phase,
            dependency,
            dependency_phase,
        } => {
            assert_eq!(system, "DrawSystem");
            assert_eq!(system_phase, "Render");
            assert_eq!(dependency, "Tick");
            assert_eq!(dependency_phase, "Update");
        }
        other => panic!("expected CrossPhaseRunAfter, got {other:?}"),
    }
}

/// The scheduler's name-based tie-break is only total if system names are unique. Two systems
/// declared with the same name in YAML must therefore be rejected at validation time, not
/// silently collapsed by the internal `name -> phase` HashMap.
#[test]
fn duplicate_system_name_is_rejected() {
    const YAML: &str = r#"
components:
  - name: Position
archetypes:
  - name: Particle
    components: [Position]
worlds:
  - name: Main
    archetypes: [Particle]
phases:
  - name: Update
systems:
  - name: Tick
    phase: Update
    outputs: [Position]
  - name: Tick
    phase: Update
    inputs: [Position]
"#;

    let reader = BufReader::new(YAML.as_bytes());
    let err = match EcsCode::generate(reader) {
        Ok(_) => panic!("duplicate system name must fail"),
        Err(e) => e,
    };
    match err {
        EcsError::DuplicateSystem(name) => assert_eq!(name, "Tick"),
        other => panic!("expected DuplicateSystem, got {other:?}"),
    }
}
