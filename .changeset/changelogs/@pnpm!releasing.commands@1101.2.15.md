## 1101.2.15

### Patch Changes

- `pnpm lane <lane> --filter <selector>` now rejects partial moves of a `versioning.fixed` group with `ERR_PNPM_VERSIONING_CONFLICTING_CONFIG` before writing `pnpm-workspace.yaml`.

- `pnpm pack` now accepts `--ignore-pnpmfile` and `--ignore-scripts`, and `pnpm publish` accepts `--ignore-pnpmfile`. With these flags, a package can be packed without running the project's pnpmfile or lifecycle scripts [#18132](https://github.com/pnpm/pnpm/issues/18132).

- `pnpm -r pack --out` now expands `%s` and `%v` placeholders only in the `--out` template. Literal `%s` and `%v` segments in the working directory path stay unchanged.

- `pnpm -C <dir> publish <tarball|folder>` and `pnpm --dir <dir> publish <tarball|folder>` now resolve relative tarball and directory arguments against `<dir>`. Git checks during `pnpm publish` also inspect the target directory.

- `pnpm version -r` now runs each bumped package's `preversion`, `version`, and `postversion` scripts unless `--ignore-scripts` or `--dry-run` is set. `pnpm version -r --dry-run --json` now prints the planned releases as a JSON array.

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.21
  - @pnpm/building.policy@1100.1.11
  - @pnpm/cli.utils@1101.0.36
  - @pnpm/config.pick-registry-for-package@1101.0.6
  - @pnpm/config.reader@1102.3.10
  - @pnpm/deps.path@1101.0.10
  - @pnpm/engine.runtime.commands@1101.1.15
  - @pnpm/engine.runtime.node-resolver@1101.3.14
  - @pnpm/exec.lifecycle@1100.1.30
  - @pnpm/exec.pnpm-cli-runner@1100.0.8
  - @pnpm/fetching.directory-fetcher@1100.0.45
  - @pnpm/fs.indexed-pkg-importer@1100.0.40
  - @pnpm/fs.packlist@1100.0.14
  - @pnpm/installing.client@1100.3.22
  - @pnpm/installing.commands@1101.4.12
  - @pnpm/installing.deps-resolver@1102.2.14
  - @pnpm/lockfile.fs@1100.2.19
  - @pnpm/lockfile.peer-edges@1100.0.6
  - @pnpm/lockfile.types@1100.1.8
  - @pnpm/network.auth-header@1101.1.19
  - @pnpm/network.fetch@1100.1.26
  - @pnpm/releasing.exportable-manifest@1100.3.12
  - @pnpm/releasing.versioning@1100.3.10
  - @pnpm/resolving.npm-resolver@1104.2.11
  - @pnpm/resolving.registry.types@1100.2.6
  - @pnpm/resolving.resolver-base@1101.3.6
  - @pnpm/types@1102.1.4
  - @pnpm/workspace.projects-filter@1100.0.55
  - @pnpm/workspace.projects-graph@1100.0.49
  - @pnpm/workspace.projects-sorter@1101.0.5
  - @pnpm/workspace.task-scheduler@1100.0.8
  - @pnpm/workspace.workspace-manifest-writer@1100.2.12
