use sillyecs_build::EcsCode;
use std::io::BufReader;

#[test]
fn it_works() {
    let file = include_str!("ecs.yaml");
    let reader = BufReader::new(file.as_bytes());
    EcsCode::generate(reader).expect("Failed to build ECS");
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
