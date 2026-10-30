---
"@pnpm/lockfile.verification": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now resolves a local directory again after its `package.json` changes when an override or a package extension adds it to the dependencies of a registry package. `pnpm install --frozen-lockfile` fails on a lockfile that still records the old dependencies of such a directory [#17486](https://github.com/pnpm/pnpm/issues/17486).
