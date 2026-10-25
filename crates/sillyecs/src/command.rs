/// Sends commands into a queue.
///
/// The generated `WorldCommandSender` is implemented for every type whose [`Self::Command`] is
/// the generated `WorldCommand<U>`, so a queue only needs to implement this trait. Because it
/// doesn't depend on any generated type, a generic queue can implement it once for every command
/// type, e.g. `impl<C> CommandSender for MyQueue<C> { type Command = C; ... }`.
pub trait CommandSender {
    /// The commands this queue carries.
    type Command;

    /// The error returned when a command can't be sent.
    type Error: core::error::Error;

    /// Sends a command. May block until sending is complete.
    fn send(&self, command: Self::Command) -> Result<(), Self::Error>;
}

/// Receives commands from a queue.
///
/// The generated `WorldCommandReceiver` is implemented for every type whose [`Self::Command`] is
/// the generated `WorldCommand<U>`; see [`CommandSender`].
pub trait CommandReceiver {
    /// The commands this queue carries.
    type Command;

    /// The error returned when receiving fails.
    type Error: core::error::Error;

    /// Receives a command without blocking. Returns `Ok(None)` if no command is queued.
    fn recv(&self) -> Result<Option<Self::Command>, Self::Error>;
}
