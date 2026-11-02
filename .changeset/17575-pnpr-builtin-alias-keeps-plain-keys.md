---
"@pnpm/pnpr": patch
---

A resolve request that points the built-in `npmjs:` or `gh:` alias at another URL, such as the npm mirror set as `registry`, no longer gets that prefix on every lockfile key of the packages resolved from that URL. Only dependencies written with the alias get qualified keys [#17575](https://github.com/pnpm/pnpm/issues/17575).
