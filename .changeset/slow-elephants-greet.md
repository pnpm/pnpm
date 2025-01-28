---
"@pnpm/hooks.read-package-hook": patch
"@pnpm/semver.peer-range": major
"@pnpm/resolve-dependencies": patch
"@pnpm/core": patch
"pnpm": patch
---

Prevent `overrides` from adding invalid versions to `peerDependencies` by moving it to `dependencies` [#8978](https://github.com/pnpm/pnpm/issues/8978).
