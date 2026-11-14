---
"pacquet": patch
---

`pnpm add crate:<pkg>` no longer corrupts multiline arrays in `Cargo.toml`. Saving an explicit version with `pnpm add crate:<pkg>@<version>` now replaces `workspace = true` on existing dependency entries. Inherited workspace settings such as `features`, `default-features`, and `package` stay on the updated entry. `pnpm install` now accepts unversioned path dependencies on prerelease Cargo workspace members. Cargo workspace inheritance now resolves `package` renames.
