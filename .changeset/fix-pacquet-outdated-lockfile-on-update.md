---
"pnpm": patch
---

Fixed `pnpm up` (and `pnpm add` / `pnpm remove`) failing with `pacquet_package_manager::outdated_lockfile` when pacquet is declared in `configDependencies`. pnpm now passes `--ignore-manifest-check` to pacquet so its `--frozen-lockfile` check doesn't fire against the (pre-mutation) `package.json` pnpm hasn't written yet [#11797](https://github.com/pnpm/pnpm/issues/11797). Requires a pacquet release that supports the flag — bump `PACQUET_VERSION` in the e2e tests once it ships.
