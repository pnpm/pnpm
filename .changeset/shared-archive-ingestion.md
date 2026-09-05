---
"pacquet": patch
---

Cargo registry metadata requests now respect pnpm's configured fetch retry budget. Python registry requests now retry HTTP 408 responses and interrupted response bodies.

Package tarballs, Cargo crates, Python wheels and runtime ZIP archives share cache validation, integrity verification, extraction retries and store publication policies.
