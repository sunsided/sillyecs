# sillyecs

A silly little compile-time generated archetype ECS in Rust.

[![Crates.io](https://img.shields.io/crates/v/sillyecs)](https://crates.io/crates/sillyecs)
[![License](https://img.shields.io/badge/license-EUPL--1.2-blue.svg)](LICENSE)

## Installation

`sillyecs` is a build-time dependency. To use it, add this to your `Cargo.toml`:

```toml
[build-dependencies]
sillyecs = "0.0.2"
```

## Usage

Use `sillyecs` in your `build.rs`:

```rust
use sillyecs::EcsCode;
use std::fs::File;
use std::io::BufReader;

fn main() -> eyre::Result<()> {
    println!("cargo:rerun-if-changed=ecs.yaml");

    let file = File::open("ecs.yaml").expect("Failed to open ecs.yaml");
    let reader = BufReader::new(file);
    EcsCode::generate(reader)?.write_files()?;
    Ok(())
}
```

Define your ECS components and systems in a YAML file:

```yaml
# ecs.yaml
components:
  - name: Position
  - name: Velocity
  - name: Health
  - name: Collider

archetypes:
  - name: Particle
    description: A particle system particle.
    components:
      - Position
      - Velocity

  - name: Player
    components:
      - Position
      - Velocity
      - Health

  - name: ForegroundObject
    components:
      - Position
      - Collider
    promotions:
      - BackgroundObject

  - name: BackgroundObject
    components:
      - Position
    promotions:
      - ForegroundObject

phases:
  - name: Startup
  - name: FixedUpdate
  - name: Update
  - name: Render

systems:
  - name: Physics
    phase: FixedUpdate
    inputs:
      - Velocity
    outputs:
      - Position
  - name: Render
    phase: Render
    inputs:
      - Position
```

Include the compile-time generated files:

```rust
include!(concat!(env!("OUT_DIR"), "/components.gen.rs"));
include!(concat!(env!("OUT_DIR"), "/archetypes.gen.rs"));
include!(concat!(env!("OUT_DIR"), "/systems.gen.rs"));
include!(concat!(env!("OUT_DIR"), "/world.gen.rs"));
```

The compiler will tell you which traits and functions to implement.
