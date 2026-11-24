---
"pnpm": patch
"@pnpm/pnpr": patch
---

pnpm now refuses a package tarball whose headers npm would read differently, such as a directory entry with a nonzero size or a PAX header that sets `size` twice. Such a tarball could install different files with pnpm than with npm.

pnpm now extracts contiguous file entries in a tarball, as npm does.
