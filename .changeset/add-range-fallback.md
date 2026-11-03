---
"pacquet": patch
---

`pnpm add <pkg>` without a version now saves the range that the other workspace projects declare for the package, or the one a matching override sets. A `catalog:` reference still comes first, and the `tag` setting is used only when none of these exists [pnpm/tasks#191](https://github.com/pnpm/tasks/issues/191).
