# sillyecs

A silly little compile-time generated archetype ECS in Rust.

[![Crates.io](https://img.shields.io/crates/v/sillyecs)](https://crates.io/crates/sillyecs)
[![License](https://img.shields.io/badge/license-EUPL--1.2-blue.svg)](LICENSE)

## Table of Contents

<!-- START doctoc generated TOC please keep comment here to allow auto update -->
<!-- DON'T EDIT THIS SECTION, INSTEAD RE-RUN doctoc TO UPDATE -->

- [Installation](#installation)
- [Usage](#usage)
    - [Command Queue and Application-Specific Commands](#command-queue-and-application-specific-commands)
- [Examples](#examples)
    - [WGPU Shader Compilation](#wgpu-shader-compilation)

<!-- END doctoc generated TOC please keep comment here to allow auto update -->

## Installation

`sillyecs` is a build-time dependency. To use it, add this to your `Cargo.toml`:

```toml
[build-dependencies]
sillyecs = "0.0.6"
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
    fixed: 60 Hz  # or "0.01666 s"
  - name: Update
  - name: Render
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
      - Particle     # request random access to particles in pre- or postflight
    inputs:
      - Velocity
    outputs:
      - Position

  - name: Render
    phase: Render
    manual: true        # Require manual call to world.apply_system_phase_render()
    # on_request: true  # call world.request_render_phase() to allow execution (resets atomically)
    states:
      - use: WgpuRender
        default: none      # The default for the below values; optional: none|read|write, defaults to read
        check: read        # optional: none|read|write
        begin_phase: none  # optional: none|read|write
        system: write      # optional: none|read|write
        end_phase: none    # optional: none|read|write
    inputs:
      - Position

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
