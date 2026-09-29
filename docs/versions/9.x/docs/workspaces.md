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

:::tip

If you are looking into monorepo management, you might also want to look into [Bit].
Bit uses pnpm under the hood but automates a lot of the things that are currently done manually in a traditional workspace managed by pnpm/npm/Yarn. There's an article about `bit install` that talks about it: [Painless Monorepo Dependency Management with Bit].

:::

[Bit]: https://bit.dev/?utm_source=pnpm&utm_medium=workspace_page
[Painless Monorepo Dependency Management with Bit]: https://bit.cloud/blog/painless-monorepo-dependency-management-with-bit-l4f9fzyw?utm_source=pnpm&utm_medium=workspace_page

## Workspace protocol (workspace:)

If [link-workspace-packages] is set to `true`, pnpm will link packages from the workspace if the available packages
match the declared ranges. For instance, `foo@1.0.0` is linked into `bar` if
`bar` has `"foo": "^1.0.0"` in its dependencies and `foo@1.0.0` is in the workspace. However, if `bar` has
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

[link-workspace-packages]: npmrc.md#link-workspace-packages

### Referencing workspace packages through aliases

Let's say you have a package in the workspace named `foo`. Usually, you would
reference it as `"foo": "workspace:*"`.

If you want to use a different alias, the following syntax will work too:
`"bar": "workspace:foo@*"`.

Before publish, aliases are converted to regular aliased dependencies. The above
example will become: `"bar": "npm:foo@1.0.0"`.


### Referencing workspace packages through their relative path

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
- [Rush](https://rushjs.io)

For how to set up a repository using Rush, read [this page][rush-setup].

For using Changesets with pnpm, read [this guide][changesets-guide].

[rush-setup]: https://rushjs.io/pages/maintainer/setup_new_repo
[changesets-guide]: using-changesets.md

## Troubleshooting

pnpm cannot guarantee that scripts will be run in topological order if there are cycles between workspace dependencies. If pnpm detects cyclic dependencies during installation, it will produce a warning. If pnpm is able to find out which dependencies are causing the cycles, it will display them too.

If you see the message `There are cyclic workspace dependencies`, please inspect workspace dependencies declared in `dependencies`, `optionalDependencies` and `devDependencies`.

## Usage examples

Here are a few of the most popular open source projects that use the workspace feature of pnpm:

| Project | Stars | Migration date | Migration commit |
| --      | --    | --             | --               |
| [Next.js](https://github.com/vercel/next.js) | ![](https://img.shields.io/github/stars/vercel/next.js) | 2022-05-29 | [`f7b81316aea4fc9962e5e54981a6d559004231aa`](https://github.com/vercel/next.js/commit/f7b81316aea4fc9962e5e54981a6d559004231aa) |
| [Material UI](https://github.com/mui/material-ui) | ![](https://img.shields.io/github/stars/mui/material-ui) | 2024-01-03 | [`a1263e3e5ef8d840252b4857f85b33caa99f471d`](https://github.com/mui/material-ui/commit/a1263e3e5ef8d840252b4857f85b33caa99f471d) |
| [Vite](https://github.com/vitejs/vite) | ![](https://img.shields.io/github/stars/vitejs/vite) | 2021-09-26 | [`3e1cce01d01493d33e50966d0d0fd39a86d229f9`](https://github.com/vitejs/vite/commit/3e1cce01d01493d33e50966d0d0fd39a86d229f9) |
| [Nuxt](https://github.com/nuxt/nuxt) | ![](https://img.shields.io/github/stars/nuxt/nuxt) | 2022-10-17 | [`74a90c566c936164018c086030c7de65b26a5cb6`](https://github.com/nuxt/nuxt/commit/74a90c566c936164018c086030c7de65b26a5cb6) |
| [Vue 3.0](https://github.com/vuejs/vue-next) | ![](https://img.shields.io/github/stars/vuejs/vue-next) | 2021-10-09 | [`61c5fbd3e35152f5f32e95bf04d3ee083414cecb`](https://github.com/vuejs/vue-next/commit/61c5fbd3e35152f5f32e95bf04d3ee083414cecb) |
| [Astro](https://github.com/withastro/astro) | ![](https://img.shields.io/github/stars/withastro/astro) | 2022-03-08 | [`240d88aefe66c7d73b9c713c5da42ae789c011ce`](https://github.com/withastro/astro/commit/240d88aefe66c7d73b9c713c5da42ae789c011ce) |
| [n8n](https://github.com/n8n-io/n8n) | ![](https://img.shields.io/github/stars/n8n-io/n8n) | 2022-11-09 | [`736777385c54d5b20174c9c1fda38bb31fbf14b4`](https://github.com/n8n-io/n8n/commit/736777385c54d5b20174c9c1fda38bb31fbf14b4) |
| [Prisma](https://github.com/prisma/prisma) | ![](https://img.shields.io/github/stars/prisma/prisma) | 2021-09-21 | [`c4c83e788aa16d61bae7a6d00adc8a58b3789a06`](https://github.com/prisma/prisma/commit/c4c83e788aa16d61bae7a6d00adc8a58b3789a06) |
| [Novu](https://github.com/novuhq/novu) | ![](https://img.shields.io/github/stars/novuhq/novu) | 2021-12-23 | [`f2ea61f7d7ac7e12db4c9e70767082841ed98b2b`](https://github.com/novuhq/novu/commit/f2ea61f7d7ac7e12db4c9e70767082841ed98b2b) |
| [Slidev](https://github.com/slidevjs/slidev) | ![](https://img.shields.io/github/stars/slidevjs/slidev) | 2021-04-12 | [`d6783323eb1ab1fc612577eb63579c8f7bc99c3a`](https://github.com/slidevjs/slidev/commit/d6783323eb1ab1fc612577eb63579c8f7bc99c3a) |
| [Turborepo](https://github.com/vercel/turborepo) | ![](https://img.shields.io/github/stars/vercel/turborepo) | 2022-03-02 | [`fd171519ec02a69c9afafc1bc5d9d1b481fba721`](https://github.com/vercel/turborepo/commit/fd171519ec02a69c9afafc1bc5d9d1b481fba721) |
| [Quasar Framework](https://github.com/quasarframework/quasar) | ![](https://img.shields.io/github/stars/quasarframework/quasar) | 2024-03-13 | [`7f8e550bb7b6ab639ce423d02008e7f5e61cbf55`](https://github.com/quasarframework/quasar/commit/7f8e550bb7b6ab639ce423d02008e7f5e61cbf55) |
| [Element Plus](https://github.com/element-plus/element-plus) | ![](https://img.shields.io/github/stars/element-plus/element-plus) | 2021-09-23 | [`f9e192535ff74d1443f1d9e0c5394fad10428629`](https://github.com/element-plus/element-plus/commit/f9e192535ff74d1443f1d9e0c5394fad10428629) |
| [NextAuth.js](https://github.com/nextauthjs/next-auth) | ![](https://img.shields.io/github/stars/nextauthjs/next-auth) | 2022-05-03 | [`4f29d39521451e859dbdb83179756b372e3dd7aa`](https://github.com/nextauthjs/next-auth/commit/4f29d39521451e859dbdb83179756b372e3dd7aa) |
| [Ember.js](https://github.com/emberjs/ember.js) | ![](https://img.shields.io/github/stars/emberjs/ember.js) | 2023-10-18 | [`b6b05da662497183434136fb0148e1dec544db04`](https://github.com/emberjs/ember.js/commit/b6b05da662497183434136fb0148e1dec544db04) |
| [Qwik](https://github.com/BuilderIO/qwik) | ![](https://img.shields.io/github/stars/BuilderIO/qwik) | 2022-11-14 | [`021b12f58cca657e0a008119bc711405513e1ee9`](https://github.com/BuilderIO/qwik/commit/021b12f58cca657e0a008119bc711405513e1ee9) |
| [VueUse](https://github.com/vueuse/vueuse) | ![](https://img.shields.io/github/stars/vueuse/vueuse) | 2021-09-25 | [`826351ba1d9c514e34426c85f3d69fb9875c7dd9`](https://github.com/vueuse/vueuse/commit/826351ba1d9c514e34426c85f3d69fb9875c7dd9) |
| [SvelteKit](https://github.com/sveltejs/kit) | ![](https://img.shields.io/github/stars/sveltejs/kit) | 2021-09-26 | [`b164420ab26fa04fd0fbe0ac05431f36a89ef193`](https://github.com/sveltejs/kit/commit/b164420ab26fa04fd0fbe0ac05431f36a89ef193) |
| [Verdaccio](https://github.com/verdaccio/verdaccio) | ![](https://img.shields.io/github/stars/verdaccio/verdaccio) | 2021-09-21 | [`9dbf73e955fcb70b0a623c5ab89649b95146c744`](https://github.com/verdaccio/verdaccio/commit/9dbf73e955fcb70b0a623c5ab89649b95146c744) |
| [Vercel](https://github.com/vercel/vercel) | ![](https://img.shields.io/github/stars/vercel/vercel) | 2023-01-12 | [`9c768b98b71cfc72e8638bf5172be88c39e8fa69`](https://github.com/vercel/vercel/commit/9c768b98b71cfc72e8638bf5172be88c39e8fa69) |
| [Vitest](https://github.com/vitest-dev/vitest) | ![](https://img.shields.io/github/stars/vitest-dev/vitest) | 2021-12-13 | [`d6ff0ccb819716713f5eab5c046861f4d8e4f988`](https://github.com/vitest-dev/vitest/commit/d6ff0ccb819716713f5eab5c046861f4d8e4f988) |
| [Cycle.js](https://github.com/cyclejs/cyclejs) | ![](https://img.shields.io/github/stars/cyclejs/cyclejs) | 2021-09-21 | [`f2187ab6688368edb904b649bd371a658f6a8637`](https://github.com/cyclejs/cyclejs/commit/f2187ab6688368edb904b649bd371a658f6a8637) |
| [Milkdown](https://github.com/Saul-Mirone/milkdown) | ![](https://img.shields.io/github/stars/Saul-Mirone/milkdown) | 2021-09-26 | [`4b2e1dd6125bc2198fd1b851c4f00eda70e9b913`](https://github.com/Saul-Mirone/milkdown/commit/4b2e1dd6125bc2198fd1b851c4f00eda70e9b913) |
| [Nhost](https://github.com/nhost/nhost) | ![](https://img.shields.io/github/stars/nhost/nhost) | 2022-02-07 | [`10a1799a1fef2f558f737de3bb6cadda2b50e58f`](https://github.com/nhost/nhost/commit/10a1799a1fef2f558f737de3bb6cadda2b50e58f) |
| [Logto](https://github.com/logto-io/logto) | ![](https://img.shields.io/github/stars/logto-io/logto) | 2021-07-29 | [`0b002e07850c8e6d09b35d22fab56d3e99d77043`](https://github.com/logto-io/logto/commit/0b002e07850c8e6d09b35d22fab56d3e99d77043) |
| [Rollup plugins](https://github.com/rollup/plugins) | ![](https://img.shields.io/github/stars/rollup/plugins) | 2021-09-21 | [`53fb18c0c2852598200c547a0b1d745d15b5b487`](https://github.com/rollup/plugins/commit/53fb18c0c2852598200c547a0b1d745d15b5b487) |
| [icestark](https://github.com/ice-lab/icestark) | ![](https://img.shields.io/github/stars/ice-lab/icestark) | 2021-12-16 | [`4862326a8de53d02f617e7b1986774fd7540fccd`](https://github.com/ice-lab/icestark/commit/4862326a8de53d02f617e7b1986774fd7540fccd) |
| [ByteMD](https://github.com/bytedance/bytemd) | ![](https://img.shields.io/github/stars/bytedance/bytemd) | 2021-02-18 | [`36ef25f1ea1cd0b08752df5f8c832302017bb7fb`](https://github.com/bytedance/bytemd/commit/36ef25f1ea1cd0b08752df5f8c832302017bb7fb) |
| [Stimulus Components](https://github.com/stimulus-components/stimulus-components) | ![](https://img.shields.io/github/stars/stimulus-components/stimulus-components) | 2024-10-26 | [`8e100d5b2c02ad5bf0b965822880a60f543f5ec3`](https://github.com/stimulus-components/stimulus-components/commit/8e100d5b2c02ad5bf0b965822880a60f543f5ec3) |
