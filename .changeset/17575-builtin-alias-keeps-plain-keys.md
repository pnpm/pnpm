---
"@pnpm/config.pick-registry-for-package": patch
"pnpm": patch
"pacquet": patch
---

Pointing the built-in `npmjs:` or `gh:` alias at another URL, such as the npm mirror set as `registry`, no longer adds that prefix to every lockfile key of the packages resolved from that URL. Only dependencies written with the alias get qualified keys [#17575](https://github.com/pnpm/pnpm/issues/17575).
