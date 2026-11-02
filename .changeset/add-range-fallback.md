---
"pacquet": patch
---

`pnpm add <pkg>` without a version now saves the range that the other workspace projects declare for the package, or the one a matching override sets. It uses the `tag` setting only when neither exists [pnpm/tasks#191](https://github.com/pnpm/tasks/issues/191).
