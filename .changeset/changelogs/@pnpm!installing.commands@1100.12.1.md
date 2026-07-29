## 1100.12.1

### Patch Changes

- Strip Unicode formatting characters from registry- and manifest-derived terminal output.

- Checking GitHub Actions dependencies for updates is now opt-in for every command. Neither `pnpm outdated` nor `pnpm update` reads the workflow files unless `--include-github-actions` is passed or `update.githubActions` is set to `true` in `pnpm-workspace.yaml`. Reading them runs `git ls-remote` against every referenced repository, which fails in environments where GitHub is not reachable the way pnpm assumes (a GitHub Enterprise Server, a custom certificate authority, or an offline network) [#13254](https://github.com/pnpm/pnpm/issues/13254).

  `pnpm outdated` accepts the `--include-github-actions` option too.

- `pnpm update --interactive` now measures its table in terminal columns rather than in characters. A package name, workspace name, or version containing wide characters (CJK, most emoji) no longer knocks its row's columns out of line with the rest of the group, and a wide character in a version no longer aborts the command with `Subject parameter value width cannot be greater than the container width` [#13357](https://github.com/pnpm/pnpm/issues/13357).

- The `Workspace` column of `pnpm update --interactive` is more informative in two cases. A dependency outdated at the same version in several workspace projects is offered as one choice, since selecting it updates every project — that choice now names all of them instead of only the first. And a workspace project without a `name` is now labelled with its path rather than left blank, so several unnamed projects can be told apart.

- The root project's `pnpm:devPreinstall` script now runs before resolution and linking, as it does in pnpm 11. It is skipped under `--ignore-scripts`, `--lockfile-only` and `--dry-run`, by `pnpm fetch` and `pnpm rebuild`, and by a repeat install that is already up to date. Workspaces that use the hook to prepare state the install depends on — such as [next.js](https://github.com/vercel/next.js), which generates a placeholder `next` bin with it — were left with dependents linked against files that were never created [#13313](https://github.com/pnpm/pnpm/issues/13313).

- `pnpm update --workspace` no longer links dependencies the user never named:

  - Running it with `updateConfig.ignoreDependencies` configured no longer fails with `ERR_PNPM_WORKSPACE_PACKAGE_NOT_FOUND` for a dependency that is only published to the registry. Such dependencies keep their specifiers, as they already did when no dependencies were ignored.
  - Passing package selectors that match no direct dependency no longer falls back to linking every workspace dependency.

- Updated dependencies:
  - @pnpm/building.after-install@1102.0.13
  - @pnpm/building.policy@1100.0.16
  - @pnpm/cli.utils@1101.0.20
  - @pnpm/config.pick-registry-for-package@1100.0.13
  - @pnpm/config.reader@1101.15.0
  - @pnpm/config.version-policy@1100.1.10
  - @pnpm/config.writer@1100.0.19
  - @pnpm/core-loggers@1100.3.0
  - @pnpm/deps.github-actions@1100.1.1
  - @pnpm/deps.inspection.outdated@1100.1.21
  - @pnpm/deps.path@1100.0.12
  - @pnpm/deps.security.signatures@1101.2.8
  - @pnpm/deps.status@1100.1.13
  - @pnpm/global.commands@1100.0.41
  - @pnpm/hooks.pnpmfile@1100.0.23
  - @pnpm/installing.context@1100.0.29
  - @pnpm/installing.dedupe.check@1100.1.6
  - @pnpm/installing.dedupe.issues-renderer@1100.0.3
  - @pnpm/installing.deps-installer@1102.3.7
  - @pnpm/installing.env-installer@1102.0.13
  - @pnpm/lockfile.fs@1100.1.15
  - @pnpm/lockfile.types@1100.0.17
  - @pnpm/network.auth-header@1101.1.7
  - @pnpm/network.fetch@1100.1.9
  - @pnpm/pkg-manifest.reader@1100.0.13
  - @pnpm/pkg-manifest.utils@1100.2.13
  - @pnpm/resolving.npm-resolver@1102.1.9
  - @pnpm/resolving.resolver-base@1100.5.5
  - @pnpm/store.connection-manager@1100.3.13
  - @pnpm/store.controller@1102.0.9
  - @pnpm/types@1101.7.0
  - @pnpm/workspace.project-manifest-reader@1100.0.21
  - @pnpm/workspace.project-manifest-writer@1100.0.12
  - @pnpm/workspace.projects-filter@1100.0.33
  - @pnpm/workspace.projects-graph@1100.0.29
  - @pnpm/workspace.projects-reader@1101.0.20
  - @pnpm/workspace.projects-sorter@1100.0.12
  - @pnpm/workspace.state@1100.0.34
  - @pnpm/workspace.workspace-manifest-reader@1100.1.3
  - @pnpm/workspace.workspace-manifest-writer@1100.0.19
