---
"@pnpm/lockfile.verification": patch
"pnpm": patch
"pacquet": patch
---

Fixed frozen installs incorrectly treating equivalent Git dependency specifiers as a stale lockfile. See [#13039](https://github.com/pnpm/pnpm/issues/13039).
