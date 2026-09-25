---
"pnpm": patch
---

Since pnpm 11.23.0, an `updateConfig` hook reads and sets the registry routes as `config.registriesByScope` and `config.registriesByPrefix`. They were named `config.registries` and `config.namedRegistries` before. A hook that still uses the old names reads `undefined`, and pnpm ignores what it writes to them [#15620](https://github.com/pnpm/pnpm/issues/15620).
