---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

A filtered `pnpm install` that changes the installed dependency groups, such as `pnpm --filter app install --prod`, no longer empties the `node_modules` directories of the workspace projects it skips.
