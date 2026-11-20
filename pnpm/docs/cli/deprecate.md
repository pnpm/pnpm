---
id: deprecate
title: pnpm deprecate
---

Set a deprecation message on a published package version. Consumers running `pnpm install` will see this message when the matching version is installed.

```sh
pnpm deprecate <pkg>[@<version-range>] <message>
```

To clear a deprecation message, use [`pnpm undeprecate`](#pnpm-undeprecate). An empty message fails with `ERR_PNPM_DEPRECATE_MESSAGE_REQUIRED`.

## Examples

Deprecate a single version:

```sh
pnpm deprecate foo@1.0.0 "Use foo@2 instead"
```

Deprecate a range of versions:

```sh
pnpm deprecate "foo@<2" "Please upgrade to foo@2"
```

Deprecate all versions:

```sh
pnpm deprecate foo "This package is no longer maintained"
```

## Options

### --registry &lt;url\>

The registry to write the deprecation to. Defaults to the registry configured for the package.

### --otp &lt;code\>

When the registry requires two-factor authentication, supply the one-time password via this flag.

## pnpm undeprecate

Remove a deprecation message from a package version.

```sh
pnpm undeprecate <pkg>[@<version-range>]
```
