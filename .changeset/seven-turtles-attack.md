---
"@pnpm/installing.commands": patch
"@pnpm/deps.inspection.tree-builder": patch
"@pnpm/lockfile.detect-dep-types": patch
"@pnpm/installing.dedupe.check": patch
pnpm: patch
---

Removed TypeScript specific syntax (such as enums and visibility modifiers in constructors) to enable `erasableSyntaxOnly`.
