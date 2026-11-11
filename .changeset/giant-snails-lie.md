---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

Conflicting non-semver peer dependencies now remain missing when `autoInstallPeersFromHighestMatch` is enabled. Identical non-semver peer specifiers are preserved. Preferred registry versions no longer replace local paths or protocol-prefixed specifiers when installing missing peers [pnpm/tasks#272](https://github.com/pnpm/tasks/issues/272).
