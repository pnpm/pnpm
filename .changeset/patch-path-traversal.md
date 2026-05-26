---
"@pnpm/patching.apply-patch": patch
"pnpm": patch
---

Reject patch files whose `diff --git` headers reference paths outside the patched package directory. Previously a malicious `.patch` file added via a pull request could write, delete, or rename arbitrary files reachable by the user running `pnpm install`.
