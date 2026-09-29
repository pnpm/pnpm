---
id: run
title: pnpm run
---

Aliases: `run-script`

Runs a script defined in the package's manifest file.

## Examples

Let's say you have a `watch` script configured in your `package.json`, like so:

```json
"scripts": {
    "watch": "build-command --watch"
}
```

You can now run that script by using `pnpm run watch`! Simple, right?
Another thing to note for those that like to save keystrokes and time is that
all scripts get aliased in as pnpm commands, so ultimately `pnpm watch` is just
shorthand for `pnpm run watch` (ONLY for scripts that do not share the same name
as already existing pnpm commands).

## Details

In addition to the shell’s pre-existing `PATH`, `pnpm run` includes
`node_modules/.bin` in the `PATH` provided to `scripts`. This means that so
long as you have a package installed, you can use it in a script like a regular
command. For example, if you have `eslint` installed, you can write up a script
like so:

```json
"lint": "eslint src --fix"
```

And even though `eslint` is not installed globally in your shell, it will run.

For workspaces, as of v3.5, `<workspace root>/node_modules/.bin` is also added
to the `PATH`, so if a tool is installed in the workspace root, it may be called
in any workspace package's `scripts`.

## Options

### script-shell

Added in: v5.10.0

* Default: **null**
* Type: **path**

The shell to use for scripts run with the `pnpm run` command.

For instance, to force usage of Git Bash on Windows:

```
pnpm config set script-shell "C:\\Program Files\\git\\bin\\bash.exe"
```

### shell-emulator

Added in: v5.8.0

* Default: **false**
* Type: **Boolean**

When `true`, pnpm will use a JavaScript implementation of a [bash-like shell] to
execute scripts.

This option simplifies cross-platform scripting. For instance, by default, the
next script will fail on non-POSIX-compliant systems:

```json
"scripts": {
  "test": "NODE_ENV=test node test.js"
}
```

But if the `shell-emulator` setting is set to `true`, it will work on all
platforms.

[bash-like shell]: https://www.npmjs.com/package/@yarnpkg/shell

### --recursive, -r

This runs an arbitrary command from each package's "scripts" object.
If a package doesn't have the command, it is skipped.
If none of the packages have the command, the command fails.

### --if-present

Added in: v4.5.0

You can use the `--if-present` flag to avoid exiting with a non-zero exit code
when the script is undefined. This lets you run potentially undefined scripts
without breaking the execution chain.

### --parallel

Added in: v5.1.0

Completely disregard concurrency and topological sorting, running a given script
immediately in all matching packages with prefixed streaming output. This is the
preferred flag for long-running processes over many packages, for instance, a
lengthy build process.

### --stream

Added in: v5.1.0

Stream output from child processes immediately, prefixed with the originating
package directory. This allows output from different packages to be interleaved.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)
