## 12.0.0-alpha.18

### Minor Changes

- The first release of a package now publishes the version written in its manifest verbatim, instead of bumping off it. `pnpm version -r` and `pnpm change status` check the registry for each release's current version; when that version is not yet published, the package debuts at it and its pending changesets apply only from the next release. A newly added package seeded at `1100.0.0` with a `minor` changeset is therefore published as `1100.0.0` rather than skipping straight to `1100.1.0`.

- `pnpm version` now supports the npm-style bump forms: `pnpm version <major|minor|patch|premajor|preminor|prepatch|prerelease>` and `pnpm version <exact-version>` (also recursively with `-r`), with `--preid`, `--allow-same-version`, `--message`, `--no-git-tag-version`, `--no-commit-hooks`, `--sign-git-tag`, `--tag-version-prefix`, and `--json`. The bump runs the `preversion`/`version`/`postversion` lifecycle scripts and records the new version as a git commit and tag.

- Added recursive workspace support to `pnpm outdated`. `pnpm list` and `pnpm ll` now inspect all workspace projects by default, matching the TypeScript CLI.

- Made recursive `pnpm rebuild` honor workspace filters with shared and dedicated lockfiles.

- Made `pnpm why` and `pnpm peers` recursive by default in workspaces. Recursive peer checks now honor workspace filters, and recursive `why` can inspect the active project when a workspace uses dedicated lockfiles.

- Added a `--changeset` flag to `pnpm update`. Set `update.changeset` to `true` in `pnpm-workspace.yaml` to enable this behavior by default, and use `--no-changeset` to override the setting for one update. After the update completes, pnpm writes a `.changeset/pnpm-update-<suffix>.md` file declaring a patch bump for every workspace package whose `dependencies` or `optionalDependencies` were changed by the update and a major bump when `peerDependencies` changed, including packages that consume an updated catalog entry via the `catalog:` protocol. Private packages, packages without a name, and packages listed in the `ignore` array of `.changeset/config.json` are skipped. If `.changeset/config.json` does not exist, a warning is printed and no changeset is generated.

- Added GitHub Actions dependencies to `pnpm outdated` and interactive `pnpm update`. Non-interactive updates can include them with `--include-github-actions` or by setting `update.githubActions` to `true` in `pnpm-workspace.yaml`. Updated actions are pinned to exact commit hashes with their release tags preserved in comments.

### Patch Changes

- Fixed `pnpm install` rewriting unrelated `pnpm-lock.yaml` entries after a small manifest change — for example, removing one dev dependency could bump other packages' open-range dependencies (such as jest's `@types/node: '*'`) to their newest versions [pnpm/pnpm#13193](https://github.com/pnpm/pnpm/pull/13193). Three resolution-reuse gaps caused still-satisfied lockfile entries to be re-resolved from the registry:

  - Direct dependencies using the `catalog:` protocol were compared against the lockfile in their resolved-range form, so every catalog-managed dependency looked changed on every install, and any package depending on one was re-resolved.
  - Auto-installed (hoisted) peer dependencies were also treated as changed direct dependencies on every install.
  - When a package had to resolve freshly but landed on the version the lockfile already recorded, its dependency subtree was still re-resolved instead of being reused, drifting open ranges pinned by the lockfile.

- `pnpm install`, `pnpm add`, `pnpm update`, and `pnpm remove` now support recursive (`-r`) and filtered (`--filter`) execution in workspaces configured with one lockfile per project (`sharedWorkspaceLockfile: false`), instead of failing with `ERR_PNPM_RECURSIVE_SHARED_LOCKFILE_UNSUPPORTED`. Each selected project is installed independently against its own `pnpm-lock.yaml`, `node_modules`, and virtual store, matching pnpm.

- Global commands (`pnpm add -g`, `pnpm runtime set -g`, ...) now create a missing global bin directory instead of failing with `ERR_PNPM_PNPM_DIR_NOT_WRITABLE`, and the universal `--silent` / `-s` shorthands for `--reporter=silent` (e.g. `pnpm store path --silent`) are supported again.

- `pnpm unlink` now reinstalls through the selection-aware install pipeline, matching pnpm: it honors `-r` / `--filter`, installs recursively by default inside a workspace, and supports both a shared workspace lockfile and one lockfile per project (`sharedWorkspaceLockfile: false`). Previously it always reinstalled only the active project.

- Added `update` and `audit` settings sections to `pnpm-workspace.yaml`, superseding the awkwardly named `updateConfig`, `auditConfig`, and top-level `auditLevel` settings:

  ```yaml
  update:
    ignoreDeps: # was updateConfig.ignoreDependencies
      - webpack
      - "@babel/*"

  audit:
    level: high # was auditLevel
    ignore: # was auditConfig.ignoreGhsas
      - GHSA-xxxx-yyyy-zzzz
  ```

  `update.ignoreDeps` lists dependency name patterns that `pnpm update` and `pnpm outdated` should skip. `audit.level` and `audit.ignore` tune `pnpm audit`.

  The deprecated `updateConfig`, `auditConfig`, and `auditLevel` settings keep working until the next major version. When both a new section value and its deprecated counterpart are set, the new section takes precedence and a warning is printed. Both the TypeScript CLI and the Rust config surface (pacquet) recognize the new sections.
