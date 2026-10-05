---
"@pnpm/deps.path": patch
"pnpm": patch
"pacquet": patch
---

Fixed distinct tarball dependencies sharing a virtual store directory when their URLs differed by a literal `+` and a path separator.
