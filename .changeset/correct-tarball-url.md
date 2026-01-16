---
"@pnpm/config.deps-installer": minor
"@pnpm/workspace.state": minor
"@pnpm/types": minor
"@pnpm/cli-utils": minor
"pnpm": minor
---

Fixed installation of config dependencies from private registries.

Added support for object type in `configDependencies` when the tarball URL returned from package metadata differs from the computed URL [#10431](https://github.com/pnpm/pnpm/pull/10431).
