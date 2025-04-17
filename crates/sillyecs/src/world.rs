use crate::WorldId;

/// Marker trait for worlds.
#[allow(dead_code)]
pub trait World {
    /// The ID of this world.
    const ID: WorldId;

    /// The ID of this world.
    #[inline]
    #[allow(dead_code)]
    fn id(&self) -> WorldId {
        Self::ID
    }
}
