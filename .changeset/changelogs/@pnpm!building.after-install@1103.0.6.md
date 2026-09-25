## 1103.0.6

### Patch Changes

- Tools installed in a custom `modulesDir` can load CommonJS plugins installed there, the same way they would from `node_modules`. When executables are symlinks, as with `preferSymlinkedExecutables` or the hoisted linker, this works for `pnpm run`, `pnpm exec`, `pnpm version` hooks, and the lifecycle scripts a project runs during install. A symlinked tool started directly from a shell does not get it. Paths containing the platform path-list separator do not receive this fallback. `extendNodePath: false` disables this fallback [#3604](https://github.com/pnpm/pnpm/issues/3604).

- `pnpm install --prod`, `pnpm fetch --prod` and `pnpm deploy --prod` no longer install a devDependency that is only there to satisfy an optional peer dependency of a production dependency. `pnpm list`, `pnpm why`, `pnpm licenses`, `pnpm sbom` and `pnpm audit` leave it out of `--prod` results too. The same applies to `--dev`. A peer that is not optional is still installed and audited [#15344](https://github.com/pnpm/pnpm/issues/15344).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.33
  - @pnpm/building.pkg-requires-build@1100.0.18
  - @pnpm/building.policy@1100.1.3
  - @pnpm/config.normalize-registries@1101.0.2
  - @pnpm/config.reader@1102.3.0
  - @pnpm/core-loggers@1101.0.1
  - @pnpm/deps.graph-hasher@1100.3.4
  - @pnpm/deps.path@1101.0.4
  - @pnpm/error@1100.2.0
  - @pnpm/exec.lifecycle@1100.1.19
  - @pnpm/fs.symlink-dependency@1100.0.21
  - @pnpm/installing.context@1101.0.6
  - @pnpm/installing.modules-yaml@1101.0.4
  - @pnpm/lockfile.types@1100.1.3
  - @pnpm/lockfile.utils@1102.1.4
  - @pnpm/lockfile.walker@1100.0.25
  - @pnpm/pkg-manifest.reader@1100.0.20
  - @pnpm/store.cafs@1100.3.4
  - @pnpm/store.connection-manager@1101.2.0
  - @pnpm/store.controller-types@1101.3.1
  - @pnpm/store.index@1100.3.2
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.task-scheduler@1100.0.2
