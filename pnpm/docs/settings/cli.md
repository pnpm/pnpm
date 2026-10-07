---
id: cli
title: "CLI & Node.js Settings"
sidebar_label: "CLI & Node.js"
---

## CLI Settings

### [no-]color

* Default: **auto**
* Type: **auto**, **always**, **never**

Controls colors in the output.

* **auto** - output uses colors when the standard output is a terminal or TTY.
* **always** - ignore the difference between terminals and pipes. You’ll rarely
  want this; in most scenarios, if you want color codes in your redirected
  output, you can instead pass a `--color` flag to the pnpm command to force it
  to use color codes. The default setting is almost always what you’ll want.
* **never** - turns off colors. This is the setting used by `--no-color`.

### progress

Added in: v12.6.0

* Default: **true**
* Type: **Boolean**

Controls whether dependency and download progress lines are printed during installation. When set to `false` (or `--no-progress` is passed), progress output is suppressed. Warnings, lifecycle script output, and the dependency summary are still printed.

### loglevel

* Default: **info**
* Type: **debug**, **info**, **warn**, **error**

Any logs at or higher than the given level will be shown.
You can instead pass `--silent` to turn off all output logs.

### reporter

Selects the log reporter. See [`--reporter`](../cli/install.md#--reportername)
for the available reporters and terminal-dependent defaults.

### useBetaCli

* Default: **false**
* Type: **Boolean**

Experimental option that enables beta features of the CLI. This means that you
may get some changes to the CLI functionality that are breaking changes, or
potentially bugs.

### recursiveInstall

* Default: **true**
* Type: **Boolean**

If this is enabled, the primary behaviour of `pnpm install` becomes that of
`pnpm install -r`, meaning the install is performed on all workspace or
subdirectory packages.

Else, `pnpm install` will exclusively build the package in the current
directory.

### engineStrict

* Default: **false**
* Type: **Boolean**

If this is enabled, pnpm will not install any package that claims to not be
compatible with the current Node version.

Regardless of this configuration, installation will always fail if a project
(not a dependency) specifies an incompatible version in its `engines` field.

Optional dependencies are exempt, because a package that pnpm may skip is not
required to be compatible. Since v12.0.0 the exemption follows the edge rather
than the subtree: an incompatible package reached through a regular
`dependencies` edge of a package that is being installed fails the install, even
when the subtree it sits in hangs off an `optionalDependencies` entry. pnpm 11
installs it and prints an install-check warning instead. A package reachable
only through optional edges, or through a package that was itself skipped, is
still skipped in both versions ([#13286](https://github.com/pnpm/pnpm/issues/13286)).

### npmPath

* Type: **path**

The location of the npm binary that pnpm uses for some actions, like publishing.

### pmOnFail

Added in: v11.0.0

* Default: **download**
* Type: **download**, **error**, **warn**, **ignore**

Overrides the `onFail` behavior of both the `packageManager` field and `devEngines.packageManager` when the running pnpm version does not match the declared one.

* `download` — download and run the declared pnpm version (this is the default and matches the previous `managePackageManagerVersions: true` behavior).
* `error` — fail the command (equivalent to the previous `packageManagerStrictVersion: true`).
* `warn` — print a warning but continue (equivalent to the previous `packageManagerStrict: false` or `COREPACK_ENABLE_STRICT=0`).
* `ignore` — skip the check entirely (equivalent to the previous `managePackageManagerVersions: false`). Useful when version management is handled by an external tool such as asdf, mise, or Volta.

Can be set via CLI flag, environment variable, or `pnpm-workspace.yaml`:

```sh
pnpm install --pm-on-fail=ignore
pnpm_config_pm_on_fail=ignore pnpm install
```

```yaml title="pnpm-workspace.yaml"
pmOnFail: ignore
```

This setting replaces the removed `managePackageManagerVersions`, `packageManagerStrict`, and `packageManagerStrictVersion` settings, as well as the `COREPACK_ENABLE_STRICT` environment variable.

Migration:

| Removed setting                       | Replace with                   |
| ------------------------------------- | ------------------------------ |
| `managePackageManagerVersions: true`  | `pmOnFail: download` (default) |
| `managePackageManagerVersions: false` | `pmOnFail: ignore`             |
| `packageManagerStrict: false`         | `pmOnFail: warn`               |
| `packageManagerStrictVersion: true`   | `pmOnFail: error`              |
| `COREPACK_ENABLE_STRICT=0`            | `pmOnFail: warn`               |

See also [`pnpm with`](../cli/with.md) for running pnpm at a specific version without changing this setting.

### ignoreWorkspaceRootCheck

* Default: **false**
* Type: **Boolean**

If this is enabled, running `pnpm install`/`pnpm add` from the project's root
folder will no longer error when `-w`/`--ignore-workspace-root-check` is not
provided.

## Node.js Settings

### nodeVersion

* Default: the value returned by **node -v**, without the v prefix
* Type: **exact semver version (not a range)**

The Node.js version to use when checking a package's `engines` setting.

If you want to prevent contributors of your project from adding new incompatible dependencies, use `nodeVersion` and `engineStrict` in a `pnpm-workspace.yaml` file at the root of the project:

```yaml title="pnpm-workspace.yaml"
nodeVersion: 12.22.0
engineStrict: true
```

This way, even if someone is using Node.js v22, they will not be able to install a new dependency that doesn't support Node.js v12.22.0.

### runtimeOnFail

Added in: v11.0.0

* Default: **undefined**
* Type: **download**, **error**, **warn**, **ignore**

Overrides the `onFail` field of [`devEngines.runtime`](../package_json.md#devenginesruntime) (and `engines.runtime`) in the root project's `package.json`. This is useful when you want a different local behavior than what is written in the manifest — for instance, forcing pnpm to download the declared runtime even when the manifest sets `onFail: "warn"`:

```yaml title="pnpm-workspace.yaml"
runtimeOnFail: download
```

Since v12.5.0, this setting also controls Python interpreter downloads when no installed interpreter satisfies a project. The unset default permits Python downloads. `warn` and `ignore` use an available interpreter even when it does not satisfy `requires-python`; `error` refuses the install.

### tools

Added in: v12.5.0

* Default: **undefined**
* Type: **Object**

Configure download sources for the programs pnpm installs. Set `tools` in the [global configuration file](../cli/config.md) or as JSON in `PNPM_CONFIG_TOOLS`. Tool mirrors in `pnpm-workspace.yaml` are ignored.

```yaml title="config.yaml"
tools:
  node:
    mirror: https://mirror.example.com/node/download
    channels:
      nightly: https://nightly.example.com/
  bun:
    mirror: https://mirror.example.com/bun
  python:
    mirror: https://mirror.example.com/python-build-standalone/releases
```

`mirror` supplies the base URL for the tool's own download layout. Only `node`, `bun`, and `python` are accepted. Only Node.js supports `channels`: a channel entry overrides the mirror for that release channel, and other channels use `mirror`.

`pnpm pack-app` downloads its embedded Node.js through `tools.node`. The legacy `node-mirror:<channel>` setting continues to work as a channel mirror.

### nodeDownloadMirrors

For pnpm v12.5.0, prefer [`tools.node`](#tools) for machine-level mirrors. `nodeDownloadMirrors` remains supported for compatibility; `pnpm pack-app` uses only `tools.node`.

Added in: v11.0.0

* Default: **undefined**
* Type: **Record&lt;string, string&gt;**

Configure custom Node.js download mirrors in `pnpm-workspace.yaml`. The keys are release channels (`release`, `rc`, `nightly`, `v8-canary`, etc.) and the values are base URLs.

Here is how pnpm may be configured to download Node.js from a mirror in China:

```yaml title="pnpm-workspace.yaml"
nodeDownloadMirrors:
  release: https://npmmirror.com/mirrors/node/
  rc: https://npmmirror.com/mirrors/node-rc/
  nightly: https://npmmirror.com/mirrors/node-nightly/
```

Since v12.4.0, a mirror is machine-level configuration as well: set it in the
[global configuration file](../cli/config.md) with
`pnpm config set --global node-download-mirrors`, or in the environment as
`PNPM_CONFIG_NODE_DOWNLOAD_MIRRORS`, so every project on the machine downloads
Node.js from it.

Since v12.2.0, downloads from a mirror carry the npm registry credentials
configured for that URL, including bearer tokens, basic auth, and `tokenHelper`,
so a mirror behind an authenticating proxy works without a separate credential.
