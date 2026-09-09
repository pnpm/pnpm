---
"@pnpm/pnpr": patch
---

pnpr no longer re-encodes a URL's query when it redacts the credentials in an error message. A query that carries no secret is now recorded exactly as it was written.
