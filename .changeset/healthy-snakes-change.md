---
"@pnpm/core": minor
"@pnpm/headless": minor
"@pnpm/real-hoist": minor
---

A new option added for avoiding hoisting some dependencies to the root of `node_modules`: `externalDependencies`. This option is a set of dependency names that were added to `node_modules` by another tool. pnpm doesn't have information about these dependencies but they shouldn't be overwritten by hoisted dependencies.
