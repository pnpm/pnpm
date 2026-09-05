---
"@pnpm/fetching.tarball-fetcher": patch
"pnpm": patch
"pacquet": patch
---

Archive download retry logs hide credentials and signed query parameters in URLs. In pnpm v12, nested network errors omit request URLs.
