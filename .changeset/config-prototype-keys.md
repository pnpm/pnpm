---
"@pnpm/config.reader": patch
"pnpm": patch
---

A registry prefix or an override version reference named like a built-in object property, such as `constructor` or `$toString`, is now handled correctly. The prefix was rejected as declared by two registries, and the override resolved to a function.
