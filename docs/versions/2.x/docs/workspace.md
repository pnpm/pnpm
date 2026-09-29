---
id: workspace
title: Workspace
original_id: workspace
---

pnpm supports concurrent actions in multi-package repositories (workspaces).

By default, when you run the [`pnpm recursive [action]`](pnpm-recursive.md) commands,
all the directories are searched for packages (directories with `package.json` file).
From `v1.35.0`, you can control what directories are searched by passing an array of globs to `packages` in `pnpm-workspace.yaml`.

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

## Linking packages inside a workspace

(This example will work with pnpm v2.14.0 or newer)

When working inside a workspace, you want your dependencies to be linked from the monorepo but declared as regular dependencies.
Let's suppose that you have this workspace:

```
.
├─ pnpm-workspace.yaml
└─ packages
   ├─ car
   └─ garage
```

If you create a `.npmrc` file in the root of your repository and set the value of `link-workspace-packages` to `true`, the install
command will try to find packages in the repository before resolving them from the registry.

So when you run `pnpm install car` in `/packages/garage`, pnpm will link `car` to `garage/node_modules/car` from `packages/car`.
Even though `car` will be linked, it will be added as a regular semver dependency to the dependencies of `garage`. So if the version of `car` in the workspace is `1.0.0` then this is
how the `package.json` of `garage` will look like after running `pnpm install car`:

```json
{
  "name": "garage",
  "version": "1.0.0",
  "dependencies": {
    "car": "^1.0.0"
  }
}
```

## Using a shared `shrinkwrap.yaml`

As of v2, pnpm creates a dedicated `shrinkwrap.yaml` and `node_modules` folder for each workspace package by default.
But from `v2.17.0`, it is possible to add [shared-workspace-shrinkwrap=true](pnpm-recursive.md#shared-workspace-shrinkwrap) to the root `.npmrc` of your monorepo.
When and `shared-workspace-shrinkwrap` is `true`, packages use a single `shrinkwrap.yaml` in the root of the monorepo.
This config will be `true` by default from pnpm v3.
