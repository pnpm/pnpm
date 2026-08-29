## 1100.0.4

### Patch Changes

- Copying a built package to its other hoisted locations no longer replaces the destination directory. With `nodeLinker: hoisted`, that replacement deleted the dependencies nested inside the destination's `node_modules`, and made concurrent copies of the same build chunk fail with `ERR_PNPM_ENOENT: no such file or directory, rename '.../node_modules/_tmp_...'` [#12880](https://github.com/pnpm/pnpm/issues/12880).

- Updated dependencies:
  - @pnpm/fs.graceful-fs@1100.2.0
