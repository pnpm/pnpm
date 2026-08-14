---
"pacquet": patch
---

`--frozen-lockfile` no longer rejects a lockfile pnpm just generated when `packageExtensions` adds a peer dependency to a workspace project. The peer is auto-installed and recorded in the importer entry, but the freshness check compared against the `package.json` on disk, which has no such peer, and reported the entry as a removed dependency [#13836](https://github.com/pnpm/pnpm/issues/13836).
