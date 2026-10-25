//! # Utility functions for `sillyecs`.

mod command;
mod entity_id;
mod flatten_copy_slices;
mod flatten_slices;
mod flatten_slices_mut;
mod frame_context;
mod world;
mod world_id;

pub use command::{CommandReceiver, CommandSender};
pub use entity_id::EntityId;
pub use flatten_copy_slices::FlattenCopySlices;
pub use flatten_slices::FlattenSlices;
pub use flatten_slices_mut::FlattenSlicesMut;
pub use frame_context::FrameContext;
pub use world::World;
pub use world_id::WorldId;

/// Re-export of [`rayon`] for the generated parallel phase runners, so user crates do not need
/// to depend on it directly.
pub use rayon;

/// Re-export of [`tracing`] for the logging in the generated code, so user crates do not need
/// to depend on it directly.
pub use tracing;

/// The `Instant` type used for frame timing.
///
/// This is `std::time::Instant`, unless the `wasm` feature is enabled; then it is
/// [`web_time::Instant`](https://docs.rs/web-time), which is backed by `performance.now()` on `wasm32-unknown-unknown`
/// (where `std::time::Instant::now()` panics) and re-exports `std::time::Instant` elsewhere.
#[cfg(not(feature = "wasm"))]
pub use std::time::Instant;
#[cfg(feature = "wasm")]
pub use web_time::Instant;

/// Includes the ECS that `sillyecs-build` generated into `OUT_DIR` as a module.
///
/// Without arguments, this declares `pub mod ecs`. Pass a module declaration to choose the name,
/// the visibility and attributes such as docs:
///
/// ```ignore
/// sillyecs::include_ecs!();
///
/// sillyecs::include_ecs!(
///     /// The game's ECS.
///     pub(crate) mod engine
/// );
/// ```
///
/// The generated items carry their own lint allows, so the module needs none. User types without
/// a `paths` entry in `ecs.yaml` are looked up in the parent module of the include site.
#[macro_export]
macro_rules! include_ecs {
    () => {
        $crate::include_ecs!(
            /// The generated ECS.
            pub mod ecs
        );
    };
    ($(#[$attr:meta])* $vis:vis mod $name:ident) => {
        $(#[$attr])*
        $vis mod $name {
            include!(concat!(env!("OUT_DIR"), "/ecs_gen.rs"));
        }
    };
}
