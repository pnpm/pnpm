---
"pacquet": patch
"pnpm": patch
---

`pnpm` warns when an `updateConfig` hook changes `registries` and some routes still use their configured registry. The warning names those routes and points to `registriesByScope`.
