---
id: settings
title: "Settings (pnpm-workspace.yaml)"
---

import SettingsAnchorRedirect from '@site/src/components/SettingsAnchorRedirect'

<SettingsAnchorRedirect />

pnpm gets its configuration from the command line, environment variables, and `pnpm-workspace.yaml`.

Only auth and registry settings are read from `.npmrc` files. All other settings are configured in `pnpm-workspace.yaml` or the [global `config.yaml`](./configuring.md#global-configuration). Some settings are limited to one of them: settings that define the structure of `node_modules` (like `hoistPattern`, `publicHoistPattern`, `nodeLinker`, or `shamefullyHoist`) can only be set in `pnpm-workspace.yaml`, while settings that name machine-level state (like `stateDir` or `pnpmHomeDir`) are not read from `pnpm-workspace.yaml`.

The `pnpm config` command can be used to read and edit the contents of the project and global configuration files.

The relevant configuration files are:

* Per-project configuration file: `/path/to/my/project/pnpm-workspace.yaml`
* [Global configuration file](./cli/config.md)

:::note

Authorization-related settings are handled via [`.npmrc`](./npmrc.md).

:::

Values in the configuration files may contain env variables using the `${NAME}` syntax. The env variables may also be specified with default values. Using `${NAME-fallback}` will return `fallback` if `NAME` isn't set. `${NAME:-fallback}` will return `fallback` if `NAME` isn't set, or is an empty string.

:::warning

Since v11.5.3, env variables are **not** expanded in settings of `pnpm-workspace.yaml` that define registry URLs: `registry` and the URL values of [`registries`](./settings/dependency-resolution.md#registries) and [`namedRegistries`](./settings/dependency-resolution.md#namedregistries). Values containing a `${...}` placeholder in these settings are ignored. In the [registry declaration shape](./registries.md) of `registries` (since v11.23.0), the URL is the key rather than the value, and the same rule applies to the keys. Because `pnpm-workspace.yaml` is committed to the repository, expanding env variables in registry URLs could be exploited by a malicious repository to leak secrets from the environment to an attacker-controlled registry. Configure dynamic registry URLs in a trusted location instead: the global configuration file or CLI options.

:::

:::note

Since v11.22.0, a project's `pnpm-workspace.yaml` cannot choose where pnpm keeps its credentials, its own installation, or other machine-level state: `bin`, `configDir`, `dir`, `globalBinDir`, `globalDir`, `npmrcAuthFile`, `pnpmHomeDir`, `stateDir`, `userconfig`, and `workspaceDir` are ignored there, with a warning. Set them in the [global configuration file](./cli/config.md) or on the command line instead. `cacheDir` and `storeDir` are unaffected.

:::

[INI-formatted]: https://en.wikipedia.org/wiki/INI_file

## packages

Besides settings, `pnpm-workspace.yaml` defines the root of the [workspace] and
enables you to include / exclude directories from the workspace. If the
`packages` field is omitted, only the root package is included in the workspace.

For example:

```yaml title="pnpm-workspace.yaml"
packages:
  # specify a package in a direct subdir of the root
  - 'my-app'
  # all packages in direct subdirs of packages/
  - 'packages/*'
  # all packages in subdirs of components/
  - 'components/**'
  # exclude packages that are inside test directories
  - '!**/test/**'
```

The root package is always included, even when custom location wildcards are
used.

Catalogs are also defined in the `pnpm-workspace.yaml` file. See [_Catalogs_](./catalogs.md) for details.

```yaml title="pnpm-workspace.yaml"
packages:
  - 'packages/*'

catalog:
  chalk: ^4.1.2

catalogs:
  react16:
    react: ^16.7.0
    react-dom: ^16.7.0
  react17:
    react: ^17.10.0
    react-dom: ^17.10.0
```

[workspace]: ./workspaces.md

## packageConfigs

Added in: v11.0.0

Allows setting project-specific configuration for individual workspace packages. This replaces workspace project-specific `.npmrc` files.

`packageConfigs` can be specified as a map of package names to config objects:

```yaml title="pnpm-workspace.yaml"
packages:
  - "packages/project-1"
  - "packages/project-2"
packageConfigs:
  "project-1":
    saveExact: true
  "project-2":
    savePrefix: "~"
```

Or as an array of pattern-matched rules:

```yaml title="pnpm-workspace.yaml"
packages:
  - "packages/project-1"
  - "packages/project-2"
packageConfigs:
  - match: ["project-1", "project-2"]
    modulesDir: "node_modules"
    saveExact: true
```

## Settings

Every setting is listed below, grouped by topic. Follow a setting to read its documentation, or open the full reference of a group.

### Dependency Resolution

[Full reference →](./settings/dependency-resolution.md)

* [overrides](./settings/dependency-resolution.md#overrides)
  * [Convergence overrides](./settings/dependency-resolution.md#convergence-overrides)
  * [Overriding peer dependencies](./settings/dependency-resolution.md#overriding-peer-dependencies)
* [packageExtensions](./settings/dependency-resolution.md#packageextensions)
* [allowedDeprecatedVersions](./settings/dependency-resolution.md#alloweddeprecatedversions)
* [update](./settings/dependency-resolution.md#update)
  * [update.ignoreDeps](./settings/dependency-resolution.md#updateignoredeps)
  * [update.changeset](./settings/dependency-resolution.md#updatechangeset)
  * [update.githubActions](./settings/dependency-resolution.md#updategithubactions)
  * [update.githubActionsServer](./settings/dependency-resolution.md#updategithubactionsserver)
* [supportedArchitectures](./settings/dependency-resolution.md#supportedarchitectures)
* [ignoredOptionalDependencies](./settings/dependency-resolution.md#ignoredoptionaldependencies)
* [forceIgnoresPlatform](./settings/dependency-resolution.md#forceignoresplatform)
* [minimumReleaseAge](./settings/dependency-resolution.md#minimumreleaseage)
* [minimumReleaseAgeExclude](./settings/dependency-resolution.md#minimumreleaseageexclude)
* [minimumReleaseAgeExcludePrune](./settings/dependency-resolution.md#minimumreleaseageexcludeprune)
* [minimumReleaseAgeIgnoreMissingTime](./settings/dependency-resolution.md#minimumreleaseageignoremissingtime)
* [minimumReleaseAgeStrict](./settings/dependency-resolution.md#minimumreleaseagestrict)
* [trustPolicy](./settings/dependency-resolution.md#trustpolicy)
* [trustPolicyExclude](./settings/dependency-resolution.md#trustpolicyexclude)
* [trustPolicyIgnoreAfter](./settings/dependency-resolution.md#trustpolicyignoreafter)
* [trustPolicyExcludePrune](./settings/dependency-resolution.md#trustpolicyexcludeprune)
* [trustLockfile](./settings/dependency-resolution.md#trustlockfile)
* [blockExoticSubdeps](./settings/dependency-resolution.md#blockexoticsubdeps)
* [registries](./settings/dependency-resolution.md#registries)
* [namedRegistries](./settings/dependency-resolution.md#namedregistries)

### Node-Modules Settings

[Full reference →](./settings/node-modules.md#node-modules-settings)

* [modulesDir](./settings/node-modules.md#modulesdir)
* [nodeLinker](./settings/node-modules.md#nodelinker)
* [nodeExperimentalPackageMap](./settings/node-modules.md#nodeexperimentalpackagemap)
* [nodePackageMapType](./settings/node-modules.md#nodepackagemaptype)
* [symlink](./settings/node-modules.md#symlink)
* [enableModulesDir](./settings/node-modules.md#enablemodulesdir)
* [confirmModulesPurge](./settings/node-modules.md#confirmmodulespurge)
* [virtualStoreDir](./settings/node-modules.md#virtualstoredir)
* [virtualStoreDirMaxLength](./settings/node-modules.md#virtualstoredirmaxlength)
* [virtualStoreOnly](./settings/node-modules.md#virtualstoreonly)
* [packageImportMethod](./settings/node-modules.md#packageimportmethod)
* [modulesCacheMaxAge](./settings/node-modules.md#modulescachemaxage)
* [dlxCacheMaxAge](./settings/node-modules.md#dlxcachemaxage)
* [virtualStoreType](./settings/node-modules.md#virtualstoretype)
* [enableGlobalVirtualStore](./settings/node-modules.md#enableglobalvirtualstore)

### Dependency Hoisting Settings

[Full reference →](./settings/node-modules.md#dependency-hoisting-settings)

* [hoist](./settings/node-modules.md#hoist)
* [hoistPattern](./settings/node-modules.md#hoistpattern)
* [publicHoistPattern](./settings/node-modules.md#publichoistpattern)
* [hoistWorkspacePackages](./settings/node-modules.md#hoistworkspacepackages)
* [shamefullyHoist](./settings/node-modules.md#shamefullyhoist)
* [hoistingLimits](./settings/node-modules.md#hoistinglimits)

### Store Settings

[Full reference →](./settings/store.md#store-settings)

* [storeDir](./settings/store.md#storedir)
* [verifyStoreIntegrity](./settings/store.md#verifystoreintegrity)
* [useRunningStoreServer](./settings/store.md#userunningstoreserver)
* [strictStorePkgContentCheck](./settings/store.md#strictstorepkgcontentcheck)
* [frozenStore](./settings/store.md#frozenstore)

### Lockfile Settings

[Full reference →](./settings/store.md#lockfile-settings)

* [lockfile](./settings/store.md#lockfile)
* [preferFrozenLockfile](./settings/store.md#preferfrozenlockfile)
* [lockfileIncludeTarballUrl](./settings/store.md#lockfileincludetarballurl)
* [gitBranchLockfile](./settings/store.md#gitbranchlockfile)
* [mergeGitBranchLockfilesBranchPattern](./settings/store.md#mergegitbranchlockfilesbranchpattern)
* [peersSuffixMaxLength](./settings/store.md#peerssuffixmaxlength)

### Network Settings

[Full reference →](./settings/network.md#network-settings)

* [httpsProxy](./settings/network.md#httpsproxy)
* [httpProxy](./settings/network.md#httpproxy)
* [noProxy](./settings/network.md#noproxy)
* [localAddress](./settings/network.md#localaddress)
* [maxsockets](./settings/network.md#maxsockets)
* [strictSsl](./settings/network.md#strictssl)

### Request Settings

[Full reference →](./settings/network.md#request-settings)

* [gitShallowHosts](./settings/network.md#gitshallowhosts)
* [networkConcurrency](./settings/network.md#networkconcurrency)
* [fetchRetries](./settings/network.md#fetchretries)
* [fetchRetryFactor](./settings/network.md#fetchretryfactor)
* [fetchRetryMintimeout](./settings/network.md#fetchretrymintimeout)
* [fetchRetryMaxtimeout](./settings/network.md#fetchretrymaxtimeout)
* [fetchTimeout](./settings/network.md#fetchtimeout)
* [fetchWarnTimeoutMs](./settings/network.md#fetchwarntimeoutms)
* [fetchMinSpeedKiBps](./settings/network.md#fetchminspeedkibps)

### Peer Dependency Settings

[Full reference →](./settings/peer-dependencies.md)

* [autoInstallPeers](./settings/peer-dependencies.md#autoinstallpeers)
  * [Version Conflicts](./settings/peer-dependencies.md#version-conflicts)
  * [Conflict Resolution](./settings/peer-dependencies.md#conflict-resolution)
* [dedupePeerDependents](./settings/peer-dependencies.md#dedupepeerdependents)
* [dedupePeers](./settings/peer-dependencies.md#dedupepeers)
* [strictPeerDependencies](./settings/peer-dependencies.md#strictpeerdependencies)
* [resolvePeersFromWorkspaceRoot](./settings/peer-dependencies.md#resolvepeersfromworkspaceroot)
* [peerDependencyRules](./settings/peer-dependencies.md#peerdependencyrules)
  * [peerDependencyRules.ignoreMissing](./settings/peer-dependencies.md#peerdependencyrulesignoremissing)
  * [peerDependencyRules.allowedVersions](./settings/peer-dependencies.md#peerdependencyrulesallowedversions)
  * [peerDependencyRules.allowAny](./settings/peer-dependencies.md#peerdependencyrulesallowany)

### CLI Settings

[Full reference →](./settings/cli.md#cli-settings)

* [[no-]color](./settings/cli.md#no-color)
* [loglevel](./settings/cli.md#loglevel)
* [useBetaCli](./settings/cli.md#usebetacli)
* [recursiveInstall](./settings/cli.md#recursiveinstall)
* [engineStrict](./settings/cli.md#enginestrict)
* [npmPath](./settings/cli.md#npmpath)
* [pmOnFail](./settings/cli.md#pmonfail)
* [ignoreWorkspaceRootCheck](./settings/cli.md#ignoreworkspacerootcheck)

### Node.js Settings

[Full reference →](./settings/cli.md#nodejs-settings)

* [nodeVersion](./settings/cli.md#nodeversion)
* [runtimeOnFail](./settings/cli.md#runtimeonfail)
* [nodeDownloadMirrors](./settings/cli.md#nodedownloadmirrors)

### Build Settings

[Full reference →](./settings/build.md)

* [ignoreScripts](./settings/build.md#ignorescripts)
* [childConcurrency](./settings/build.md#childconcurrency)
* [sideEffectsCache](./settings/build.md#sideeffectscache)
* [sideEffectsCacheReadonly](./settings/build.md#sideeffectscachereadonly)
* [sideEffectsCache.remote](./settings/build.md#sideeffectscacheremote)
* [unsafePerm](./settings/build.md#unsafeperm)
* [nodeOptions](./settings/build.md#nodeoptions)
* [verifyDepsBeforeRun](./settings/build.md#verifydepsbeforerun)
* [strictDepBuilds](./settings/build.md#strictdepbuilds)
* [allowBuilds](./settings/build.md#allowbuilds)
* [dangerouslyAllowAllBuilds](./settings/build.md#dangerouslyallowallbuilds)

### Versioning Settings

[Full reference →](./settings/versioning.md)

* [versioning.fixed](./settings/versioning.md#versioningfixed)
* [versioning.ignore](./settings/versioning.md#versioningignore)
* [versioning.maxBump](./settings/versioning.md#versioningmaxbump)
* [versioning.lanes](./settings/versioning.md#versioninglanes)
* [versioning.epics](./settings/versioning.md#versioningepics)
* [versioning.changelog.storage](./settings/versioning.md#versioningchangelogstorage)

### Other Settings

[Full reference →](./settings/other.md)

* [savePrefix](./settings/other.md#saveprefix)
* [tag](./settings/other.md#tag)
* [globalDir](./settings/other.md#globaldir)
* [globalBinDir](./settings/other.md#globalbindir)
* [npmrcAuthFile](./settings/other.md#npmrcauthfile)
* [stateDir](./settings/other.md#statedir)
* [cacheDir](./settings/other.md#cachedir)
* [useStderr](./settings/other.md#usestderr)
* [updateNotifier](./settings/other.md#updatenotifier)
* [preferSymlinkedExecutables](./settings/other.md#prefersymlinkedexecutables)
* [ignoreCompatibilityDb](./settings/other.md#ignorecompatibilitydb)
* [resolutionMode](./settings/other.md#resolutionmode)
* [registrySupportsTimeField](./settings/other.md#registrysupportstimefield)
* [extendNodePath](./settings/other.md#extendnodepath)
  * [Why this is needed](./settings/other.md#why-this-is-needed)
  * [When to disable](./settings/other.md#when-to-disable)
* [deployAllFiles](./settings/other.md#deployallfiles)
* [dedupeDirectDeps](./settings/other.md#dedupedirectdeps)
* [optimisticRepeatInstall](./settings/other.md#optimisticrepeatinstall)
* [requiredScripts](./settings/other.md#requiredscripts)
* [enablePrePostScripts](./settings/other.md#enableprepostscripts)
* [scriptShell](./settings/other.md#scriptshell)
* [shellEmulator](./settings/other.md#shellemulator)
* [catalogMode](./settings/other.md#catalogmode)
* [ci](./settings/other.md#ci)
* [catalogPrune](./settings/other.md#catalogprune)

### Workspace Settings

These settings are configured in `pnpm-workspace.yaml` as well, but are documented together with the workspace feature they belong to.

[Full reference →](./workspaces.md#configuration)

* [linkWorkspacePackages](./workspaces.md#linkworkspacepackages)
* [injectWorkspacePackages](./workspaces.md#injectworkspacepackages)
* [dedupeInjectedDeps](./workspaces.md#dedupeinjecteddeps)
* [syncInjectedDepsAfterScripts](./workspaces.md#syncinjecteddepsafterscripts)
* [preferWorkspacePackages](./workspaces.md#preferworkspacepackages)
* [sharedWorkspaceLockfile](./workspaces.md#sharedworkspacelockfile)
* [saveWorkspaceProtocol](./workspaces.md#saveworkspaceprotocol)
* [includeWorkspaceRoot](./workspaces.md#includeworkspaceroot)
* [ignoreWorkspaceCycles](./workspaces.md#ignoreworkspacecycles)
* [disallowWorkspaceCycles](./workspaces.md#disallowworkspacecycles)
* [failIfNoMatch](./workspaces.md#failifnomatch)

### Settings documented elsewhere

* [patchedDependencies](./cli/patch.md#patcheddependencies)
* [pnpmfile](./pnpmfile.md#pnpmfile), [globalPnpmfile](./pnpmfile.md#globalpnpmfile) and [ignorePnpmfile](./pnpmfile.md#ignorepnpmfile)
* [audit.level](./cli/audit.md#auditlevel), [audit.ignore](./cli/audit.md#auditignore) and [audit.ignorePrune](./cli/audit.md#auditignoreprune)
* [initVersion](./cli/init.md#initversion), [initLicense](./cli/init.md#initlicense) and [initAuthorName / initAuthorEmail / initAuthorUrl](./cli/init.md#initauthorname-initauthoremail-initauthorurl)
* [legacyDirFiltering](./filtering.md#legacydirfiltering)
* Authorization settings, which are read from [`.npmrc`](./npmrc.md)
