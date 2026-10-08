## 12.11.0

### Minor Changes

- `rebuild` accepts `skipIfHasSideEffectsCache`. With it set, a package whose build the side-effects cache already holds is restored from the cache, and its build scripts do not run again.

### Patch Changes

- `pnpm rebuild <pkg>` and `pnpm rebuild --pending` no longer read the manifest of every installed package to find build scripts. Only the selected packages are inspected and built. On a large `node_modules` served lazily, such as over a network or FUSE mount, this turned a rebuild of a few packages into a fetch of every package.

- `pnpm rebuild` no longer removes and recreates `node_modules` when the settings recorded in `node_modules/.modules.yaml` differ from the current configuration. The rebuild runs the build scripts against the installed packages as they are.

- The Node.js addon now sizes rayon's thread pool according to the engine thread-pool sizing policy [pnpm/tasks#54](https://github.com/pnpm/tasks/issues/54).
