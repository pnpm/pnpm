---
"pacquet": patch
"pnpm": patch
---

`pnpm` warns when an `updateConfig` hook returns registry routes under `registries`. The warning names the routes that still use their configured registry and points to `registriesByScope`.
