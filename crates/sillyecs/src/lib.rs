//! # Utility functions for `sillyecs`.

mod archetypes;
mod entity_id;
mod flatten_slices;
mod flatten_slices_mut;
mod world_id;

pub use entity_id::EntityId;
pub use flatten_slices::FlattenSlices;
pub use flatten_slices_mut::FlattenSlicesMut;
pub use world_id::WorldId;
