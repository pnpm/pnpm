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

Added in: v3.7.0.

By default, pnpm will link packages from the workspace if the available packages
match the declared ranges. For instance, `foo@1.0.0` is linked into `bar` if
`bar` has `"foo": "^1.0.0"` in its dependencies and `foo@1.0.0` is not in the workspace. However, if `bar` has
`"foo": "2.0.0"` in dependencies and `foo@2.0.0` is not in the workspace,
`foo@2.0.0` will be installed from the registry. This behavior introduces some
uncertainty.

Luckily, pnpm supports the `workspace:` protocol. When
this protocol is used, pnpm will refuse to resolve to anything other than a
local workspace package. So, if you set `"foo": "workspace:2.0.0"`, this time
installation will fail because `"foo@2.0.0"` isn't present in the workspace.

This protocol is especially useful when the [link-workspace-packages] option is
set to `false`. In that case, pnpm will only link packages from the workspace if
the `workspace:` protocol is used.

[link-workspace-packages]: #link-workspace-packages

### Referencing workspace packages through aliases

Added in: v5.12.0

Let's say you have a package in the workspace named `foo`. Usually, you would
reference it as `"foo": "workspace:*"`.

If you want to use a different alias, the following syntax will work too:
`"bar": "workspace:foo@*"`.

Before publish, aliases are converted to regular aliased dependencies. The above
example will become: `"bar": "npm:foo@1.0.0"`.


### Referencing workspace packages through their relative path

Added in: v5.12.0

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

* The corresponding version in the target workspace (if you use `workspace:*`, `workspace:~`, or `workspace:^`)
* The associated semver range (for any other range type)

So for example, if we have `foo`, `bar`, `qar`, `zoo` in the workspace and they all are at version `1.5.0`, the following:

```json
{
	"dependencies": {
		"foo": "workspace:*",
		"bar": "workspace:~",
		"qar": "workspace:^",
		"zoo": "workspace:^1.5.0"
	}
}
```

Will be transformed into:

```json
{
	"dependencies": {
		"foo": "1.5.0",
		"bar": "~1.5.0",
		"qar": "^1.5.0",
		"zoo": "^1.5.0"
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

## Troubleshooting

pnpm cannot guarantee that scripts will be run in topological order if there are cycles between workspace dependencies. If pnpm detects cyclic dependencies during installation, it will produce a warning. If pnpm is able to find out which dependencies are causing the cycles, it will display them too.

If you see the message `There are cyclic workspace dependencies`, please inspect workspace dependencies declared in `dependencies`, `optionalDependencies` and `devDependencies`.

## Usage examples

Here are a few open source projects that use the workspace feature of pnpm:

* [icestark](https://github.com/ice-lab/icestark) (as of 12/16/2021, commit `4862326a8de53d02f617e7b1986774fd7540fccd`)
* [Vue 3.0](https://github.com/vuejs/vue-next) (as of 10/9/2021, commit `61c5fbd3e35152f5f32e95bf04d3ee083414cecb`)
* [Vite](https://github.com/vitejs/vite) (as of 9/26/2021, commit `3e1cce01d01493d33e50966d0d0fd39a86d229f9`)
* [Cycle.js](https://github.com/cyclejs/cyclejs) (as of 9/21/2021, commit `f2187ab6688368edb904b649bd371a658f6a8637`)
* [Prisma](https://github.com/prisma/prisma) (as of 9/21/2021, commit `c4c83e788aa16d61bae7a6d00adc8a58b3789a06`)
* [Verdaccio](https://github.com/verdaccio/verdaccio) (as of 9/21/2021, commit `9dbf73e955fcb70b0a623c5ab89649b95146c744`)
* [Rollup plugins](https://github.com/rollup/plugins) (as of 9/21/2021, commit `53fb18c0c2852598200c547a0b1d745d15b5b487`)
* [Milkdown](https://github.com/Saul-Mirone/milkdown) (as of 9/26/2021, commit `4b2e1dd6125bc2198fd1b851c4f00eda70e9b913`)
* [ByteMD](https://github.com/bytedance/bytemd) (as of 2/18/2021, commit `36ef25f1ea1cd0b08752df5f8c832302017bb7fb`)
* [VueUse](https://github.com/vueuse/vueuse) (as of 9/25/2021, commit `826351ba1d9c514e34426c85f3d69fb9875c7dd9`)
* [Slidev](https://github.com/slidevjs/slidev) (as of 4/12/2021, commit `d6783323eb1ab1fc612577eb63579c8f7bc99c3a`)
* [SvelteKit](https://github.com/sveltejs/kit) (as of 9/26/2021, commit `b164420ab26fa04fd0fbe0ac05431f36a89ef193`)
* [Telecraft](https://github.com/telecraft/telecraft) (as of 9/26/2021, commit `73a9c48c9d4f160d758b8881f404cc52c20a7454`)
* [GiraphQL](https://github.com/hayes/giraphql) (as of 8/4/2021, commit `3dd3ff148da382d6f406f20626a9a5c25707c0c8`)
* [Tailchat](https://github.com/msgbyte/tailchat) (as of 12/27/2021, commit `298af71aa0619e0a8fa8717777afe2fb32739db4`)
* [Vitest](https://github.com/vitest-dev/vitest) (as of 12/13/2021, commit `d6ff0ccb819716713f5eab5c046861f4d8e4f988`)
* [Element Plus](https://github.com/element-plus/element-plus) (as of 9/23/2021, commit `f9e192535ff74d1443f1d9e0c5394fad10428629`)
