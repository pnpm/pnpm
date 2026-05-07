---
"@pnpm/fetching.tarball-fetcher": patch
"pnpm": patch
---

Fixed `ERR_PNPM_BAD_TARBALL_SIZE` when a registry serves tarballs with an end-to-end `Content-Encoding` (e.g. `gzip`). Tarballs are already compressed, so the fetcher now requests them with `Accept-Encoding: identity` (matching pnpm v10's effective behavior) and, as defense in depth against misbehaving servers, no longer enforces the strict `Content-Length` check when the response declares a `Content-Encoding` — `Content-Length` in that case refers to the encoded payload, not the decoded bytes the fetch implementation yields [#11506](https://github.com/pnpm/pnpm/issues/11506).
