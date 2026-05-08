---
"@pnpm/deps.inspection.commands": patch
"@pnpm/installing.commands": patch
"pnpm": patch
---

Upgrade `@pnpm/semver-diff` and `@pnpm/colorize-semver-diff` to v2, which expose their helpers as named exports instead of CommonJS default exports. This eliminates the `.default` property accesses that broke under Node.js ESM interop in tests and could fail at runtime in some module loaders.
