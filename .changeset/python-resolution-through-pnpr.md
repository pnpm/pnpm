---
"@pnpm/pnpr": minor
"pacquet": minor
---

`pnpm install` now resolves Python dependencies through the server configured in `pnprServer`. The client no longer downloads a wheel to find out what it requires, because pnpr reads the metadata file the index publishes beside it. pnpm still downloads the wheels the returned lockfile names, checks them against the index's digests, and re-resolves the project against them. If the server does not serve Python resolution, pnpm resolves Python dependencies locally.
