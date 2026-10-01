---
"pacquet": minor
---

`pnpm install` now records every project it installs in the store's `projects` directory, as a symlink to the project directory. Only projects that used the global virtual store were recorded before [#6929](https://github.com/pnpm/pnpm/issues/6929).
