---
"@pnpm/npm-resolver": patch
"pnpm": patch
---

Normalize the tarball URLs before saving them to the lockfile. URLs should not contain default ports, like :80 for http and :443 for https [#10273](https://github.com/pnpm/pnpm/pull/10273).
