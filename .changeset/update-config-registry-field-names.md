---
"pnpm": patch
---

The registry lookups that an `updateConfig` hook in `.pnpmfile.cjs` reads and returns were renamed in pnpm 11.23.0. Before that release, they were `config.registries`, `config.namedRegistries`, and `config.registryOptions`. They are now `config.registriesByScope`, `config.registriesByPrefix`, and `config.registryOptionsByUrl`, and a hook has to use the new names [#15620](https://github.com/pnpm/pnpm/issues/15620).
