---
id: motivation
title: Motivation
---

## Saving disk space and boosting installation speed

[![](/img/cafs-illustration.jpg)](https://twitter.com/HemSays/status/1434921646083563525/photo/1)

When using npm or Yarn, if you have 100 projects using a dependency, you will
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

## Creating a non-flat node_modules directory

[![](/img/node-modules-structure.jpg)](https://twitter.com/xiaokedada/status/1471691763102679041/photo/1)

When installing dependencies with npm or Yarn Classic, all packages are hoisted to the root of the
modules directory. As a result, source code has access to dependencies that are
not added as dependencies to the project.

By default, pnpm uses symlinks to add only the direct dependencies of the project into the root of the modules directory.
If you'd like more details about the unique `node_modules` structure that pnpm
creates and why it works fine with the Node.js ecosystem, read:
- [Flat node_modules is not the only way](/blog/2020/05/27/flat-node-modules-is-not-the-only-way)
- [Symlinked node_modules structure](symlinked-node-modules-structure.md)

:::tip

If your tooling doesn't work well with symlinks, you may still use pnpm and set the [node-linker](npmrc#node-linker) setting to `hoisted`. This will instruct pnpm to create a node_modules directory that is similar to those created by npm and Yarn Classic.

:::
