---
id: install
title: pnpm install
---

Aliases: `i`

`pnpm install` is used to install all dependencies for a project.

Large package downloads use temporary disk space to keep memory usage bounded.
Temporary download files are removed after extraction or a failed download.
Package manifests and archive metadata that need buffering are limited to 64 MiB.

In a CI environment, installation fails if a lockfile is present but needs an
update. To allow lockfile updates in CI, run `pnpm install --no-frozen-lockfile`.

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
| `pnpm i --dry-run`                | Preview changes without writing     |

## Options for filtering dependencies

Without a lockfile, pnpm has to create one, and it must be consistent regardless of dependencies
filtering, so running `pnpm install --prod` on a directory without a lockfile would still resolve the
dev dependencies, and it would error if the resolution is unsuccessful. The only exception for this rule
are `link:` dependencies.

Without `--frozen-lockfile`, pnpm will check for outdated information from `file:` dependencies, so
running `pnpm install --prod` without `--frozen-lockfile` on an environment where the target of `file:`
has been removed would error.

### --prod, -P

* Default: **false**
* Type: **Boolean**

If `true`, pnpm will not install any package listed in `devDependencies` and will remove
those insofar they were already installed.
If `false`, pnpm will install all packages listed in `devDependencies` and `dependencies`.

### --dev, -D

Only `devDependencies` are installed and `dependencies` are removed insofar they
were already installed.

### --no-optional

`optionalDependencies` are not installed.

### --no-runtime

Added in: v11.1.0

Skip installing runtime entries (e.g. Node.js downloaded via [`devEngines.runtime`](../package_json.md#devenginesruntime)). The lockfile is left untouched, so frozen installs still validate; only the runtime fetch and bin-linking are skipped.

This is useful in CI matrices where the runtime is provisioned externally (e.g. via `pnpm runtime -g set node <version>`) before `pnpm install` runs.

This can also be set via the `runtime=false` config in `pnpm-workspace.yaml`.

## Options

### --force

Force reinstall dependencies: refetch packages modified in store, recreate a lockfile and/or modules directory created by a non-compatible version of pnpm. Install all optionalDependencies even they don't satisfy the current environment(cpu, os, arch).

Since v11.28.0, set [`forceIgnoresPlatform`](../settings/dependency-resolution.md#forceignoresplatform) to `false` to keep skipping optional dependencies whose `os`, `cpu`, or `libc` do not match the host under `--force`.

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

### --no-lockfile

Don't read or generate a `pnpm-lock.yaml` file.

### --lockfile-only

* Default: **false**
* Type: **Boolean**

When used, only updates `pnpm-lock.yaml` and `package.json`. Nothing gets written to the `node_modules` directory.

### --dry-run

Added in: v11.8.0

Run a full dependency resolution and report what a real install would change, without writing anything to disk. No lockfile, manifest, or `node_modules` changes are saved.

A completed dry run exits with code 0, even when it reports that a real install would update the lockfile.

`--dry-run` cannot be used with a configured pnpr server, because that install path resolves and links through the server.

### --fix-lockfile

Fix broken lockfile entries automatically.

### --update-checksums

Added in: v11.4.0

Refresh the locked tarball integrity values from what the registry currently serves, when a downloaded tarball's hash doesn't match the integrity recorded in `pnpm-lock.yaml`.

By default, since v11.4.0, an integrity mismatch is a hard failure: `pnpm install` exits with `ERR_PNPM_TARBALL_INTEGRITY` rather than silently re-resolving from the registry and overwriting the locked integrity. This protects projects that ship a committed lockfile from a compromised registry, proxy, or republished version substituting attacker-controlled content on a clean machine.

`--update-checksums` is the narrowly-scoped opt-in for the legitimate case (e.g. a registry rewrote its tarballs and you've verified the new bytes are correct). A warning still prints when the bypass takes effect so the operation is auditable.

`--force` and `pnpm update` deliberately do **not** bypass the integrity check. `--frozen-lockfile` is unchanged, and `--fix-lockfile` keeps its documented purpose (filling in missing lockfile entries) and is also not a bypass.

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

### --no-frozen-lockfile

Allows lockfile updates during installation. This is the inverse of `--frozen-lockfile`.

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

[loglevel]: ../settings/cli.md#loglevel

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

[Read more about filtering.](../filtering.md)

### --resolution-only

Re-runs resolution: useful for printing out peer dependency issues.

:::info

To list peer dependency issues without re-running resolution, use [`pnpm peers check`](./peers.md#check). It reads them from the lockfile.

:::

import CpuFlag from '../settings/_cpuFlag.mdx'

<CpuFlag />

import OsFlag from '../settings/_osFlag.mdx'

<OsFlag />

import LibcFlag from '../settings/_libcFlag.mdx'

<LibcFlag />
