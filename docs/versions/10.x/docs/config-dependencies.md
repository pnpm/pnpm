---
id: config-dependencies
title: Config Dependencies
---

Config dependencies allow you to share and centralize configuration files, settings, and hooks across multiple projects. They are installed before all regular dependencies ("dependencies", "devDependencies", "optionalDependencies"), making them ideal for setting up custom hooks, patches, and catalog entries.

Config dependencies help you keep all the hooks, settings, patches, overrides, catalogs, rules in a single place and use them across multiple repositories.

If your config dependency is named following the `pnpm-plugin-*` or `@*/pnpm-plugin-*` pattern, pnpm will automatically load the `pnpmfile.cjs` from its root.

## How to Add a Config Dependency

Config dependencies are defined in your `pnpm-workspace.yaml` and must be installed using an exact version and an integrity checksum.

For example, running `pnpm add --config my-configs` will add this entry to your `pnpm-workspace.yaml`:

```yaml title="pnpm-workspace.yaml"
configDependencies:
  my-configs: "1.0.0+sha512-30iZtAPgz+LTIYoeivqYo853f02jBYSd5uGnGpkFV0M3xOt9aN73erkgYAmZU43x4VfqcnLxW9Kpg3R5LC4YYw=="
```

**Important:**

* Config dependencies **cannot** have their own dependencies.
* Config dependencies **cannot** define lifecycle scripts (like `preinstall`, `postinstall`, etc.).

## Usage

### Loading an Allow List of Built Dependencies

You can load a list of package names that are allowed to be built, using the [`onlyBuiltDependenciesFile`] setting.

Example `allow.json` file inside a config dependency:

```json title="allow.json"
[
  "@airbnb/node-memwatch",
  "@apollo/protobufjs",
  ...
]
```

Your workspace configuration:

```yaml title="pnpm-workspace.yaml"
configDependencies:
  '@myorg/trusted-deps': 0.1.0+sha512-IERT0uXPBnSZGsCmoSuPzYNWhXWWnKkuc9q78KzLdmDWJhnrmvc7N4qaHJmaNKIusdCH2riO3iE34Osohj6n8w==
onlyBuiltDependenciesFile: node_modules/.pnpm-config/@myorg/trusted-deps/allow.json
```

[`onlyBuiltDependenciesFile`]: settings.md#onlybuiltdependenciesfile

### Installing Dependencies Used in Hooks

Config dependencies are installed **before** hooks from your [`.pnpmfile.cjs`] are loaded, allowing you to import logic from config packages.

Example:

```js title=".pnpmfile.cjs"
const { readPackage } = require('.pnpm-config/my-hooks')

module.exports = {
  hooks: {
    readPackage
  }
}
```

[`.pnpmfile.cjs`]: ./pnpmfile.md

### Updating pnpm Settings Dynamically

Using the [`updateConfig`] hook, you can dynamically update pnpm’s settings using config dependencies.

For example, the following `pnpmfile` adds a new [catalog] entry to pnpm's configuration:

```js title="@myorg/pnpm-plugin-my-catalogs/pnpmfile.cjs"
module.exports = {
  hooks: {
    updateConfig (config) {
      config.catalogs.default ??= {}
      config.catalogs.default['is-odd'] = '1.0.0'
      return config
    }
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
  my-patches: "1.0.0+sha512-30iZtAPgz+LTIYoeivqYo853f02jBYSd5uGnGpkFV0M3xOt9aN73erkgYAmZU43x4VfqcnLxW9Kpg3R5LC4YYw=="
patchedDependencies:
  react: "node_modules/.pnpm-config/my-patches/react.patch"
```

[patch files]: ./cli/patch.md
