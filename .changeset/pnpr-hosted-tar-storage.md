---
"@pnpm/pnpr": minor
---

A hosted registry that declares `tarballs: tar` now stores each published version only as its tar, compressed with zstd at level 19, and keeps no `.tgz`. The first download of a version is served at level 19, and the store holds one file per version.

pnpr refuses to start when a router without `tarballs: tar` lists such a registry as a source. Versions published to such a registry by an earlier pnpr are not served. Publish them again.
