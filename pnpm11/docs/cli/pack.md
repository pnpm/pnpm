---
id: pack
title: pnpm pack
---

Create a tarball from a package.

Since v11.28.0:

* [`bundleDependencies`](https://docs.npmjs.com/cli/v11/configuring-npm/package-json#bundledependencies) are included when using the isolated linker, including bundled workspace packages and the dependencies of each bundled package, and also when `publishConfig.directory` selects a build directory.
* Symlinks that point to files or directories included in the package are kept. Symlinks that point outside the package are left out.
* Executable permissions of the source files are preserved in the tarball.
* When the project uses a `package.yaml` or `package.json5` manifest, its `files` field is honored and the tarball contains exactly one `package.json`.

## Options

### --recursive, -r

Added in: v10.11.0

Pack all packages from the workspace.

### --out &lt;path\>

Customizes the output path for the tarball. Use `%s` and `%v` to include the package name and version, e.g., `%s.tgz` or `some-dir/%s-%v.tgz`. By default, the tarball is saved in the current working directory with the name `<package-name>-<version>.tgz`.

### --pack-destination &lt;dir\>

Directory in which `pnpm pack` will save tarballs. The default is the current working directory.

### --pack-gzip-level &lt;level\>

Specifying custom compression level.

### --json

Log output in JSON format.

### --filter &lt;package_selector\>

Added in: v10.11.0

[Read more about filtering.](../filtering.md)

### --dry-run

Added in: v10.26.0

Does everything a normal run does, except actually packing the tarball. Useful for verifying the contents of the tarball.

### --skip-manifest-obfuscation

Added in: v11.3.0

Keep the original `packageManager` field and publish lifecycle scripts in the packed manifest instead of stripping them. The pnpm-specific `pnpm` field is still omitted.

## Life Cycle Scripts

* `prepack`
* `prepare`
* `postpack`

:::tip

You can also use the [`beforePacking` hook](../pnpmfile.md#hooksbeforepackingpkg-pkg--promisepkg) to programmatically modify the `package.json` contents before the tarball is created. This is useful for removing development-only fields or adding publication metadata without modifying your local `package.json`.

:::
