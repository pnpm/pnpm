---
id: view
title: pnpm view
---

Added in: v11.0.0

Aliases: `info`, `show`

View package metadata from the registry.

```sh
pnpm view <pkg>
pnpm view <pkg> [field]
pnpm view [field]
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

Show metadata for a specific version:

```sh
pnpm view express@4.18.0
```

When no package name is provided, `pnpm view` searches upward for the nearest project manifest (`package.json`, `package.yaml`, or `package.json5`) and uses its `name` field. If the manifest exists but has no `name`, the command fails.

## Options

### --json

Output the metadata in JSON format.
