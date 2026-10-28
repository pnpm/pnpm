---
id: errors
title: Error Codes
---

An error pnpm raises carries a code that starts with `ERR_PNPM_`. The code is printed with the error message. A script that reacts to a failure should match on the code, because the message may change between releases and the code does not.

The codes are listed in alphabetical order. A code that is not listed here is explained by its own message.

## ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE

A frozen install (`pnpm install --frozen-lockfile`, the default in CI) cannot proceed from the lockfile as it is. There are two causes:

- `pnpm-lock.yaml` was written by a version of pnpm whose lockfile format this version cannot install from.
- The env document of `pnpm-lock.yaml` is not up to date. It records the [config dependencies](./config-dependencies.md) and the pinned package manager, and changes when `configDependencies` in `pnpm-workspace.yaml` or the pnpm version pinned in `package.json` changes.

Run `pnpm install` without `--frozen-lockfile` with the pnpm version the project uses, and commit the updated lockfile.

## ERR_PNPM_HOIST_PATTERN_DIFF

`pnpm add`, `pnpm remove`, `pnpm update`, or an install that selects only some workspace projects found a modules directory that was installed with a different [`hoistPattern`](./settings/node-modules.md#hoistpattern).

Run `pnpm install` to recreate the modules directory with the new `hoistPattern`.

## ERR_PNPM_IGNORED_BUILDS

The install skipped the build scripts of one or more dependencies, because [`allowBuilds`](./settings/build.md#allowbuilds) does not allow them, and [`strictDepBuilds`](./settings/build.md#strictdepbuilds) is on. The message lists the packages whose scripts were skipped.

Run [`pnpm approve-builds`](./cli/approve-builds.md) to allow the builds you trust. To install without failing on skipped builds, set `strictDepBuilds` to `false`.

## ERR_PNPM_INVALID_NODE_VERSION

The [`nodeVersion`](./settings/cli.md#nodeversion) setting is not an exact semver version, such as `22.22.3`.

## ERR_PNPM_LOCKFILE_CONFIG_MISMATCH

A frozen install found that a setting recorded in `pnpm-lock.yaml` differs from the current configuration. The message names the setting. The lockfile records `overrides`, `patchedDependencies`, `catalogs`, `ignoredOptionalDependencies`, the checksums of `packageExtensions` and the pnpmfile, and the resolution settings in its `settings` block, such as `autoInstallPeers` and `dedupePeers`.

Run `pnpm install` without `--frozen-lockfile` and commit the updated lockfile.

`pnpm rebuild` raises the same error when `patchedDependencies` differs from the installed lockfile. Run `pnpm install` first.

## ERR_PNPM_NODE_LINKER_DIFF

`pnpm add`, `pnpm remove`, `pnpm update`, or an install that selects only some workspace projects found a modules directory that was installed with a different `nodeLinker`.

Run `pnpm install` to recreate the modules directory with the new `nodeLinker`.

## ERR_PNPM_NO_MATCHING_VERSION_INSIDE_WORKSPACE

A project has a workspace dependency that does not exist in the workspace.

For instance, package `foo` has `bar@1.0.0` in the `dependencies`:

```json title="package.json"
{
  "name": "foo",
  "version": "1.0.0",
  "dependencies": {
    "bar": "workspace:1.0.0"
  }
}
```

However, there is only `bar@2.0.0` in the workspace, so `pnpm install` will fail.

To fix this error, all dependencies that use the [workspace protocol] should be updated to use versions of packages that are present in the workspace. This can be done either manually or using the `pnpm -r update` command.

[workspace protocol]: ./workspaces.md#workspace-protocol-workspace

## ERR_PNPM_NO_OFFLINE_META

pnpm runs with `--offline`, and the metadata cache has no entry for a package it has to resolve. The message names the package and the cache file it looked for.

Run the command without `--offline` once, so pnpm downloads the metadata. [`--prefer-offline`](./cli/install.md#--prefer-offline) uses the cache when it can and downloads what is missing.

## ERR_PNPM_OUTDATED_LOCKFILE

This error happens when installation cannot be performed without changes to the lockfile. This might happen in a CI environment if someone has changed a `package.json` file in the repository without running `pnpm install` afterwards. Or someone forgot to commit the changes to the lockfile.

To fix this error, run `pnpm install` and commit the changes to the lockfile.

## ERR_PNPM_PEER_DEP_ISSUES

With [`strictPeerDependencies`](./settings/peer-dependencies.md#strictpeerdependencies) enabled, `pnpm install` fails if the project has unresolved peer dependencies or the peer dependencies are not matching the wanted ranges. To fix this, install the missing peer dependencies.

You may also selectively ignore these errors using the [peerDependencyRules.ignoreMissing] and [peerDependencyRules.allowedVersions] settings.

[peerDependencyRules.ignoreMissing]: settings/peer-dependencies.md#peerdependencyrulesignoremissing
[peerDependencyRules.allowedVersions]: settings/peer-dependencies.md#peerdependencyrulesallowedversions

## ERR_PNPM_PUBLIC_HOIST_PATTERN_DIFF

`pnpm add`, `pnpm remove`, `pnpm update`, or an install that selects only some workspace projects found a modules directory that was installed with a different [`publicHoistPattern`](./settings/node-modules.md#publichoistpattern).

Run `pnpm install` to recreate the modules directory with the new `publicHoistPattern`.

## ERR_PNPM_REGISTRY_NAME_MISMATCH

The registry answered a request for one package with the metadata of a different package. pnpm stops instead of installing the package under a name the registry was not asked about.

Check that the registry configured for the package, or for its scope, is correct. A misconfigured proxy or a registry that serves the wrong document can cause this error.

## ERR_PNPM_TARBALL_INTEGRITY

This error indicates that the downloaded package's tarball did not match the expected integrity checksum.

If you use the npm registry (`registry.npmjs.org`), then this usually means that the integrity in your lockfile is incorrect.
This might happen if a lockfile had badly resolved merge conflicts.

If you use a registry that allows to override existing versions of a package, then it might mean that in your local metadata cache you have the integrity checksum of an older version of the package. In this case, you should run `pnpm store prune`. This command will remove your local metadata cache. Then you can retry the command that failed.

But also be careful and verify that the package is downloaded from the right URL. The URL should be printed in the error message.

## ERR_PNPM_TASK_CYCLE

A recursive run's task graph contains a dependency cycle. The error lists the
participating tasks as `<project>#<script>` identifiers.

Remove or change the cycle under [`tasks`](./workspace-task-orchestration.md).
If it is deliberate, set
[`ignoreWorkspaceCycles`](./workspaces.md#ignoreworkspacecycles) to `true` to
downgrade the error to a warning and run the cycle's tasks without a guaranteed
order.

## ERR_PNPM_UNEXPECTED_STORE

A modules directory is present and is linked to a different store directory.

If you changed the store directory intentionally, run `pnpm install` and pnpm will reinstall the dependencies using the new store.

## ERR_PNPM_UNSUPPORTED_ENGINE

A package's `engines` field does not accept the current Node.js or pnpm version. The message names the package, the wanted ranges, and the current versions.

A project that declares `engines.pnpm` always raises this error when its `engines` field does not accept the current versions. A project without `engines.pnpm`, and every dependency, raise it only with [`engineStrict`](./settings/cli.md#enginestrict) enabled. Without `engineStrict`, those produce a warning.

To fix this error, switch to a Node.js or pnpm version the package accepts. For a dependency, or a project without `engines.pnpm`, turning `engineStrict` off downgrades the error to a warning.

## ERR_PNPM_VIRTUAL_STORE_DIR_MAX_LENGTH_DIFF

`pnpm add`, `pnpm remove`, `pnpm update`, or an install that selects only some workspace projects found a modules directory that was installed with a different [`virtualStoreDirMaxLength`](./settings/node-modules.md#virtualstoredirmaxlength).

Run `pnpm install` to recreate the modules directory with the new `virtualStoreDirMaxLength`.

## ERR_PNPM_WORKSPACE_PKG_NOT_FOUND

A project declares a dependency with the [workspace protocol](./workspaces.md#workspace-protocol-workspace), but no project in the workspace has that name. The message names the project, the dependency, and its specifier.

Check the spelling of the dependency name, and check that the project it refers to is matched by the `packages` patterns in `pnpm-workspace.yaml`.
