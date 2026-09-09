---
"pacquet": patch
---

The `updateConfig` pnpmfile hook now receives the resolved configuration, including settings that came from `.npmrc`, the command line, or a default. Scoped registries are reported under `registriesByScope`, and a hook may rewrite that map to change where packages are fetched from. Registry credentials are reported under `configByUri`, as pnpm 11 reports them. A setting nothing set is omitted rather than reported as `null` [#14676](https://github.com/pnpm/pnpm/issues/14676).
