use crate::WorldId;

/// A frame context.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FrameContext {
    /// The world ID.
    pub world_id: WorldId,
    /// The frame number.
    pub frame_number: u64,
    /// The delta time since the last frame.
    pub delta_time_secs: f32,
    /// The fixed time for fixed-time systems. Defaults to 60 Hz (~16.66 ms).
    pub fixed_time_secs: f32,
    /// The start time of the current frame.
    pub current_frame_start: std::time::Instant,
    /// The start time of the last frame.
    pub last_frame_start: std::time::Instant,
}

#[allow(dead_code)]
impl FrameContext {
    /// Constructs a new frame context.
    #[doc(hidden)]
    pub fn new(world_id: WorldId) -> Self {
        Self {
            world_id,
            frame_number: 0,
            delta_time_secs: 0.0,
            fixed_time_secs: 1.0 / 60.0,
            current_frame_start: std::time::Instant::now(),
            last_frame_start: std::time::Instant::now(),
        }
    }

    /// Resets the frame context, e.g. after the application came back to foreground.
    #[doc(hidden)]
    pub fn reset(&mut self) {
        self.current_frame_start = std::time::Instant::now();
        self.last_frame_start = std::time::Instant::now();
    }
}
