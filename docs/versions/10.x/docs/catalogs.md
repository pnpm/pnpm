---
id: catalogs
title: Catalogs
---

"_Catalogs_" are a [workspace feature](./workspaces.md) for defining dependency version ranges as reusable constants. Constants defined in catalogs can later be referenced in `package.json` files.

<iframe width="560" height="315" src="https://www.youtube-nocookie.com/embed/PuRUk4mV2jc" title="pnpm Catalogs — A New Tool to Manage Dependencies in monorepos" frameborder="0" allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; fullscreen"></iframe>

## The Catalog Protocol (`catalog:`)

Once a catalog is defined in `pnpm-workspace.yaml`,

```yaml title="pnpm-workspace.yaml"
packages:
  - packages/*

# Define a catalog of version ranges.
catalog:
  react: ^18.3.1
  redux: ^5.0.1
```

The `catalog:` protocol can be used instead of the version range itself.

```json title="packages/example-app/package.json"
{
  "name": "@example/app",
  "dependencies": {
    "react": "catalog:",
    "redux": "catalog:"
  }
}
```

This is equivalent to writing a version range (e.g. `^18.3.1`) directly.

```json title="packages/example-app/package.json"
{
  "name": "@example/app",
  "dependencies": {
    "react": "^18.3.1",
    "redux": "^5.0.1"
  }
}
```

You may use the `catalog:` protocol in the next fields:

* `package.json`:
  * `dependencies`
  * `devDependencies`
  * `peerDependencies`
  * `optionalDependencies`
* `pnpm-workspace.yaml`
    * `overrides`

The `catalog:` protocol allows an optional name after the colon (ex: `catalog:name`) to specify which catalog should be used. When a name is omitted, the default catalog is used.

Depending on the scenario, the `catalog:` protocol offers a few [advantages](#advantages) compared to writing version ranges directly that are detailed next.

## Advantages

In a workspace (i.e. monorepo or multi-package repo) it's common for the same dependency to be used by many packages. Catalogs reduce duplication when authoring `package.json` files and provide a few benefits in doing so:

- **Maintain unique versions** — It's usually desirable to have only one version of a dependency in a workspace. Catalogs make this easier to maintain. Duplicated dependencies can conflict at runtime and cause bugs. Duplicates also increase size when using a bundler.
- **Easier upgrades** — When upgrading a dependency, only the catalog entry in `pnpm-workspace.yaml` needs to be edited rather than all `package.json` files using that dependency. This saves time — only one line needs to be changed instead of many.
- **Fewer merge conflicts** — Since `package.json` files do not need to be edited when upgrading a dependency, git merge conflicts no longer happen in these files.

## Defining Catalogs

Catalogs are defined in the `pnpm-workspace.yaml` file. There are two ways to define catalogs.

1. Using the (singular) `catalog` field to create a catalog named `default`.
2. Using the (plural) `catalogs` field to create arbitrarily named catalogs.

:::tip

If you have an existing workspace that you want to migrate to using catalogs, you can use the following [codemod](https://go.codemod.com/pnpm-catalog):

```
pnpx codemod pnpm/catalog
```

:::

### Default Catalog

The top-level `catalog` field allows users to define a catalog named `default`.

```yaml title="pnpm-workspace.yaml"
catalog:
  react: ^18.2.0
  react-dom: ^18.2.0
```

These version ranges can be referenced through `catalog:default`. For the default catalog only, a special `catalog:` shorthand can also be used. Think of `catalog:` as a shorthand that expands to `catalog:default`.

### Named Catalogs

Multiple catalogs with arbitrarily chosen names can be configured under the `catalogs` key.

```yaml title="pnpm-workspace.yaml"
catalogs:
  # Can be referenced through "catalog:react17"
  react17:
    react: ^17.0.2
    react-dom: ^17.0.2

  # Can be referenced through "catalog:react18"
  react18:
    react: ^18.2.0
    react-dom: ^18.2.0
```

A default catalog can be defined alongside multiple named catalogs. This might be useful in a large multi-package repo that's migrating to a newer version of a dependency piecemeal.

```yaml title="pnpm-workspace.yaml"
catalog:
  react: ^16.14.0
  react-dom: ^16.14.0

catalogs:
  # Can be referenced through "catalog:react17"
  react17:
    react: ^17.0.2
    react-dom: ^17.0.2

  # Can be referenced through "catalog:react18"
  react18:
    react: ^18.2.0
    react-dom: ^18.2.0
```

## Publishing

The `catalog:` protocol is removed when running `pnpm publish` or `pnpm pack`. This is similar to the [`workspace:` protocol](./workspaces.md#workspace-protocol-workspace), which is [also replaced on publish](./workspaces.md#publishing-workspace-packages).

For example,

```json title="packages/example-components/package.json"
{
  "name": "@example/components",
  "dependencies": {
    "react": "catalog:react18",
  }
}
```

Will become the following on publish.

```json title="packages/example-components/package.json"
{
  "name": "@example/components",
  "dependencies": {
    "react": "^18.3.1",
  }
}
```

The `catalog:` protocol replacement process allows the `@example/components` package to be used by other workspaces or package managers.

## Settings

import CatalogMode from './settings/_catalogMode.mdx'

<CatalogMode />

import CleanupUnusedCatalogs from './settings/_cleanupUnusedCatalogs.mdx'

<CleanupUnusedCatalogs />
