---
id: workspaces
title: Workspace
---

pnpm has built-in support for monorepositories (AKA multi-package repositories,
multi-project repositories, or monolithic repositories). You can create a
workspace to unite multiple projects inside a single repository.

A workspace must have a [`pnpm-workspace.yaml`] file in its
root. A workspace also may have an [`.npmrc`] in its root.

[`pnpm-workspace.yaml`]: pnpm-workspace_yaml.md
[`.npmrc`]: npmrc.md

## Workspace protocol (workspace:)

Supported since v3.7.0.

By default, pnpm will link packages from the workspace if the available packages
match the declared ranges. For instance, `foo@1.0.0` is linked into `bar` if
`bar` has `"foo": "^1.0.0"` in its dependencies and `foo@1.0.0` is not in the workspace. However, if `bar` has
`"foo": "2.0.0"` in dependencies and `foo@2.0.0` is not in the workspace,
`foo@2.0.0` will be installed from the registry. This behavior introduces some
uncertainty.

Luckily, pnpm supports the `workspace:` protocol (the same as in Yarn v2). When
this protocol is used, pnpm will refuse to resolve to anything other than a
local workspace package. So, if you set `"foo": "workspace:2.0.0"`, this time
installation will fail because `"foo@2.0.0"` isn't present in the workspace.

This protocol is especially useful when the [link-workspace-packages] option is
set to `false`. In that case, pnpm will only link packages from the workspace if
the `workspace:` protocol is used.

[link-workspace-packages]: #link-workspace-packages

### Referencing workspace packages through aliases

Added in 5.12.0

Let's say you have a package in the workspace named `foo`. Usually, you would
reference it as `"foo": "workspace:*"`.

If you want to use a different alias, the following syntax will work too:
`"bar": "workspace:foo@*"`.

Before publish, aliases are converted to regular aliased dependencies. The above
example will become: `"bar": "npm:foo@1.0.0"`.


### Referencing workspace packages through their relative path

Added in 5.12.0

In a workspace with 2 packages:

```
+ packages
	+ foo
	+ bar
```

`bar` may have `foo` in its dependencies declared as
`"foo": "workspace:../foo"`. Before publishing, these specs are converted to
regular version specs supported by all package managers.

### Publishing workspace packages

When a workspace package is packed into an archive (whether it's through
`pnpm pack` or one of the publish commands like `pnpm publish`), we dynamically
replace any `workspace:` dependency by:

* The corresponding version in the target workspace (if you use `*`)
* The associated semver range (for any other range type)

So for example, if we have a workspace package at version `1.5.0`, the following:

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

This feature allows you to depend on your local workspace packages while still
being able to publish the resulting packages to the remote registry without
needing intermediary publish steps - your consumers will be able to use your
published workspaces as any other package, still benefitting from the guarantees
semver offers.

## Release workflow

Versioning packages inside a workspace is a complex task and pnpm currently does
not provide a built-in solution for it. However, there are 2 well tested tools
that handle versioning and support pnpm:
- [changesets](https://github.com/changesets/changesets)
- [Rush](https://rushjs.io).

For how to set up a repository using Rush, read [this page][rush-setup].

For using Changesets with pnpm, read [this guide][changesets-guide].

[rush-setup]: https://rushjs.io/pages/maintainer/setup_new_repo
[changesets-guide]: using-changesets.md

## Options

### link-workspace-packages

Added in: v2.14.0

* Default: **true**
* Type: **true**, **false**, **deep**

If this is enabled, locally available packages are linked to `node_modules`
instead of being downloaded from the registry. This is very convenient in a
monorepo. If you need local packages to also be linked to subdependencies, you
can use the `deep` setting (since v5).

Else, packages are downloaded and installed from the registry. However,
workspace packages can still be linked by using the `workspace:` range protocol.

### prefer-workspace-packages

Added in: v5.13.0

* Default: **false**
* Type: **Boolean**

If this is enabled, local packages from the workspace are preferred over
packages from the registry, even if there is a newer version of the package in
the registry.

This setting is only useful if the workspace doesn't use
`save-workspace-protocol`.

### shared-workspace-lockfile

Added in: v2.17.0 as `shared-workspace-shrinkwrap`

* Default: **true**
* Type: **Boolean**

If this is enabled, pnpm creates a single `pnpm-lock.yaml` file in the root of
the workspace. This also means that all dependencies of workspace packages will
be in a single `node_modules` (and get symlinked to their package `node_modules`
folder for Node's module resolution).

Advantages of this option:
* every dependency is a singleton
* faster installations in a monorepo
* fewer changes in code reviews as they are all in one file

:::note

Even though all the dependencies will be hard linked into the root
`node_modules`, packages will have access only to those dependencies
that are declared in their `package.json`, so pnpm's strictness is preserved.
This is a result of the aforementioned symbolic linking.

:::

### save-workspace-protocol

* Default: **true**
* Type: **Boolean**

If this is enabled, new dependencies will be added with the workspace protocol
IF (and only if) they are present in the workspace.

You might want to change this setting to `false` if the tooling in your
repository does not understand the workspace protocol (and ideally submit a PR
to your tooling to get it added in the future).
