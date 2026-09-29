---
id: dependency-resolution
title: "Dependency Resolution Settings"
sidebar_label: "Dependency resolution"
---

### overrides

This field allows you to instruct pnpm to override any dependency in the
dependency graph, including peer dependencies. This is useful for enforcing all your packages to use a single
version of a dependency, backporting a fix, replacing a dependency with a fork, or
removing an unused dependency.

Note that the overrides field can only be set at the root of the project.

An example of the `overrides` field:

```yaml
overrides:
  "foo": "^1.0.0"
  "quux": "npm:@myorg/quux@^1.0.0"
  "bar@^2.1.0": "3.0.0"
  "qar@1>zoo": "2"
```

You may specify the package the overridden dependency belongs to by
separating the package selector from the dependency selector with a ">", for
example `qar@1>zoo` will only override the `zoo` dependency of `qar@1`, not for
any other dependencies.

To keep an overridden version in sync with the version used elsewhere in your workspace, define the version in a [catalog](../catalogs.md) and reference it with the `catalog:` protocol. This way the version is maintained in a single place and referenced from both your dependencies and your overrides:

```yaml title="pnpm-workspace.yaml"
catalog:
  foo: "^1.0.0"

overrides:
  foo: "catalog:"
```

You may also reference a named catalog with `catalog:<name>`. See [Catalogs](../catalogs.md) for more details.

If you find that your use of a certain package doesn't require one of its dependencies, you may use `-` to remove it. For example, if package `foo@1.0.0` requires a large package named `bar` for a function that you don't use, removing it could reduce install time:

```yaml
overrides:
  "foo@1.0.0>bar": "-"
```

This feature is especially useful with `optionalDependencies`, where most optional packages can be safely skipped.

#### Convergence overrides

Added in: v11.13.0

A selector with an **empty range** — `"pkg@"` — is a convergence override. Unlike a regular override, which rewrites every matching edge unconditionally, a convergence override rewrites a dependency edge only when its version satisfies the range that edge declares:

```yaml title="pnpm-workspace.yaml"
overrides:
  "form-data@": 4.0.6
```

With the above, a dependency that declares `form-data: "^4.0.5"` is pinned to `4.0.6`, while one that declares `^3.0.0` keeps its own resolution. This lets compatible consumers converge on a single version — now and for any dependent added in the future — without forcing an incompatible version on the rest of the graph.

Rules:

- The value must be an **exact version**. A range, a dist-tag, or a `-` removal fails with `ERR_PNPM_INVALID_CONVERGENCE_OVERRIDE`. A `catalog:` reference is allowed as long as the catalog entry resolves to an exact version.
- Only plain semver edges participate. Edges declared with `workspace:`, `catalog:`, `npm:`, a dist-tag, or a git/URL specifier have no meaningful "satisfies" relation and are left untouched.
- Convergence overrides cannot be combined with a parent selector: `"parent>pkg@"` is rejected.
- A regular override always wins over a convergence override for the same edge.

When a full resolution finds that every declared range also admits a newer version, pnpm warns that the override is stale and names the version to converge on instead.

:::note

Before v11.13.0, an empty range in an override selector was undocumented and behaved like a bare (unscoped) override.

:::

#### Overriding peer dependencies

Overrides also apply to `peerDependencies`. The behavior depends on the type of version specifier used in the override:

- **Semver ranges** (e.g., `^1.0.0`), **workspace**, and **catalog** protocols: the peer dependency is overridden and remains a peer dependency.
- **Non-range specifiers** such as `link:` or `file:` protocols: the peer dependency is overridden and moved to `dependencies`, since these are not valid peer dependency ranges.
- **Removal** (`-`): the peer dependency is removed entirely.

For example, to override the `react` peer dependency of `react-dom`:

```yaml title="pnpm-workspace.yaml"
overrides:
  "react-dom>react": "18.1.0"
```

### packageExtensions

The `packageExtensions` fields offer a way to extend the existing package definitions with additional information. For example, if `react-redux` should have `react-dom` in its `peerDependencies` but it has not, it is possible to patch `react-redux` using `packageExtensions`:

```yaml
packageExtensions:
  react-redux:
    peerDependencies:
      react-dom: "*"
```

The keys in `packageExtensions` are package names or package names and semver ranges, so it is possible to patch only some versions of a package:

```yaml
packageExtensions:
  react-redux@1:
    peerDependencies:
      react-dom: "*"
```

The following fields may be extended using `packageExtensions`: `dependencies`, `optionalDependencies`, `peerDependencies`, and `peerDependenciesMeta`.

A bigger example:

```yaml
packageExtensions:
  express@1:
    optionalDependencies:
      typescript: "2"
  fork-ts-checker-webpack-plugin:
    dependencies:
      "@babel/core": "1"
    peerDependencies:
      eslint: ">= 6"
    peerDependenciesMeta:
      eslint:
        optional: true
```

:::tip

Together with Yarn, we maintain a database of `packageExtensions` to patch broken packages in the ecosystem.
If you use `packageExtensions`, consider sending a PR upstream and contributing your extension to the [`@yarnpkg/extensions`] database.

:::

[`@yarnpkg/extensions`]: https://github.com/yarnpkg/berry/blob/master/packages/yarnpkg-extensions/sources/index.ts

### allowedDeprecatedVersions

This setting allows muting deprecation warnings of specific packages.

Example:

```yaml
allowedDeprecatedVersions:
  express: "1"
  request: "*"
```

With the above configuration pnpm will not print deprecation warnings about any version of `request` and about v1 of `express`.

### update

Added in: v11.16.0

Settings in this section tune the [`pnpm update`](../cli/update.md) and [`pnpm outdated`](../cli/outdated.md) commands.

#### update.ignoreDeps

Sometimes you can't update a dependency. For instance, the latest version of the dependency started to use ESM but your project is not yet in ESM. Annoyingly, such a package will be always printed out by the `pnpm outdated` command and updated, when running `pnpm update --latest`. However, you may list packages that you don't want to upgrade in the `ignoreDeps` field:

```yaml
update:
  ignoreDeps:
  - load-json-file
```

Patterns are also supported, so you may ignore any packages from a scope: `@babel/*`.

#### update.changeset

Added in: v11.16.0

* Default: **false**
* Type: **Boolean**

When `true`, `pnpm update` writes a [change intent](../versioning.md) after updating workspace manifests, declaring a `patch` bump for every workspace package whose `dependencies` or `optionalDependencies` were changed by the update and a `major` bump when its `peerDependencies` changed. Same as passing [`--changeset`](../cli/update.md#--changeset); pass `--no-changeset` to override the setting for a single run.

#### update.githubActions

Added in: v11.16.0

* Default: **false**
* Type: **Boolean**

When `true`, `pnpm update` and `pnpm outdated` also check the GitHub Actions referenced by the repository's workflow files. Same as passing [`--include-github-actions`](../cli/update.md#--include-github-actions). See [Updating GitHub Actions](../cli/update.md#updating-github-actions).

#### update.githubActionsServer

Added in: v11.17.0

* Default: the `GITHUB_SERVER_URL` environment variable, falling back to **https://github.com**
* Type: **URL**

The base URL of the GitHub server that hosts the repositories of the GitHub Actions referenced by the workflow files (for example, a GitHub Enterprise Server). The URL must use the `https://` or `http://` protocol. Only use `http://` for a trusted server on a trusted network: the refs used to pin actions to commit hashes are fetched over this URL, and unencrypted traffic can be tampered with.

:::info

Before v11.16.0, `update.ignoreDeps` was named `updateConfig.ignoreDependencies`. The deprecated `updateConfig` setting keeps working until the next major version; when both are set, the `update` section takes precedence and a warning is printed.

:::

### supportedArchitectures

You can specify architectures for which you'd like to install optional dependencies, even if they don't match the architecture of the system running the install.

For example, the following configuration tells to install optional dependencies for Windows x64:

```yaml
supportedArchitectures:
  os:
  - win32
  cpu:
  - x64
```

Whereas this configuration will install optional dependencies for Windows, macOS, and the architecture of the system currently running the install. It includes artifacts for both x64 and arm64 CPUs:

```yaml
supportedArchitectures:
  os:
  - win32
  - darwin
  - current
  cpu:
  - x64
  - arm64
```

Additionally, `supportedArchitectures` also supports specifying the `libc` of the system.

### ignoredOptionalDependencies

If an optional dependency has its name included in this array, it will be skipped. For example:

```yaml
ignoredOptionalDependencies:
- fsevents
- "@esbuild/*"
```

### forceIgnoresPlatform

Added in: v11.28.0

* Default: **true**
* Type: **Boolean**

Controls whether [`pnpm install --force`](../cli/install.md#--force) installs optional dependencies that do not match the host platform. When `true`, `--force` installs every optional dependency, whatever its `os`, `cpu`, or `libc` fields say. When `false`, `--force` still refetches packages and lifts [`engineStrict`](./cli.md#enginestrict), but skips optional dependencies built for other platforms, the same way a regular install does:

```yaml title="pnpm-workspace.yaml"
forceIgnoresPlatform: false
```

To install optional dependencies for specific other platforms, use [`supportedArchitectures`](#supportedarchitectures) instead.

### minimumReleaseAge

Added in: v10.16.0

* Default: **1440** (since v11), **0** (before v11)
* Type: **number (minutes)**

To reduce the risk of installing compromised or defective packages, you can delay the installation of newly published versions. In most cases, malicious releases are discovered and removed from the registry within an hour.

`minimumReleaseAge` defines the minimum number of minutes that must pass after a version is published before pnpm will install it. This applies to **all dependencies**, including transitive ones.

For example, the following setting ensures that only packages released at least one day ago can be installed:

```yaml
minimumReleaseAge: 1440
```

### minimumReleaseAgeExclude

Added in: v10.16.0

* Default: **undefined**
* Type: **string[]**

If you set `minimumReleaseAge` but need certain dependencies to always install the newest version immediately, you can list them under `minimumReleaseAgeExclude`. The exclusion works by **package name** and applies to all versions of that package.

Example:

```yaml
minimumReleaseAge: 1440
minimumReleaseAgeExclude:
- webpack
- react
```

In this case, all dependencies must be at least a day old, except `webpack` and `react`, which are installed immediately upon release.

Added in: v10.17.0

You may also use patterns. For instance, allow all packages from your org:

```yaml
minimumReleaseAge: 1440
minimumReleaseAgeExclude:
- '@myorg/*'
```

Added in: v10.19.0

You may also exempt specific versions (or a list of specific versions using a disjunction with `||`). This allows pinning exceptions to mature-time rules:

```yaml
minimumReleaseAge: 1440
minimumReleaseAgeExclude:
- nx@21.6.5
- webpack@4.47.0 || 5.102.1
```

### minimumReleaseAgeExcludePrune

Added in: v11.22.0

* Default: **false**
* Type: **Boolean**

When set to `true`, `pnpm add`, `pnpm update`, and `pnpm remove` prune the entries of [`minimumReleaseAgeExclude`](#minimumreleaseageexclude) in `pnpm-workspace.yaml` that the freshly written lockfile no longer resolves: a version that is gone is dropped (an entry is removed once none of its versions remain), and an entry for a package that is no longer in the lockfile is removed too. Name patterns (`@myorg/*`) are always kept.

The cleanup is skipped when the install's lockfile does not cover the whole workspace ([`sharedWorkspaceLockfile: false`](../workspaces.md#sharedworkspacelockfile)), since entries another project still needs would look stale.

### minimumReleaseAgeIgnoreMissingTime

Added in: v11.0.0

* Default: **true**
* Type: **Boolean**

When `true`, pnpm skips the [`minimumReleaseAge`](#minimumreleaseage) check for a package whose registry metadata does not include the `time` field (some private registries and mirrors omit it). Set to `false` to fail resolution in that case instead of installing the package.

```yaml
minimumReleaseAgeIgnoreMissingTime: false
```

Since v11.23.0, the setting also governs [`trustPolicy`](#trustpolicy), which reads the same publish dates: `trustPolicy: no-downgrade` skips a package the registry cannot date, with a warning, rather than failing the install with `ERR_PNPM_MISSING_TIME`.

The opt-in covers a registry that cannot date its releases, not a package a registry says it never published: during [lockfile verification](../supply-chain-security.md), a lockfile entry missing from a packument that does date every version it lists remains a hard failure.

### minimumReleaseAgeStrict

Added in: v11.0.0

* Default: **true** if [`minimumReleaseAge`](#minimumreleaseage) is explicitly configured, **false** otherwise
* Type: **Boolean**

Controls how pnpm behaves when no version of a dependency satisfies the [`minimumReleaseAge`](#minimumreleaseage) constraint within the requested range. When `false`, pnpm falls back to a version that doesn't meet the `minimumReleaseAge` constraint so installation can still succeed. When `true`, pnpm fails resolution instead.

The default depends on whether you configured `minimumReleaseAge` yourself: if you set it explicitly (via `pnpm-workspace.yaml`, the CLI, or environment variables), strict mode is on by default so the setting is enforced. The built-in default of `minimumReleaseAge` (1440 minutes) is non-strict for backward compatibility.

```yaml
minimumReleaseAgeStrict: true
```

### trustPolicy

Added in: v10.21.0

* Default: **off**
* Type: **no-downgrade** | **off**

When set to `no-downgrade`, pnpm will fail if a package's trust level has decreased compared to previous releases. For example, if a package was previously published by a trusted publisher but now only has provenance or no trust evidence, installation will fail. This helps prevent installing potentially compromised versions. Trust checks are based solely on publish date, not semver. A package cannot be installed if any earlier-published version had stronger trust evidence. Starting in v10.24.0, prerelease versions are ignored when evaluating trust evidence for a non-prerelease install, so a trusted prerelease cannot block a stable release that lacks trust evidence.

### trustPolicyExclude

Added in: v10.22.0

* Default: **[]**
* Type: **string[]**

A list of package selectors that should be excluded from the trust policy check. This allows you to install specific packages or versions even if they don't satisfy the `trustPolicy` requirement.

For example:

```yaml
trustPolicy: no-downgrade
trustPolicyExclude:
  - 'chokidar@4.0.3'
  - 'webpack@4.47.0 || 5.102.1'
  - '@babel/core@7.28.5'
```

### trustPolicyIgnoreAfter

Added in: v10.27.0

* Default: **undefined**
* Type: **number (minutes)**

Allows ignoring the trust policy check for packages published more than the specified number of minutes ago. This is useful when enabling strict trust policies, as it allows older versions of packages (which may lack a process for publishing with signatures or provenance) to be installed without manual exclusion, assuming they are safe due to their age.

### trustPolicyExcludePrune

Added in: v11.27.0

* Default: **false**
* Type: **Boolean**

When set to `true`, `pnpm add`, `pnpm update`, and `pnpm remove` prune the entries of [`trustPolicyExclude`](#trustpolicyexclude) in `pnpm-workspace.yaml` that the freshly written lockfile no longer resolves: a version that is gone is dropped (an entry is removed once none of its versions remain), and an entry for a package that is no longer in the lockfile is removed too. Name patterns (`@myorg/*`) are always kept.

The cleanup is skipped when the install's lockfile does not cover the whole workspace ([`sharedWorkspaceLockfile: false`](../workspaces.md#sharedworkspacelockfile)), since entries another project still needs would look stale.

### trustLockfile

Added in: v11.3.0

* Default: **false**
* Type: **Boolean**

When `true`, `pnpm install` skips the supply-chain verification pass that re-applies [`minimumReleaseAge`](#minimumreleaseage) and [`trustPolicy`](#trustpolicy) to every entry in the loaded lockfile. The install treats the lockfile as already trusted.

Useful in environments where the lockfile is effectively part of the trusted base — closed-source projects where every commit comes from a trusted author. A poisoned lockfile (one a contributor authored under a weaker policy than CI enforces) can slip through, so leave this `false` whenever outside collaborators can edit the lockfile.

On large workspaces the verification pass holds per-package registry metadata in memory for the duration of the install; disabling it cuts memory usage at the cost of the supply-chain check. Most projects with the default `frozenLockfile` CI workflow do not need to set this.

### blockExoticSubdeps

Added in: v10.26.0

* Default: **true**
* Type: **Boolean**

When set to `true`, only direct dependencies (those listed in your root `package.json`) may use exotic sources (like git repositories or direct tarball URLs). All transitive dependencies must be resolved from a trusted source, such as the configured registry, local file paths, workspace links, or trusted GitHub repositories (node, bun, deno).

This setting helps secure the dependency supply chain by preventing transitive dependencies from pulling in code from untrusted locations.

Exotic sources include:
* Git repositories (`git+ssh://...`)
* Direct URL links to tarballs (`https://.../package.tgz`)

### registries

Added in: v11.0.0

* Default: **undefined**
* Type: **Record&lt;string, RegistryDeclaration&gt;** or **Record&lt;string, string&gt;**

Declares the registries the project installs from. Since v11.23.0, each registry is declared once, keyed by its URL, with everything pnpm knows about it in the entry: the `scopes` routed to it, the bare-specifier `prefix` it answers to, and how the server lays out tarball URLs (`serverType`, `supportsTimeField`). The full description of each field is on the dedicated [Registries](../registries.md) page.

```yaml
registries:
  https://npm.corp.example.com/:
    serverType: artifactory
    scopes: ["@my-org", "@internal"]
    prefix: work
```

The older shape, mapping scopes to URLs, is still accepted. The `default` key sets the main registry (equivalent to the `registry` `.npmrc` setting), and scoped keys configure registries for specific package scopes:

```yaml
registries:
  default: https://registry.npmjs.org/
  "@my-org": https://private.example.com/
  "@internal": https://nexus.corp.com/
```

The two shapes cannot be mixed in one map.

Since v11.11.0, this setting may also be defined in the [global configuration file](../cli/config.md) (`config.yaml`), which is useful for registries that should apply to every project on the machine rather than to a single repository. Only the routes (`scopes` and `prefix`) are read from there; `serverType` and `supportsTimeField` shape the lockfile, so they are read only from `pnpm-workspace.yaml` — see [where the setting may live](../registries.md#where-the-setting-may-live).

### namedRegistries

Added in: v11.1.0

* Default: **undefined**
* Type: **Record&lt;string, string&gt;**

:::note

Deprecated since v11.23.0: declare a `prefix` in [`registries`](#registries) instead — see the [Registries](../registries.md) page. `namedRegistries` is still read, but only for prefixes that `registries` does not declare; when both settings declare prefixes, pnpm warns. Everything below about aliases — the built-in ones, reserved names, and the lockfile keys — applies to `prefix`-declared aliases the same way.

:::

Defines named registry aliases that can be used as a prefix when installing packages, in the style of [vlt's named-registry aliases](https://docs.vlt.sh/cli/registries). For example, with the following configuration:

```yaml title="pnpm-workspace.yaml"
namedRegistries:
  gh: https://npm.pkg.github.example.com/
  work: https://npm.work.example.com/
```

`pnpm add work:@corp/lib@^2.0.0` resolves `@corp/lib@^2.0.0` against `https://npm.work.example.com/`.

Authentication is picked up from the existing per-URL `.npmrc` entries (e.g. `//npm.pkg.github.com/:_authToken=...`), so no separate auth mechanism is required.

Since v11.11.0, this setting may also be defined in the [global configuration file](../cli/config.md) (`config.yaml`), so an alias like `work:` can be shared across every project on the machine.

#### Built-in aliases

Two aliases work without any configuration:

| Alias    | Registry                      | Notes                                                                                                                                         |
|----------|-------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------|
| `gh:`    | `https://npm.pkg.github.com/` | The [GitHub Packages npm registry](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-npm-registry). |
| `npmjs:` | `https://registry.npmjs.org/` | The public npm registry. Added in v11.20.0.                                                                                                   |

Entries you define under `namedRegistries` are merged on top of these, so either one can be overridden — GitHub Enterprise Server users point `gh` at their own host, and an organization that mirrors or proxies npmjs should point `npmjs` at the mirror:

```yaml title="pnpm-workspace.yaml"
namedRegistries:
  gh: https://npm.pkg.github.example.com/
  npmjs: https://npm.internal.example.com/
```

`npmjs:` pins a dependency to the public registry even when the default [`registry`](#registries) points somewhere else, such as an internal proxy:

```json title="package.json"
{
  "dependencies": {
    "left-pad": "npmjs:^1.3.0"
  }
}
```

The `npm:` prefix cannot do this — it is the [alias protocol](../aliases.md) (`npm:<name>@<range>`) and resolves through whatever `registry` points at.

The built-in URLs are also the prefixes that a tarball URL recorded in the lockfile is matched against when pnpm verifies a package. If you proxy npmjs and do not override the alias, an entry whose tarball URL is on `registry.npmjs.org` is verified against the public registry rather than against your mirror. This only affects lockfiles that record such a URL — a canonical URL for your configured registry is omitted from the lockfile — and only when a tarball-URL, [`minimumReleaseAge`](#minimumreleaseage), or [`trustPolicy`](#trustpolicy) check runs.

#### Reserved alias names

Since v11.20.0, an alias that shadows a reserved dependency specifier prefix (`file`, `link`, `workspace`, `runtime`, `npm`, `jsr`, `git`, `github`, `gitlab`, `bitbucket`, `catalog`, `custom`, `http`, `https`, `ssh`) is rejected with `ERR_PNPM_RESERVED_NAMED_REGISTRY_NAME`. Previously such an alias was silently shadowed by the corresponding resolver. An alias must also start with a letter and contain only letters, digits, `.`, `_`, and `-`.

#### Named registries in the lockfile

Since v11.20.0, a package resolved from a named registry is recorded in `pnpm-lock.yaml` under a registry-qualified key, `<name>@<registryName>:<version>`:

```yaml title="pnpm-lock.yaml"
packages:
  foo@work:1.0.0:
    resolution: {integrity: sha512-...}
```

Before v11.20.0, packages were keyed by `name@version` alone, so the same name and version served by two registries collapsed onto a single entry and whichever resolved first decided the tarball that every consumer got. That is a package-substitution risk: a package you expect from your private registry could be installed from another registry that publishes the same name and version, with nothing in the lockfile to reveal it. Registry-qualified keys give each registry its own entry and pin which one a dependency came from.

The lockfile format version is unchanged, and qualified keys appear only for packages resolved from a named registry — including the built-in `gh:` and `npmjs:` aliases, which need no `namedRegistries` entry. A project that installs nothing through an alias sees no difference, and older pnpm versions keep reading the file.

:::caution

If any dependency is installed through an alias, your first non-frozen install on v11.20.0 or newer re-keys those entries, which shows up as a lockfile diff. Commit it — that diff is the fix being applied. Review it too: an entry that moves to a registry you did not expect is worth investigating.

Have everyone working on the project move to v11.20.0 or newer first. An older pnpm reads the re-keyed lockfile fine, and frozen installs are unaffected, but it does not produce registry-qualified keys itself: any install that updates the lockfile writes those entries back to the old shape, and the next install on a current pnpm re-qualifies them. The lockfile then flips back and forth, and while it is in the old shape the project is exposed again. Because the lockfile format version is deliberately unchanged, pnpm cannot detect this and warn you.

There is no setting to keep the old behavior — the old shape is the vulnerability.

:::

Every non-built-in alias that the lockfile references must stay declared — through `prefix` in [`registries`](#registries) or through `namedRegistries`. Reading an entry whose alias is gone fails with `ERR_PNPM_MISSING_NAMED_REGISTRY` rather than falling back to the default registry, since that would fetch a different package. Renaming an alias re-resolves the packages that used it.

Tarball URLs that follow the standard registry layout are no longer written to the lockfile for named-registry packages; they are recomputed from the alias's declared URL on demand.
