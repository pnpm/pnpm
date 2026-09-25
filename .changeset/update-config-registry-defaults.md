---
"pnpm": patch
---

An `updateConfig` hook that returns `registriesByScope` without the `default` or `@jsr` entry no longer crashes the install with `Invalid URL`. A missing `default` keeps the configured `registry`, and a missing `@jsr` falls back to the built-in JSR registry [#15619](https://github.com/pnpm/pnpm/issues/15619).

The hook's `registry` and the `default` entry of its `registriesByScope` now set one default registry, which installs, `pnpm publish`, and `pnpm login` all use. If a hook changes both, `registry` wins. A route that is not a string fails with `ERR_PNPM_INVALID_UPDATE_CONFIG_RESULT`.
