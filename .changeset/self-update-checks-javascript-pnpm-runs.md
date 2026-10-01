---
"pacquet": patch
---

`pnpm self-update` now checks that a version installed as the JavaScript `pnpm` can start before making it the global `pnpm`. If Node.js is missing, the update fails and the current `pnpm` stays in place.
