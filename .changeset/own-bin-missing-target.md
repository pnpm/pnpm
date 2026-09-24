---
"pacquet": patch
---

`pnpm install` no longer links a package's bin into that package's own `node_modules/.bin` before the bin's file exists. On Windows, this let the `node` package's preinstall script call its own missing bin instead of the system Node.js, which failed the install [pnpm/pnpm#15501](https://github.com/pnpm/pnpm/issues/15501).
