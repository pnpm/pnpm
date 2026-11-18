---
"pacquet": patch
---

`pnpm pipeline` no longer deletes a `fingerprint` directory that a build script writes to its output directory when it restores a shared Cargo build cache created by Cargo 1.99 or older.
