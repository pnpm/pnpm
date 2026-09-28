## 1103.0.7

### Patch Changes

- The global virtual store and the side-effects cache now key built packages by the Node.js version that the root project's `devEngines.runtime` or `engines.runtime` pins. That is the Node.js their build scripts run with. A dependency that declares its own `engines.runtime` no longer changes the key for every other package.

- Fixed `pnpm rebuild` modifying packages shared with projects that have not approved their build scripts when using the global virtual store.

- `pnpm rebuild` with `nodeLinker: hoisted` no longer puts one package's parent `node_modules/.bin` directories on the `PATH` of the packages it builds after it.

- `pnpm install` now removes an optional dependency from `node_modules` if its install script fails. Code that checks whether the package is installed no longer finds a package that cannot load [#8756](https://github.com/pnpm/pnpm/issues/8756).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.34
  - @pnpm/building.pkg-requires-build@1100.0.19
  - @pnpm/building.policy@1100.1.4
  - @pnpm/config.reader@1102.3.1
  - @pnpm/deps.graph-hasher@1100.3.5
  - @pnpm/deps.path@1101.0.5
  - @pnpm/error@1100.2.1
  - @pnpm/exec.lifecycle@1100.1.20
  - @pnpm/fs.symlink-dependency@1100.0.22
  - @pnpm/installing.context@1101.0.7
  - @pnpm/installing.deps-restorer@1103.2.1
  - @pnpm/installing.modules-yaml@1101.0.5
  - @pnpm/lockfile.settings-checker@1100.2.9
  - @pnpm/lockfile.types@1100.1.4
  - @pnpm/lockfile.utils@1102.1.5
  - @pnpm/lockfile.walker@1100.0.26
  - @pnpm/patching.config@1100.1.8
  - @pnpm/pkg-manifest.reader@1100.0.21
  - @pnpm/store.cafs@1100.3.5
  - @pnpm/store.connection-manager@1101.3.0
  - @pnpm/store.controller-types@1101.3.2
  - @pnpm/store.index@1100.3.3
  - @pnpm/workspace.task-scheduler@1100.0.3
