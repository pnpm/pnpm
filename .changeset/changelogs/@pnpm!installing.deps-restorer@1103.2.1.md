## 1103.2.1

### Patch Changes

- `pnpm install --dev` and `pnpm fetch --dev` now install the optional dependencies of devDependencies, such as the platform binaries of Biome and oxlint. The project's own `optionalDependencies` are still skipped [#9678](https://github.com/pnpm/pnpm/issues/9678).

- The global virtual store and the side-effects cache now key built packages by the Node.js version that the root project's `devEngines.runtime` or `engines.runtime` pins. That is the Node.js their build scripts run with. A dependency that declares its own `engines.runtime` no longer changes the key for every other package.

- `pnpm install` now restores cached build artifacts when reinstalling a workspace that uses separate lockfiles [#12942](https://github.com/pnpm/pnpm/issues/12942).

- Concurrent installs that share a global virtual store now run a package's build in its shared slot one at a time. A failed build leaves the slot in place and marks it for the next install to rebuild [#15568](https://github.com/pnpm/pnpm/issues/15568).

- `pnpm install` no longer fails for an injected workspace dependency whose package publishes from a `publishConfig.directory` that its own `prepare` script builds. The injected copy now picks up that directory once `prepare` finishes building it. `pnpm install --frozen-lockfile` no longer reports the dependency as outdated while the directory has not been built yet. [pnpm/pnpm#7811](https://github.com/pnpm/pnpm/issues/7811)

- With `sharedWorkspaceLockfile: false`, an injected workspace package that has lifecycle scripts is now hard linked into the projects that depend on it. Before, pnpm left a plain copy, so later edits to the package did not reach those projects [#9828](https://github.com/pnpm/pnpm/issues/9828).

- `pnpm install` no longer fails when a package from the registry declares a `file:` dependency on a directory inside itself, such as `"@types/css-tree": "file:./typings/css-tree"`. pnpm links that dependency to the directory inside the package, as npm and Yarn do. The lockfile records it as `link:<root>/typings/css-tree` [#9141](https://github.com/pnpm/pnpm/issues/9141).

- `pnpm install` now links the executables of auto-installed peer dependencies into the workspace root's `node_modules/.bin`, including after a frozen-lockfile reinstall [#8511](https://github.com/pnpm/pnpm/issues/8511).

- Under `nodeLinker: hoisted`, `pnpm install` now clears orphaned package directories that an interrupted or failed install leaves in a project's `node_modules`. A directory recorded by the previous install is removed, while an unrecorded directory is moved to `node_modules/.ignored`. A copy already in `.ignored` is never overwritten [pnpm/pnpm#13676](https://github.com/pnpm/pnpm/issues/13676).

- `pnpm install` no longer creates a `node_modules` symlink inside the `publishConfig.directory` of a workspace package linked with `linkDirectory`. A build tool that cleaned its output directory through that symlink deleted the files of the package's dependencies. `pnpm install` also removes a symlink that an earlier install left there [#16226](https://github.com/pnpm/pnpm/issues/16226).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.34
  - @pnpm/bins.remover@1100.0.26
  - @pnpm/building.during-install@1102.2.4
  - @pnpm/building.policy@1100.1.4
  - @pnpm/config.package-is-installable@1100.2.1
  - @pnpm/deps.graph-builder@1101.1.1
  - @pnpm/deps.graph-hasher@1100.3.5
  - @pnpm/deps.path@1101.0.5
  - @pnpm/error@1100.2.1
  - @pnpm/exec.lifecycle@1100.1.20
  - @pnpm/fs.read-modules-dir@1100.0.3
  - @pnpm/fs.symlink-dependency@1100.0.22
  - @pnpm/installing.linking.direct-dep-linker@1100.0.22
  - @pnpm/installing.linking.hoist@1100.0.34
  - @pnpm/installing.linking.modules-cleaner@1100.1.26
  - @pnpm/installing.linking.real-hoist@1100.1.21
  - @pnpm/installing.modules-yaml@1101.0.5
  - @pnpm/installing.package-requester@1102.2.1
  - @pnpm/lockfile.filtering@1100.2.10
  - @pnpm/lockfile.fs@1100.2.10
  - @pnpm/lockfile.to-pnp@1101.0.7
  - @pnpm/lockfile.utils@1102.1.5
  - @pnpm/patching.config@1100.1.8
  - @pnpm/pkg-manifest.reader@1100.0.21
  - @pnpm/pnpr.client@3.1.0
  - @pnpm/store.controller-types@1101.3.2
  - @pnpm/workspace.project-manifest-reader@1100.1.1
