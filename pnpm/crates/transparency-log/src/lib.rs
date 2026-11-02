#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]

//! A transparency log in the formats Go's checksum database uses, as the
//! [C2SP](https://github.com/C2SP/C2SP) specifications define them.
//!
//! - [`keys`]: [signed-note](https://c2sp.org/signed-note) verifier keys,
//!   written `<name>+<key hash>+<key>`, and the signers that produce note
//!   signatures and Ed25519 checkpoint cosignatures.
//! - [`checkpoint`]: signing [checkpoints](https://c2sp.org/tlog-checkpoint)
//!   and verifying one against a log key threshold and a witness quorum
//!   ([tlog-cosignature](https://c2sp.org/tlog-cosignature)).
//! - [`tiles`] and [`bundle`]: the [tlog-tiles](https://c2sp.org/tlog-tiles)
//!   resource paths and entry bundle encoding.
//! - [`integrate`](mod@integrate): turning entries that were sequenced
//!   elsewhere, such as in a SQL transaction, into stored hashes, tiles, and a
//!   new tree head. Running it again for the same entries produces the same
//!   output.
//! - [`proof`]: RFC 6962 inclusion and consistency proofs, and following a log
//!   from one verified checkpoint to the next.
//!
//! The Merkle tree arithmetic is the [`tlog_tiles`] port of Go's
//! `golang.org/x/mod/sumdb/tlog`, and notes are parsed by [`signed_note`], the
//! port of `golang.org/x/mod/sumdb/note`.

pub mod bundle;
pub mod checkpoint;
pub mod integrate;
pub mod keys;
pub mod proof;
pub mod tiles;

pub use bundle::*;
pub use checkpoint::*;
pub use integrate::*;
pub use keys::*;
pub use proof::*;
pub use tiles::*;
pub use tlog_tiles::{Checkpoint, EMPTY_HASH, HASH_SIZE, Hash, HashReader, record_hash};
