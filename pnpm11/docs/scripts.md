---
id: scripts
title: Scripts
---

How pnpm handles the `scripts` field of `package.json`.

## Hidden scripts

Added in: v11.0.0

Scripts with names starting with `.` are hidden. They cannot be run directly via `pnpm run` and are omitted from the `pnpm run` listing. Hidden scripts can only be called from other scripts.

```json title="package.json"
{
  "scripts": {
    ".helper": "echo 'I am hidden'",
    "build": "pnpm run .helper && tsc"
  }
}
```

In this example, `pnpm run .helper` would fail, but `pnpm run build` would succeed because `.helper` is called from another script.

## Environment variables

pnpm sets the following environment variables during lifecycle script execution:

- `npm_package_name`: the package name
- `npm_package_version`: the package version
- `npm_package_config_*`, `npm_package_engines_*`, `npm_package_bin_*`: the
  `config`, `engines`, and `bin` fields of the manifest, one variable per entry
  (for example, `npm_package_config_port`). A string `bin` sets `npm_package_bin`.
- `npm_package_json`: the path to the package's `package.json`
- `npm_lifecycle_event`: the name of the running script (e.g., `postinstall`)
- `npm_lifecycle_script`: the command of the running script
- `npm_node_execpath` and `NODE`: the path to the Node.js executable
- `npm_execpath`: the path to the running pnpm
- `npm_config_user_agent`: pnpm's user agent string
- `npm_config_node_gyp`: the path to the bundled `node-gyp`, unless the
  environment already sets it
- `INIT_CWD`: the directory where pnpm was started
- `PNPM_SCRIPT_SRC_DIR`: the directory of the package whose script runs
- `TMPDIR`: `node_modules/.tmp` in the package, unless
  [`unsafePerm`](./settings/build.md#unsafeperm) is `true`

:::note

Apart from the variables above, pnpm does not set `npm_config_*` variables from
its configuration. Other `npm_config_*` variables in the environment reach the
script unchanged, except auth, scope-registry, and per-registry settings (names
starting with `_`, `/`, or `@`, or containing `:_`).

:::

## Built-in command and script name conflicts

Added in: v11.0.0

The following built-in commands prefer user scripts: `clean`, `setup`, `deploy`, and `rebuild`. If your `package.json` defines a script with one of these names, `pnpm <name>` will execute the script instead of the built-in command.

To force the built-in command, use [`pnpm pm <name>`](./cli/pm.md).

## Lifecycle scripts

### `pnpm:devPreinstall`

Runs when `pnpm install` runs in the project itself, including in CI. It does not run when the package is installed as a dependency of another project.

Runs before any dependency is installed.

This script is executed only when set in the root project's `package.json`.

Since v11.28.0, installs that skip `devDependencies`, such as `pnpm install --prod`, do not run it.

### `preinstall` of the root project

Since v11.28.0, the root project's `preinstall` script runs before dependencies are resolved and linked. A guard such as `npx only-allow pnpm` can therefore stop the install before pnpm populates `node_modules`.

### `prepare`

The root project's `prepare` script runs after `pnpm install`. It is skipped, since v11.28.0, by installs that skip `devDependencies` (such as `pnpm install --prod`), by `pnpm install` given package arguments, and for the project that [`pnpm deploy`](./cli/deploy.md) deploys.

### Uninstall scripts

Added in: v11.28.0

[`pnpm remove`](./cli/remove.md) runs the project's own `preuninstall`, `uninstall`, and `postuninstall` scripts. `preuninstall` and `uninstall` run before dependencies are unlinked, and a failure in either aborts the removal. `postuninstall` runs after unlinking completes. The [`ignoreScripts`](./settings/build.md#ignorescripts) setting and `--lockfile-only` skip all three.
