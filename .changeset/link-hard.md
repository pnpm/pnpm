---
"pacquet": minor
---

Added a `--hard` option to `pnpm link`. With `--hard`, pnpm saves the linked package with the `file:` protocol, so its files are hard linked into `node_modules` and its dependencies and peer dependencies resolve from your project. `pnpm unlink` removes these links too [#9895](https://github.com/pnpm/pnpm/issues/9895).
