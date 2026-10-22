//! Registering a handler for the Windows console's control events.

use crate::Interrupt;
use std::sync::atomic::{AtomicUsize, Ordering};
use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler};

/// A handler for one console control event.
///
/// It runs on a thread the console creates, which the console terminates
/// if the handler does not return, so it must not block indefinitely.
///
/// Returning [`Handled::Yes`] stops the event here, which keeps pnpm
/// running; [`Handled::No`] passes it to the next handler and finally to
/// the default one, which ends the process.
pub type Handler = fn(Interrupt) -> Handled;

/// Whether a handler dealt with the event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handled {
    /// The event is dealt with, and pnpm goes on running.
    Yes,
    /// The event is for the next handler, and finally for the default one.
    No,
}

/// Register `handler` for the console's interrupt and break events.
///
/// Handlers run newest first, so a later registration sees an event before
/// an earlier one does.
pub fn on_console_event(handler: Handler) {
    HANDLERS.push(handler as usize);
    // SAFETY: the handler is a plain `extern "system"` function with static
    // storage, and `SetConsoleCtrlHandler` takes it and the add flag by
    // value. Registering the same function twice would make the console
    // call it twice, which is why one registration covers every handler.
    unsafe {
        REGISTER.call_once(|| {
            SetConsoleCtrlHandler(Some(dispatch), 1);
        });
    }
}

static REGISTER: std::sync::Once = std::sync::Once::new();

/// The registered handlers, newest first. A fixed table because the
/// dispatch below runs on a console-created thread and cannot wait on an
/// allocator another thread may be holding.
static HANDLERS: HandlerTable = HandlerTable::new();

/// Room for the handlers pnpm registers, plus the same again. An
/// additional registration past the table is dropped rather than growing
/// it, which costs that handler its events and nothing else.
const HANDLER_CAPACITY: usize = 4;

struct HandlerTable {
    entries: [AtomicUsize; HANDLER_CAPACITY],
}

impl HandlerTable {
    const fn new() -> Self {
        Self { entries: [const { AtomicUsize::new(0) }; HANDLER_CAPACITY] }
    }

    fn push(&self, handler: usize) {
        for entry in &self.entries {
            if entry.compare_exchange(0, handler, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                return;
            }
        }
    }

    /// The handlers, newest first, which is the order the console itself
    /// calls its own handlers in.
    fn newest_first(&self) -> impl Iterator<Item = Handler> {
        self.entries
            .iter()
            .rev()
            .filter_map(|entry| {
                let handler = entry.load(Ordering::Acquire);
                // SAFETY: a non-zero entry is the address of the `Handler`
                // a `push` stored, an ordinary `fn` item of exactly this
                // signature that lives for the whole program.
                (handler != 0).then(|| unsafe { std::mem::transmute::<_, Handler>(handler) })
            })
    }
}

/// The handler the console actually gets, which calls each registered one
/// until one deals with the event.
unsafe extern "system" fn dispatch(event: u32) -> windows_sys::core::BOOL {
    let interrupt = match event {
        CTRL_C_EVENT => Interrupt::Interrupt,
        CTRL_BREAK_EVENT => Interrupt::Terminate,
        // Every other event is for the default handler, which is what
        // reporting it unhandled asks for.
        _ => return 0,
    };
    for handler in HANDLERS.newest_first() {
        if handler(interrupt) == Handled::Yes {
            return 1;
        }
    }
    0
}
