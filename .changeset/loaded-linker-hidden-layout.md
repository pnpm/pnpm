---
"pacquet": patch
---

With the default `modulesDir`, the experimental loaded linker stores installation state and bin shims in `.pnpm`. Fresh installs do not create an application `node_modules` directory. An explicit `modulesDir` is preserved.
