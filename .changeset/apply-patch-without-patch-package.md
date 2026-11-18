---
"@pnpm/patching.apply-patch": patch
"pnpm": patch
---

pnpm no longer stalls on a patch file whose `diff --git` line contains a long run of spaces. `@pnpm/patching.apply-patch` no longer depends on `@pnpm/patch-package`.
