pub use core::num::NonZeroU64;
pub use core::sync::atomic::AtomicU64;

/// The ID of a world.
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct WorldId(NonZeroU64);

#[allow(dead_code)]
impl WorldId {
    /// Returns a new, unique world ID.
    ///
    /// Uniqueness is guaranteed by using a monotonically increasing `AtomicU64` counter
    /// for generating IDs, starting from 1.
    ///
    /// # Implementation
    /// This function uses a thread-safe counter with sequential consistency ordering
    /// to ensure unique IDs even under concurrent access.
    pub fn new() -> Self {
        static WORLD_IDS: AtomicU64 = AtomicU64::new(1);
        let id = WORLD_IDS.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
        WorldId(core::num::NonZeroU64::new(id).expect("ID was zero"))
    }

    /// Constructs a new [`WorldId`] from a known [`NonZeroU64`].
    /// Used internally by the engine to generate valid IDs.
    #[doc(hidden)]
    pub const fn new_from(id: NonZeroU64) -> Self {
        WorldId(id)
    }

    /// Returns this ID as a [`NonZeroU64`](NonZeroU64) value.
    pub const fn as_nonzero_u64(&self) -> NonZeroU64 {
        self.0
    }

    /// Returns this ID as a `u64` value.
    pub const fn as_u64(&self) -> u64 {
        self.0.get()
    }
}

impl core::hash::Hash for WorldId {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl From<WorldId> for core::num::NonZeroU64 {
    fn from(value: WorldId) -> core::num::NonZeroU64 {
        value.as_nonzero_u64()
    }
}

impl From<WorldId> for u64 {
    fn from(value: WorldId) -> u64 {
        value.as_u64()
    }
}
