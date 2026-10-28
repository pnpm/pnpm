---
"pacquet": patch
---

`pnpm install` no longer writes broken dependency links when the project's `node_modules` is a symlink to another directory, such as another project's `node_modules`. The install reported success, but the links it wrote did not resolve [#17448](https://github.com/pnpm/pnpm/issues/17448).
