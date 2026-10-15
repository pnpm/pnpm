## 1103.2.5

### Patch Changes

- Removing a `link:` dependency now also removes its symlink from `node_modules` when `nodeLinker` is `hoisted` [#16884](https://github.com/pnpm/pnpm/issues/16884).

- `pnpm dedupe` now preserves a hoisted workspace package when another package uses a separately installed peer with the same name [pnpm/pnpm#16935](https://github.com/pnpm/pnpm/issues/16935).

- Sped up frozen installs of unchanged injected workspace packages that use hardlinks.

- Updated dependencies:
  - @pnpm/building.during-install@1102.2.8
  - @pnpm/building.policy@1100.1.7
  - @pnpm/deps.graph-builder@1101.1.5
  - @pnpm/exec.lifecycle@1100.1.24
  - @pnpm/installing.linking.hoist@1100.0.38
  - @pnpm/installing.package-requester@1102.2.5
