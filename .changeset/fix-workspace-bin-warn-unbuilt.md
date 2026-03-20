---
"@pnpm/bins.linker": patch
"pnpm": patch
---

fix: suppress false warning when workspace package bin file is not built yet

In a monorepo, a workspace package may declare a `bin` entry (e.g. `dist/cli.js`)
that is produced by a build step. On a clean `pnpm install` the build has not run
yet, so the file does not exist. pnpm was emitting a `WARN  Failed to create bin`
message in this situation even though it is perfectly normal [#10524](https://github.com/pnpm/pnpm/issues/10524).

A new `warnOnMissingBin` option has been added to `LinkBinOptions`. It defaults to
`true` (existing behaviour preserved everywhere) but is set to `false` at the
install call-site that links direct dependencies of workspace projects, since only
that path includes workspace-linked packages whose build artifacts may not yet exist.
