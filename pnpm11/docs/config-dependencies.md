---
id: config-dependencies
title: Config Dependencies
---

Config dependencies allow you to share and centralize configuration files, settings, and hooks across multiple projects. They are installed before all regular dependencies ("dependencies", "devDependencies", "optionalDependencies"), making them ideal for setting up custom hooks, patches, and catalog entries.

Config dependencies help you keep all the hooks, settings, patches, overrides, catalogs, rules in a single place and use them across multiple repositories.

If your config dependency is named following the `pnpm-plugin-*`, `@*/pnpm-plugin-*`, or `@pnpm/plugin-*` pattern, pnpm will automatically load its `pnpmfile.mjs` (falling back to `pnpmfile.cjs`) from the package root.

## How to Add a Config Dependency

Config dependencies are defined in your `pnpm-workspace.yaml`. Their integrity checksums are stored in `pnpm-lock.yaml` (in a dedicated [env lockfile document](./lockfile.md)).

For example, running `pnpm add --config my-configs` will add this entry to your `pnpm-workspace.yaml`:

```yaml title="pnpm-workspace.yaml"
configDependencies:
  my-configs: "1.0.0"
```

**Important:**

* Config dependencies **cannot** have their own regular `dependencies`. They **can** declare `optionalDependencies`, but only one level deep — `optionalDependencies` of `optionalDependencies` are ignored.
* Config dependencies **cannot** define lifecycle scripts (like `preinstall`, `postinstall`, etc.).

### Platform-specific binaries via `optionalDependencies`

A config dependency may ship platform-specific binaries via `optionalDependencies`, the same pattern used by tools like esbuild and swc. Each platform-binary package declares its supported platform with `os`, `cpu`, and/or `libc` fields, and pnpm installs only the variant that matches the current host. The matching binary is symlinked next to the parent config dependency in the global virtual store, so `require('my-config-platform-arch')` from inside the config dependency resolves at runtime.

The env lockfile records all platform variants regardless of host platform, so the lockfile stays portable across machines.

Each entry in `optionalDependencies` must be declared with an **exact** version (e.g. `"1.2.3"`) — ranges (`"^1.0.0"`, `"~1.0.0"`) and tags (`"latest"`) are rejected. This keeps config-dep installs reproducible: the resolved subdep can't drift between machines for a parent that's pinned by integrity.

## Usage

### Installing Dependencies Used in Hooks

Config dependencies are installed **before** hooks from your [`.pnpmfile.mjs`] are loaded, allowing you to import logic from config packages.

Example:

```js title=".pnpmfile.mjs"
import { readPackage } from '.pnpm-config/my-hooks'

export const hooks = {
  readPackage
}
```

[`.pnpmfile.mjs`]: ./pnpmfile.md

### Updating pnpm Settings Dynamically

Using the [`updateConfig`] hook, you can dynamically update pnpm’s settings using config dependencies.

For example, the following `pnpmfile` adds a new [catalog] entry to pnpm's configuration:

```js title="@myorg/pnpm-plugin-my-catalogs/pnpmfile.mjs"
export const hooks = {
  updateConfig (config) {
    config.catalogs.default ??= {}
    config.catalogs.default['is-odd'] = '1.0.0'
    return config
  }
}
```

If you install it as config dependency:

```
pnpm add --config @myorg/pnpm-plugin-my-catalogs
```

Then you can run:

```
pnpm add is-odd@catalog:
```

This will install `is-odd@1.0.0` and add the following to your `package.json`:

```json
{
  "dependencies": {
    "is-odd": "catalog:"
  }
}
```

This makes it easy to maintain and share centralized configuration and dependency versions across projects.

[`updateConfig`]: ./pnpmfile.md#hooksupdateconfigconfig-config--promiseconfig
[catalog]: ./catalogs.md

### Loading Patch Files

You can reference [patch files] stored inside config dependencies.

Example:

```yaml title="pnpm-workspace.yaml"
configDependencies:
  my-patches: "1.0.0"
patchedDependencies:
  react: "node_modules/.pnpm-config/my-patches/react.patch"
```

[patch files]: ./cli/patch.md
