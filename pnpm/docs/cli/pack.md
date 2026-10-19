---
id: pack
title: pnpm pack
---

Create a tarball from a package.

## Files included in the tarball

Use the `files` field in `package.json` to select files and directories. Patterns
support extglobs such as `lib/!(*.test.js)` and `@(dist|lib)`. A `files` field
replaces the root `.npmignore` or `.gitignore`; ignore files inside selected
subdirectories still apply. An explicitly named file or internal symlink is
included even when its parent directory is excluded.

Without `files`, pnpm reads `.npmignore`, falling back to `.gitignore`.
Negated ignore patterns can re-include default exclusions such as `*.orig` and
`npm-debug.log`. Root lockfiles and `.npmrc` stay excluded.

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
