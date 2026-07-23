---
"pacquet": patch
---

`engineStrict` now fails the install when an incompatible package is reached through a regular dependency edge of an installable package, even if the package is also optionally reachable — matching pnpm. Packages reachable only through optional edges or skipped parents are still skipped [#13143](https://github.com/pnpm/pnpm/issues/13143).
