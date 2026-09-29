---
id: workspace
title: Workspace
original_id: workspace
---

pnpm supports concurrent actions in multi-package repositories (workspaces).

By default, when you run the [`pnpm recursive [action]`](pnpm-recursive.md) commands,
all the directories are searched for packages (directories with `package.json` file).
You can control what directories are searched by passing an array of globs to `packages` in `pnpm-workspace.yaml`.

An example of a `pnpm-workspace.yaml`:

```yaml
packages:
  # the root package.json
  - '.'
  # all packages in subdirs of packages/ and components/
  - 'packages/**'
  - 'components/**'
  # exclude packages that are inside test/ directories
  - '!**/test/**'
```

`pnpm-workspace.yaml` should be in the root of the workspace.

## Workspace ranges (workspace:)

Supported since v3.7.0.

By default, pnpm will link packages from the workspace if the available packages match
the declared ranges. For instance, `foo@1.0.0` is linked into `bar` if `bar` has `"foo": "^1.0.0"` in dependencies.
But if `bar` has `"foo": "2.0.0"` in dependencies and `foo@2.0.0` is not in the workspace then
`foo@2.0.0` will be installed from the registry. This behavior introduces some uncertainty.

Luckily, pnpm supports the `workspace:` protocol (same as in Yarn v2). When this protocol is used pnpm will refuse
to resolve to anything else than a local workspace package. So if you set `"foo": "workspace:2.0.0"` installation
will fail telling that no `"foo@2.0.0"` is present in the workspace.

##  Publishing workspace packages

When a workspace package is packed into an archive (whether it's through `pnpm pack` or one of the publish commands like `pnpm publish`), we dynamically replace any `workspace:` dependency by:

* The corresponding version in the target workspace (if you use `*`)
* The associated semver range (for any other range type)

So for example, if we assume we have three workspace packages whose current version is `1.5.0`, the following:

```json
{
  "dependencies": {
    "foo": "workspace:*",
    "bar": "workspace:^1.2.3"
  }
}
```

Will be transformed into:

```json
{
  "dependencies": {
    "foo": "1.5.0",
    "bar": "^1.2.3"
  }
}
```

This feature allows you to not have to depend on something else than your local workspace packages, while still being able to publish the resulting packages to the remote registry without having to run intermediary publish steps - your consumers will be able to use your published workspaces as any other package, still benefiting from the guarantees semver offers.
