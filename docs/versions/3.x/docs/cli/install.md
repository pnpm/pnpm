---
id: install
title: pnpm install
original_id: install
---

`pnpm install` is used to install all dependencies for a project.

![](/img/demos/pnpm-install.svg)

## tl;dr

|Command|Meaning|
|--|--|
|`pnpm i --offline` |no network requests |
|`pnpm i --frozen-lockfile` |`pnpm-lock.yaml` is not updated |
|`pnpm i --prefer-frozen-lockfile` |when possible, `pnpm-lock.yaml` is not updated |

## Options

### --recursive, -r

Concurrently runs install in all subdirectories with a `package.json` (excluding node_modules).

### --offline

* Default: **false**
* Type: **Boolean**

If true, pnpm will use only packages already available in the store.
If a package won't be found locally, the installation will fail.

### --prefer-offline

Added in: v1.28.0

* Default: **false**
* Type: **Boolean**

If true, staleness checks for cached data will be bypassed, but missing data will be requested from the server.
To force full offline mode, use `--offline`.

### --ignore-scripts

* Default: **false**
* Type: **Boolean**

Do not execute any scripts defined in the project `package.json` and its dependencies.

> Note: this flag does not prevent the execution of [pnpmfile.js](../pnpmfile.md)

### --production[=true|false]

* Default: **false**
* Type: **Boolean**

pnpm will not install any package listed in `devDependencies` if the `NODE_ENV` environment variable is set to production. Use this flag to instruct pnpm to ignore `NODE_ENV` and take its production-or-not status from this flag instead.

> Notes: `--production` is the same as `--production=true`. `--prod` is an alias of `--production`.

### --lockfile-only

Added in: v1.26.0 (initially named `shrinkwrap-only`)

* Default: **false**
* Type: **Boolean**

When used, only updates `pnpm-lock.yaml` and `package.json` instead of checking `node_modules` and downloading dependencies.

### --frozen-lockfile

Added in: v1.37.1 (initially named `frozen-shrinkwrap`)

* Default: **false**
* Type: **Boolean**

If `true`, pnpm doesn't generate a lockfile and fails if an update is needed.

### --reporter=&lt;name>

* Default:
    * For TTY stdout: **default**
    * For non-TTY stdout: **append-only**
* Type: **default**, **append-only**, **ndjson**, **silent**

Allows to choose the reporter that will print info about
the installation progress.

* **silent** - no output is logged to the console, except fatal errors
* **default** - the default reporter when the stdout is TTY
* **append-only** (Added in v1.29.1) - the output is always appended to the end. No cursor manipulations are performed
* **ndjson** - the most verbose reporter. Prints all logs in [ndjson](https://github.com/ndjson/ndjson-spec) format

### --use-store-server

Added in: v1.30.0

* Default: **false**
* Type: **Boolean**

Starts a store server in the background. The store server will keep running after installation is done.
To stop the store server, run `pnpm server stop`

### --filter &lt;package_selector>

[Read more about filtering.](../filtering.md)
