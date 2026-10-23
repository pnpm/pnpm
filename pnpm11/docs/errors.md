---
id: errors
title: Error Codes
---

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

## ERR_PNPM_PEER_DEP_ISSUES

With [`strictPeerDependencies`](./settings/peer-dependencies.md#strictpeerdependencies) enabled, `pnpm install` fails if the project has unresolved peer dependencies or the peer dependencies are not matching the wanted ranges. To fix this, install the missing peer dependencies.

You may also selectively ignore these errors using the [peerDependencyRules.ignoreMissing] and [peerDependencyRules.allowedVersions] settings.

[peerDependencyRules.ignoreMissing]: settings/peer-dependencies.md#peerdependencyrulesignoremissing
[peerDependencyRules.allowedVersions]: settings/peer-dependencies.md#peerdependencyrulesallowedversions

## ERR_PNPM_OUTDATED_LOCKFILE

This error happens when installation cannot be performed without changes to the lockfile. This might happen in a CI environment if someone has changed a `package.json` file in the repository without running `pnpm install` afterwards. Or someone forgot to commit the changes to the lockfile.

To fix this error, run `pnpm install` and commit the changes to the lockfile.

## ERR\_PNPM\_TARBALL\_INTEGRITY

This error indicates that the downloaded package's tarball did not match the expected integrity checksum.

If you use the npm registry (`registry.npmjs.org`), then this usually means that the integrity in your lockfile is incorrect.
This might happen if a lockfile had badly resolved merge conflicts.

If you use a registry that allows to override existing versions of a package, then it might mean that in your local metadata cache you have the integrity checksum of an older version of the package. In this case, you should run `pnpm store prune`. This command will remove your local metadata cache. Then you can retry the command that failed.

But also be careful and verify that the package is downloaded from the right URL. The URL should be printed in the error message.

## ERR_PNPM_REGISTRY_NAME_MISMATCH

The registry answered a request for one package with the metadata of a different package. pnpm stops instead of installing the package under a name the registry was not asked about.

Check that the registry configured for the package, or for its scope, is correct. A misconfigured proxy or a registry that serves the wrong document can cause this error.

## ERR_PNPM_INVALID_NODE_VERSION

The [`nodeVersion`](./settings/cli.md#nodeversion) setting is not an exact semver version, such as `22.22.3`.
