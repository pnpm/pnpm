## 1101.1.17

### Patch Changes

- Tab completion suggests project scripts when you type `pnpm` followed by a script name [#18239](https://github.com/pnpm/pnpm/issues/18239).

- `pnpm dlx`, `pnx`, and `pnpm create` now handle local isolation, script skipping, and non-registry package specifiers consistently:

  - `pnpm` ignores local `.pnpmfile.cjs` hooks and project `.npmrc` proxy or TLS settings in the working directory.
  - Running with `--ignore-scripts` or `ignoreScripts: true` uses a separate `dlx` cache key, keeping cached installs that skipped build scripts separate from normal runs.
  - Relative local directory and tarball specifiers such as `./my-cli` and `./my-cli.tgz` resolve against the caller's working directory.
  - Explicitly requested tarball URL, `file:`, and git URL packages run their build scripts by default unless `--ignore-scripts` or a negated `--allow-build` selector disables them.

- `pnpm run` and `pnpm exec` now find the Plug'n'Play loader and experimental package map when `lockfileDir` is configured. This also applies to recursive commands.

- Added the `--minimum-release-age` and `--minimum-release-age-exclude` options to `pnpm install`, `pnpm add`, `pnpm update`, and `pnpm dlx`. They set `minimumReleaseAge` and `minimumReleaseAgeExclude` for a single run without editing a config file [#11224](https://github.com/pnpm/pnpm/issues/11224).

- The workspace project graph now follows `overrides`. An override that points a dependency at a workspace package, such as `foo: workspace:*`, makes that package a dependency of every project whose declaration the override matches, even with `linkWorkspacePackages` off. `pnpm install` now runs that package's lifecycle scripts before the scripts of the projects depending on it. `pnpm -r run` orders the projects the same way. `--filter` selectors that follow dependencies now see the edge.

- `pnpm run` and `pnpm exec` no longer trigger a reinstall solely because the last install used `--trust-lockfile` [#18218](https://github.com/pnpm/pnpm/issues/18218).

- A `[<since>]` filter such as `--filter "...[origin/main]"` now selects a project whose dependency an `overrides` entry points at a changed catalog entry. For example, with `lib: catalog:local` in `overrides`, changing `lib` in the `local` catalog selects every project that depends on `lib`.

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.22
  - @pnpm/building.commands@1101.2.17
  - @pnpm/cli.common-cli-options-help@1100.0.4
  - @pnpm/cli.utils@1101.0.37
  - @pnpm/config.parse-overrides@1100.1.9
  - @pnpm/config.reader@1102.4.0
  - @pnpm/config.version-policy@1100.2.12
  - @pnpm/core-loggers@1101.0.6
  - @pnpm/crypto.hash@1100.0.10
  - @pnpm/deps.status@1100.1.37
  - @pnpm/engine.runtime.commands@1101.1.16
  - @pnpm/engine.runtime.system-version@1100.0.17
  - @pnpm/error@1100.2.3
  - @pnpm/exec.esm-node-path-loader@1100.0.3
  - @pnpm/exec.lifecycle@1100.1.31
  - @pnpm/exec.npm-lifecycle@1100.0.10
  - @pnpm/exec.pnpm-cli-runner@1100.0.9
  - @pnpm/fs.dir-lock@1100.0.2
  - @pnpm/installing.client@1100.3.23
  - @pnpm/installing.commands@1101.4.13
  - @pnpm/pkg-manifest.reader@1100.0.26
  - @pnpm/store.path@1100.1.4
  - @pnpm/types@1102.1.5
  - @pnpm/workspace.injected-deps-syncer@1100.0.51
  - @pnpm/workspace.project-manifest-reader@1100.1.8
  - @pnpm/workspace.projects-filter@1100.0.56
  - @pnpm/workspace.projects-graph@1100.0.50
  - @pnpm/workspace.projects-reader@1101.1.9
  - @pnpm/workspace.projects-sorter@1101.0.6
  - @pnpm/workspace.task-scheduler@1100.0.9
