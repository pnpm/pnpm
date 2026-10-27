---
"@pnpm/pnpr": minor
---

A hosted registry that declares `tarballs: tar` now serves the first download of each version at zstd level 19. It keeps one file per version and no `.tgz`.

pnpr now refuses to start when a router without `tarballs: tar` lists such a registry as a source. Versions that an earlier pnpr published to such a registry are no longer served. Publish them again.
