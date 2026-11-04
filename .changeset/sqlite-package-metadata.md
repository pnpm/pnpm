---
"@pnpm/pnpr": minor
---

pnpr can keep hosted package metadata in an embedded SQLite database. With SQLite metadata, pnpr refuses any change to a published npm version's manifest. pnpr never accepts an unpublished version again, even when its tombstone is lost. pnpr records who moved each dist-tag, and when. Hosted npm package responses carry a `pnpr-generation` header.
