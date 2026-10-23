# Local libsql cursor patch

Source: crates.io `libsql` 0.9.30, repository https://github.com/tursodatabase/libsql.
Upstream revision: `0653c5788d77ef16a97c56ff3e9fdc11717a72d9`.
The original MIT license is retained in `LICENSE.md`.

This patch changes `OwnedCursorStep` in `src/hrana/cursor.rs`. Once a step's
`step_end` entry arrives, the step reads the rest of the cursor response to
its end before it reports no more rows. Before, it dropped the response body
there, before hyper had read the final chunk, so hyper closed the connection
and the next query opened a new one. Against a remote database every query
then paid a TCP connect and, for Turso, a TLS handshake.

Upstream fix: https://github.com/tursodatabase/libsql/pull/2301. Remove this
local patch when a release carries it.

`src/lib.rs` also allows all warnings. Cargo caps the lints of registry
dependencies but not of path dependencies, so the workspace's `-D warnings`
would otherwise fail on lints in this crate.

`rustfmt.toml` disables formatting here. `cargo fmt --all` formats path
dependencies too, and the pre-commit hook formats staged files as edition
2024, which this edition 2021 crate does not parse as.
