---
"pacquet": patch
"pnpm": patch
---

`pnpm` warns when an `updateConfig` hook returns registry routes under `registries`. The warning names scopes whose routes remain unchanged and points to `registriesByScope`.
