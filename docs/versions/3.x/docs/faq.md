---
id: faq
title: Frequently Asked Questions
original_id: faq
---

## Why does my `node_modules` folder use disk space if packages are stored in a global store?

pnpm creates [hard links](https://en.wikipedia.org/wiki/Hard_link) from the global store to project's `node_modules` folders.
Hard links point to the same place on the disk where the original files are.
So, for example, if you have `foo` in your project as a dependency and it occupies 1MB of space,
then it will look like it occupies 1MB of space in the project's `node_modules` folder and
the same amount of space in the global store. However, that 1MB is *the same space* on the disk
addressed from two different locations. So in total `foo` occupies 1MB,
not 2MB.

For more on this subject:

* [Why do hard links seem to take the same space as the originals?](https://unix.stackexchange.com/questions/88423/why-do-hard-links-seem-to-take-the-same-space-as-the-originals)
* [A thread from the pnpm chat room](https://gist.github.com/zkochan/106cfef49f8476b753a9cbbf9c65aff1)
* [An issue in the pnpm repo](https://github.com/pnpm/pnpm/issues/794)

## Does it work on Windows? It is harder to create symlinks on Windows

Using symlinks on Windows is problematic indeed. That is why pnpm uses junctions instead of symlinks on Windows OS.

## Does it work on Windows? Nested `node_modules` approach is basically incompatible with Windows

Early versions of npm had issues because of nesting all `node_modules` (see [Node's nested node_modules approach is basically incompatible with Windows](https://github.com/nodejs/node-v0.x-archive/issues/6960)). However, pnpm does not create deep folders, it stores all packages flatly and uses symlinks to create the dependency tree structure.

## What about circular symlinks?

Although pnpm uses symlinks to put dependencies into `node_modules` folders, circular symlinks are avoided because parent packages are placed into the same `node_modules` folder in which their dependencies are. So `foo`'s dependencies are not in `foo/node_modules` but `foo` is in `node_modules/foo`, together with its own dependencies.

## Why have hard links at all? Why not symlink directly to the global store?

One package can have different sets of dependencies on one machine.

In project **A** `foo@1.0.0` can have dependency resolved to `bar@1.0.0` but in project **B** the same dependency of `foo` might resolve to `bar@1.1.0`. So pnpm hard links `foo@1.0.0` to every project where it is used, in order to create different sets of dependencies for it.

Direct symlinking to the global store would work with Node's `--preserve-symlinks` flag. But `--preserve-symlinks` comes
with a bunch of different issues, so we decided to stick with hard links.
For more details about why this decision was made, see: https://github.com/nodejs/node-eps/issues/46.

## Does pnpm work across multiple hard drives or filesystems?

The package store should be on the same disk as installations.
Otherwise packages will be copied, not linked.
This is due to a OS limitation in hard-linking. See [Issue #712](https://github.com/pnpm/pnpm/issues/712) for more details.

pnpm functions differently based on the 2 cases below:

### Store path is specified

If the store path is specified via [the store config](configuring.md), then copying occurs between the store and any projects that are on a different disk.

If you run `pnpm install` on disk `D:`, then the pnpm store must be on disk `D:`.
If the pnpm store is located on disk `C:`, then all required packages will be directly copied to the project location.
This severely reduces the benefits of pnpm.

### Store path is NOT specified

If the store path is not set, then multiple stores are created (one per each drive or filesystem).

If installation is run on disk `D:`, the store will be created in `D:\.pnpm-store`.
If later the installation is run on disk `C:`, an independent store will be created in `C:\.pnpm-store`.
The projects would still maintain the benefits of pnpm, but each drive may have redundant packages.

## What does `pnpm store prune` do? Is it harmful?

The command `pnpm store prune` removes _unreferenced packages_.

Unreferenced packages are packages that are not used by any projects on the system.
Packages can become unreferenced after most installation operations.

For example: during `pnpm install`, package `foo@1.0.0` is updated to `foo@1.0.1`.
pnpm will keep `foo@1.0.0` in the store, as it does not automatically remove packages.
If package `foo@1.0.0` is not used by any other project on the system, it becomes unreferenced.
Running `pnpm store prune` would remove `foo@1.0.0` from the store.

Running `pnpm store prune` is not harmful and has no side effects on your projects.
If future installations require removed packages, pnpm will download them again.

It is best practice to run `pnpm store prune` occasionally to clean up the store, but not too frequently.
Sometimes, unreferenced packages become required again.
This could occur when switching branches and installing older dependencies.
Then pnpm would need to re-download all removed packages, briefly slowing down the installation process.

## What does `pnpm` stand for?

`pnpm` stands for `performant npm`. [@rstacruz](https://github.com/rstacruz/) came up with the name.

## `pnpm` does not work with &lt;YOUR-PROJECT-HERE\>?

In most cases it means that one of the dependencies require packages not declared in `package.json`.
It is a common mistake caused by flat `node_modules`. If this happens, this is an error in the dependency and the
dependency should be fixed. That might take time though, so pnpm supports workarounds to make the buggy packages work.

### Solution 1

In the following example, a dependency does **not** have the `iterall` module in its own list of deps.

The easiest solution to resolve missing dependencies of the buggy packages is to **add `iterall` as a dependency to our project's `package.json`**.

You can do so, by installing it via:

`pnpm i iterall`

It will be automatically added to your project's `package.json`.

```json
  "dependencies": {
    ...
    "iterall": "^1.2.2",
    ...
  }
```

### Solution 2

One of the solutions is to use [hooks](pnpmfile.md#hooks) for adding the missing dependencies to the package's `package.json`.

An example was [Webpack Dashboard](https://github.com/pnpm/pnpm/issues/1043) which wasn't working with `pnpm`. It has since been resolved such that it works with `pnpm` now.

It used to throw an error:

```console
Error: Cannot find module 'babel-traverse'
  at /node_modules/.registry.npmjs.org/inspectpack/2.2.3/node_modules/inspectpack/lib/actions/parse
```

The problem was that `babel-traverse` was used in `inspectpack` library which was used by `webpack-dashboard`. But `babel-traverse` wasn't specified in `inspectpack`'s `package.json`. It still worked with `npm` and `yarn` because they create flat `node_modules`.

Solution was to create a `pnpmfile.js` with the following contents:

```js
module.exports = {
  hooks: {
    readPackage (pkg) {
      switch (pkg.name) {
        case 'inspectpack':
          pkg.dependencies['babel-traverse'] = '^6.26.0'
          break
      }
      return pkg
    }
  }
}
```

After creating `pnpmfile.js`, delete `pnpm-lock.yaml` only. No need to delete `node_modules`. Then install the dependencies & it should be working.

### Solution 3

In case there are too many issues, you can use the `shamefully-flatten` config. This creates a flat `node_modules` structure similar to the one created by `npm` or `yarn`.

To use it, try `pnpm install --shamefully-flatten`.
