---
"@pnpm/fetching.tarball-fetcher": patch
"pnpm": patch
"pacquet": patch
---

Archive download retry logs hide credentials and signed query parameters in URLs. Rust pnpm also removes request URLs from nested network errors.
