use core::num::NonZeroU64;
use core::sync::atomic::AtomicU64;

/// The ID of an entity.
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct EntityId(NonZeroU64);

#[allow(dead_code)]
impl EntityId {
    /// Returns a new, unique entity ID.
    ///
    /// Uniqueness is guaranteed by using a monotonically increasing `AtomicU64` counter
    /// for generating IDs, starting from 1.
    ///
    /// # Implementation
    /// This function uses a thread-safe counter with sequential consistency ordering
    /// to ensure unique IDs even under concurrent access.
    // No `Default`: each call hands out a fresh ID, which a default value should not do.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        static ENTITY_IDS: AtomicU64 = AtomicU64::new(1);
        let id = ENTITY_IDS.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
        EntityId(NonZeroU64::new(id).expect("ID was zero"))
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

impl core::hash::Hash for EntityId {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl From<EntityId> for NonZeroU64 {
    fn from(value: EntityId) -> NonZeroU64 {
        value.as_nonzero_u64()
    }
}

impl From<EntityId> for u64 {
    fn from(value: EntityId) -> u64 {
        value.as_u64()
    }
}

impl core::fmt::Display for EntityId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> Result<(), core::fmt::Error> {
        core::fmt::Display::fmt(&self.0.get(), f)
    }
}
