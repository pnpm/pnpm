---
id: feature-comparison
title: Feature Comparison
---

| Feature                          |pnpm              |npm               | Notes |
| ---                              |:--:              |:--:              | ---   |
| [Workspace support]              |:white_check_mark:|:white_check_mark:|
| Isolated `node_modules`          |:white_check_mark:|:white_check_mark:| Default in pnpm. |
| [Hoisted `node_modules`]         |:white_check_mark:|:white_check_mark:| Default in npm. |
| Plug'n'Play                      |:white_check_mark:|:x:               |
| [Autoinstalling peers]           |:white_check_mark:|:white_check_mark:|
| [Patching dependencies]          |:white_check_mark:|:x:               |
| [Managing runtimes]              |:white_check_mark:|:x:               |
| [Managing versions of itself]    |:white_check_mark:|:x:               |
| Has a lockfile                   |:white_check_mark:|:white_check_mark:| `pnpm-lock.yaml`, `package-lock.json`. |
| [Overrides support]              |:white_check_mark:|:white_check_mark:|
| Content-addressable storage      |:white_check_mark:|:x:               |
| [Dynamic package execution]      |:white_check_mark:|:white_check_mark:| `pnx`, `npx`. |
| [Side-effects cache]             |:white_check_mark:|:x:               |
| [Catalogs]                       |:white_check_mark:|:x:               |
| [Config dependencies]            |:white_check_mark:|:x:               |
| [JSR registry support]           |:white_check_mark:|:x:               |
| [Auto-install before script run] |:white_check_mark:|:x:               |
| [Hooks]                          |:white_check_mark:|:x:               |
| [Build script security]          |:white_check_mark:|:x:               |
| [SBOM generation]                |:white_check_mark:|:white_check_mark:| `pnpm sbom`, `npm sbom`. |
| [Listing licenses]               |:white_check_mark:|:x:               | pnpm supports it via `pnpm licenses list`. |

[Auto-install before script run]: ./settings/build.md#verifydepsbeforerun
[Autoinstalling peers]: ./settings/peer-dependencies.md#autoinstallpeers
[Catalogs]: ./catalogs.md
[Config dependencies]: ./config-dependencies.md
[Dynamic package execution]: ./cli/pnx.md
[Hoisted `node_modules`]: ./settings/node-modules.md#nodelinker
[JSR registry support]: ./cli/add.md#install-from-the-jsr-registry
[Listing licenses]: ./cli/licenses.md
[Build script security]: ./settings/build.md#allowbuilds
[Managing runtimes]: ./cli/runtime.md
[Managing versions of itself]: ./settings/cli.md#pmonfail
[Overrides support]: ./settings/dependency-resolution.md#overrides
[Patching dependencies]: ./cli/patch.md
[SBOM generation]: ./cli/sbom.md
[Side-effects cache]: ./settings/build.md#sideeffectscache
[Workspace support]: ./workspaces.md
[hooks]: ./pnpmfile.md

**Note:** To keep the comparison concise, we include only features likely to be used frequently.
