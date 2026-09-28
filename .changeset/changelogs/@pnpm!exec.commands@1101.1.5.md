## 1101.1.5

### Patch Changes

- Scripts listed in `syncInjectedDepsAfterScripts` now update injected dependencies while they run. A watcher on the injected package, such as a dev server, sees each change before the script exits [pnpm/pnpm#4410](https://github.com/pnpm/pnpm/issues/4410).

- `pnpm run` and `pnpm exec` no longer install dependencies automatically when the root `package.json` still keeps `overrides`, `packageExtensions`, `patchedDependencies`, or `ignoredOptionalDependencies` in its `pnpm` field. pnpm no longer reads that field, so the install rewrote the lockfile without those settings. The command now fails and asks to move the settings to `pnpm-workspace.yaml` [#16278](https://github.com/pnpm/pnpm/issues/16278).

- `pnpm -r run /regexp/` now honors the `tasks` `dependsOn` declared for each script the selector matches, like running the script by name does. Matched scripts that depend on each other run in order. Each matched script runs once [#15596](https://github.com/pnpm/pnpm/issues/15596).

- `pnpm run` exits with the code of a script that handles Ctrl+C and shuts down. A script that finished cleanly is not reported as a lifecycle failure. The commands after it in the same script still run [pnpm/pnpm#9945](https://github.com/pnpm/pnpm/issues/9945).

- Published the cross-process directory lock as `@pnpm/fs.dir-lock` [#15568](https://github.com/pnpm/pnpm/issues/15568).

- When `verifyDepsBeforeRun` triggers an install before a filtered `pnpm run` or `pnpm exec`, pnpm now installs only the selected projects and their dependencies. A later filtered command also installs a selected project that an earlier filtered install skipped [#11865](https://github.com/pnpm/pnpm/issues/11865).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/building.commands@1101.2.5
  - @pnpm/catalogs.resolver@1100.1.1
  - @pnpm/cli.utils@1101.0.30
  - @pnpm/config.reader@1102.3.1
  - @pnpm/config.version-policy@1100.2.5
  - @pnpm/crypto.hash@1100.0.7
  - @pnpm/deps.status@1100.1.25
  - @pnpm/engine.runtime.commands@1101.1.5
  - @pnpm/error@1100.2.1
  - @pnpm/exec.lifecycle@1100.1.20
  - @pnpm/exec.npm-lifecycle@1100.0.2
  - @pnpm/installing.client@1100.3.12
  - @pnpm/installing.commands@1101.4.1
  - @pnpm/pkg-manifest.reader@1100.0.21
  - @pnpm/resolving.parse-wanted-dependency@1100.0.3
  - @pnpm/store.path@1100.1.0
  - @pnpm/workspace.injected-deps-syncer@1100.0.40
  - @pnpm/workspace.project-manifest-reader@1100.1.1
  - @pnpm/workspace.task-scheduler@1100.0.3
