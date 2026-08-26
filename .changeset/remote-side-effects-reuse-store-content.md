---
"@pnpm/store.controller-types": minor
"@pnpm/store.controller": minor
"@pnpm/pnpr.client": minor
"pacquet": patch
"pnpm": patch
---

Restoring a dependency's build from the remote side-effects cache no longer downloads files the store already holds. A built package's files are mostly its own, and artifacts share files with one another, so most of an artifact is content the store has already addressed by the same digest.
