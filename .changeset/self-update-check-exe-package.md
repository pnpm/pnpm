---
"pacquet": patch
---

`pnpm self-update` no longer reinstalls pnpm when the active version was installed by the standalone installation script. That script installs the engine under the `@pnpm/exe` package name. The check for an existing global install looked only for `pnpm` [#14823](https://github.com/pnpm/pnpm/issues/14823).
