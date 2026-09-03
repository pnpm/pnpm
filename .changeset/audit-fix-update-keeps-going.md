---
"pacquet": patch
---

`pnpm audit --fix update` no longer aborts when a vulnerable package has no safe version inside its declared range [#14508](https://github.com/pnpm/pnpm/issues/14508). The run updates every package it can and lists the rest as remaining.
