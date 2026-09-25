---
"pnpm": patch
---

An `updateConfig` hook that returns `registriesByScope` without the `default` or `@jsr` entry no longer crashes the install with `Invalid URL`. A missing `default` falls back to the configured `registry`, and a missing `@jsr` falls back to the built-in JSR registry. A `default` entry the hook sets now also becomes the `registry` that commands such as `pnpm publish` and `pnpm login` use [#15619](https://github.com/pnpm/pnpm/issues/15619).
