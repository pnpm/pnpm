---
"@pnpm/installing.deps-restorer": patch
"pacquet": patch
"pnpm": patch
---

With `nodeLinker: hoisted` and a custom `modulesDir`, a workspace project's dependencies that cannot be hoisted to the root are now installed into the project's `modulesDir`. They used to go in the project's `node_modules`. Package directories that pnpm did not install are now also moved to `.ignored` in a custom `modulesDir`.
