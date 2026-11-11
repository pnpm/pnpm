---
"@pnpm/pnpr": minor
---

Added `cloudflareCdn` to purge cached public packages after publishes, unpublishes, deprecations, and access-rule changes. The Cloudflare cache example serves anonymous cache hits without contacting the registry and limits cached responses to five minutes.
