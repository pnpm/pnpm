---
"pacquet": patch
---

With the default `modulesDir`, the experimental loaded linker stores installation state and bin shims in `.pnpm`. Fresh installs do not create an application `node_modules` directory. An explicit `modulesDir` is preserved. `pnpm pack` and `pnpm publish` leave out a `.pnpm` directory at the package root.
