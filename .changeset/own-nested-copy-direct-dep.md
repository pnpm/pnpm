---
"pacquet": patch
---

With `nodeLinker: hoisted`, when two workspace projects need the same package version and it cannot be hoisted to the root, each project now keeps the copy installed in its own `node_modules`. Build scripts run in each of these copies. Previously the second project's copy was replaced with a symlink to the first project's, so every repeat install imported it again [#17498](https://github.com/pnpm/pnpm/issues/17498).
