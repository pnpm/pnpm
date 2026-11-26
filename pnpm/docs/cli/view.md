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

The summary ends with the registry the metadata was read from. If a version is missing or a dist-tag points to an older version, check whether that registry is a mirror that has not synced yet.

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

## Node.js

`pnpm view node` reads the Node.js release index, the same source [`pnpm add node`](./add.md) installs from. It honors the Node.js mirror settings. A version selector or release channel picks the release, as it does for [`pnpm runtime set`](./runtime.md):

```sh
pnpm view node versions
pnpm view node@22 npm
pnpm view node@lts version
pnpm view node@runtime:rc/24 version
```

The fields of a release are the ones the index lists, such as `npm`, `v8`, and `lts`. To read the npm package named `node`, use `pnpm view node@npm:node`.

Show the release notes of a version, and of the release that published it, from a registry that stores them, such as [pnpr](https://github.com/pnpm/pnpm/tree/main/pnpr):

```sh
pnpm view express@4.18.0 releaseNotes
```

pnpm prints notes as plain text. It removes control and bidirectional override characters and shows each link's target next to its text.

## Options

### --json

Output the metadata in JSON format.

### --registry &lt;url\>

The registry to read the metadata from. Defaults to the configured default registry.
