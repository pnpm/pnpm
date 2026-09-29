---
id: pack
title: pnpm pack
---

Create a tarball from a package.

## Options

### --out &lt;path\>

Customizes the output path for the tarball. Use `%s` and `%v` to include the package name and version, e.g., `%s.tgz` or `some-dir/%s-%v.tgz`. By default, the tarball is saved in the current working directory with the name `<package-name>-<version>.tgz`.

### --pack-destination &lt;dir\>

Directory in which `pnpm pack` will save tarballs. The default is the current working directory.

### --pack-gzip-level &lt;level\>

Specifying custom compression level.

### --json

Added in: v9.14.1

Log output in JSON format.

## Life Cycle Scripts

* `prepack`
* `prepare`
* `postpack`
