---
"@pnpm/engine.pm.commands": minor
"pnpm": minor
---

`pnpm self-update` and `packageManager` version-switching can now install and link pnpm v12 (the Rust port), published with equal content under both the `pnpm` and `@pnpm/exe` names on the `alpha` dist-tag. Its native binaries ship as `@pnpm/exe.<platform>-<arch>` packages, which pnpm's built-in installer links directly — no Node.js launcher, so the command pays no Node startup cost. v12 is initialized exactly like `@pnpm/exe`, including per-platform global-virtual-store hashing.
