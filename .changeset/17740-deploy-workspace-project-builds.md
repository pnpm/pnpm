---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` now runs the build scripts of a workspace project that it installs as a `file:` dependency, without a machine-specific `allowBuilds` key. An `allowBuilds` entry that denies the project still applies [pnpm/pnpm#17740](https://github.com/pnpm/pnpm/issues/17740).
