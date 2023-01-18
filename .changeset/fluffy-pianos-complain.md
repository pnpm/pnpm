---
"@pnpm/package-requester": patch
"@pnpm/cafs": patch
"pnpm": patch
---

The store integrity check should validate the side effects cache of the installed package. If the side effects cache is broken, the package needs to be rebuilt [#4997](https://github.com/pnpm/pnpm/issues/4997).
