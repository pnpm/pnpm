---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm pack` now accepts `--ignore-pnpmfile` and `--ignore-scripts`, and `pnpm publish` accepts `--ignore-pnpmfile`. With these flags, a package can be packed without running the project's pnpmfile or lifecycle scripts [#18132](https://github.com/pnpm/pnpm/issues/18132).
