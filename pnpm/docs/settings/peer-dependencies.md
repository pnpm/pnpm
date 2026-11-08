---
id: peer-dependencies
title: "Peer Dependency Settings"
sidebar_label: "Peer dependencies"
---

### autoInstallPeers

* Default: **true**
* Type: **Boolean**

When `true`, any missing non-optional peer dependencies are automatically installed.

The version picked is the highest one satisfying the peer range. Since an automatically installed peer is not a dependency the project declares, [`resolutionMode`](./other.md#resolutionmode) treats it as a subdependency rather than a direct one, so `lowest-direct` does not lower it.

#### Optional peer dependencies

pnpm installs a missing optional peer dependency only if a version of it that satisfies the optional peer range is already installed. In that case, pnpm resolves the optional peer to that version.

When [`dedupePeerDependents`](#dedupepeerdependents) is `true`, that version can come from any project in the workspace, including another project's `devDependencies`. For example, if one project depends on `webpack` and another has `webpack-cli` in its `devDependencies`, both projects get the `webpack` instance that has `webpack-cli` as its optional peer.

When `dedupePeerDependents` is `false`, or [`projectScopedOptionalPeers`](#projectscopedoptionalpeers) is `true`, that version must come from the project's own dependencies. The workspace root's dependencies also count if [`resolvePeersFromWorkspaceRoot`](#resolvepeersfromworkspaceroot) is `true`.

pnpm skips that version if installing it would leave the project with an unmet peer dependency. This covers the version's own peers, the required peers that pnpm would install with it, and their peers in turn. `dedupePeerDependents` does not give the skipped peer back to the project by sharing another project's instance of the package that declares it.

pnpm also resolves optional peers this way when `autoInstallPeers` is `false` and `dedupePeerDependents` is `true`. To stop it, set both settings to `false`.

#### Version conflicts

If there are conflicting version requirements for a peer dependency from different packages, pnpm will not install any version of the conflicting peer dependency automatically. Instead, a warning is printed. For example, if one dependency requires `react@^16.0.0` and another requires `react@^17.0.0`, these requirements conflict, and no automatic installation will occur.

#### Conflict resolution

In case of a version conflict, you'll need to evaluate which version of the peer dependency to install yourself, or update the dependencies to align their peer dependency requirements. You can also set [`autoInstallPeersFromHighestMatch`](#autoinstallpeersfromhighestmatch) to `true`.

### autoInstallPeersFromHighestMatch

* Default: **false**
* Type: **Boolean**

When set to `true`, pnpm joins conflicting peer dependency ranges with `||` and installs the highest version that satisfies any of them. For example, if one dependency requires `react@^16.0.0` and another requires `react@^17.0.0`, pnpm installs the highest `react@17`.

### dedupePeerDependents

* Default: **true**
* Type: **Boolean**

When this setting is set to `true`, packages with peer dependencies will be deduplicated after peers resolution.

This setting also lets pnpm resolve missing required and optional peers using compatible versions available elsewhere in the workspace, including transitive dependencies of other projects. Required peers can be resolved this way even when `autoInstallPeers` is `false`. A required peer satisfied this way is not reported as missing. With `autoInstallPeers: false`, [`pnpm peers check`](../cli/peers.md) reports a separate advisory when the project and its dependency ancestors do not declare a provider.

To prevent this workspace-wide peer reuse, set both `autoInstallPeers` and `dedupePeerDependents` to `false`. Dependencies of the workspace root can still provide peers when [`resolvePeersFromWorkspaceRoot`](#resolvepeersfromworkspaceroot) is enabled.

Resolving a package's peer does not add that peer to your project's declared dependencies. If your project's scripts run a peer package's executable, declare that package directly in your project's `dependencies` or `devDependencies`.

For instance, let's say we have a workspace with two projects and both of them have `webpack` in their dependencies. `webpack` has `esbuild` in its optional peer dependencies, and one of the projects has `esbuild` in its dependencies. In this case, pnpm will link two instances of `webpack` to the `node_modules/.pnpm` directory: one with `esbuild` and another one without it:

```text
node_modules
  .pnpm
    webpack@1.0.0_esbuild@1.0.0
    webpack@1.0.0
project1
  node_modules
    webpack -> ../../node_modules/.pnpm/webpack@1.0.0/node_modules/webpack
project2
  node_modules
    webpack -> ../../node_modules/.pnpm/webpack@1.0.0_esbuild@1.0.0/node_modules/webpack
    esbuild
```

This makes sense because `webpack` is used in two projects, and one of the projects doesn't have `esbuild`, so the two projects cannot share the same instance of `webpack`. However, this is not what most developers expect, especially since in a hoisted `node_modules`, there would only be one instance of `webpack`. Therefore, with the default `dedupePeerDependents: true`, pnpm deduplicates `webpack` when it has no conflicting peer dependencies (explanation at the end). Both projects use the same `webpack` instance, which is the one that has `esbuild` resolved:

```text
node_modules
  .pnpm
    webpack@1.0.0_esbuild@1.0.0
project1
  node_modules
    webpack -> ../../node_modules/.pnpm/webpack@1.0.0_esbuild@1.0.0/node_modules/webpack
project2
  node_modules
    webpack -> ../../node_modules/.pnpm/webpack@1.0.0_esbuild@1.0.0/node_modules/webpack
    esbuild
```

**What are conflicting peer dependencies?** By conflicting peer dependencies we mean a scenario like the following one:

```text
node_modules
  .pnpm
    webpack@1.0.0_react@16.0.0_esbuild@1.0.0
    webpack@1.0.0_react@17.0.0
project1
  node_modules
    webpack -> ../../node_modules/.pnpm/webpack@1.0.0_react@17.0.0/node_modules/webpack
    react (v17)
project2
  node_modules
    webpack -> ../../node_modules/.pnpm/webpack@1.0.0_react@16.0.0_esbuild@1.0.0/node_modules/webpack
    esbuild
    react (v16)
```

In this case, we cannot dedupe `webpack` as `webpack` has `react` in its peer dependencies and `react` is resolved from two different versions in the context of the two projects.

### projectScopedOptionalPeers

Added in: v12.19.0

* Default: **false**
* Type: **Boolean**

When `true`, a project gets a missing optional peer dependency only if the project's own dependencies already install a version of it. The workspace root's dependencies also count if [`resolvePeersFromWorkspaceRoot`](#resolvepeersfromworkspaceroot) is `true`. A version that only another workspace project installs does not count.

[`dedupePeerDependents`](#dedupepeerdependents) keeps working for everything else. It still deduplicates packages across projects, unless deduplicating would give a project an optional peer its own dependencies don't install. Required peers are not affected.

In the [`dedupePeerDependents`](#dedupepeerdependents) example, `project1` keeps the `webpack` instance without `esbuild`.

This setting has no effect when `dedupePeerDependents` is `false`, because optional peers are already resolved only from the project's own dependencies then.

Changing this setting changes the lockfile. pnpm records it in the lockfile's `settings` while it takes effect, so the next install resolves the lockfile again.

### dedupePeers

* Default: **false**
* Type: **Boolean**

When enabled, peer dependency suffixes use version-only identifiers (`name@version`) instead of full dep paths, eliminating nested suffixes like `(foo@1.0.0(bar@2.0.0))`. This reduces the number of package instances in projects with many recursive peer dependencies.

This is different from [`dedupePeerDependents`](#dedupepeerdependents), which deduplicates packages that have the same peer dependencies across different workspace projects. `dedupePeers` simplifies the peer dependency suffix format itself.

### addMissingPeerTypes

Added in: v12.12.0

* Default: **false**
* Type: **Boolean**

When `true`, every peer dependency of a dependency also makes the matching `@types` package an optional peer of it. A package with a peer dependency on `react` gets an optional peer dependency on `@types/react`, and one on `@babel/core` gets `@types/babel__core`. A package that already declares the `@types` package keeps its own declaration. pnpm does not install an `@types` package that nothing in your workspace depends on.

Many packages peer-depend on a library without declaring its `@types` package. TypeScript then looks for the types next to the package's own location. With [`enableGlobalVirtualStore`](./node-modules.md#enableglobalvirtualstore) that location is outside your project, so the types your project installs are not found. With this setting, pnpm links the `@types` package next to every package that peer-depends on the library, as it does with any [optional peer dependency](#optional-peer-dependencies).

Changing this setting changes the lockfile.

### strictPeerDependencies

* Default: **false**
* Type: **Boolean**

If this is enabled, commands will fail if there is a missing or invalid peer
dependency in the tree.

### resolvePeersFromWorkspaceRoot

* Default: **true**
* Type: **Boolean**

When enabled, dependencies of the root workspace project are used to resolve peer dependencies of any projects in the workspace.
It is a useful feature as you can install your peer dependencies only in the root of the workspace, and you can be sure that all projects in the workspace use the same versions of the peer dependencies.

### peerDependencyRules

:::note

Deprecated: `peerDependencyRules` only suppresses peer dependency warnings after resolution without changing `peerDependencies`. When you want to change or remove the declared peer dependency ranges so peer resolution itself is updated, use [`overrides`](./dependency-resolution.md#overriding-peer-dependencies) with `:peer`:

- `peerDependencyRules.ignoreMissing: ["react", "@babel/*"]` only silences missing-peer warnings. The `overrides` entries `"react:peer": "-"` and `"@babel/*:peer": "-"` remove the peer declarations and their `peerDependenciesMeta` entries, so packages no longer receive those peers from their parents.
- `peerDependencyRules.allowedVersions: { "button@2>react": "17" }` only changes peer-mismatch reporting and has no behavior-preserving `overrides` replacement. Use `"button@2>react:peer": "16 || 17"` in `overrides` when you intend to widen the declared peer range and change peer resolution.
- `peerDependencyRules.allowAny: ["@babel/*"]` only suppresses peer-version mismatch warnings and does not rewrite `peerDependencies`, so it has no direct `overrides` equivalent. Use `"@babel/*:peer": "*"` in `overrides` only when you intend to widen the declared peer range and change peer resolution.

`peerDependencyRules` continues to work for backward compatibility.

:::

#### peerDependencyRules.ignoreMissing

pnpm will not print warnings about missing peer dependencies from this list.

For instance, with the following configuration, pnpm will not print warnings if a dependency needs `react` but `react` is not installed:

```yaml title="pnpm-workspace.yaml"
peerDependencyRules:
  ignoreMissing:
  - react
```

Package name patterns may also be used:

```yaml title="pnpm-workspace.yaml"
peerDependencyRules:
  ignoreMissing:
  - "@babel/*"
  - "@eslint/*"
```

`ignoreMissing` matches the peer package name or a glob. It does not support
the `parent>peer` or `parent@range>peer` selectors accepted by
[`allowedVersions`](#peerdependencyrulesallowedversions).

#### peerDependencyRules.allowedVersions

Unmet peer dependency warnings will not be printed for peer dependencies of the specified range.

For instance, if you have some dependencies that need `react@16` but you know that they work fine with `react@17`, then you may use the following configuration:

```yaml title="pnpm-workspace.yaml"
peerDependencyRules:
  allowedVersions:
    react: "17"
```

This will tell pnpm that any dependency that has react in its peer dependencies should allow `react` v17 to be installed.

It is also possible to suppress the warnings only for peer dependencies of specific packages. For instance, with the following configuration `react` v17 will be only allowed when it is in the peer dependencies of the `button` v2 package or in the dependencies of any `card` package:

```yaml title="pnpm-workspace.yaml"
peerDependencyRules:
  allowedVersions:
    "button@2>react": "17"
    "card>react": "17"
```

#### peerDependencyRules.allowAny

`allowAny` is an array of package name patterns, any peer dependency matching the pattern will be resolved from any version, regardless of the range specified in `peerDependencies`. For instance:

```yaml title="pnpm-workspace.yaml"
peerDependencyRules:
  allowAny:
  - "@babel/*"
  - "eslint"
```

The above setting will mute any warnings about peer dependency version mismatches related to `@babel/` packages or `eslint`.

`allowAny` matches the peer package name or a glob. It does not support
the `parent>peer` or `parent@range>peer` selectors accepted by
[`allowedVersions`](#peerdependencyrulesallowedversions).
