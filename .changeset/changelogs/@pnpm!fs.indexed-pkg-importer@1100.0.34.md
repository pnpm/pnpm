## 1100.0.34

### Patch Changes

- `pnpm install` hardlinks files from a group-writable or world-writable store again. Before this fix, pnpm copied every file from such a store into `node_modules` [#16677](https://github.com/pnpm/pnpm/issues/16677).

- Sped up frozen installs of unchanged injected workspace packages that use hardlinks.

- The warning about renamed package files with invalid names no longer prints control characters from those file names.
