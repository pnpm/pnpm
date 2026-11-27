---
"pnpm": patch
---

`pnpm store prune` now removes the Python environments that no project uses and the Cargo crates that no Cargo workspace links. Python and Cargo installs now register their projects in the store, and a prune keeps what a registered project still uses.
