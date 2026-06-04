---
"@pnpm/pnpr.client": patch
"@pnpm/worker": patch
"pnpm": patch
---

The pnpr install accelerator now serves resolved files only in the single gzipped `POST /v1/install` response and authorizes every package whose bytes it serves against the server's access policy. The separate unauthenticated `POST /v1/files` endpoint has been removed: the client materializes the inlined files straight into its content-addressable store, and a content-addressed digest is no longer a bearer capability for a package the caller cannot read.
