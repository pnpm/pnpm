---
"@pnpm/installing.deps-restorer": patch
"pacquet": patch
"pnpm": patch
---

With `nodeLinker: hoisted` and a custom `modulesDir`, pnpm now installs a workspace project's dependencies that cannot be hoisted to the root into the project's `modulesDir`. The next install that relinks the project removes the copies that earlier installs left in its `node_modules`. Package directories that pnpm did not install are now also moved to `.ignored` in a custom `modulesDir`.
