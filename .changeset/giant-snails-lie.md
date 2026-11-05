---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

Conflicting non-semver peer dependencies are no longer joined with ` || ` when `autoInstallPeersFromHighestMatch` is enabled. Identical non-semver peer specifiers are preserved.
