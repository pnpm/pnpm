## 1101.0.6

### Patch Changes

- `pnpm outdated` and `pnpm update` now apply `minimumReleaseAge` to GitHub Actions. `minimumReleaseAgeExclude` entries match action names such as `actions/checkout` [#13923](https://github.com/pnpm/pnpm/issues/13923).

- `pnpm list --only-projects` now lists the workspace projects when `sharedWorkspaceLockfile` is `false` [#7151](https://github.com/pnpm/pnpm/issues/7151).

- `pnpm list --only-projects` now lists a workspace project that sets `publishConfig.directory`. Dependents link such a project through its publish directory, which `--only-projects` did not recognize as a project [#10635](https://github.com/pnpm/pnpm/issues/10635).

- `pnpm list --only-projects` now prints every project selected with `--filter` or `--recursive`, including a project that has no workspace dependencies [#9770](https://github.com/pnpm/pnpm/issues/9770).

  `pnpm list --only-projects` no longer reports packages in `node_modules` that are missing from the lockfile [#9528](https://github.com/pnpm/pnpm/issues/9528).

- `pnpm outdated` and `pnpm -r outdated` now fail with `ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES` when a requested package selector does not match any dependency in the inspected projects [#2319](https://github.com/pnpm/pnpm/issues/2319).

- `pnpm -r outdated --json` now includes every outdated workspace dependency when multiple projects depend on different versions or dependency types of the same package. Such a package is keyed by its current version and dependency type, for example `vue@2.7.14 (dev)` [#7693](https://github.com/pnpm/pnpm/issues/7693).

- `pnpm install --prod`, `pnpm fetch --prod` and `pnpm deploy --prod` no longer install a devDependency that is only there to satisfy an optional peer dependency of a production dependency. `pnpm list`, `pnpm why`, `pnpm licenses`, `pnpm sbom` and `pnpm audit` leave it out of `--prod` results too. The same applies to `--dev`. A peer that is not optional is still installed and audited [#15344](https://github.com/pnpm/pnpm/issues/15344).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.29
  - @pnpm/config.pick-registry-for-package@1101.0.2
  - @pnpm/config.reader@1102.3.0
  - @pnpm/deps.github-actions@1100.1.11
  - @pnpm/deps.inspection.list@1101.0.6
  - @pnpm/deps.inspection.outdated@1100.1.32
  - @pnpm/deps.inspection.peers-checker@1100.0.35
  - @pnpm/deps.inspection.peers-issues-renderer@1100.0.15
  - @pnpm/error@1100.2.0
  - @pnpm/global.commands@1102.0.3
  - @pnpm/global.packages@1101.1.4
  - @pnpm/installing.modules-yaml@1101.0.4
  - @pnpm/lockfile.fs@1100.2.9
  - @pnpm/network.auth-header@1101.1.14
  - @pnpm/network.fetch@1100.1.18
  - @pnpm/resolving.default-resolver@1101.0.6
  - @pnpm/resolving.npm-resolver@1104.2.1
  - @pnpm/resolving.registry.types@1100.2.1
  - @pnpm/store.path@1100.0.8
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.project-manifest-reader@1100.1.0
