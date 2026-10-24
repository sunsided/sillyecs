# sillyecs

A generator for archetype-based entity-component systems: a schema describing an ECS is turned
into the Rust code of that ECS at build time.

## Language

**Schema**:
The document that describes an ECS: its components, archetypes, views, states, phases, systems
and worlds.
_Avoid_: config, definition, ecs.yaml

### Data

**Entity**:
A thing living in a world, made up of the components of its archetype.
_Avoid_: object, instance, row

**Entity ID**:
The identity of an entity, stable for the entity's whole lifetime wherever it is stored.
_Avoid_: handle, index

**Component**:
A named piece of data that entities can carry.
_Avoid_: field, attribute, property

**Archetype**:
A fixed set of components. Every entity belongs to exactly one archetype.
_Avoid_: entity type, kind, table

**World**:
A self-contained set of entities, restricted to chosen archetypes, together with the systems and
states that act on them.
_Avoid_: scene, registry, universe

**View**:
A named subset of components, giving access to those components on every entity whose archetype
has all of them.
_Avoid_: query, projection

**Spawn**:
Creating an entity in an archetype of a world.
_Avoid_: create, insert, add

**Despawn**:
Removing an entity from its world.
_Avoid_: destroy, delete, kill

**Promotion**:
Turning the entities of one archetype into entities of another archetype that shares some of its
components. Deprecated in favour of despawning and spawning.
_Avoid_: migration, conversion

**Frontloading**:
Reordering an archetype's entities so that a chosen subset comes first.
_Avoid_: partitioning, sorting

### Behaviour

**System**:
A unit of behaviour that runs over every entity whose archetype has all of the system's inputs and
outputs.
_Avoid_: job, processor, script

**Input**:
A component a system reads.
_Avoid_: dependency, read

**Output**:
A component a system writes.
_Avoid_: result, write

**Lookup**:
A system's access to components of entities other than the one it is processing, by entity ID.
_Avoid_: random access, query

**State**:
Application-owned data that lives outside any entity and that systems and phases may read or
write, such as a renderer.
_Avoid_: resource, user state, global

**Frame context**:
The per-frame information a world provides to systems: frame number and timing.
_Avoid_: time, clock, delta

**Readiness check**:
A system's decision, each time its phase starts, whether it runs in that run of the phase.
_Avoid_: guard, filter, condition

**Preflight**:
A pass a system makes over its entities before its main pass.
_Avoid_: pre-pass, setup

**Postflight**:
A pass a system makes over its entities after its main pass.
_Avoid_: post-pass, teardown

### Frames and phases

**Frame**:
One run through all phases in order, except manual phases.
_Avoid_: tick, update, step

**Phase**:
A named stage of a frame in which a fixed set of systems runs. Phases run in the order they are
declared.
_Avoid_: stage, system phase

**Fixed phase**:
A phase that runs at a fixed rate, so it may run several times or not at all in a given frame.
_Avoid_: fixed update, tick

**Manual phase**:
A phase that is never part of a frame and runs only when the application invokes it.
_Avoid_: custom phase, triggered phase

**On-request phase**:
A phase that runs in a frame only if it was requested since it last ran. Repeated requests
collapse into one run.
_Avoid_: conditional phase, event phase

### Scheduling

**Conflict**:
Two systems of the same phase accessing the same component or state, at least one of them
writing it. Conflicting systems never run at the same time.
_Avoid_: dependency, data race, clash

**Run-after constraint**:
A declared requirement that a system runs after another system of the same phase.
_Avoid_: dependency, forced edge, preference, ordering

**Group**:
A set of systems of one phase without conflicts among them, which may therefore run in parallel.
A phase's groups run one after another.
_Avoid_: batch, layer, stage

**Schedule**:
The ordered groups of a phase.
_Avoid_: plan, execution order

### Commands

**Command**:
A request to change a world, such as a spawn or despawn, that takes effect between phases rather
than immediately.
_Avoid_: event, message, world command

**User command**:
A command defined by the application rather than by sillyecs.
_Avoid_: custom command, application command

**Command queue**:
The application-provided channel through which commands reach a world.
_Avoid_: event bus, channel, mailbox
