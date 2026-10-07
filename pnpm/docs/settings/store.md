---
id: store
title: "Store & Lockfile Settings"
sidebar_label: "Store & lockfile"
---

## Store Settings

### storeDir

* Default:
  * If the **$PNPM_HOME** env variable is set, then **$PNPM_HOME/store**
  * If the **$XDG_DATA_HOME** env variable is set, then **$XDG_DATA_HOME/pnpm/store**
  * On Windows: **~/AppData/Local/pnpm/store**
  * On macOS: **~/Library/pnpm/store**
  * On Linux: **~/.local/share/pnpm/store**
* Type: **path**

The location where all the packages are saved on the disk.

The store should be always on the same disk on which installation is happening,
so there will be one store per disk. If there is a home directory on the current
disk, then the store is created inside it. If there is no home on the disk,
then the store is created at the root of the filesystem. For
example, if installation is happening on a filesystem mounted at `/mnt`,
then the store will be created at `/mnt/.pnpm-store`. The same goes for Windows
systems.

It is possible to set a store from a different disk but in that case pnpm will
copy packages from the store instead of hard-linking them, as hard links are
only possible on the same filesystem.

If no directory above the project accepts a hard link at all — an agent sandbox
that grants write access only to the project, or a container with just the
project bind-mounted writable — the store is created inside the project instead:
at `node_modules/.pnpm-store`.
Putting it in the home directory there would either fail on a read-only home or
land on another volume, which copies every package instead of hard-linking it
([#13525](https://github.com/pnpm/pnpm/issues/13525)).

pnpm keeps the locks that coordinate processes sharing a store outside the
store, in a per-user directory. On Linux and macOS that directory is
**$XDG_RUNTIME_DIR** when it is an absolute path to a directory the user owns
and can create files in (write and search permission), and other users cannot
write to. Otherwise the directory is **/tmp**. On Windows it is **~/AppData/Local**. A sandbox that blocks writes to
`/tmp` can set `XDG_RUNTIME_DIR` to a directory it allows. Processes that use
the same store should see the same `XDG_RUNTIME_DIR`, since pnpm processes only
wait for each other when they share the lock directory.

:::important

The pnpm store is intended to be shared only between mutually trusted users, jobs, and processes. If you configure a shared `storeDir`, protect it with filesystem permissions so untrusted users cannot write to it. The store is part of pnpm's trust domain: packages may be hard linked from it, and the store index (`index.db`) records the hashes used to verify cached files.

:::

### verifyStoreIntegrity

* Default: **true**
* Type: **Boolean**

By default, if a file in the store has been modified, the content of this file is checked before linking it to a project's `node_modules`. If `verifyStoreIntegrity` is set to `false`, files in the content-addressable store will not be checked during installation.

This setting helps detect accidental store corruption. It does not make a store that is writable by untrusted users safe, because an attacker who can write to the store can alter both cached package contents and the metadata used to verify them.

### useRunningStoreServer

:::danger

Deprecated feature

:::

* Default: **false**
* Type: **Boolean**

Only allows installation with a store server. If no store server is running,
installation will fail.

### strictStorePkgContentCheck

* Default: **true**
* Type: **Boolean**

Some registries allow the exact same content to be published under different package names and/or versions. This breaks the validity checks of packages in the store. To avoid errors when verifying the names and versions of such packages in the store, you may set the `strictStorePkgContentCheck` setting to `false`.

### frozenStore

Added in: v11.7.0

* Default: **false**
* Type: **Boolean**

Lets `pnpm install` run against a package store that lives on a read-only filesystem — for example a [Nix](https://nixos.org/) store, a read-only bind mount, or an OCI image layer. When enabled, pnpm opens the store's SQLite `index.db` in immutable mode (bypassing the WAL/`-shm` sidecar files that otherwise can't be created on a read-only directory) and suppresses every code path that would write to the store.

Ordinary installs can run in parallel and populate the same store on a local filesystem on the same machine. Leave `frozenStore` disabled for that use case.

With `frozenStore` enabled, no other process may modify the store during the install. Immutable reads bypass SQLite locking and can report "database disk image is malformed" if another job changes the database. This restriction does not apply to `--frozen-lockfile`, which still allows writes to the store.

Pair it with `--offline` and `--frozen-lockfile` against a fully-populated store:

```sh
pnpm install --frozen-store --offline --frozen-lockfile
```

The store must already contain everything the install needs, including the build output of any package whose lifecycle scripts are approved (or that has a patch applied). Under the [global virtual store](./node-modules.md#enableglobalvirtualstore), those package directories live inside the store, so if a required build is missing the install fails up front with `ERR_PNPM_FROZEN_STORE_NEEDS_BUILD` — seed the store with those builds first. If the store is missing its content directory entirely, the install fails fast with `ERR_PNPM_FROZEN_STORE_INCOMPLETE` rather than trying to initialize it.

`frozenStore` is incompatible with `--force` and with a configured pnpr server, since both write into the store. The [side effects cache](./build.md#sideeffectscache) is not written either.

:::note

The read-only store open requires Node.js >=22.15.0, >=23.11.0, or >=24.0.0. On older runtimes, `--frozen-store` fails with `ERR_PNPM_FROZEN_STORE_UNSUPPORTED_NODE`.

:::

## Lockfile Settings

### lockfile

* Default: **true**
* Type: **Boolean**

When set to `false`, pnpm won't read or generate a `pnpm-lock.yaml` file.

### frozenLockfile

* Default:
  * For non-CI: **false**
  * For CI: **true**, if a lockfile is present
* Type: **Boolean**

When set to `true`, pnpm fails the installation if the lockfile needs updating
or is missing.

In [CI environments](../cli/install.md#--frozen-lockfile), this setting defaults
to `true` when a lockfile is present.

### preferFrozenLockfile

* Default: **true**
* Type: **Boolean**

When set to `true` and the available `pnpm-lock.yaml` satisfies the
`package.json` dependencies directive, a headless installation is performed. A
headless installation skips all dependency resolution as it does not need to
modify the lockfile.

### lockfileIncludeTarballUrl

* Default: **false**
* Type: **Boolean**

Add the full URL to the package's tarball to every entry in `pnpm-lock.yaml`.

### lockfile.includeResolutionSettings

Added in: v12.10.0

* Default: **false**
* Type: **Boolean**

When set to `true`, pnpm records these settings in the `settings` section of `pnpm-lock.yaml`:

* [autoDedupe](./dependency-resolution.md#autodedupe)
* [dedupeInjectedDeps](../workspaces.md#dedupeinjecteddeps)
* [dedupePeerDependents](./peer-dependencies.md#dedupepeerdependents)
* [linkWorkspacePackages](../workspaces.md#linkworkspacepackages)

```yaml title="pnpm-workspace.yaml"
lockfile:
  includeResolutionSettings: true
```

A lockfile that records other values than the current settings is outdated. `pnpm install` resolves it again, and `pnpm install --frozen-lockfile` fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH`. Turning this setting on makes the next install resolve the lockfile once.

With `autoDedupe`, an install reuses a lockfile that records `autoDedupe: true` when nothing else changed. This also holds on a machine that did not write the lockfile, such as a server that runs `pnpm install --frozen-lockfile` and then `pnpm run`.

pnpm v11 and earlier v12 versions do not keep these settings and remove them when they rewrite the lockfile. pnpm v12 versions before v12.10.0 fail to read a `pnpm-workspace.yaml` that sets `lockfile` to an object. Turn this setting on when everyone working on the project uses v12.10.0 or later.

This setting needs local dependency resolution, so an install that is not frozen fails when `pnprServer` is set.

### gitBranchLockfile

* Default: **false**
* Type: **Boolean**

When set to `true`, the generated lockfile name after installation will be named
based on the current branch name to completely avoid merge conflicts. For example,
if the current branch name is `feature-foo`, the corresponding lockfile name will
be `pnpm-lock.feature-foo.yaml` instead of `pnpm-lock.yaml`. It is typically used
in conjunction with the command line argument `--merge-git-branch-lockfiles` or by
setting `mergeGitBranchLockfilesBranchPattern` in the `pnpm-workspace.yaml` file.

### mergeGitBranchLockfilesBranchPattern

* Default: **null**
* Type: **Array or null**

This configuration matches the current branch name to determine whether to merge
all git branch lockfile files. By default, you need to manually pass the
`--merge-git-branch-lockfiles` command line parameter. This configuration allows
this process to be automatically completed.

For instance:

```yaml title="pnpm-workspace.yaml"
mergeGitBranchLockfilesBranchPattern:
- main
- release*
```

You may also exclude patterns using `!`.

### peersSuffixMaxLength

* Default: **1000**
* Type: **number**

Max length of the peer IDs suffix added to dependency keys in the lockfile. If the suffix is longer, it is replaced with a hash.
