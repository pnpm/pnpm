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

The pnpm npm package provides bundled `node-gyp` to lifecycle scripts even when pnpm's own install scripts were skipped.

:::note

Apart from the variables above, pnpm does not set `npm_config_*` variables from
its configuration. Other `npm_config_*` variables in the environment reach the
script unchanged, except auth, scope-registry, and per-registry settings (names
starting with `_`, `/`, or `@`, or containing `:_`).

:::

### Build options for native addons

`pnpm install` does not turn extra command-line flags into `npm_config_*`
variables the way `npm install --build-from-source` does. Set the variables
in the environment instead. Native addons that use `node-pre-gyp` or
`prebuild-install`, such as `sqlite3`, read them. For example, to build
`sqlite3` from source for Electron, approve its build in
[`permissions`](./settings/build.md#permissions), then put the variables in an
env file:

```ini title=".env.electron"
npm_config_build_from_source=true
npm_config_runtime=electron
npm_config_target=38.0.0
npm_config_disturl=https://electronjs.org/headers
```

Then load it with [`--env-file`](./pnpm-cli.md#--env-file-path):

```sh
pnpm --env-file .env.electron install
```

To set them for every install of a project without a flag, return them in
`extraEnv` from the
[`updateConfig`](./pnpmfile.md#hooksupdateconfigconfig-config--promiseconfig)
hook:

```js title=".pnpmfile.mjs"
export const hooks = {
  updateConfig (config) {
    return {
      ...config,
      extraEnv: {
        ...config.extraEnv,
        npm_config_build_from_source: 'true',
        npm_config_runtime: 'electron',
        npm_config_target: '38.0.0',
        npm_config_disturl: 'https://electronjs.org/headers',
      },
    }
  },
}
```

The [side-effects cache](./settings/build.md#sideeffectscache) does not include
these variables in its key, so pnpm can restore a build made with different
values. List the package in
[`sideEffectsCache.exclude`](./settings/build.md#sideeffectscacheexclude) so
pnpm never restores its build from the cache:

```yaml title="pnpm-workspace.yaml"
sideEffectsCache:
  exclude:
    - sqlite3
```

A repeat install does not rebuild a package that is already installed. After
changing the variables, rebuild the package in every project with them:

```sh
pnpm --env-file .env.electron pm rebuild -r sqlite3
```

With the `updateConfig` hook, drop `--env-file .env.electron`.
[`pnpm pm`](./cli/pm.md) runs the built-in command even if `package.json`
defines a `rebuild` script.

## Processes a script leaves running

A process that a script or a command run by `pnpm exec` or `pnpm dlx` starts
and leaves running, such as a daemon, an editor, or a browser opened by the
`open` package, keeps running after pnpm exits. It does so whether the script
succeeded or failed, and after `Ctrl+C`, just as it would if the script ran
without pnpm. Closing the terminal ends the processes attached to it, as the
operating system always does.

pnpm ends what it started in two cases only:

- When pnpm itself is killed or crashes while a script runs, for example when
  a CI job is cancelled. On Windows this ends everything the script started,
  except a process started with `CREATE_BREAKAWAY_FROM_JOB`. On Unix it ends
  the scripts that run in a process group of their own, which is every script
  of a parallel run and every script started without a terminal. A process
  that leaves its process group, such as a daemon, keeps running.
- When a script fails in a recursive or parallel run, pnpm stops the scripts
  of the other packages that are still running, unless
  [`--no-bail`](./cli/recursive.md#--no-bail) is set.

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
