---
"@pnpm/pkg-manifest.utils": patch
"@pnpm/installing.deps-resolver": patch
pnpm: patch
---

Fix `--save-peer` to write valid semver ranges to `peerDependencies` for protocol-based installs (e.g. `jsr:`) by deriving from resolved versions when available and falling back to `*` if none is available [#10417](https://github.com/pnpm/pnpm/issues/10417).
