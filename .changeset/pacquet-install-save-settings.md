---
"pacquet": minor
---

The Rust engine now reads four more settings from `pnpm-workspace.yaml` and `PNPM_CONFIG_*`, instead of only accepting them as CLI flags:

- `frozenLockfile` — `pnpm install` grows a `--no-frozen-lockfile` flag so the setting can be overridden in both directions. As in pnpm, it cannot be set in the global `config.yaml`.
- `savePrefix` — the range operator `pnpm add` saves, still overridable with `--save-prefix` / `--save-exact`.
- `savePeer` — `pnpm add` also records the new dependency in `peerDependencies`. `pnpm add --no-save-peer` overrides it back off.
- `saveCatalogName` — the catalog `pnpm add` saves into.
