---
"pacquet": minor
---

`pnpm publish` now supports `--new-version <version>`, which sets the version in `package.json` before publishing. With `--recursive`, every selected workspace package is set to the new version [#2168](https://github.com/pnpm/pnpm/issues/2168).
