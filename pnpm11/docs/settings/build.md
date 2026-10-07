---
id: build
title: "Build Settings"
sidebar_label: "Build"
---

### ignoreScripts

* Default: **false**
* Type: **Boolean**

Do not execute any scripts defined in the project `package.json` and its
dependencies.

:::note

This flag does not prevent the execution of [.pnpmfile.mjs](../pnpmfile.md)

:::

### childConcurrency

* Default: **5**
* Type: **Number**

The maximum number of child processes to allocate simultaneously to build
node_modules.

### sideEffectsCache

* Default: **true**
* Type: **Boolean or Object**

Use and cache the results of (pre/post)install hooks.

When a pre/post install script modify the contents of a package (e.g. build output), pnpm saves the modified package in the global store. On future installs on the same machine, pnpm reuses this cached, prebuilt version—making installs significantly faster.

:::note

You may want to disable this setting if:

1. The install scripts modify files *outside* the package directory (pnpm cannot track or cache these changes).
1. The scripts perform side effects that are unrelated to building the package.

:::

An object says the same thing in more detail, and is the canonical way to
declare the remote tier:

```yaml title="pnpm-workspace.yaml"
sideEffectsCache:
  read: true
  write: true
  remote:
    org: acme
    packages:
      - native-addon
```

* `read` — restore a build from the cache when one is present. Default **true**.
* `write` — save a package's build output to the cache. Default **true**.
* `remote` — reuse builds across machines; see below.

`sideEffectsCache: true` is the shorthand for reading and writing, and is what
the setting meant before it grew a remote tier. Writing without reading
(`read: false, write: true`) populates a cache the run never consumes, which is
what a job that warms one for others wants.

Since v11.26.0, the `--side-effects-cache` and `--no-side-effects-cache` flags, and
`PNPM_CONFIG_SIDE_EFFECTS_CACHE`, toggle the **local** cache only. A remote tier
declared under `sideEffectsCache.remote` is left in place, so a run can turn the
local cache off without losing the shared one.

### sideEffectsCacheReadonly

* Default: **false**
* Type: **Boolean**

Only use the side effects cache if present, do not create it for new packages.
The older spelling of `sideEffectsCache: { read: true, write: false }`.

### sideEffectsCache.remote

Added in: v11.25.0

* Default: **undefined**
* Type: **Object**

Opt in to reusing a dependency's build output across machines, by restoring
signed, organization-scoped artifacts through a [pnpr](/pnpr) server instead of
running the package's lifecycle scripts locally. This is a proof of concept: it
is off unless configured, it needs a pnpr server started with
`artifacts.enabled: true`, and it restores artifacts on Linux/glibc, macOS, and
Windows, on x64 and arm64. Other operating systems and libc families build
locally.

A repository declares eligibility and nothing else:

```yaml title="pnpm-workspace.yaml"
pnprServer: http://127.0.0.1:7677
allowBuilds:
  native-addon: true
sideEffectsCache:
  remote:
    org: acme
    packages:
      - native-addon
```

Declaring only `remote` leaves `read` and `write` at their defaults, so the
local cache keeps working as it did.

`packages` is an eligibility list, not a permission: a package is only a
candidate when it also passes [`allowBuilds`](#allowbuilds), has
`requiresBuild: true`, and has a verified source integrity. Listing a package
here does not review its build scripts for you — under the default
[`strictDepBuilds`](#strictdepbuilds) an install still fails with
`ERR_PNPM_IGNORED_BUILDS` when it reaches a build nobody has ruled on, and a
package denied with `allowBuilds: false` is never built, from the cache or
otherwise.

Everything describing the *act of signing* — `publish`, `keyId`, `builderId`,
`imageDigest`, `architectureBaseline`, `buildEnv`, `trustedKeys` and
`privateKey` — is refused in `pnpm-workspace.yaml` with
`ERR_PNPM_WORKSPACE_REMOTE_SIDE_EFFECTS_TRUST`, and is read from the [global
configuration file](../cli/config.md) or the environment instead. A cloned
repository is not a trust root, and must not be able to turn the machine's
signing key into a signing oracle.

Any cache failure — an unreachable server, an unverifiable signature, an
incompatible platform, a bad blob — falls back to the ordinary local build. See
[Shared side-effects cache](/pnpr/shared-side-effects-cache) for the full setup,
including the server flag, the trust material, and how an artifact is published.

Since v11.25.0, a restored artifact is saved in the shared store
together with its signed origin. Before a later install reuses it, pnpm checks
the signature again against the machine's current keys and revalidates its
owner, package and source identity, platform, policy, and stored files. A bad
remote variant is quarantined for that pnpr server and is not selected again.

:::note

`remoteSideEffectsCache` is the older spelling of this setting, and
`organization` of its `org` field; both still work.

The two spellings are merged, not chosen between: a field set under both takes
its value from `sideEffectsCache.remote`, and a field set under only one is kept
either way. That is what lets a repository declare the org in
`pnpm-workspace.yaml` while the machine supplies the signing key from the global
config under whichever spelling it already used.

:::

### unsafePerm

* Default: **false** IF running as root, ELSE **true**
* Type: **Boolean**

Set to true to enable UID/GID switching when running package scripts.
If set explicitly to false, then installing as a non-root user will fail.

### nodeOptions

* Default: **NULL**
* Type: **String**

Options to pass through to Node.js via the `NODE_OPTIONS` environment variable. This does not impact how pnpm itself is executed but it does impact how lifecycle scripts are called.

To preserve existing `NODE_OPTIONS` you can reference the existing environment variable using `${NODE_OPTIONS}` in your configuration:

```yaml title="pnpm-workspace.yaml"
nodeOptions: "${NODE_OPTIONS:- } --experimental-vm-modules"
```

### verifyDepsBeforeRun

* Default: **install**
* Type: **install**, **warn**, **error**, **prompt**, **false**

This setting allows the checking of the state of dependencies before running scripts. The check runs on `pnpm run` and `pnpm exec` commands. The following values are supported:

- `install` - Automatically runs install if `node_modules` is not up to date. If that install fails, for example because the store is read-only or there is no network, pnpm prints a warning and runs the command anyway.
- `warn` - Prints a warning if `node_modules` is not up to date.
- `prompt` - Prompts the user for permission to run install if `node_modules` is not up to date.
- `error` - Throws an error if `node_modules` is not up to date.
- `false` - Disables dependency checks.

### strictDepBuilds

Added in: v10.3.0

* Default: **true**
* Type: **Boolean**

When `strictDepBuilds` is enabled, the installation will exit with a non-zero exit code if any dependencies have unreviewed build scripts (aka postinstall scripts).

### allowBuilds

Added in: v10.26.0

A map of package matchers to explicitly allow (`true`) or disallow (`false`) script execution.

```yaml title="pnpm-workspace.yaml"
allowBuilds:
  esbuild: true
  core-js: false
  # nx versions with build scripts not listed below will
  # fail by default with ERR_PNPM_IGNORED_BUILDS
  nx@21.6.4 || 21.6.5: true
  nx@21.6.0: false
```

**Git-hosted packages:** a package name on its own never approves builds for a git or tarball dependency — the name alone does not identify the artifact. Approve one either by its exact resolved path (including the commit) or, since v11.11.0, by its repository URL:

```yaml title="pnpm-workspace.yaml"
allowBuilds:
  # Approves any commit from this repository
  'foo@git+ssh://git@example.com/org/foo.git': true
  # Approves only this exact commit
  'bar@git+https://github.com/org/bar.git#abc123': true
```

The repository form lets a trusted git dependency keep running its build scripts across branch updates without re-approving each new commit. The key is the package name, followed by `@` and the git URL, with no `#<ref>` suffix. Matching is exact, so `git+ssh://` and `git+https://` URLs for the same repository are separate keys.

Since v11.19.0, the repository form also approves git-hosted packages that pnpm downloads as a tarball rather than clones — such as `github:` dependencies, which are fetched from `codeload.github.com`. A `foo@git+https://github.com/org/foo.git` entry approves `foo` whether pnpm clones the repository or downloads a tarball. GitLab and Bitbucket tarball downloads are matched the same way. Approving or denying a specific resolved commit by its full tarball dep path continues to work.

Denials by package name are not restricted this way: `foo: false` blocks `foo` whether it comes from the registry or from git.

**Default behavior:** Packages not listed in `allowBuilds` are disallowed by default and are treated as unreviewed. By default, an error is printed ([`strictDepBuilds`](#strictdepbuilds) defaults to `true`). If `strictDepBuilds` is set to `false`, a warning is printed instead.

During install, dependencies with ignored builds that are not yet listed in `allowBuilds` are automatically added to `pnpm-workspace.yaml` with a placeholder value, so you can manually set them to `true` or `false`. The [`--allow-build`](../cli/add.md) flag on `pnpm add` and `pnpm approve-builds` writes its entries here as well.

:::info Migrating from older settings

To migrate these settings automatically, run `pnpx codemod run pnpm-v10-to-v11` from the [Migrating from v10 to v11](../migration.md) guide.

The following settings have been removed in v11 and replaced by `allowBuilds`: `onlyBuiltDependencies`, `onlyBuiltDependenciesFile`, `neverBuiltDependencies`, `ignoredBuiltDependencies`, and `ignoreDepScripts`.

Before:

```yaml title="pnpm-workspace.yaml"
onlyBuiltDependencies:
  - electron
neverBuiltDependencies:
  - core-js
ignoredBuiltDependencies:
  - esbuild
```

After:

```yaml title="pnpm-workspace.yaml"
allowBuilds:
  electron: true
  core-js: false
  esbuild: false
```

:::

### dangerouslyAllowAllBuilds

Added in: v10.9.0

* Default: **false**
* Type: **Boolean**

If set to `true`, all build scripts (e.g. `preinstall`, `install`, `postinstall`) from dependencies will run automatically, without requiring approval.

:::warning

This setting allows all dependencies—including transitive ones—to run install scripts, both now and in the future.
Even if your current dependency graph appears safe:

* Future updates may introduce new, untrusted dependencies.
* Existing packages may add scripts in later versions.
* Packages can be hijacked or compromised and begin executing malicious code.

For maximum safety, only enable this if you’re fully aware of the risks and trust the entire ecosystem you’re pulling from. It’s recommended to review and allow builds explicitly.

:::
