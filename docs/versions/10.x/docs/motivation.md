---
id: motivation
title: Motivation
---

## Saving disk space

![An illustration of the pnpm content-addressable store. On the illustration there are two projects with node_modules. The files in the node_modules directories are hard links to the same files in the content-addressable store.](/img/pnpm-store.svg)

When using npm, if you have 100 projects using a dependency, you will
have 100 copies of that dependency saved on disk. With pnpm, the dependency will be
stored in a content-addressable store, so:

1. If you depend on different versions of the dependency, only the files that
differ are added to the store. For instance, if it has 100 files, and a new
version has a change in only one of those files, `pnpm update` will only add 1
new file to the store, instead of cloning the entire dependency just for the
singular change.
1. All the files are saved in a single place on the disk. When packages are
installed, their files are hard-linked from that single place, consuming no
additional disk space. This allows you to share dependencies of the same version
across projects.

As a result, you save a lot of space on your disk proportional to the number of
projects and dependencies, and you have a lot faster installations!

## Boosting installation speed

pnpm performs installation in three stages:

1. Dependency resolution. All required dependencies are identified and fetched to the store.
1. Directory structure calculation. The `node_modules` directory structure is calculated based on the dependencies.
1. Linking dependencies. All remaining dependencies are fetched and hard linked from the store to `node_modules`.

![An illustration of the pnpm install process. Packages are resolved, fetched, and hard linked as soon as possible.](/img/installation-stages-of-pnpm.svg)

This approach is significantly faster than the traditional three-stage installation process of resolving, fetching, and writing all dependencies to `node_modules`.

![An illustration of how package managers like Yarn Classic or npm install dependencies.](/img/installation-stages-of-other-pms.svg)

## Creating a non-flat node_modules directory

When installing dependencies with npm or Yarn Classic, all packages are hoisted to the root of the
modules directory. As a result, source code has access to dependencies that are
not added as dependencies to the project.

By default, pnpm uses symlinks to add only the direct dependencies of the project into the root of the modules directory.

![An illustration of a node_modules directory created by pnpm. Packages in the root node_modules are symlinks to directories inside the node_modules/.pnpm directory](/img/isolated-node-modules.svg)

If you'd like more details about the unique `node_modules` structure that pnpm
creates and why it works fine with the Node.js ecosystem, read:
- [Flat node_modules is not the only way](/blog/2020/05/27/flat-node-modules-is-not-the-only-way)
- [Symlinked node_modules structure](symlinked-node-modules-structure.md)

:::tip

If your tooling doesn't work well with symlinks, you may still use pnpm and set the [nodeLinker](settings#nodeLinker) setting to `hoisted`. This will instruct pnpm to create a node_modules directory that is similar to those created by npm and Yarn Classic.

:::
