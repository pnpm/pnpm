---
"pacquet": patch
---

Redact credentials and sensitive query parameters from the URL printed when `pnpm docs` cannot open a browser. Previously, the fallback removed credentials but left sensitive query parameters visible.
