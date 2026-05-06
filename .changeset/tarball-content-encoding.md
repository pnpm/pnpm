---
"@pnpm/fetching.tarball-fetcher": patch
"pnpm": patch
---

Fixed `ERR_PNPM_BAD_TARBALL_SIZE` when a registry serves tarballs with an end-to-end `Content-Encoding` (e.g. `gzip`). Per the HTTP spec, `Content-Length` in this case refers to the encoded payload, not the decoded bytes that the fetch layer yields, so the strict size check no longer applies when the response is content-encoded [#11506](https://github.com/pnpm/pnpm/issues/11506).
