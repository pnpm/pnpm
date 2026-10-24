---
id: install
title: pnpm install
---

Aliases: `i`

`pnpm install` is used to install all dependencies for a project.

Package manifests and archive metadata larger than 64 MiB are rejected.

In a CI environment, installation fails if a lockfile is present but needs an
update. To allow lockfile updates in CI, run `pnpm install --no-frozen-lockfile`.

Inside a [workspace], `pnpm install` installs all dependencies in all the
projects. If you want to disable this behavior, set the
[`recursiveInstall`](../settings/cli.md#recursiveinstall) setting to `false`.

When a workspace project changes, unchanged direct dependencies in other projects
keep their locked versions if those versions still satisfy their declared ranges.
Use [`pnpm update`](update.md) to request dependency updates.

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

### --optional

Install `optionalDependencies` even when the [`optional`](../settings/dependency-resolution.md#optional) setting excludes them.

### --no-runtime

Skip installing runtime entries (e.g. Node.js downloaded via [`devEngines.runtime`](../package_json.md#devenginesruntime)). The lockfile is left untouched, so frozen installs still validate; only the runtime fetch and bin-linking are skipped.

This is useful in CI matrices where the runtime is provisioned externally (e.g. via `pnpm runtime -g set node <version>`) before `pnpm install` runs.

This can also be set with `runtime: false` in `pnpm-workspace.yaml`.

## Options

### --ignore-workspace

Install the current project as a standalone project, without discovering a
`pnpm-workspace.yaml` in a parent directory. The workspace's settings and other
projects do not apply. See the [`--ignore-workspace` global option](../pnpm-cli.md#--ignore-workspace).

### --force

Force reinstall dependencies: refetch packages modified in store, recreate a lockfile and/or modules directory created by a non-compatible version of pnpm. `--force` also lifts [`engineStrict`](../settings/cli.md#enginestrict).

Since v12.7.0, `--force` no longer installs optional dependencies whose `os`, `cpu`, or `libc` do not match the host. Set [`forceIgnoresPlatform`](../settings/dependency-resolution.md#forceignoresplatform) to `true` to install them anyway.

### --offline

* Default: **false**
* Type: **Boolean**

If `true`, pnpm will use only packages already available in the store.
If a package won't be found locally, the installation will fail.

### --no-offline

Allow network requests even when the `offline` setting is `true`.

### --prefer-offline

* Default: **false**
* Type: **Boolean**

If `true`, staleness checks for cached data will be bypassed, but missing data
will be requested from the server. To force full offline mode, use `--offline`.

### --no-prefer-offline

Don't prefer cached data even when the `preferOffline` setting is `true`.

### --no-lockfile

Don't read or generate a `pnpm-lock.yaml` file.

### --lockfile-only

* Default: **false**
* Type: **Boolean**

When used, only updates `pnpm-lock.yaml` and `package.json`. Nothing gets written to the `node_modules` directory.

### --lockfile-dir &lt;dir\>

The directory in which `pnpm-lock.yaml` is created. Several projects can share one lockfile. It cannot be combined with `--global`.

### --dry-run

Run a full dependency resolution and report what a real install would change, without writing anything to disk. No lockfile, manifest, or `node_modules` changes are saved.

A completed dry run exits with code 0, even when it reports that a real install would update the lockfile.

`--dry-run` cannot be used with a configured pnpr server, because that install path resolves and links through the server.

### --fix-lockfile

Fix broken lockfile entries automatically.

### --update-checksums

Re-resolve every locked package and record the integrity the registry reports now in `pnpm-lock.yaml`. Use it when a registry rewrote its tarballs, or after a registry starts serving [tars](../registries.md#tar-registries).

By default, an integrity mismatch is a hard failure: `pnpm install` exits with `ERR_PNPM_TARBALL_INTEGRITY` rather than silently re-resolving from the registry and overwriting the locked integrity. This protects projects that ship a committed lockfile from a compromised registry, proxy, or republished version substituting attacker-controlled content on a clean machine.

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

This setting is `true` by default in [CI environments]. pnpm detects a CI environment with the same rules as the [ci-info](https://github.com/watson/ci-info) package. Set `CI=false` to turn detection off.

[CI environments]: https://github.com/watson/ci-info#supported-ci-tools

### --no-frozen-lockfile

Allows lockfile updates during installation. This is the inverse of `--frozen-lockfile`.

### --prefer-frozen-lockfile

Use the existing lockfile without re-resolving, even when the manifest might have changed. The inverse is `--no-prefer-frozen-lockfile`, which always re-resolves against the registry. See [`preferFrozenLockfile`](../settings/store.md#preferfrozenlockfile).

### --ignore-manifest-check

Skip the check that `pnpm-lock.yaml` is up to date with `package.json` under `--frozen-lockfile`. Use it only when you just wrote the lockfile yourself.

### --trust-lockfile

Skip verifying the lockfile against supply-chain policies. `--no-trust-lockfile` verifies it even when the configuration trusts it. See [`trustLockfile`](../settings/dependency-resolution.md#trustlockfile).

### --auto-dedupe

Deduplicate compatible dependency versions during installation. `--no-auto-dedupe` turns it off when the configuration turns it on. See [`autoDedupe`](../settings/dependency-resolution.md#autodedupe).

### --merge-git-branch-lockfiles

Merge all git branch lockfiles.
[Read more about git branch lockfiles.](../git_branch_lockfiles)

### --merge-git-branch-lockfiles-branch-pattern &lt;glob\>

Merge the git branch lockfiles when the current branch matches the pattern. The option can be repeated. See [`mergeGitBranchLockfilesBranchPattern`](../settings/store.md#mergegitbranchlockfilesbranchpattern).


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
* **ndjson** - the most verbose reporter. Prints all logs in [ndjson](https://github.com/ndjson/ndjson-spec) format. Fatal error records include the error code. Dependency resolution errors also include the parent packages in `pkgsStack`.

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

### --no-ignore-scripts

Run lifecycle scripts even when the `ignoreScripts` setting is `true`.

### --ignore-pnpmfile

Don't load the hooks in `.pnpmfile.cjs`, including the pnpmfiles of config dependencies.

### --node-linker &lt;linker\>

* Type: **isolated**, **hoisted**, **pnp**

Override the [`nodeLinker`](../settings/node-modules.md#nodelinker) setting for this install.

### --frozen-store

Open the store read-only and skip all store writes, for a store on a read-only filesystem. Use it with `--offline --frozen-lockfile`. `--no-frozen-store` allows store writes when the configuration turns the read-only store on. See [`frozenStore`](../settings/store.md#frozenstore).

### --network-concurrency &lt;number\>

Override the [`networkConcurrency`](../settings/network.md#networkconcurrency) setting.

### --fetch-timeout &lt;ms\>

Override the [`fetchTimeout`](../settings/network.md#fetchtimeout) setting.

### --fetch-warn-timeout-ms &lt;ms\>

Override the [`fetchWarnTimeoutMs`](../settings/network.md#fetchwarntimeoutms) setting.

### --fetch-min-speed-ki-bps &lt;number\>

Override the [`fetchMinSpeedKiBps`](../settings/network.md#fetchminspeedkibps) setting.

### --user-agent &lt;string\>

The `User-Agent` header to send with registry requests.

### --pnpr-server &lt;url\>

Override the [`pnprServer`](../settings/dependency-resolution.md#pnprserver) setting.

### --allow-build

Added in: v12.7.0

Allow the named package to run its lifecycle scripts during this install, and
record it in the [`allowBuilds`](../settings/build.md#allowbuilds) field of
`pnpm-workspace.yaml` so it stays allowed. Prefix a name with `!` to deny its
scripts instead. The option can be repeated:

```sh
pnpm install --allow-build=esbuild --allow-build='!core-js'
```

The quotes keep a POSIX shell from treating `!` as history expansion. In
Windows `cmd.exe`, leave them out.

writes:

```yaml title="pnpm-workspace.yaml"
allowBuilds:
  esbuild: true
  core-js: false
```

### --filter &lt;package_selector>

[Read more about filtering.](../filtering.md)

import CpuFlag from '../settings/_cpuFlag.mdx'

<CpuFlag />

import OsFlag from '../settings/_osFlag.mdx'

<OsFlag />

import LibcFlag from '../settings/_libcFlag.mdx'

<LibcFlag />
