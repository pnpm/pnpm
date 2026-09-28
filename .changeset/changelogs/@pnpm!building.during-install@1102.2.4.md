## 1102.2.4

### Patch Changes

- The global virtual store and the side-effects cache now key built packages by the Node.js version that the root project's `devEngines.runtime` or `engines.runtime` pins. That is the Node.js their build scripts run with. A dependency that declares its own `engines.runtime` no longer changes the key for every other package.

- Concurrent installs that share a global virtual store now run a package's build in its shared slot one at a time. A failed build leaves the slot in place and marks it for the next install to rebuild [#15568](https://github.com/pnpm/pnpm/issues/15568).

- `engineStrict` now checks the patched `package.json` when a `patchedDependencies` entry changes `engines`. A patch that relaxes `engines.node` no longer fails the install against the published range [pnpm/pnpm#9603](https://github.com/pnpm/pnpm/issues/9603).

- `pnpm install` now removes an optional dependency from `node_modules` if its install script fails. Code that checks whether the package is installed no longer finds a package that cannot load [#8756](https://github.com/pnpm/pnpm/issues/8756).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.34
  - @pnpm/building.pkg-requires-build@1100.0.19
  - @pnpm/config.package-is-installable@1100.2.1
  - @pnpm/config.reader@1102.3.1
  - @pnpm/deps.graph-hasher@1100.3.5
  - @pnpm/deps.path@1101.0.5
  - @pnpm/error@1100.2.1
  - @pnpm/exec.lifecycle@1100.1.20
  - @pnpm/fs.hard-link-dir@1100.0.8
  - @pnpm/lockfile.types@1100.1.4
  - @pnpm/patching.apply-patch@1100.0.10
  - @pnpm/pkg-manifest.reader@1100.0.21
  - @pnpm/pnpr.client@3.1.0
  - @pnpm/store.controller-types@1101.3.2
  - @pnpm/workspace.task-scheduler@1100.0.3
