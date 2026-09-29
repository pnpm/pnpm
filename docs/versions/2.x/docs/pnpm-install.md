---
id: pnpm-install
title: pnpm install
original_id: pnpm-install
---

`pnpm install` is used to install all dependencies for a project.

![](/img/demos/pnpm-install.svg)

## tl;dr

|Command|Meaning|
|--|--|
|`pnpm i --offline` |no network requests |
|`pnpm i --frozen-shrinkwrap` |`shrinkwrap.yaml` is not updated |
|`pnpm i --prefer-frozen-shrinkwrap` |when possible, `shrinkwrap.yaml` is not updated |

## store

* Default: **~/.pnpm-store**
* Type: **path**

The location where all the packages are saved on the disk.

The store should be always on the same disk on which installation is happening. So there will be one store per disk.
If there is a home directory on the current disk, then the store is created in `<home dir>/.pnpm-store`. If there is no
homedir on the disk, then the store is created in the root. For example, if installation is happening on disk `D`
then the store will be created in `D:\.pnpm-store`.

It is possible to set a store from a different disk but in that case pnpm will copy, not link, packages from the store.
Hard links are possible only inside a filesystem.

## offline

* Default: **false**
* Type: **Boolean**

If true, pnpm will use only packages already available in the store.
If a package won't be found locally, the installation will fail.

## prefer-offline

Added in: v1.28.0

* Default: **false**
* Type: **Boolean**

If true, staleness checks for cached data will be bypassed, but missing data will be requested from the server.
To force full offline mode, use `--offline`.

## network-concurrency

* Default: **16**
* Type: **Number**

Controls the maximum number of HTTP requests that can be done simultaneously.

## child-concurrency

* Default: **5**
* Type: **Number**

Controls the number of child processes run parallelly to build node modules.

## lock

* Default: **true**
* Type: **Boolean**

Dangerous! If false, the store is not locked. It means that several installations using the same
store can run simultaneously.

Can be passed in via a CLI option. `--no-lock` to set it to false. E.g.: `pnpm install --no-lock`.

> If you experience issues similar to the ones described in [#594](https://github.com/pnpm/pnpm/issues/594), use this option to disable locking.
> In the meanwhile, we'll try to find a solution that will make locking work for everyone.

## ignore-pnpmfile

Added in: v1.25.0

* Default: **false**
* Type: **Boolean**

`pnpmfile.js` will be ignored. Useful together with `--ignore-scripts` when you want to make sure that
no script gets executed during install.

## pnpmfile

Added in: v1.39.0

* Default: **pnpmfile.js**
* Type: **path**
* Example: **.pnpm/pnpmfile.js**

The location of the local pnpmfile.

## global-pnpmfile

Added in: v1.39.0

* Default: **null**
* Type: **path**
* Example: **~/.pnpm/global_pnpmfile.js**

The location of a global pnpmfile. A global pnpmfile is used by all projects during installation.

**NOTE:** It is recommended to use local pnpmfiles. Only use a global pnpmfile, if you use pnpm on projects
that don't use pnpm as the primary package manager.

## independent-leaves

* Default: **false**
* Type: **Boolean**

If true, symlinks leaf dependencies directly from the global store. Leaf dependencies are
packages that have no dependencies of their own. Setting this config to `true` might break some packages
that rely on location but gives an average of **8% installation speed improvement**.

## verify-store-integrity

Added in: v1.8.0

* Default: **true**
* Type: **Boolean**

If false, doesn't check whether packages in the store were mutated.

## package-import-method

Added in: v1.25.0

* Default: **auto**
* Type: **auto**, **hardlink**, **copy**, **reflink**

Controls the way packages are imported from the store.

* **auto** - try to hardlink packages from the store. If it fails, fallback to copy
* **hardlink** - hardlink packages from the store
* **copy** - copy packages from the store
* **reflink** - reflink (aka copy-on-write) packages from the store

## shrinkwrap

Added in: v1.32.0

* Default: **true**
* Type: **Boolean**

When set to `false`, pnpm won't read or generate a `shrinkwrap.yaml` file.

## shrinkwrap-only

Added in: v1.26.0

* Default: **false**
* Type: **Boolean**

When used, only updates `shrinkwrap.yaml` and `package.json` instead of checking `node_modules` and downloading dependencies.

## frozen-shrinkwrap

Added in: v1.37.1

* Default: **false**
* Type: **Boolean**

If `true`, pnpm doesn't generate a shrinkwrap file and fails if an update is needed.

## prefer-frozen-shrinkwrap

Added in: v1.37.1

* Default: **true** (from v1.38.0)
* Type: **Boolean**

When `true` and the available `shrinkwrap.yaml` satisfies the `package.json`
then a headless installation is performed. A headless installation is faster than a regular one
because it skips dependencies resolution and peers resolution.

## reporter

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

## use-store-server

Added in: v1.30.0

* Default: **false**
* Type: **Boolean**

Starts a store server in the background. The store server will keep running after installation is done.
To stop the store server, run `pnpm server stop`

## use-running-store-server

Added in: v2.5.0

* Default: **false**
* Type: **Boolean**

Only allows installation with a store server. If no store server is running, installation will fail.

## side-effects-cache

Added in: v1.31.0

> Stability: Experimental

* Default: **false**
* Type: **Boolean**

Use and cache the results of (pre/post)install hooks.

## side-effects-cache-readonly

Added in: v1.31.0

> Stability: Experimental

* Default: **false**
* Type: **Boolean**

Only use the side effects cache if present, do not create it for new packages.

## shamefully-flatten

Added in: v1.34.0

* Default: **false**
* Type: **Boolean**

If true, pnpm creates a flat `node_modules` that look almost like a `node_modules` created by npm or Yarn.
Please only use this option when there is no other way to make a project work with pnpm.
The strict `node_modules` created by pnpm should always work, if it does not, most likely a dependency is
missing from `package.json`. Use this config only as a temporary fix.

## strict-peer-dependencies

Added in: v2.15.0

* Default: **false**
* Type: **Boolean**

If true, commands fail on missing or invalid peer dependencies.
