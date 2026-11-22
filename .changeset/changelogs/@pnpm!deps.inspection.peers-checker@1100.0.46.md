## 1100.0.46

### Patch Changes

- `pnpm peers check` now resolves the `catalog:` peer ranges of injected workspace packages through the workspace catalogs. With `autoInstallPeers: false`, such a peer was reported as unmet even when the installed version matched the catalog entry [#10279](https://github.com/pnpm/pnpm/issues/10279).

- Updated dependencies:
  - @pnpm/config.parse-overrides@1100.1.9
  - @pnpm/deps.path@1101.0.11
  - @pnpm/error@1100.2.3
  - @pnpm/lockfile.fs@1100.2.20
  - @pnpm/lockfile.walker@1100.0.32
  - @pnpm/types@1102.1.5
