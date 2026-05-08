---
"@pnpm/deps.inspection.commands": patch
"@pnpm/installing.commands": patch
"@pnpm/lockfile.make-dedicated-lockfile": patch
"@pnpm/resolving.npm-resolver": patch
---

Upgrade `@pnpm/semver-diff`, `@pnpm/colorize-semver-diff`, `@pnpm/exec`, and `parse-npm-tarball-url` to versions that expose their helpers as named exports instead of CommonJS default exports. This eliminates the `.default` property accesses that broke under Node.js ESM interop in tests and could fail at runtime in some module loaders.
