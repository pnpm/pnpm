---
"@pnpm/installing.deps-installer": patch
"@pnpm/pnpr.client": minor
"@pnpm/pnpr": minor
"pacquet": minor
"pnpm": patch
---

A resolve request now carries the client's `resolutionMode`, so an install delegated to a pnpr server picks versions the way the client would. `time-based` and `lowest-direct` reached the server as nothing at all, leaving it on its `highest` default: the returned lockfile pinned the highest satisfying version of every dependency, and the setting appeared to be ignored.

This adds a field to the resolve request body. A server older than its client ignores it and keeps resolving `highest`; the protocol is still experimental and unversioned.
