---
"pacquet": patch
---

`pnpm install` sends fewer registry metadata requests when the lockfile already decides which version a range resolves to. This now also covers ranges that several locked versions satisfy when one of them outranks the others, and direct dependencies kept at their locked version. Packages that `minimumReleaseAgeExclude` lists without a version now reuse cached registry metadata the same way they do when `minimumReleaseAge` is not set [#16458](https://github.com/pnpm/pnpm/issues/16458).
