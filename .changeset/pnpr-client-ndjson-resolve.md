---
"@pnpm/pnpr.client": patch
"pnpm": patch
---

The pnpr client now reads the `POST /v1/resolve` response as an `application/x-ndjson` stream, matching the server's streaming protocol [#12234](https://github.com/pnpm/pnpm/issues/12234). It parses the terminal `done` / `error` / `violations` frame instead of expecting a single buffered JSON object.
