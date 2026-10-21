## 1100.0.2

### Patch Changes

- With `enableGlobalVirtualStore`, a Node.js resolve hook that a script registers now sees the importer as `context.parentURL` after pnpm's `NODE_PATH` fallback fails to find a bare specifier. Hooks that resolve tsconfig `paths` aliases from the importer failed with `ERR_MODULE_NOT_FOUND` [#16979](https://github.com/pnpm/pnpm/issues/16979).
