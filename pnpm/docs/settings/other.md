---
id: other
title: "Other Settings"
sidebar_label: "Other"
---

### savePrefix

* Default: **'^'**
* Type: **'^'**, **'~'**, **''**, **'='**

Configure how versions of packages installed to a `package.json` file get
prefixed.

For example, if a package has version `1.2.3`, by default its version is set to
`^1.2.3` which allows minor upgrades for that package, but after
`pnpm config set save-prefix='~'` it would be set to `~1.2.3` which only allows
patch upgrades.

Since v11.19.0, `=` is also accepted: newly added dependencies are saved with an
explicit `=` operator (`=1.2.3`), which pins the exact version. `pnpm update`
keeps the `=` operator when it updates such a pin.

This setting is ignored when the added package has a range specified. For
instance, `pnpm add foo@2` will set the version of `foo` in `package.json` to
`2`, regardless of the value of `savePrefix`.

### saveTypes

Added in: v12.6.0

* Default: **false**
* Type: **Boolean**

When set to `true`, `pnpm add` saves available `@types/*` packages in
`devDependencies` alongside registry dependencies. Packages that declare bundled
TypeScript types are skipped.

```yaml title="pnpm-workspace.yaml"
saveTypes: true
```

You can also pass it as a CLI flag: `pnpm add --save-types express`.

### tag

* Default: **latest**
* Type: **String**

If you `pnpm add` a package and you don't provide a specific version, then it
will install the package at the version registered under the tag from this
setting.

This also sets the tag that is added to the `package@version` specified by the
`pnpm tag` command if no explicit tag is given.

### globalDir

* Default:
  * If the **$XDG_DATA_HOME** env variable is set, then **$XDG_DATA_HOME/pnpm/global**
  * On Windows: **~/AppData/Local/pnpm/global**
  * On macOS: **~/Library/pnpm/global**
  * On Linux: **~/.local/share/pnpm/global**
* Type: **path**

Specify a custom directory to store global packages.

### globalBinDir

* Default:
  * If the **$XDG_DATA_HOME** env variable is set, then **$XDG_DATA_HOME/pnpm/bin**
  * On Windows: **~/AppData/Local/pnpm/bin**
  * On macOS: **~/Library/pnpm/bin**
  * On Linux: **~/.local/share/pnpm/bin**
* Type: **path**

Allows to set the target directory for the bin files of globally installed packages.

:::tip

In pnpm v11, globally installed binaries are stored in a `bin` subdirectory of `PNPM_HOME` instead of directly in `PNPM_HOME`. This prevents internal directories like `global/` and `store/` from polluting shell autocompletion when `PNPM_HOME` is on PATH. After upgrading, run `pnpm setup` to update your shell configuration.

:::

### globalShims

Added in: v12.0.0-rc.2

* Default: **\{ node: auto, deno: auto, bun: auto \}**
* Type: **Boolean**, **Object**

Controls which globally installed packages get [project-aware shims](../global-packages.md#project-aware-global-bins) — global commands that run the version the current project asks for instead of the globally installed one.

The setting is a map from **package name** to policy. The key is the name of the package that *provides* the command, not the command itself, so an entry for `typescript` covers its `tsc` bin.

```yaml
globalShims:
  node: auto
  deno: false
  typescript: prompt
```

The supported policies are:

| Value | Behavior |
|---|---|
| `auto` (or `true`) | Switch automatically when the candidate is authenticated by a publisher signature; otherwise ask for confirmation once. |
| `prompt` | Put every candidate through the confirmation gate, including signature-verified ones. Answers are still remembered, so this asks once per project and candidate, not on every run. |
| `always` | Always switch, never ask. Usable in CI, where a prompt would fall back to the global version. |
| `false` | Disable the project-aware shim for this package. |

Since v12.0.0-rc.6, two commands write entries here for you: [`pnpm shim add <pkg>`](../cli/shim.md) records the package it links a shim for, and installing a [package manager](../package-managers.md) globally (`pnpm add -g yarn`) records that package manager so it follows a project's pin. Neither overwrites an entry you set yourself — including `false`, and including a `globalShims: false` that turns every shim off.

Layers merge key by key over the built-in defaults, so a single entry can change one package without restating the rest — `globalShims: { bun: false }` leaves `node` and `deno` at `auto`.

The scalar shorthands replace the whole map instead of merging: `globalShims: false` disables every project-aware shim, and `globalShims: true` resets to the defaults.

:::warning

This setting is only read from locations a project cannot write to: the [global configuration file](../cli/config.md), a `pnpm-workspace.yaml` in the pnpm home directory itself, and the `PNPM_CONFIG_GLOBAL_SHIMS` environment variable (a JSON value), applied in that order. A project's own `pnpm-workspace.yaml` is ignored — otherwise a repository could grant itself the right to run its own binaries in place of your global ones.

:::

Disabling a package, or changing its policy, takes effect on the very next command — the setting is re-read on each dispatch, so no reinstall is needed.

Newly *enabling* a package is the exception. pnpm decides at install time which bins to write dispatching shims for, so a package that was already installed globally while it was disabled needs to be reinstalled to pick up the change:

```sh
pnpm add -g typescript
```

To bypass dispatch for a single invocation, set `PNPM_SHIM_BYPASS=1`:

```sh
PNPM_SHIM_BYPASS=1 node --version
```

:::note

Project-aware shims are a pnpm v12 feature and are not available in v11.

:::

### npmrcAuthFile

Added in: v11.0.0

* Default: **~/.npmrc**
* Type: **path**

The path to a file containing registry authentication tokens. By default, pnpm reads auth tokens from `~/.npmrc` as a fallback for registry authentication. Use this setting to point to a different file instead.

This setting cannot be set in `pnpm-workspace.yaml` at the project level; set it in the global configuration file, via the `--npmrc-auth-file` CLI option, or via the `PNPM_CONFIG_NPMRC_AUTH_FILE` environment variable (the npm-style `NPM_CONFIG_USERCONFIG` is honored as a fallback). A relative path is resolved against the working directory.

### stateDir

* Default:
  * If the **$XDG_STATE_HOME** env variable is set, then **$XDG_STATE_HOME/pnpm**
  * On Windows: **~/AppData/Local/pnpm-state**
  * On macOS: **~/.pnpm-state**
  * On Linux: **~/.local/state/pnpm**
* Type: **path**

The directory where pnpm stores machine-level state, including the update checker's `pnpm-state.json` file.

Since v12.5.0, [task concurrency groups](../workspace-task-orchestration.md#concurrencygroups) also store their shared slot files under `stateDir/run-slots`. Processes must use the same `stateDir` to share group limits. Different state directories create independent slot pools, so their combined task count can exceed the configured limit.

Set `stateDir` in the [global configuration file](../cli/config.md) or on the command line. It is ignored in project `pnpm-workspace.yaml` files.

### cacheDir

* Default:
  * If the **$XDG_CACHE_HOME** env variable is set, then **$XDG_CACHE_HOME/pnpm**
  * On Windows: **~/AppData/Local/pnpm-cache**
  * On macOS: **~/Library/Caches/pnpm**
  * On Linux: **~/.cache/pnpm**
* Type: **path**

The location of the cache (package metadata, dlx cache, and some install verification results).

Like the store, the cache directory is intended to be shared only between mutually trusted users, jobs, and processes. If you configure or restore a shared `cacheDir`, protect it with filesystem permissions so untrusted users cannot write to it.

### useStderr

* Default: **false**
* Type: **Boolean**

When true, all the output is written to stderr.

### updateNotifier

* Default: **true**
* Type: **Boolean**

Set to `false` to suppress the update notification when using an older version of pnpm than the latest.

`pnpm install` and `pnpm add` check at most once a day, and print how to get the
newer version. pnpm 12 recognized but ignored this setting until v12.0.0, where
it started checking too.

### preferSymlinkedExecutables

* Default: **true**, when **node-linker** is set to **hoisted** and the system is POSIX
* Type: **Boolean**

Create symlinks to executables in `node_modules/.bin` instead of command shims. This setting is ignored on Windows, where only command shims work.

### ignoreCompatibilityDb

* Default: **false**
* Type: **Boolean**

During installation the dependencies of some packages are automatically patched. If you want to disable this, set this config to `true`.

The patches are applied from Yarn's [`@yarnpkg/extensions`] package, plus pnpm's
own curated entries.

Since v12.0.0, the database no longer carries entries that were derived from
static analysis of published packages. Those entries named packages a dependent
imported only for its *types*. Adding them was at best unnecessary and at worst
broke the dependent: `@typescript-eslint/types` gained a `typescript` dependency
resolved to the newest release, which put TypeScript 7 under older
`@typescript-eslint` versions and made ESLint fail with `Cannot read properties
of undefined (reading 'Intrinsic')`.

### resolutionMode

* Default: **highest** (was **lowest-direct** from v8.0.0 to v8.6.12)
* Type: **highest**, **time-based**, **lowest-direct**

When `resolutionMode` is set to `time-based`, dependencies will be resolved the following way:

1. Direct dependencies will be resolved to their lowest versions. So if there is `foo@^1.1.0` in the dependencies, then `1.1.0` will be installed.
1. Subdependencies will be resolved from versions that were published no later than one hour after the newest direct dependency. If none of those versions satisfies a subdependency's range, pnpm installs the lowest version that does.

If [`minimumReleaseAge`](./dependency-resolution.md#minimumreleaseage) is also set, pnpm resolves subdependencies against whichever of the two cutoffs is earlier. If no version published before that cutoff satisfies a subdependency's range, pnpm installs the lowest matching version that is older than the `minimumReleaseAge` cutoff. If every matching version is newer than the `minimumReleaseAge` cutoff, pnpm falls back to the lowest matching version, and [`minimumReleaseAgeStrict`](./dependency-resolution.md#minimumreleaseagestrict) decides whether pnpm installs it or fails. Only the `minimumReleaseAge` cutoff decides which versions pnpm reports as too new. A version that is newer than the time-based cutoff but older than the `minimumReleaseAge` cutoff is installed without a warning or error.

With this resolution mode installations with warm cache are faster. It also reduces the chance of subdependency hijacking as subdependencies will be updated only if direct dependencies are updated.

This resolution mode works only with npm's [full metadata]. So it is slower in some scenarios. However, if you use [Verdaccio] v5.15.1 or newer, you may set the `registrySupportsTimeField` setting to `true`, and it will be really fast.

When `resolutionMode` is set to `lowest-direct`, direct dependencies will be resolved to their lowest versions.

Only the dependencies declared in `package.json` count as direct here. A peer dependency that [`autoInstallPeers`](./peer-dependencies.md#autoinstallpeers) adds is not something the project declared, so it is resolved like a subdependency: to the highest version satisfying the peer range, or under `time-based`, to the highest version within the publish-date cutoff.

### registrySupportsTimeField

* Default: **false**
* Type: **Boolean**

Set this to `true` if the registry that you are using returns the "time" field in the abbreviated metadata. [Verdaccio] supports this from v5.15.1, as do some registry proxies.

Since v11.23.0, this can also be declared per registry, through the `supportsTimeField` field of a [registry declaration](../registries.md#supportstimefield). A registry's own declaration wins; this setting is the answer for every registry the project does not describe.

### extendNodePath

* Default: **true**
* Type: **Boolean**

When `true`, pnpm sets the `NODE_PATH` environment variable in command shims
(the wrapper scripts created in `node_modules/.bin`). When `false`, `NODE_PATH`
is not set.

#### Why this is needed

pnpm's [isolated `node_modules` layout] means that a package can only access its
own declared dependencies. However, when a CLI tool runs via a command shim, some
libraries (notably [`import-local`], used by jest, eslint, and others) resolve
modules from the **current working directory** rather than from the binary's own
location. Since the working directory is the project root — not the package inside
the virtual store — the standard `node_modules` resolution from the CWD won't
find the binary's transitive dependencies.

To bridge this gap, pnpm includes two types of paths in `NODE_PATH`:

1. **The package's own dependencies directory** (e.g.,
   `.pnpm/pkg@version/node_modules`) — this allows CWD-based resolution to find
   the correct versions of the package's sibling dependencies.
2. **The hoisted `node_modules` directory** (e.g., `.pnpm/node_modules`) — this
   is the directory where hoisted packages are placed when [`hoistPattern`] is
   set. Node.js cannot discover this directory through its standard resolution
   algorithm, so it must be provided via `NODE_PATH`.

`NODE_PATH` is also essential when [`enableGlobalVirtualStore`] is enabled.
With a global virtual store, packages are symlinked from a central location
outside the project, so Node.js's standard upward `node_modules` traversal from
the binary's real path won't reach the project's own `node_modules` or its hoisted
dependencies. In this case, `NODE_PATH` must include both the project's root
`node_modules` and the hoisted directory at `node_modules/.pnpm/node_modules` to
ensure correct resolution.

#### When to disable

You may set this to `false` if you are certain that none of the CLI tools in your
project resolve modules from the working directory and you are not using a global
virtual store. Disabling it produces slightly simpler command shims.

[isolated `node_modules` layout]: ../symlinked-node-modules-structure.md
[`import-local`]: https://github.com/sindresorhus/import-local
[`hoistPattern`]: ./node-modules.md#hoistpattern
[`enableGlobalVirtualStore`]: ./node-modules.md#enableglobalvirtualstore

[`@yarnpkg/extensions`]: https://github.com/yarnpkg/berry/blob/master/packages/yarnpkg-extensions/sources/index.ts
[full metadata]: https://github.com/npm/registry/blob/master/docs/responses/package-metadata.md#full-metadata-format
[Verdaccio]: https://verdaccio.org/

### deployAllFiles

* Default: **false**
* Type: **Boolean**

When deploying a package or installing a local package, all files of the package are copied. By default, if the package has a `"files"` field in the `package.json`, then only the listed files and directories are copied.

### dedupeDirectDeps

* Default: **false**
* Type: **Boolean**

When set to `true`, dependencies that are already symlinked to the root `node_modules` directory of the workspace will not be symlinked to subproject `node_modules` directories.

### optimisticRepeatInstall

Added in: v10.1.0

* Default: **true**
* Type: **Boolean**

When enabled, a fast check will be performed before proceeding to installation. This way a repeat install or an install on a project with everything up-to-date becomes a lot faster.

When the check finds that nothing changed, pnpm skips the install, including the projects' own lifecycle scripts, such as `prepare`. Set this to `false` to run those scripts on every install.

### requiredScripts

Scripts listed in this array will be required in each project of the workspace. Otherwise, `pnpm -r run <script name>` will fail.

```yaml title="pnpm-workspace.yaml"
requiredScripts:
- build
```

import EnablePrePostScripts from './_enablePrePostScripts.mdx'

<EnablePrePostScripts />

import ScriptShell from './_scriptShell.mdx'

<ScriptShell />

import ShellEmulator from './_shellEmulator.mdx'

<ShellEmulator />

import CatalogMode from './_catalogMode.mdx'

<CatalogMode />

### ci

Added in: v10.12.1

* Default: **true** (when the environment is detected as CI)
* Type: **Boolean**

This setting explicitly tells pnpm whether the current environment is a CI (Continuous Integration) environment.

import CatalogPrune from './_catalogPrune.mdx'

<CatalogPrune />

### macosBackup.excludeModulesDir

Added in: v12.6.0

* Default: **false**
* Type: **Boolean**

When set to `true` on macOS, pnpm marks newly created `node_modules`, virtual-store, and similar directories with the `com.apple.metadata:com_apple_backup_excludeItem` extended attribute, so Time Machine skips them.

Set it in the [global configuration file](../cli/config.md) or via the `PNPM_CONFIG_MACOS_BACKUP_EXCLUDE_MODULES_DIR` environment variable. This setting is ignored in project `pnpm-workspace.yaml` files and on non-macOS platforms.

```yaml title="config.yaml"
macosBackup:
  excludeModulesDir: true
```

### macosBackup.excludeStoreDir

Added in: v12.6.0

* Default: **false**
* Type: **Boolean**

When set to `true` on macOS, pnpm marks newly created package-store directories with the Time Machine exclusion attribute.

Set it in the [global configuration file](../cli/config.md) or via the `PNPM_CONFIG_MACOS_BACKUP_EXCLUDE_STORE_DIR` environment variable. This setting is ignored in project `pnpm-workspace.yaml` files and on non-macOS platforms.

```yaml title="config.yaml"
macosBackup:
  excludeStoreDir: true
```
