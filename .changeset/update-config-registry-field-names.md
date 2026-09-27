---
"pnpm": patch
---

An `updateConfig` hook in `.pnpmfile.cjs` receives the registry lookups under new names since pnpm 11.23.0. The per-scope registries are `config.registriesByScope`, the named registries are `config.registriesByPrefix`, and the per-URL registry options are `config.registryOptionsByUrl`. A hook that reads or returns `config.registries`, `config.namedRegistries`, or `config.registryOptions` has to use the new names [#15620](https://github.com/pnpm/pnpm/issues/15620).
