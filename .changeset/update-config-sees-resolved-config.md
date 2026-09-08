---
"pacquet": patch
---

The `updateConfig` pnpmfile hook now receives the configuration pnpm resolved, so a hook can read settings that came from `.npmrc`, the command line, or a default. Scoped registries declared in `.npmrc` are visible under `registriesByScope`, and a hook may rewrite that map to change where packages are fetched from [#14676](https://github.com/pnpm/pnpm/issues/14676).
