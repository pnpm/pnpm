---
"@pnpm/pnpr": minor
---

A hosted registry that declares `tarballs: tar` now stores each published version as one tar compressed with zstd at level 19, and no `.tgz`. A client that accepts zstd gets those bytes, so even its first download is at level 19. Any other client gets the tar decoded, gzip-encoded if it asks for gzip.

pnpr now refuses to start when a router without `tarballs: tar` lists such a registry as a source. Versions that an earlier pnpr published to such a registry are no longer served. Publish them again.
