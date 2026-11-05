## 1100.0.54

### Patch Changes

- `pnpm install` now links the runtime that an earlier `pnpm install --no-runtime` skipped [pnpm/pnpm#17542](https://github.com/pnpm/pnpm/issues/17542).

- Packages from the default registry keep plain lockfile keys, even when a prefix such as `npmjs` points at the registry set as `registry`. A dependency written with such a prefix shares the plain entry of the same package. The next install rewrites keys that earlier versions qualified this way [#17575](https://github.com/pnpm/pnpm/issues/17575).

- A repeat `pnpm install` now rewrites the bin shims after `extendNodePath` changes. Before, it reported "Already up to date" and the shims kept the `NODE_PATH` of the previous setting.

- Updated dependencies:
  - @pnpm/config.pick-registry-for-package@1101.0.5
