# sillyecs

A zero-overhead, compile-time-scheduled archetype ECS in Rust.
Started as a silly little ECS implementation, strives to be more.

[![Crates.io](https://img.shields.io/crates/v/sillyecs)](https://crates.io/crates/sillyecs)
[![License](https://img.shields.io/badge/license-EUPL--1.2-blue.svg)](LICENSE)

## Table of Contents

<!-- START doctoc generated TOC please keep comment here to allow auto update -->
<!-- DON'T EDIT THIS SECTION, INSTEAD RE-RUN doctoc TO UPDATE -->

- [Features](#features)
- [Installation](#installation)
- [Usage](#usage)
  - [Stateless Systems](#stateless-systems)
  - [Archetype Component Views](#archetype-component-views)
  - [Command Queue and Application-Specific Commands](#command-queue-and-application-specific-commands)
- [Examples](#examples)
  - [WGPU Shader Compilation](#wgpu-shader-compilation)

<!-- END doctoc generated TOC please keep comment here to allow auto update -->

## Features

`sillyecs` generates a fully monomorphized ECS at build time from a YAML schema. There is no
type-erased component storage, no dynamic dispatch in the hot path, and no runtime registration.
The runtime crate is intentionally tiny; nearly all behavior lives in the code that is generated
into your project.

- **Compile-time codegen.** A YAML schema describes components, archetypes, systems, phases,
  worlds, and user states. `sillyecs-build` validates it (duplicate components, undefined
  references, cyclic system dependencies, archetype coverage, …) and emits strongly-typed Rust
  via Jinja2 templates.
- **Archetype storage in Struct-of-Arrays layout.** Each archetype keeps a `Vec<EntityId>` and one
  `Vec<C>` per component, so systems iterate over contiguous component slices and stay
  cache-friendly.
- **Automatic system scheduling.** Per phase, the build crate analyzes each system's component
  reads/writes, user-state reads/writes, frame-context use, and explicit `run_after` edges, and
  emits groups of systems that can run in parallel (Kahn's algorithm). Every `run_after` edge is
  honored, and two systems that conflict on a resource (at least one of them writes it) never
  share a group. A writer runs before its readers unless a `run_after` chain says otherwise; when
  data dependencies form a cycle, the edge out of the alphabetically-greatest writer gives way:
  its reader runs first and sees the previous tick's data, and the build emits a
  `cargo:warning`. Two writers of the same
  resource run in alphabetical name order. Systems with `lookup` count as reading every
  component. Within-group order is also sorted by name, so scheduling is independent of the order
  in which systems appear in YAML; only renaming a system affects it. Contradictory `run_after`
  edges (a cycle) and cross-phase `run_after` edges are rejected at build time, the former with
  the full offending path.
- **Sequential and Rayon-parallel execution paths.** Every phase gets both
  `apply_system_phase_X()` and `par_apply_system_phase_X()` variants.
- **Rich phase lifecycle.** Each system exposes `is_ready` → `on_begin_phase` → optional
  `preflight` → `apply_single`/`apply_many`/`apply_all` → optional `postflight` → `on_end_phase`,
  with phase-level `on_begin_phase`/`on_end_phase` events on top.
- **Fine-grained state access.** User states declared with `use: …` can be configured per
  lifecycle hook (`check`, `begin_phase`, `preflight`, `system`, `postflight`, `end_phase`) as
  `none`/`read`/`write`. Generated signatures match exactly; the scheduler accounts for state
  conflicts the same way it accounts for component conflicts.
- **Flexible phase types.** Phases can be `manual` (caller drives them), `on_request` (atomic
  request flag, swap-on-read), or fixed-step with an accumulator loop (`60 Hz` / `0.016 s`
  syntax).
- **Deferred world commands.** Spawn, despawn, and user-defined commands flow through a
  pluggable `WorldCommandSender`/`WorldCommandReceiver`. Commands are drained before and after
  each phase, not between systems.
- **`NonZeroU64` IDs.** `ArchetypeId`, `SystemId`, `WorldId`, and `EntityId` are niche-optimized
  enums with `const` value tables and `Display` impls.
- **Cross-archetype component iteration.** For every component, generated traits
  (`IterXComponents`, `IterMutXComponents`, `IterXEntities`) yield flat iterators over every
  archetype that carries it.
- **Archetype component views.** A `views:` block names a fixed subset of components shared
  across multiple archetypes. The build crate auto-resolves which archetypes qualify (any whose
  component set is a superset of the view). Generated `ViewAccess` / `ViewAccessMut` traits expose
  `get_<view>_view(EntityId)` and `get_<view>_view_mut(EntityId)`: one entity-location lookup,
  one archetype match, all view components by index - no further map lookups.
- **Entity frontloading.** Archetypes support `frontload`/`frontload_by_indices_sorted`/
  `frontload_scan` to partition entities (e.g. for quadtree-driven culling) without breaking the
  archetype invariants.
- **Pluggable `EntityLocationMap`.** The entity → archetype/index lookup is left as a type alias
  so callers can drop in `fxhash`, `ahash`, or anything else.
- **Optional unchecked accessors.** Setting `allow_unsafe: true` enables `get_*_unchecked`
  variants for hot loops; the default safe paths remain available.

## Installation

`sillyecs` is a mainly build-time dependency (`sillyecs-build`) that comes with a utility crate (`sillyecs`).
To use them, add this to your `Cargo.toml`:

```toml
[dependencies]
sillyecs = "0.1.0"

[build-dependencies]
sillyecs-build = "0.1.0"
```

The generated code is `include!`d into your crate and refers to `::sillyecs` by path, so `sillyecs`
must be a direct dependency of the crate that includes it. Besides the runtime types
(`::sillyecs::EntityId`, `::sillyecs::FrameContext`, …), it re-exports `rayon` (for the parallel
phase runners, `par_apply_system_phases` and friends) and `tracing` (for logging in command handling
and phase hooks), so neither needs to be added separately. Install a `tracing` subscriber in your
application to see the generated code's log output.

For browser builds (`wasm32-unknown-unknown`), enable the `wasm` feature. Frame timing uses
`sillyecs::Instant`, which is `std::time::Instant` by default; `std::time::Instant::now()` panics in the
browser, so the feature swaps in [`web-time`](https://crates.io/crates/web-time). It also enables Rayon's
`web_spin_lock`, which is needed when running multithreaded via
[`wasm-bindgen-rayon`](https://github.com/RReverser/wasm-bindgen-rayon). Without that setup, Rayon
runs the parallel phase runners on the current thread.

```toml
[dependencies]
sillyecs = { version = "0.1.0", features = ["wasm"] }
```

## Usage

Use `sillyecs` in your `build.rs`:

```rust
use sillyecs_build::EcsCode;
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
states:
  - name: WgpuRender
    description: The WGPU render state; will be initialized in the Render phase hooks

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
    promotions:         # deprecated: bypasses world entity-location map; despawn+respawn instead
      - BackgroundObject

  - name: BackgroundObject
    components:
      - Position
    promotions:         # deprecated: see above
      - ForegroundObject

views:
  - name: Movable        # auto-resolves to Particle and Player (both have Position + Velocity)
    components:
      - Position
      - Velocity

phases:
  - name: Startup
  - name: FixedUpdate
    fixed: 60 Hz  # or "0.01666 s"
  - name: Update
  - name: Render
    manual: true        # Require manual call to world.apply_system_phase_render()
    # on_request: true  # call world.request_render_phase() to allow execution (resets atomically)
    states:
      - use: WgpuRender    # Use state in phase begin/end hooks
        begin_phase: write # optional: none|read|write, defaults to read
        end_phase: write   # optional: none|read|write

systems:
  - name: Physics
    phase: FixedUpdate
    context: true
    run_after: [ ]    # optional
    preflight: true  # optional, extra scan before system run
    postflight: true # optional, extra scan after system run
    lookup: # optional
      - Position     # read Position of other entities by ID in pre- or postflight
    inputs:
      - Velocity
    outputs:
      - Position

  - name: Render
    phase: Render
    states:
      - use: WgpuRender
        default: none      # The default for the below values; optional: none|read|write, defaults to read
        check: read        # optional: none|read|write
        begin_phase: none  # optional: none|read|write
        system: write      # optional: none|read|write
        end_phase: none    # optional: none|read|write
    inputs:
      - Position

  - name: Spin
    phase: Update
    stateless: true    # optional: generate the system data and factory impl
    outputs:
      - Velocity

worlds:
  - name: Main
    archetypes:
      - Particle
      - Player
      - ForegroundObject
      - BackgroundObject

# Optional, if you're feeling lucky
allow_unsafe: true
```

Include the compile-time generated files:

```rust
include!(concat!(env!("OUT_DIR"), "/components_gen.rs"));
include!(concat!(env!("OUT_DIR"), "/archetypes_gen.rs"));
include!(concat!(env!("OUT_DIR"), "/systems_gen.rs"));
include!(concat!(env!("OUT_DIR"), "/world_gen.rs"));
```

The compiler will tell you which traits and functions to implement.

### Stateless Systems

Every system wraps a `<Name>SystemData` struct, and `World::new` takes a factory implementing
`CreateSystem<<Name>System>` for each system. For systems that keep no state of their own, set
`stateless: true` and the generator emits both: a unit `SpinSystemData` struct and a blanket
`CreateSystem<SpinSystem>` impl for every type. Your factory then only covers the stateful systems:

```rust
pub struct SystemFactory;

impl CreateSystem<PhysicsSystem> for SystemFactory {
    fn create(&self) -> PhysicsSystem {
        PhysicsSystem::from(PhysicsSystemData::default())
    }
}

// ... likewise for `RenderSystem`.
// `Spin` needs no factory impl; writing one is a conflicting-impl error.
let world = MainWorld::new(&SystemFactory, states, queue);
```

If every system of a world is stateless, any value works as the factory, e.g. `MainWorld::new(&(), states, queue)`.

### Archetype Component Views

A view names a fixed component subset shared by multiple archetypes. With the `Movable` view from
the example above, the world exposes a single-lookup accessor:

```rust
// One entity-location lookup, one archetype match, both components by index.
if let Some(view) = world.get_movable_view(entity_id) {
    println!("pos={:?} vel={:?}", view.position, view.velocity);
}

// Mutable access:
if let Some(mut view) = world.get_movable_view_mut(entity_id) {
    view.velocity.x *= 0.9;
}
```

Compare to the pre-views approach, which required two separate map lookups:

```rust
// Before views: two lookups, two archetype matches.
let pos = world.get_position_component(entity_id);
let vel = world.get_velocity_component(entity_id);
```

### Command Queue and Application-Specific Commands

You will have to implement a command queue. Below is an example for a queue based on
[`crossbeam-channel`](https://docs.rs/crossbeam/latest/crossbeam/channel):

```rust
struct CommandQueue<UserCommand> {
    sender: crossbeam_channel::Sender<WorldCommand<UserCommand>>,
    receiver: crossbeam_channel::Receiver<WorldCommand<UserCommand>>,
}

impl<UserCommand> CommandQueue<UserCommand> {
    pub fn new() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        Self {
            sender,
            receiver,
        }
    }
}

impl<UserCommand> WorldUserCommand for CommandQueue<UserCommand>
where
    UserCommand: Send + Debug
{
    type UserCommand = UserCommand;
}

impl<UserCommand> WorldCommandReceiver for CommandQueue<UserCommand>
where
    UserCommand: Send + Debug
{
    type Error = TryRecvError;

    fn recv(&self) -> Result<Option<WorldCommand>, Self::Error> {
        match self.receiver.try_recv() {
            Ok(cmd) => Ok(Some(cmd)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<UserCommand> WorldCommandSender for CommandQueue<UserCommand>
where
    UserCommand: Send + Debug
{
    type Error = crossbeam_channel::SendError<WorldCommand>;

    fn send(&self, command: WorldCommand) -> Result<(), Self::Error> {
        self.sender.send(command)
    }
}
```

For each world, the `WorldUserCommandHandler` trait must be implemented:

```rust

impl<E, Q> WorldUserCommandHandler for MainWorld<E, Q>
where
    Q: WorldUserCommand
{
    fn handle_user_command(&mut self, command: Self::UserCommand) {
        warn!(?command, "Unhandled user command");
    }
}
```

## Examples

### WGPU Shader Compilation

Define the `WgpuRender` state, the `WgpuShader` component, a `WgpuShader` archetype that holds it, a `WgpuReinit`
system phase and a `WgpuInitShader` system that uses the state to update the component:

```yaml

allow_unsafe: true

states:
  - name: WgpuRender
    description: The WGPU render state (e.g. device, queue, ...)

components:
  - name: WgpuShader

worlds:
  - name: Main
    archetypes:
      - WgpuShader

archetypes:
  - name: WgpuShader
    components:
      - WgpuShader

phases:
  - name: WgpuReinit
    manual: true

systems:
  - name: WgpuInitShader
    phase: WgpuReinit
    states:
      - use: WgpuRender
        default: none
        check: read
        system: write
    outputs:
      - WgpuShader
```

Implement `WgpuShaderData` to hold shader definitions and the handle:

```rust
use wgpu::{ShaderModule, ShaderModuleDescriptor, ShaderSource};

#[derive(Debug, Clone)]
pub struct WgpuShaderData {
    pub descriptor: ShaderModuleDescriptor<'static>,
    pub module: Option<ShaderModule>
}

impl WgpuShaderData {
    pub const fn new(descriptor: ShaderModuleDescriptor<'static>) -> Self {
        Self { descriptor, module: None }
    }
}
```

Implement the `WgpuInitShaderSystem` to compile and upload the shader:

```rust
use std::convert::Infallible;
use wgpu_resource_manager::DeviceId;
use crate::engine::{ApplyWgpuInitShaderSystem, CreateSystem, PhaseEvents, SystemFactory, SystemWgpuReinitPhaseEvents, WgpuInitShaderSystem, WgpuShaderComponent};
use crate::engine::phases::render::WgpuRenderState;

#[derive(Debug, Default)]
pub struct WgpuInitShaderSystemData {
    device_id: DeviceId
}

impl CreateSystem<WgpuInitShaderSystem> for SystemFactory {
    fn create(&self) -> WgpuInitShaderSystem {
        WgpuInitShaderSystem(WgpuInitShaderSystemData::default())
    }
}

impl ApplyWgpuInitShaderSystem for WgpuInitShaderSystem {
    type Error = Infallible;

    fn is_ready(&self, gpu: &WgpuRenderState) -> bool {
        gpu.is_ready()
    }

    fn apply_many(&mut self, gpu: &mut WgpuRenderState, shaders: &mut [WgpuShaderComponent]) {
        let Ok(device) = gpu.device() else {
            return;
        };

        let device_changed = device.id() != self.device_id;
        self.device_id = device.id();

        for shader in shaders {
            // Skip over all already initialized shaders.
            if shader.module.is_some() && !device_changed {
                continue;
            }

            let module = device.device().create_shader_module(shader.descriptor.clone());
            shader.module = Some(module);
        }
    }
}
```

In your world, you can now instantiate shaders and get them back:

```rust
fn register_example_shader<E, Q>(world: &MainWorld<E, Q>) -> WgpuShaderEntityRef {
    let entity_id = world.spawn_wgpu_shader(WgpuShaderEntityData {
        wgpu_shader: WgpuShaderData::new(include_wgsl!("shader.wgsl")),
    });

    // Get it back
    self.get_wgpu_shader_entity(entity_id).unwrap()
}
```

Since the phase is marked manual, it has to be called explicitly:

```rust
fn initialize_gpu_resources<E, Q>(world: &MainWorld<E, Q>) {
    world.apply_system_phase_wgpu_reinit();
}
```
