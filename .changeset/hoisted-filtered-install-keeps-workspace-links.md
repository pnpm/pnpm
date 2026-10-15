---
"pacquet": patch
---

With `nodeLinker: hoisted`, a filtered install such as `pnpm --filter app add <pkg>` keeps the root `node_modules` links and bins of the workspace projects it did not select. A project with no dependencies of its own lost them until the next full install.
