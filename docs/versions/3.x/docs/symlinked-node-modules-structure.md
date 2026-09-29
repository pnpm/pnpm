---
id: symlinked-node-modules-structure
title: Symlinked `node_modules` structure
original_id: symlinked-node-modules-structure
---

> This article only describes how pnpm's `node_modules` are structured when there are no packages with peer dependencies.
> For the more complex scenario of dependencies with peers, see [how peers are resolved](how-peers-are-resolved.md).

pnpm's `node_modules` layout uses symbolic links to create a nested structure of dependencies.

Every `package@version` is linked to `node_modules` from the [global store](about-the-package-store.md) only once.
Let's say you install `foo@1.0.0` that depends on `bar@1.0.0`. pnpm will hard link both packages to `node_modules` like this:

```
node_modules
└─ .registry.npmjs.org
   ├─ bar/1.0.0/node_modules/bar
   |  ├─ index.js
   |  └─ package.json
   └─ foo/1.0.0/node_modules/foo
      ├─ index.js
      └─ package.json
```

These are the only "real files" in `node_modules`. Once all the packages are hard linked to `node_modules`, symlinks are
created to build the nested dependency graph structure.

As you might have noticed, both packages are hard linked into a subfolder inside a `node_modules` folder (`foo/1.0.0/node_modules/foo`).
This is needed to:

1. **allow packages to require themselves.** `foo` should be able to do `require('foo/package.json')`.
2. **avoid circular symlinks.** Dependencies of packages are placed in the same folder in which the dependent packages are.
For Node.js it doesn't make a difference whether dependencies are inside the package's `node_modules` or in any other
`node_modules` in the parent directories.

The next stage of installation is symlinking dependencies. `bar` is going to be symlinked to the `foo/1.0.0/node_modules` folder:

```
node_modules
└─ .registry.npmjs.org
   ├─ bar/1.0.0/node_modules/bar
   └─ foo/1.0.0/node_modules
      ├─ foo
      └─ bar -> ../../../bar/1.0.0/node_modules/bar
```

`foo` is going to be symlinked to the root `node_modules` folder because `foo` is a dependency of the project:

```
node_modules
├─ foo -> .registry.npmjs.org/foo/1.0.0/node_modules/foo
└─ .registry.npmjs.org
   ├─ bar/1.0.0/node_modules/bar
   └─ foo/1.0.0/node_modules
      ├─ foo
      └─ bar -> ../../../bar/1.0.0/node_modules/bar
```

This is a very simple example. However, the layout will stay flat in the file system regardless of the number of dependencies
and the depth of the dependency graph.

Let's add `qar@2.0.0` as a dependency of `bar` and `foo`. This is how the `node_modules` will look like:

```
node_modules
├─ foo -> .registry.npmjs.org/foo/1.0.0/node_modules/foo
└─ .registry.npmjs.org
   ├─ qar/2.0.0/node_modules/qar
   ├─ bar/1.0.0/node_modules
   |  ├─ bar
   |  └─ qar -> ../../../qar/2.0.0/node_modules/qar
   └─ foo/1.0.0/node_modules
      ├─ foo
      ├─ qar -> ../../../qar/2.0.0/node_modules/qar
      └─ bar -> ../../../bar/1.0.0/node_modules/bar
```

As you can see, even though the depth of the graph is bigger (`foo > bar > qar`), the directory depth in the file system is still the same.

This layout might look weird at first glance, but it is completely Node.js-compatible! When resolving modules, Node.js ignores symlinks.
So when `bar` is required from `foo/1.0.0/node_modules/foo/index.js`, Node.js is not using `bar` from `foo/1.0.0/node_modules/bar`.
`bar` is resolved to its real location: `bar/1.0.0/node_modules/bar`. As a consequence, `bar` can also resolve its dependencies
which are in `bar/1.0.0/node_modules`.

A great bonus of this layout is that only packages that are really in the dependencies are accessible. With flattened `node_modules`, all hoisted
packages are accessible. To read more about why this is an advantage, see [pnpm's strictness helps to avoid silly bugs](https://www.kochan.io/nodejs/pnpms-strictness-helps-to-avoid-silly-bugs.html).
