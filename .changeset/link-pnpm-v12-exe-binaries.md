---
"@pnpm/engine.pm.commands": minor
"@pnpm/installing.commands": patch
"pnpm": minor
---

`pnpm self-update` and `packageManager` version-switching can now install and link pnpm v12 (the Rust port, published as the `pnpm` package on the `alpha` dist-tag). Its native binaries ship as `@pnpm/exe.<platform>-<arch>` packages, which pnpm's built-in installer links directly — no Node.js launcher, so the command pays no Node startup cost. The pacquet install engine (declared via `configDependencies`) resolves its binary from the same `@pnpm/exe.<platform>-<arch>` packages.
