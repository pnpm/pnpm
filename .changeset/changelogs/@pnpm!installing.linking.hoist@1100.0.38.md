## 1100.0.38

### Patch Changes

- `pnpm dedupe` now preserves a hoisted workspace package when another package uses a separately installed peer with the same name [pnpm/pnpm#16935](https://github.com/pnpm/pnpm/issues/16935).

- Fixed `pnpm install` failing on Windows with `EPERM` on `rmdir node_modules/@scope` when several workspace packages under one scope are hoisted, for example with `nodeLinker: hoisted` [#16690](https://github.com/pnpm/pnpm/issues/16690).
