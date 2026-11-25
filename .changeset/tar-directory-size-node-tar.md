---
"@pnpm/store.cafs": patch
"pnpm": patch
---

pnpm now reads a directory entry in a package tarball as having no content, as npm does. A tarball whose directory entry declared a nonzero size could install different files with pnpm than with npm. A regular file entry whose path ends in `/` is now also read as a directory, as npm reads it.
