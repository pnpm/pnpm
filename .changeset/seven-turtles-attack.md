---
"@pnpm/plugin-commands-installation": patch
"@pnpm/reviewing.dependencies-hierarchy": patch
"@pnpm/lockfile.detect-dep-types": patch
"@pnpm/dedupe.check": patch
pnpm: patch
---

Removed TypeScript specific syntax (such as enums and visibility modifiers in constructors) to enable `erasableSyntaxOnly`.
