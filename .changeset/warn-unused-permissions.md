---
"pnpm": minor
---

`pnpm install` warns when a locally declared package permission matches no dependency during a full resolution. The permission stays saved for future installs. Set `permissions.unused` to `ignore` alongside `permissions.rules` to turn off the warning [#18429](https://github.com/pnpm/pnpm/issues/18429).

`overrides.rules` accepts override selectors alongside `overrides.unused` (`warn` or `ignore`). `catalog.entries` accepts default catalog entries alongside `catalog.unused` (`ignore` or `prune`). `catalog.mode` and `catalog.defaultSaveTarget` configure how `pnpm add` uses catalogs and where it saves new entries. The existing map forms and flat settings remain supported.
