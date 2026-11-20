---
"@pnpm/pnpr": minor
"pacquet": minor
---

`pnpm install` now resolves Python dependencies through the server configured in `pnprServer`. The client no longer downloads a wheel to find out what it requires. If the server does not serve Python resolution, pnpm resolves Python dependencies locally.
