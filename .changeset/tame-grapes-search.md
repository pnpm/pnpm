---
"@pnpm/fetch": patch
"pnpm": patch
---

HTTP requests should be retried when the server responds with on of 408, 409, 420, 429 status codes.
