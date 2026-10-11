---
"pnpm": patch
"pacquet": patch
---

`pnpm publish` now uses an `NPM_ID_TOKEN` only if its `aud` claim matches the selected registry, such as `npm:registry.npmjs.org`. A token for another registry is skipped with a warning, and pnpm falls back to the configured credentials.
