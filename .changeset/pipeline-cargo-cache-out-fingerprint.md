---
"pacquet": patch
---

`pnpm pipeline` no longer deletes files from a build script's output directory when it restores a shared Cargo build cache. Before, it could delete a `fingerprint` or `.fingerprint` directory there, such as one in a nested Cargo build.
