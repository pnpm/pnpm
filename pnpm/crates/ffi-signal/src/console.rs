//! Registering a handler for the Windows console's control events.

use crate::Interrupt;
use std::{
    marker::PhantomData,
    sync::atomic::{AtomicUsize, Ordering},
};
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
    register_dispatch();
}

/// A handler for the events that mean the console itself is going away: it
/// being closed, the user logging off, and the system shutting down.
///
/// It runs under the same constraints as [`Handler`].
pub type ClosingHandler = fn();

/// Register `handler` for the events that mean the console is going away.
///
/// These cannot be dealt with the way an interrupt can: the operating
/// system ends the process shortly after the handlers return, whatever they
/// report, and then ends every other process attached to the console. A
/// handler here is for releasing what must not outlive the console, which
/// is why it reports nothing. Every registered one runs, newest first.
pub fn on_console_closing(handler: ClosingHandler) {
    CLOSING_HANDLERS.push(handler as usize);
    register_dispatch();
}

fn register_dispatch() {
    static REGISTER: std::sync::Once = std::sync::Once::new();

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

/// The registered handlers, newest first. A fixed table because the
/// dispatch below runs on a console-created thread and cannot wait on an
/// allocator another thread may be holding.
static HANDLERS: HandlerTable<Handler> = HandlerTable::new();

/// The handlers for the console going away, held as [`HANDLERS`] is.
static CLOSING_HANDLERS: HandlerTable<ClosingHandler> = HandlerTable::new();

/// Room for the handlers pnpm registers, plus the same again. An
/// additional registration past the table is dropped rather than growing
/// it, which costs that handler its events and nothing else.
const HANDLER_CAPACITY: usize = 4;

/// A table of `fn`-pointer handlers of one signature, addressed as the
/// integers an atomic can hold.
struct HandlerTable<F: Copy> {
    entries: [AtomicUsize; HANDLER_CAPACITY],
    signature: PhantomData<F>,
}

impl<F: Copy> HandlerTable<F> {
    const fn new() -> Self {
        const { assert!(size_of::<F>() == size_of::<usize>()) };
        Self { entries: [const { AtomicUsize::new(0) }; HANDLER_CAPACITY], signature: PhantomData }
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
    fn newest_first(&self) -> impl Iterator<Item = F> {
        self.entries
            .iter()
            .rev()
            .filter_map(|entry| {
                let handler = entry.load(Ordering::Acquire);
                // SAFETY: a non-zero entry is the address of the `F` a
                // `push` stored, an ordinary `fn` item of exactly this
                // signature that lives for the whole program, and `F` is a
                // pointer-sized `fn` type by the assertion in `new`.
                (handler != 0).then(|| unsafe { std::mem::transmute_copy::<usize, F>(&handler) })
            })
    }
}

/// The handler the console actually gets, which calls each registered one
/// until one deals with the event.
unsafe extern "system" fn dispatch(event: u32) -> windows_sys::core::BOOL {
    let interrupt = match event {
        CTRL_C_EVENT => Interrupt::Interrupt,
        CTRL_BREAK_EVENT => Interrupt::Terminate,
        // Every other event says the console is going away, which no
        // handler can stop, so each one runs and the event still goes to
        // the default handler.
        _ => {
            for handler in CLOSING_HANDLERS.newest_first() {
                handler();
            }
            return 0;
        }
    };
    for handler in HANDLERS.newest_first() {
        if handler(interrupt) == Handled::Yes {
            return 1;
        }
    }
    0
}
