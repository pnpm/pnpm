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

### --offline

* Default: **false**
* Type: **Boolean**

If true, pnpm will use only packages already available in the store.
If a package won't be found locally, the installation will fail.

### --prefer-offline

Added in: v1.28.0

* Default: **false**
* Type: **Boolean**

If true, staleness checks for cached data will be bypassed, but missing data
will be requested from the server. To force full offline mode, use `--offline`.

### --ignore-scripts

* Default: **false**
* Type: **Boolean**

Do not execute any scripts defined in the project `package.json` and its
dependencies.

:::note

This flag does not prevent the execution of [pnpmfile.js](../pnpmfile.md)

:::

### --prod, -P

pnpm will not install any package listed in `devDependencies` if the `NODE_ENV`
environment variable is set to production. Use this flag to instruct pnpm to
ignore `NODE_ENV` and take its production status from this flag instead.

### --dev, -D

Only `devDependencies` are installed regardless of the `NODE_ENV`.

### --no-optional

`optionalDependencies` are not installed.

### --lockfile-only

Added in: v1.26.0 (initially named `shrinkwrap-only`)

* Default: **false**
* Type: **Boolean**

When used, only updates `pnpm-lock.yaml` and `package.json` instead of checking
`node_modules` and downloading dependencies.

### --frozen-lockfile

Added in: v1.37.1 (initially named `frozen-shrinkwrap`)

* Default:
  * For non-CI: **false**
  * For CI: **true**, if a lockfile is present
* Type: **Boolean**

If `true`, pnpm doesn't generate a lockfile and fails to install if the lockfile
is out of sync with the manifest / an update is needed or no lockfile is
present.

### --reporter=&lt;name\>

* Default:
    * For TTY stdout: **default**
    * For non-TTY stdout: **append-only**
* Type: **default**, **append-only**, **ndjson**, **silent**

Allows you to choose the reporter that will log debug info to the terminal about
the installation progress.

* **silent** - no output is logged to the console, except fatal errors
* **default** - the default reporter when the stdout is TTY
* **append-only** (Added in v1.29.1) - the output is always appended to the end. No cursor manipulations are performed
* **ndjson** - the most verbose reporter. Prints all logs in [ndjson](https://github.com/ndjson/ndjson-spec) format

### --use-store-server

Added in: v1.30.0

* Default: **false**
* Type: **Boolean**

Starts a store server in the background. The store server will keep running
after installation is done. To stop the store server, run `pnpm server stop`

### --shamefully-hoist

* Default: **false**
* Type: **Boolean**

Creates a flat `node_modules` structure, similar to that of `npm` or `yarn`.
**WARNING**: This is highly discouraged.

### --filter &lt;package_selector>

[Read more about filtering.](../filtering.md)
