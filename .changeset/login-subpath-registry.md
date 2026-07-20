---
"pnpm": patch
---

Fixed `pnpm login`, `pnpm adduser`, and `pnpm logout` against a registry hosted under a URL subpath (e.g. `https://example.com/npm/registry`) when the configured URL has no trailing slash. Such URLs were left unnormalized, so the last path segment was dropped when building the login and token endpoints and the auth token was stored under a truncated key. Registry URLs with a path now always get a trailing slash appended during normalization, matching how root-level registry URLs are handled.
