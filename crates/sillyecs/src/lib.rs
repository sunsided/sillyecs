//! # Utility functions for `sillyecs`.

mod entity_id;
mod flatten_copy_slices;
mod flatten_slices;
mod flatten_slices_mut;
mod frame_context;
mod izip;
mod world;
mod world_id;

pub use entity_id::EntityId;
pub use flatten_copy_slices::FlattenCopySlices;
pub use flatten_slices::FlattenSlices;
pub use flatten_slices_mut::FlattenSlicesMut;
pub use frame_context::FrameContext;
pub use world::World;
pub use world_id::WorldId;

// `izip!` is defined in the `izip` module with `#[macro_export]`, which
// already places it at the crate root for downstream users (`sillyecs::izip!`).
// The module itself stays private; the macro definition carries its own docs.
