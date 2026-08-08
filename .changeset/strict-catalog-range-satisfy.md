---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

When `catalogMode: strict` is enabled, permit installing or updating to concrete versions that satisfy a range catalog specifier rather than rejecting them with a mismatch error [pnpm/pnpm#13715](https://github.com/pnpm/pnpm/issues/13715).
