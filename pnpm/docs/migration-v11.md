---
id: migration-v11
title: Migrating from pnpm 11
---

pnpm 12 reads the same `pnpm-lock.yaml`, `pnpm-workspace.yaml`, and store as pnpm 11, so most projects upgrade without edits. This page lists the steps and the behavior changes that can need action.

## Steps

1. If the project still uses pnpm 10, follow [Migrating from v10](./migration.md) first. Its codemod also applies when moving to pnpm 12. No codemod exists for the move from pnpm 11.
2. Update pnpm. In a project that pins pnpm through `packageManager` or `devEngines.packageManager`, run this in the workspace root to update the pin as well:

   ```sh
   pnpm self-update 12
   ```

   See [Installation](./installation.md) for other ways to install pnpm 12. If you install it another way, also update the pin, for example by running `pnpm self-update 12` with pnpm 12. Otherwise pnpm 12 runs the pinned pnpm 11. pnpm 12 is a native executable and does not need Node.js to run.
3. Run `pnpm install` and fix any setting it reports as not recognized in `pnpm-workspace.yaml`. See [Unrecognized settings fail](#unrecognized-settings-fail).
4. Read the [behavior changes](#behavior-changes) below and act on those that apply to your project. CI scripts, private Git dependencies, and pnpmfile hooks are the usual places to look.
5. Commit the updated `package.json` and `pnpm-lock.yaml`.

## Behavior changes

### Unrecognized settings fail

pnpm 11 warned about a setting in `pnpm-workspace.yaml` that it did not recognize. In pnpm 12, if the project pins a pnpm version the running pnpm satisfies, that warning is an error, `ERR_PNPM_UNRECOGNIZED_WORKSPACE_SETTINGS`. Otherwise it stays a warning. pnpm 12 also rejects `confirmModulesPurge`, a pnpm 11 setting. Remove the setting or fix its spelling. `pnpm config` commands never fail on it.

### Boolean flags with a separate value

pnpm rejects a boolean flag followed by its value as a separate argument, such as `--frozen-lockfile false` or `--prod false`. Write `--frozen-lockfile=false` or `--no-frozen-lockfile` instead.

### Git-hosted dependencies

A dependency on GitHub, GitLab, or Bitbucket always resolves through the host's HTTPS URL, and the lockfile never records an SSH URL for it. pnpm no longer probes the network to choose between HTTPS and SSH. Self-hosted Git servers and URLs with embedded credentials keep their exact URL. An SSH URL already in the lockfile changes only when pnpm re-resolves that entry.

To fetch a private hosted repository over SSH, configure Git on the machine. See [Using SSH for private repositories](./package-sources.md#using-ssh-for-private-repositories).

### pnpmfile hooks

- `importPackage` is not run. Choose the import method with [`packageImportMethod`](./settings/node-modules.md#packageimportmethod).
- pnpm ignores `filterLog` and warns when a pnpmfile defines it. Use [`loglevel`](./settings/cli.md#loglevel) instead.

### `engineStrict`

- An install fails when an incompatible package is reached through a regular `dependencies` edge, even if that subtree hangs off an `optionalDependencies` entry. pnpm 11 installs the package and warns. Packages reachable only through optional edges are still skipped.
- With [`--no-runtime`](./cli/install.md#--no-runtime) or `runtime: false`, pnpm checks the root project's `engines.node` against the active Node.js, not the version pinned in `devEngines.runtime` or `engines.runtime`.

### Package managers and runtimes named as packages

`pnpm add yarn`, `pnpm add -g node`, `pnx node`, and similar commands provision the package manager or runtime instead of installing the npm package that shares its name. See [What changes for a project coming from v11](./package-managers.md#what-changes-for-a-project-coming-from-v11).

### Global Node.js follows the project

A Node.js installed globally with pnpm runs the version the current project pins in `devEngines.runtime`, `engines.runtime`, `.node-version`, or `.nvmrc`. pnpm downloads that version on first use. A version without a publisher signature, such as a Node.js prerelease, asks once whether you trust the project. See [Project-aware global bins](./global-packages.md#project-aware-global-bins). To turn this off, set [`globalShims: false`](./settings/other.md#globalshims) in the global configuration, or set `PNPM_SHIM_BYPASS=1` for one command.

### Global commands under `sudo`

`pnpm setup`, `pnpm self-update`, and commands that change the global installation, such as `pnpm add --global`, fail with `ERR_PNPM_SUDO_NOT_SUPPORTED` when run through `sudo`. Run them as your own user. Read-only global commands still work.

### TLS certificates

pnpm trusts the operating system's certificate store for registry connections. If pnpm cannot load that store, it uses a bundled set of Mozilla CA certificates. pnpm 11 used the CA certificates bundled with Node.js. A [`ca`](./npmrc.md#ca) or [`cafile`](./npmrc.md#cafile) setting still replaces these defaults.

### Connections per origin

[`maxsockets`](./settings/network.md#maxsockets) no longer defaults to 50. When it is unset, only [`networkConcurrency`](./settings/network.md#networkconcurrency) bounds direct connections. Connections through a proxy are still limited to 50 per origin. Set `maxsockets` if a registry limits concurrent connections.

### `shellEmulator`

[`shellEmulator`](./settings/other.md#shellemulator) runs scripts with pnpm's built-in shell instead of `@yarnpkg/shell`. In that shell:

- The command receives `${NAME:=word}`, `${NAME=word}`, and `${#NAME}` as literal text.
- `${NAME:-word}` and `${NAME:+word}` read `NAME` from the environment the script starts with, not from an assignment earlier in the same script.
- A script the shell cannot parse fails with `ERR_PNPM_EXECUTOR_SHELL_EMULATOR_PARSE`.

### `packageImportMethod: auto` on Linux

On Linux, `auto` tries hard links before cloning. On a filesystem that supports cloning, such as btrfs, set [`packageImportMethod: clone`](./settings/node-modules.md#packageimportmethod) to keep copy-on-write files in `node_modules`.

### Lockfile entries for dependency cycles

pnpm breaks dependency cycles the same way regardless of install order. The first install that re-resolves dependencies can rewrite peer variants of packages inside a cycle once. Frozen-lockfile installs use the existing lockfile unchanged. See [Cyclic dependencies](./how-peers-are-resolved.md#cyclic-dependencies).

### `pnpm dlx` and `pnx`

`pnpm dlx` and `pnx` do not use the [global virtual store](./global-virtual-store.md). Each cache entry keeps its own virtual store, and the command it runs does not get the global virtual store's module resolution hook.

### `pnpm pack-app`

Without `--runtime` or `pnpm.app.runtime`, [`pnpm pack-app`](./cli/pack-app.md) embeds Node.js 25.5.0. pnpm 11 embedded the version of Node.js that ran pnpm. pnpm downloads the Node.js it needs, so the host does not need Node.js installed.

### `--frozen-store`

[`--frozen-store`](./cli/install.md#--frozen-store) has no Node.js version requirement. The `ERR_PNPM_FROZEN_STORE_UNSUPPORTED_NODE` error no longer exists.

See also the [release notes for 12.0.0](https://github.com/pnpm/pnpm/releases/tag/v12.0.0).
