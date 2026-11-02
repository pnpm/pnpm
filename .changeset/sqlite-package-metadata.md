---
"@pnpm/pnpr": minor
---

pnpr can keep hosted package metadata in an embedded SQLite database with `metadata: { backend: sqlite }`. A published npm version's manifest then cannot change, dist-tag moves are recorded with who made them, and hosted npm package responses carry a `pnpr-generation` header.
