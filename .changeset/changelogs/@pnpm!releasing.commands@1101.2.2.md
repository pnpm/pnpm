## 1101.2.2

### Patch Changes

- `pnpm deploy` no longer installs the dependencies of the workspace root project into the deploy directory [#6437](https://github.com/pnpm/pnpm/issues/6437).

- `pnpm deploy` now writes plain versions for registry dependencies with peer dependencies in the deployed `package.json`. The deployed lockfile retains the resolved peer bindings. npm aliases keep their target package names [#14873](https://github.com/pnpm/pnpm/issues/14873).

- `pnpm publish` now allows a detached Git HEAD in CI, including checkouts of release tags. The working tree must still be clean. Branch and remote-history checks still apply when HEAD is attached [pnpm/pnpm#5894](https://github.com/pnpm/pnpm/issues/5894).

- `pnpm pack` now writes tarball entries grouped by file extension and file name, the order npm uses. Packages that ship many same-named files, such as template collections, pack much smaller [#14766](https://github.com/pnpm/pnpm/issues/14766).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.28
  - @pnpm/config.reader@1102.2.1
  - @pnpm/deps.path@1101.0.3
  - @pnpm/engine.runtime.commands@1101.1.3
  - @pnpm/engine.runtime.node-resolver@1101.3.2
  - @pnpm/exec.lifecycle@1100.1.18
  - @pnpm/exec.pnpm-cli-runner@1100.0.3
  - @pnpm/fetching.directory-fetcher@1100.0.34
  - @pnpm/fs.indexed-pkg-importer@1100.0.29
  - @pnpm/installing.client@1100.3.10
  - @pnpm/installing.commands@1101.3.1
  - @pnpm/lockfile.fs@1100.2.8
  - @pnpm/lockfile.types@1100.1.2
  - @pnpm/network.fetch@1100.1.17
  - @pnpm/network.git-utils@1100.0.4
  - @pnpm/releasing.exportable-manifest@1100.3.2
  - @pnpm/releasing.versioning@1100.3.2
  - @pnpm/resolving.npm-resolver@1104.2.0
  - @pnpm/resolving.resolver-base@1101.3.0
  - @pnpm/text.sanitize@1100.0.1
  - @pnpm/workspace.projects-filter@1100.0.43
  - @pnpm/workspace.projects-graph@1100.0.38
  - @pnpm/workspace.workspace-manifest-writer@1100.2.1
