---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

Conflicting non-semver peer dependencies now remain missing when `autoInstallPeersFromHighestMatch` is enabled. Identical non-semver peer specifiers are preserved [pnpm/tasks#272](https://github.com/pnpm/tasks/issues/272).
