// Hand-written user-side stubs for the `full_coverage` compile fixture. Pairs
// with `ecs.yaml` in this directory; included from the synthetic library crate
// built by `tests/compile_generated.rs`.
//
// This module is a *sibling* of the generated code, like in a real game, so it
// only reaches the generated items' public API: systems are built with `From`,
// not through their private tuple fields.

use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::sync::Mutex;

// Exercises `paths.entity_location_map` pointing into the user crate (think
// `fxhash::FxHashMap`).
pub type LocationMap<K, V> = HashMap<K, V>;

// --- Component data structs ----------------------------------------------------
//
// The component template emits `pub struct XComponent(XData);` and impls
// `Deref<Target = XData>` etc., so each component named in the YAML needs a
// matching `XData` type that derives `Debug + Clone + Default`.

#[derive(Debug, Default, Clone)]
pub struct PositionData {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Default, Clone)]
pub struct VelocityData {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Default, Clone)]
pub struct HealthData(pub i32);

#[derive(Debug, Default, Clone)]
pub struct SpriteData(pub u32);

// --- States -------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct InputState;

#[derive(Debug, Default)]
pub struct RendererState;

// --- System data + Default for system newtypes --------------------------------

#[derive(Debug, Default)]
pub struct StepSystemData;

#[derive(Debug, Default)]
pub struct HealSystemData;

#[derive(Debug, Default)]
pub struct DrawSystemData;

impl Default for StepSystem {
    fn default() -> Self {
        Self::from(StepSystemData)
    }
}

impl Default for HealSystem {
    fn default() -> Self {
        Self::from(HealSystemData)
    }
}

impl Default for DrawSystem {
    fn default() -> Self {
        Self::from(DrawSystemData)
    }
}

// --- System factory + CreateSystem impls --------------------------------------
//
// `Spin` and `Tint` are `stateless: true`: their data structs and `CreateSystem` impls are
// generated, so the factory only covers the stateful systems.

pub struct SystemFactory;

impl CreateSystem<StepSystem> for SystemFactory {
    fn create(&self) -> StepSystem {
        StepSystem::default()
    }
}

impl CreateSystem<HealSystem> for SystemFactory {
    fn create(&self) -> HealSystem {
        HealSystem::default()
    }
}

impl CreateSystem<DrawSystem> for SystemFactory {
    fn create(&self) -> DrawSystem {
        DrawSystem::default()
    }
}

// --- Apply<X>System impls -----------------------------------------------------
//
// The Apply traits provide defaults for every method, so the minimum a real
// consumer must spell out is `type Error`. That's what we do here.

impl ApplyStepSystem for StepSystem {
    type Error = Infallible;

    fn preflight(
        &mut self,
        _context: &::sillyecs::FrameContext,
        _lookup: &dyn StepComponentLookup,
        _velocities: &[VelocityComponent],
        _positions: &[PositionComponent],
    ) {
    }

    fn postflight(
        &mut self,
        _context: &::sillyecs::FrameContext,
        _lookup: &dyn StepComponentLookup,
        _velocities: &[VelocityComponent],
        _positions: &[PositionComponent],
    ) {
    }
}

impl ApplyHealSystem for HealSystem {
    type Error = Infallible;
}

impl ApplyDrawSystem for DrawSystem {
    type Error = Infallible;
}

impl ApplySpinSystem for SpinSystem {
    type Error = Infallible;
}

impl ApplyTintSystem for TintSystem {
    type Error = Infallible;
}

// --- User command + queue -----------------------------------------------------
//
// Issue #39 explicitly calls for a non-trivial `WorldCommandQueue` with a real
// `UserCommand` enum. PR #37 / issue #37 were exactly the class of bug that
// surfaces only when `Q::UserCommand != NoUserCommand`.

#[derive(Debug, Clone)]
pub enum UserCommand {
    Heal { amount: i32 },
    Spawn,
}

// The queue is generic over its command type and implements only the runtime
// traits from `sillyecs`, like a queue from a support crate would (e.g.
// game-toolkit-ecs's `ChannelQueue`). The generated `WorldCommandSender` /
// `WorldCommandReceiver` come from blanket impls.

pub struct Queue<C> {
    queue: Mutex<VecDeque<C>>,
}

/// The fixture world's command queue.
pub type CommandQueue = Queue<WorldCommand<UserCommand>>;

impl<C> Queue<C> {
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
        }
    }
}

impl<C> Default for Queue<C> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct CommandQueueClosed;

impl std::fmt::Display for CommandQueueClosed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("command queue mutex poisoned")
    }
}

impl std::error::Error for CommandQueueClosed {}

impl<C> ::sillyecs::CommandSender for Queue<C> {
    type Command = C;
    type Error = CommandQueueClosed;

    fn send(&self, command: C) -> Result<(), Self::Error> {
        self.queue
            .lock()
            .map_err(|_| CommandQueueClosed)?
            .push_back(command);
        Ok(())
    }
}

impl<C> ::sillyecs::CommandReceiver for Queue<C> {
    type Command = C;
    type Error = CommandQueueClosed;

    fn recv(&self) -> Result<Option<C>, Self::Error> {
        Ok(self
            .queue
            .lock()
            .map_err(|_| CommandQueueClosed)?
            .pop_front())
    }
}

impl<E, Q> WorldUserCommandHandler for MainWorld<E, Q>
where
    Q: WorldUserCommand<UserCommand = UserCommand>,
{
    fn handle_user_command(&mut self, command: Self::UserCommand) {
        // Discard for the fixture; the goal is to compile, not to act.
        let _ = command;
    }
}

// --- Smoke construction -------------------------------------------------------
//
// Forces monomorphization of the generic `apply_system_phases*` family with a
// concrete `Q = CommandQueue` (real `UserCommand`) and `E = NoOpPhaseEvents`.
// Without an instantiation, some monomorphization-time bugs slip past
// `cargo check`.

#[allow(dead_code)]
pub fn smoke() {
    let factory = SystemFactory;
    let states = MainWorldStates::default();
    let queue = CommandQueue::new();
    let mut world: MainWorld<NoOpPhaseEvents, CommandQueue> =
        MainWorld::new(&factory, states, queue);

    // Stateless systems can be created by any factory, even `()`.
    let _spin: SpinSystem = CreateSystem::<SpinSystem>::create(&());
    world.apply_system_phases();
    world.par_apply_system_phases();
    world.apply_system_phase_render();
    world.par_apply_system_phase_render();
    world.request_update_phase();

    // Force monomorphization of the view accessors.
    let id = world.spawn_particle(ParticleEntityComponents {
        position: PositionComponent::new(PositionData::default()),
        velocity: VelocityComponent::new(VelocityData::default()),
    });
    let _view: Option<MovableView<'_>> = world.get_movable_view(id);
    let _view_mut: Option<MovableViewMut<'_>> = world.get_movable_view_mut(id);
}
