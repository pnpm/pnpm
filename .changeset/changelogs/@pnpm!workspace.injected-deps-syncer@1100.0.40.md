## 1100.0.40

### Patch Changes

- `pnpm install` no longer fails for an injected workspace dependency whose package publishes from a `publishConfig.directory` that its own `prepare` script builds. The injected copy now picks up that directory once `prepare` finishes building it. `pnpm install --frozen-lockfile` no longer reports the dependency as outdated while the directory has not been built yet. [pnpm/pnpm#7811](https://github.com/pnpm/pnpm/issues/7811)

- With `sharedWorkspaceLockfile: false`, an injected workspace package that has lifecycle scripts is now hard linked into the projects that depend on it. Before, pnpm left a plain copy, so later edits to the package did not reach those projects [#9828](https://github.com/pnpm/pnpm/issues/9828).

- Scripts listed in `syncInjectedDepsAfterScripts` now update injected dependencies while they run. A watcher on the injected package, such as a dev server, sees each change before the script exits [pnpm/pnpm#4410](https://github.com/pnpm/pnpm/issues/4410).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.34
  - @pnpm/bins.remover@1100.0.26
  - @pnpm/error@1100.2.1
  - @pnpm/fetching.directory-fetcher@1100.0.36
  - @pnpm/fs.graceful-fs@1100.2.4
  - @pnpm/installing.modules-yaml@1101.0.5
  - @pnpm/pkg-manifest.reader@1100.0.21
  - @pnpm/workspace.projects-reader@1101.1.1
