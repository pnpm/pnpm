---
"pacquet": minor
---

`pnpm pipeline` can share task results between machines. A result is a signed artifact stored on a pnpr server or on a server that speaks the Turborepo Remote Cache API, such as Vercel Remote Cache.

The remote side-effects cache can also store dependency builds on a Turborepo Remote Cache server. Both caches read the new `remoteCache` setting, which names the server and holds the signing keys. `pnpm pipeline --report` records the run under the organization `remoteCache.org` names.
