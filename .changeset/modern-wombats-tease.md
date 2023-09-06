---
"@pnpm/store.cafs": patch
"pnpm": patch
---

Tarballs that have hard links are now unpacked successfully. This fixes a regression introduced in v8.7, which was shipped with our new in-house tarball parser.
