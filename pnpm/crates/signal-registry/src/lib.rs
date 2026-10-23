//! Append-only registries a signal handler can walk.
//!
//! A handler may not allocate, lock, or unwind, because it interrupts
//! whatever the process was doing: a `malloc` it interrupts holds the
//! allocator's lock, so allocating again deadlocks. So a registry a handler
//! reads cannot be a `Vec` behind a `Mutex`, and this crate is the raw
//! structure that replaces one.
//!
//! The `unsafe` policy in `UNSAFE_POLICY.md` permits this crate's
//! `unsafe` under its second category: a raw-pointer data structure that a
//! safe one cannot replace. The constraint earns this crate and nothing
//! wider, so the handlers that walk a registry hold no `unsafe` of their
//! own.
//!
//! Both types here leak their nodes rather than freeing them, which is what
//! makes a walk safe: a pointer a handler may be holding is never
//! invalidated. The node count therefore grows to the peak number of
//! concurrent registrations and no further, since a released node is reused
//! by the next registration.
//!
//! Those leaks are what they look like, so Miri has to be told to expect
//! them: `MIRIFLAGS=-Zmiri-ignore-leaks cargo miri test -p
//! pnpm-signal-registry`. Running it is worth the trouble on a change to
//! either type, because Stacked Borrows rejects aliasing a plain test
//! cannot see.

#![allow(unsafe_code, reason = "exempt by UNSAFE_POLICY.md, enforced by pnpm-unsafe-audit")]

pub use list::SignalList;
pub use registry::{Registration, SignalRegistry};

mod list;
mod registry;
