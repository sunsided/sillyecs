//! # Utility functions for `sillyecs`.

mod entity_id;
mod flatten_copy_slices;
mod flatten_slices;
mod flatten_slices_mut;
mod frame_context;
mod world;
mod world_id;

pub use entity_id::EntityId;
pub use flatten_copy_slices::FlattenCopySlices;
pub use flatten_slices::FlattenSlices;
pub use flatten_slices_mut::FlattenSlicesMut;
pub use frame_context::FrameContext;
pub use world::World;
pub use world_id::WorldId;

// Re-exported so generated code can emit a flat zip-and-destructure expression
// without forcing downstream crates to depend on `itertools` directly.
pub use itertools::izip;
