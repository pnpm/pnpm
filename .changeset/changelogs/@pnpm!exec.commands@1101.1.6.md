## 1101.1.6

### Patch Changes

- `pnpm run` and `pnpm exec` no longer install a project that has never been installed and has nothing to install. Such a project declares no dependencies, no peer dependencies that `autoInstallPeers` would fetch, and no install lifecycle scripts. The command now runs without writing `node_modules` or `pnpm-lock.yaml` [#16313](https://github.com/pnpm/pnpm/issues/16313).

- Updated dependencies:
  - @pnpm/building.commands@1101.2.6
  - @pnpm/deps.status@1100.1.26
  - @pnpm/installing.commands@1101.4.2
