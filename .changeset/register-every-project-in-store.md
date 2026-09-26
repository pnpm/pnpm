---
"pacquet": minor
---

`pnpm install` now records every project it installs in the store's `projects` directory, not only projects using the global virtual store. Each entry is a symlink to a project that has a `node_modules` directory linked from that store, so the store can be emptied after removing them [#6929](https://github.com/pnpm/pnpm/issues/6929).
