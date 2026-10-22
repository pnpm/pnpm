---
id: view
title: pnpm view
---

Aliases: `info`, `show`, `v`

View package metadata from the registry.

```sh
pnpm view [<pkg>]
pnpm view <pkg> [field...]
```

## Usage

Show all metadata for a package:

```sh
pnpm view express
```

Show a specific field:

```sh
pnpm view express version
pnpm view express dependencies
pnpm view express dist-tags
```

Several fields and dotted paths are accepted:

```sh
pnpm view express dist.tarball version
```

Show metadata for a specific version:

```sh
pnpm view express@4.18.0
```

When no package name is provided, `pnpm view` searches upward for the nearest project manifest (`package.json`, `package.yaml`, or `package.json5`) and uses its `name` field. If the manifest exists but has no `name`, the command fails. To select fields, name the package: the first argument is always the package, so `pnpm view version` looks up a package named `version`.

## Options

### --json

Output the metadata in JSON format.

### --registry &lt;url\>

The registry to read the metadata from. Defaults to the configured default registry.
