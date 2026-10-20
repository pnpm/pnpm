---
"pacquet": patch
---

With the default `modulesDir`, the experimental loaded linker stores installation state and bin shims in `.pnpm` and does not create an application `node_modules` directory on fresh installs. pnpm keeps an explicit `modulesDir`. `pnpm pack` and `pnpm publish` exclude a `.pnpm` directory at the package root.
