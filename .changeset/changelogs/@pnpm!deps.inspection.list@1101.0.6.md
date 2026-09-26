## 1101.0.6

### Patch Changes

- `pnpm list --only-projects` now lists the workspace projects when `sharedWorkspaceLockfile` is `false` [#7151](https://github.com/pnpm/pnpm/issues/7151).

- `pnpm list --only-projects` now lists a workspace project that sets `publishConfig.directory`. Dependents link such a project through its publish directory, which `--only-projects` did not recognize as a project [#10635](https://github.com/pnpm/pnpm/issues/10635).

- `pnpm install --prod`, `pnpm fetch --prod` and `pnpm deploy --prod` no longer install a devDependency that is only there to satisfy an optional peer dependency of a production dependency. `pnpm list`, `pnpm why`, `pnpm licenses`, `pnpm sbom` and `pnpm audit` leave it out of `--prod` results too. The same applies to `--dev`. A peer that is not optional is still installed and audited [#15344](https://github.com/pnpm/pnpm/issues/15344).

- Updated dependencies:
  - @pnpm/deps.inspection.tree-builder@1101.0.6
  - @pnpm/lockfile.fs@1100.2.9
  - @pnpm/pkg-manifest.reader@1100.0.20
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.project-manifest-reader@1100.1.0
