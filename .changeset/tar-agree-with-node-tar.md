---
"pnpm": patch
"@pnpm/pnpr": patch
---

pnpm now reads every package tarball the way npm does, or refuses it. A tarball whose headers npm could read differently, such as one with a malformed PAX header, could install different files with pnpm than with npm. A directory entry with a nonzero size and a PAX header that sets one key twice are now read as npm reads them.

pnpm now extracts contiguous file entries in a tarball, as npm does.
