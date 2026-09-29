---
id: install
title: pnpm install
---

Aliases: `i`

`pnpm install` is used to install all dependencies for a project.

In a CI environment, installation fails if a lockfile is present but needs an
update.

Inside a [workspace], `pnpm install` installs all dependencies in all the
projects. If you want to disable this behavior, set the `recursive-install`
setting to `false`.

![](/img/demos/pnpm-install.svg)

[workspace]: ../workspaces.md

## TL;DR

| Command                           | Meaning                             |
|-----------------------------------|-------------------------------------|
| `pnpm i --offline`                | Install offline from the store only |
| `pnpm i --frozen-lockfile`        | `pnpm-lock.yaml` is not updated     |
| `pnpm i --lockfile-only`          | Only `pnpm-lock.yaml` is updated    |

## Options

### --force

Force reinstall dependencies: refetch packages modified in store, recreate a lockfile and/or modules directory created by a non-compatible version of pnpm. Install all optionalDependencies even they don't satisfy the current environment(cpu, os, arch).

### --offline

* Default: **false**
* Type: **Boolean**

If `true`, pnpm will use only packages already available in the store.
If a package won't be found locally, the installation will fail.

### --prefer-offline

* Default: **false**
* Type: **Boolean**

If `true`, staleness checks for cached data will be bypassed, but missing data
will be requested from the server. To force full offline mode, use `--offline`.

### --prod, -P

* Default:
  * If `NODE_ENV` is `production`: `true`
  * If `NODE_ENV` is **not** `production`: `false`
* Type: Boolean

If set, pnpm will ignore `NODE_ENV` and instead use this boolean value for
determining the environment.

If `true`, pnpm will not install any package listed in `devDependencies` and will remove
those insofar they were already installed.
If `false`, pnpm will install all packages listed in `devDependencies` and `dependencies`.

### --dev, -D

Only `devDependencies` are installed and `dependencies` are removed insofar they
were already installed, regardless of the `NODE_ENV`.

### --no-optional

`optionalDependencies` are not installed.

### --lockfile-only

* Default: **false**
* Type: **Boolean**

When used, only updates `pnpm-lock.yaml` and `package.json`. Nothing gets written to the `node_modules` directory.

### --fix-lockfile

Fix broken lockfile entries automatically.

### --frozen-lockfile

* Default:
  * For non-CI: **false**
  * For CI: **true**, if a lockfile is present
* Type: **Boolean**

If `true`, pnpm doesn't generate a lockfile and fails to install if the lockfile
is out of sync with the manifest / an update is needed or no lockfile is
present.

This setting is `true` by default in [CI environments]. The following code is used to detect CI environments:

```js title="https://github.com/watson/ci-info/blob/44e98cebcdf4403f162195fbcf90b1f69fc6e047/index.js#L54-L61"
exports.isCI = !!(
  env.CI || // Travis CI, CircleCI, Cirrus CI, GitLab CI, Appveyor, CodeShip, dsari
  env.CONTINUOUS_INTEGRATION || // Travis CI, Cirrus CI
  env.BUILD_NUMBER || // Jenkins, TeamCity
  env.RUN_ID || // TaskCluster, dsari
  exports.name ||
  false
)
```

[CI environments]: https://github.com/watson/ci-info#supported-ci-tools

### --merge-git-branch-lockfiles

Merge all git branch lockfiles.
[Read more about git branch lockfiles.](../git_branch_lockfiles)


### --reporter=&lt;name\>

* Default:
    * For TTY stdout: **default**
    * For non-TTY stdout: **append-only**
* Type: **default**, **append-only**, **ndjson**, **silent**

Allows you to choose the reporter that will log debug info to the terminal about
the installation progress.

* **silent** - no output is logged to the console, not even fatal errors
* **default** - the default reporter when the stdout is TTY
* **append-only** - the output is always appended to the end. No cursor manipulations are performed
* **ndjson** - the most verbose reporter. Prints all logs in [ndjson](https://github.com/ndjson/ndjson-spec) format

If you want to change what type of information is printed, use the [loglevel] setting.

[loglevel]: ../npmrc.md#loglevel

### --use-store-server

* Default: **false**
* Type: **Boolean**

:::danger

Deprecated feature

:::

Starts a store server in the background. The store server will keep running
after installation is done. To stop the store server, run `pnpm server stop`

### --shamefully-hoist

* Default: **false**
* Type: **Boolean**

Creates a flat `node_modules` structure, similar to that of `npm` or `yarn`.
**WARNING**: This is highly discouraged.

### --ignore-scripts

* Default: **false**
* Type: **Boolean**

Do not execute any scripts defined in the project `package.json` and its
dependencies.

### --filter &lt;package_selector>

:::warning

Filter currently does not work properly with v8 default config, you have to implicitly set [dedupe-peer-dependents](../npmrc.md#dedupe-peer-dependents) to `false` to have that work. For more info and progress please refer to [#6300](https://github.com/pnpm/pnpm/issues/6300)

:::

[Read more about filtering.](../filtering.md)

### --resolution-only

Re-runs resolution: useful for printing out peer dependency issues.
