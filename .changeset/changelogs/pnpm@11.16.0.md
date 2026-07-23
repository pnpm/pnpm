## 11.16.0

### Minor Changes

- The first release of a package now publishes the version written in its manifest verbatim, instead of bumping off it. `pnpm version -r` and `pnpm change status` check the registry for each release's current version; when that version is not yet published, the package debuts at it and its pending changesets apply only from the next release. A newly added package seeded at `1100.0.0` with a `minor` changeset is therefore published as `1100.0.0` rather than skipping straight to `1100.1.0`.

- Added a `--changeset` flag to `pnpm update`. Set `update.changeset` to `true` in `pnpm-workspace.yaml` to enable this behavior by default, and use `--no-changeset` to override the setting for one update. After the update completes, pnpm writes a `.changeset/pnpm-update-<suffix>.md` file declaring a patch bump for every workspace package whose `dependencies` or `optionalDependencies` were changed by the update and a major bump when `peerDependencies` changed, including packages that consume an updated catalog entry via the `catalog:` protocol. Private packages, packages without a name, and packages listed in the `ignore` array of `.changeset/config.json` are skipped. If `.changeset/config.json` does not exist, a warning is printed and no changeset is generated.

- Added GitHub Actions dependencies to `pnpm outdated` and interactive `pnpm update`. Non-interactive updates can include them with `--include-github-actions` or by setting `update.githubActions` to `true` in `pnpm-workspace.yaml`. Updated actions are pinned to exact commit hashes with their release tags preserved in comments.

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

### Patch Changes

- Fixed `pnpm add --save-exact`/`--save-prefix` and `pnpm update` writing a package's version with the `peerDependencies` range's prefix (e.g. `^19.2.7` instead of the requested `19.2.7`) whenever the same package also appeared in `peerDependencies`. A real `dependencies`/`devDependencies`/`optionalDependencies` entry now takes precedence over a same-named `peerDependencies` entry when computing the current specifiers [#13108](https://github.com/pnpm/pnpm/issues/13108).
