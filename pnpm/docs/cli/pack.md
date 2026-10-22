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

Brace ranges expand to individual values. For example, `part{1..3}.js`
matches `part1.js`, `part2.js`, and `part3.js`, and `part{a..c}.js` matches
`parta.js`, `partb.js`, and `partc.js`. Ranges support descending values,
steps such as `part{1..5..2}.js`, and zero padding such as `part{01..03}.js`.
A zero step uses the default increment of 1. Padding in a step such as
`part{1..10..02}.js` also pads the results to the wider endpoint's width.
These ranges also work in `.npmignore` and `.gitignore`.

Without `files`, pnpm reads `.npmignore`, falling back to `.gitignore`.
Negated ignore patterns can re-include default exclusions such as `*.orig` and
`npm-debug.log`. Root lockfiles and `.npmrc` stay excluded.

## Options

### --recursive, -r

Pack all packages from the workspace.

### --out &lt;path\>

Customizes the output path for the tarball. Use `%s` and `%v` to include the package name and version, e.g., `%s.tgz` or `some-dir/%s-%v.tgz`. By default, the tarball is saved in the current working directory with the name `<package-name>-<version>.tgz`.

### --pack-destination &lt;dir\>

Directory in which `pnpm pack` will save tarballs. The default is the current working directory.

### --pack-gzip-level &lt;level\>

The gzip compression level of the tarball, from `0` to `9`. Values outside that range are rejected.

### --json

Log output in JSON format.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)

### --dry-run

Does everything a normal run does, except actually packing the tarball. Useful for verifying the contents of the tarball.

### --skip-manifest-obfuscation

Keep the original `packageManager` field and publish lifecycle scripts in the packed manifest instead of stripping them. The `pnpm` field is still omitted, except for `pnpm.config`.

## Life cycle scripts

* `prepack`
* `prepare`
* `postpack`

:::tip

You can also use the [`beforePacking` hook](../pnpmfile.md#hooksbeforepackingpkg-pkg--promisepkg) to programmatically modify the `package.json` contents before the tarball is created. This is useful for removing development-only fields or adding publication metadata without modifying your local `package.json`.

:::
