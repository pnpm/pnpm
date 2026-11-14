---
"pacquet": patch
---

`pnpm add crate:<pkg>` now keeps multiline arrays in `Cargo.toml` intact and replaces `workspace = true` when saving an explicit version. `pnpm install` now accepts unversioned path dependencies on prerelease Cargo workspace members, and Cargo workspace inheritance now honors `package` renames.
