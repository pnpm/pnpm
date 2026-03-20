---
"@pnpm/bins.linker": patch
"pnpm": patch
---

fix: suppress false warning when workspace package bin file is not built yet

In a monorepo, a workspace package may declare a `bin` entry (e.g. `dist/cli.js`)
that is produced by a build step. On a clean `pnpm install` the build has not run
yet, so the file does not exist. pnpm was emitting a `WARN  Failed to create bin`
message in this situation even though it is perfectly normal [#10524](https://github.com/pnpm/pnpm/issues/10524).

A `warnOnMissingBin` option has been added to `LinkBinOptions` (global) and to the
per-package entries accepted by `linkBinsOfPackages` (per-package). The global flag
defaults to `true` so existing behaviour is preserved everywhere. At the install
call-site that links direct dependencies of workspace projects, local `link:`
protocol dependencies (which include workspace packages) get `warnOnMissingBin: false`
per-package so the warning is suppressed only for them. All packages are processed in
a single `linkBinsOfPackages` call so cross-package bin conflict resolution still
works correctly.
