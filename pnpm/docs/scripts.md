---
id: scripts
title: Scripts
---

How pnpm handles the `scripts` field of `package.json`.

## Hidden scripts

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

The following built-in commands prefer user scripts: `clean` (and its alias `purge`), `setup`, `deploy`, `rebuild` (and its alias `rb`), and `tasks`. If your `package.json` defines a script with one of these names, `pnpm <name>` will execute the script instead of the built-in command.

To force the built-in command, use [`pnpm pm <name>`](./cli/pm.md).

## Lifecycle scripts

`pnpm install` runs these scripts of each project in the workspace, in order:
`preinstall`, `install`, `postinstall`, `preprepare`, `prepare`, and
`postprepare`. With `--prod`, or when it installs specific packages, it runs
only `preinstall`, `install`, and `postinstall`.

For `pre` and `post` scripts of other scripts run by [`pnpm run`](./cli/run.md),
see [`enablePrePostScripts`](./settings/other.md#enableprepostscripts).

### `pnpm:devPreinstall`

Runs when `pnpm install` runs in the project itself, including in CI. It does not run when the package is installed as a dependency of another project.

Runs before any dependency is installed.

This script is executed only when set in the root project's `package.json`.
