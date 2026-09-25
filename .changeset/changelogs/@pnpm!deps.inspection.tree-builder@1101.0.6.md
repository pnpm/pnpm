## 1101.0.6

### Patch Changes

- `pnpm list --only-projects` now lists the workspace projects when `sharedWorkspaceLockfile` is `false` [#7151](https://github.com/pnpm/pnpm/issues/7151).

- `pnpm list --only-projects` now lists a workspace project that sets `publishConfig.directory`. Dependents link such a project through its publish directory, which `--only-projects` did not recognize as a project [#10635](https://github.com/pnpm/pnpm/issues/10635).

- `pnpm list --only-projects` now prints every project selected with `--filter` or `--recursive`, including a project that has no workspace dependencies [#9770](https://github.com/pnpm/pnpm/issues/9770).

  `pnpm list --only-projects` no longer reports packages in `node_modules` that are missing from the lockfile [#9528](https://github.com/pnpm/pnpm/issues/9528).

- `pnpm install --prod`, `pnpm fetch --prod` and `pnpm deploy --prod` no longer install a devDependency that is only there to satisfy an optional peer dependency of a production dependency. `pnpm list`, `pnpm why`, `pnpm licenses`, `pnpm sbom` and `pnpm audit` leave it out of `--prod` results too. The same applies to `--dev`. A peer that is not optional is still installed and audited [#15344](https://github.com/pnpm/pnpm/issues/15344).

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.2
  - @pnpm/deps.path@1101.0.4
  - @pnpm/installing.modules-yaml@1101.0.4
  - @pnpm/lockfile.detect-dep-types@1100.0.25
  - @pnpm/lockfile.fs@1100.2.9
  - @pnpm/lockfile.utils@1102.1.4
  - @pnpm/pkg-manifest.reader@1100.0.20
  - @pnpm/store.cafs@1100.3.4
  - @pnpm/store.index@1100.3.2
  - @pnpm/types@1102.1.1
