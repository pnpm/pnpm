---
"pnpm": patch
---

An `updateConfig` hook that returns `registriesByScope` without the `default` or `@jsr` entry no longer crashes the install with `Invalid URL`. pnpm now falls back to the built-in registry for any route the hook omits [#15619](https://github.com/pnpm/pnpm/issues/15619).
