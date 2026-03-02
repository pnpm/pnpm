---
"@pnpm/global.commands": minor
"pnpm": minor
---

Allow `pnpm add -g` to override conflicting bin names when the new package owns the bin. A package owns a bin when its package name matches the bin name (e.g., the `npm` package owns the `npm` bin). The `npx` bin is also treated as owned by the `npm` package. Unrelated bin conflicts still produce an error.
