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

/// Issue #4: an archetype component view defines a fixed subset of components that may be
/// shared across multiple archetypes. The world template must emit per-view struct and
/// accessor pairs so that a single archetype match can return all requested components by
/// index instead of relying on N separate entity-location lookups.
#[test]
fn view_emits_structs_and_accessors() {
    const YAML: &str = r#"
components:
  - name: Position
  - name: Velocity
  - name: Sprite
archetypes:
  - name: Particle
    components: [Position, Velocity]
  - name: Decoration
    components: [Position, Sprite]
views:
  - name: Movable
    components: [Position, Velocity]
worlds:
  - name: Main
    archetypes: [Particle, Decoration]
phases:
  - name: Update
systems:
  - name: Move
    phase: Update
    inputs: [Velocity]
    outputs: [Position]
"#;

    let code = EcsCode::generate(BufReader::new(YAML.as_bytes())).expect("Failed to build ECS");

    assert!(
        code.world.contains("pub struct MovableView<'archetype>"),
        "MovableView struct missing from generated world output"
    );
    assert!(
        code.world.contains("pub struct MovableViewMut<'archetype>"),
        "MovableViewMut struct missing from generated world output"
    );
    assert!(
        code.world.contains("pub trait ViewAccess"),
        "ViewAccess trait missing from generated world output"
    );
    assert!(
        code.world.contains("pub trait ViewAccessMut: ViewAccess"),
        "ViewAccessMut trait missing from generated world output"
    );
    assert!(
        code.world.contains("fn get_movable_view("),
        "get_movable_view accessor missing from generated world output"
    );
    assert!(
        code.world.contains("fn get_movable_view_mut("),
        "get_movable_view_mut accessor missing from generated world output"
    );
    // Only Particle (Position + Velocity) satisfies the Movable view; Decoration must be excluded.
    let body_start = code
        .world
        .find("fn get_movable_view(")
        .expect("get_movable_view emitted");
    let body = &code.world[body_start..body_start.saturating_add(2000)];
    assert!(
        body.contains("ParticleArchetype::ID"),
        "Movable view must dispatch on the Particle archetype"
    );
    assert!(
        !body.contains("DecorationArchetype::ID"),
        "Movable view must not dispatch on the Decoration archetype (missing Velocity)"
    );
}

/// A view referencing an undefined component must be rejected at validation time so users get a
/// clear error instead of a cryptic codegen failure later.
#[test]
fn view_with_unknown_component_is_rejected() {
    const YAML: &str = r#"
components:
  - name: Position
archetypes:
  - name: Particle
    components: [Position]
views:
  - name: Bogus
    components: [Velocity]
worlds:
  - name: Main
    archetypes: [Particle]
phases:
  - name: Update
systems:
  - name: Tick
    phase: Update
    outputs: [Position]
"#;

    let err = match EcsCode::generate(BufReader::new(YAML.as_bytes())) {
        Ok(_) => panic!("view referencing undefined component must fail"),
        Err(e) => e,
    };
    match err {
        EcsError::MissingComponentInView(component, view) => {
            assert_eq!(component, "VelocityComponent");
            assert_eq!(view, "Bogus");
        }
        other => panic!("expected MissingComponentInView, got {other:?}"),
    }
}

/// A view whose component set is not satisfied by any archetype is a configuration mistake; the
/// resulting accessor would only ever return `None`. Reject it at build time instead.
#[test]
fn view_without_matching_archetype_is_rejected() {
    const YAML: &str = r#"
components:
  - name: Position
  - name: Velocity
archetypes:
  - name: Static
    components: [Position]
views:
  - name: Movable
    components: [Position, Velocity]
worlds:
  - name: Main
    archetypes: [Static]
phases:
  - name: Update
systems:
  - name: Tick
    phase: Update
    outputs: [Position]
"#;

    let err = match EcsCode::generate(BufReader::new(YAML.as_bytes())) {
        Ok(_) => panic!("view with no matching archetype must fail"),
        Err(e) => e,
    };
    match err {
        EcsError::NoMatchingArchetypeForView(name) => assert_eq!(name, "Movable"),
        other => panic!("expected NoMatchingArchetypeForView, got {other:?}"),
    }
}

/// Views that match archetypes outside the world's own archetype list must be filtered out per
/// world; otherwise the generated `ViewAccess` impl on `<W>Archetypes` would try to dispatch on
/// archetype IDs the world does not own.
#[test]
fn view_accessor_only_emits_for_world_archetypes() {
    const YAML: &str = r#"
components:
  - name: Position
  - name: Velocity
archetypes:
  - name: Particle
    components: [Position, Velocity]
  - name: Static
    components: [Position]
views:
  - name: Movable
    components: [Position, Velocity]
worlds:
  - name: Main
    archetypes: [Static]
phases:
  - name: Update
systems:
  - name: Tick
    phase: Update
    outputs: [Position]
"#;

    let code = EcsCode::generate(BufReader::new(YAML.as_bytes())).expect("Failed to build ECS");

    // Top-level view struct still emits because Movable matches Particle at the ECS level.
    assert!(
        code.world.contains("pub struct MovableView<'archetype>"),
        "MovableView struct must still emit at the ECS level"
    );
    // The MainWorld owns only Static, which does not satisfy Movable; no per-world accessor impl
    // should be emitted. The trait itself still carries default `fn get_movable_view(...)` methods,
    // so check specifically for the impl block on the world's archetype storage.
    assert!(
        !code
            .world
            .contains("impl ViewAccess for MainWorldArchetypes"),
        "MainWorld must not emit a ViewAccess impl because none of its archetypes satisfy any view"
    );
    assert!(
        !code
            .world
            .contains("impl ViewAccessMut for MainWorldArchetypes"),
        "MainWorld must not emit a ViewAccessMut impl because none of its archetypes satisfy any view"
    );
}

/// A multi-line YAML description on a view must not leak un-commented continuation lines into
/// generated Rust. The doc-comment line in the template emits a single `///` prefix; the
/// `doc_lines` filter has to re-prefix every embedded newline so the output stays syntactically
/// valid as a doc comment block.
#[test]
fn view_description_multiline_renders_as_doc_lines() {
    const YAML: &str = "
components:
  - name: Position
  - name: Velocity
archetypes:
  - name: Particle
    components: [Position, Velocity]
views:
  - name: Movable
    description: |
      First line of the description.
      Second line that would otherwise break the doc comment.
    components: [Position, Velocity]
worlds:
  - name: Main
    archetypes: [Particle]
phases:
  - name: Update
systems:
  - name: Tick
    phase: Update
    outputs: [Position]
";

    let code = EcsCode::generate(BufReader::new(YAML.as_bytes())).expect("Failed to build ECS");

    let struct_start = code
        .world
        .find("pub struct MovableView<'archetype>")
        .expect("MovableView struct missing");
    let preceding = &code.world[..struct_start];
    let doc_block_start = preceding
        .rfind("/// A read-only view of an entity")
        .expect("MovableView doc block missing");
    let doc_block = &preceding[doc_block_start..];

    assert!(
        doc_block.contains("/// First line of the description."),
        "first description line should be emitted with `///` prefix"
    );
    assert!(
        doc_block.contains("/// Second line that would otherwise break the doc comment."),
        "second description line must be re-prefixed with `///` instead of leaking as bare Rust"
    );

    for line in doc_block.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        assert!(
            trimmed.starts_with("///") || trimmed.starts_with("#[") || trimmed.starts_with("pub "),
            "doc block above MovableView contains non-comment line: {line:?}"
        );
    }
}

/// Issue #2: a system with `iteration: dirty` triggers dirty-tracking codegen for every
/// archetype it touches. The archetype emits a `dirty_indices` field plus `mark_dirty` /
/// `clear_dirty` / `collect_dirty_sorted` helpers; the system trait gains `apply_many_dirty`
/// (default forwards each dirty index to `apply_single`) and `apply_all_dirty`; the world
/// emits `mark_<archetype>_dirty*` helpers and clears the dirty set at the end of every
/// phase that contains a dirty-iteration system.
#[test]
fn dirty_iteration_system_emits_full_codegen() {
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
    iteration: dirty
    inputs: [Velocity]
    outputs: [Position]
"#;

    let code = EcsCode::generate(BufReader::new(YAML.as_bytes())).expect("Failed to build ECS");

    // Archetype gains dirty storage + helpers.
    assert!(
        code.archetypes
            .contains("pub dirty_indices: ::std::collections::HashSet<usize>"),
        "ParticleArchetype must declare a dirty_indices HashSet"
    );
    assert!(
        code.archetypes.contains("pub fn mark_dirty("),
        "ParticleArchetype must expose mark_dirty"
    );
    assert!(
        code.archetypes.contains("pub fn clear_dirty("),
        "ParticleArchetype must expose clear_dirty"
    );
    assert!(
        code.archetypes.contains("pub fn collect_dirty_sorted("),
        "ParticleArchetype must expose collect_dirty_sorted for world dispatch"
    );

    // System trait + newtype gain dirty entry points.
    assert!(
        code.systems.contains("fn apply_many_dirty("),
        "ApplyMoveSystem trait must declare apply_many_dirty"
    );
    assert!(
        code.systems.contains("fn apply_all_dirty("),
        "Dirty systems must emit apply_all_dirty"
    );
    assert!(
        code.systems.contains("for &idx in dirty {"),
        "Default apply_many_dirty must iterate the dirty index slice"
    );

    // World dispatches via apply_all_dirty + clears at end of phase + offers mark helpers.
    assert!(
        code.world.contains(".apply_all_dirty("),
        "World must dispatch dirty-iteration systems via apply_all_dirty"
    );
    assert!(
        code.world
            .contains("self.archetypes.collection.particle.clear_dirty();"),
        "World must clear the dirty set of every touched archetype at end of phase"
    );
    assert!(
        code.world.contains("pub fn mark_particle_dirty("),
        "World must emit mark_<archetype>_dirty helper for dirty archetypes"
    );
    assert!(
        code.world.contains("pub fn mark_particle_dirty_by_id("),
        "World must emit mark_<archetype>_dirty_by_id helper for dirty archetypes"
    );
    assert!(
        code.world.contains("pub fn clear_particles_dirty("),
        "World must emit clear_<archetypes>_dirty helper for dirty archetypes"
    );
}

/// An archetype not referenced by any dirty system must not pay the dirty-tracking codegen
/// tax (no `dirty_indices` field, no `mark_dirty` method, no clear at end of phase).
#[test]
fn non_dirty_archetype_emits_no_dirty_codegen() {
    const YAML: &str = r#"
components:
  - name: Position
archetypes:
  - name: Static
    components: [Position]
worlds:
  - name: Main
    archetypes: [Static]
phases:
  - name: Update
systems:
  - name: Tick
    phase: Update
    outputs: [Position]
"#;

    let code = EcsCode::generate(BufReader::new(YAML.as_bytes())).expect("Failed to build ECS");

    assert!(
        !code.archetypes.contains("dirty_indices"),
        "Non-dirty archetypes must not carry dirty_indices storage"
    );
    assert!(
        !code.systems.contains("apply_many_dirty"),
        "Non-dirty systems must not emit apply_many_dirty"
    );
    assert!(
        !code.world.contains("apply_all_dirty"),
        "World must not call apply_all_dirty when no system opts in"
    );
    assert!(
        !code.world.contains("clear_dirty"),
        "World must not emit clear_dirty calls when no dirty system runs"
    );
    assert!(
        !code.world.contains("mark_static_dirty"),
        "World must not emit mark_<archetype>_dirty for non-dirty archetypes"
    );
}

/// An explicit `dirty: true` on an archetype is honored even if no system uses dirty
/// iteration. This lets users mark archetypes dirty-aware ahead of wiring up the systems.
#[test]
fn explicit_archetype_dirty_flag_is_honored() {
    const YAML: &str = r#"
components:
  - name: Position
archetypes:
  - name: Particle
    components: [Position]
    dirty: true
worlds:
  - name: Main
    archetypes: [Particle]
phases:
  - name: Update
systems:
  - name: Tick
    phase: Update
    outputs: [Position]
"#;

    let code = EcsCode::generate(BufReader::new(YAML.as_bytes())).expect("Failed to build ECS");

    assert!(
        code.archetypes
            .contains("pub dirty_indices: ::std::collections::HashSet<usize>"),
        "Explicit dirty: true must emit dirty_indices storage even without a dirty system"
    );
    assert!(
        code.world.contains("pub fn mark_particle_dirty("),
        "Explicit dirty: true must still emit world-side helpers"
    );
    // No system opted into dirty iteration, so the world must not emit the end-of-phase
    // clear block (the per-archetype helper that *the user* may call still exists).
    assert!(
        !code.world.contains(
            "// Clear the dirty sets of archetypes touched by any dirty-iteration system"
        ),
        "Without a dirty system, the world must not auto-clear the archetype's dirty set at end of phase"
    );
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
