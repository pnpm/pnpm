---
id: deprecate
title: pnpm deprecate
---

Added in: v11.0.0

Set a deprecation message on a published package version. Consumers running `pnpm install` will see this message when the matching version is installed.

```sh
pnpm deprecate <pkg>[@<version-range>] <message>
```

To clear a deprecation message, use [`pnpm undeprecate`](#pnpm-undeprecate) or pass an empty string:

```sh
pnpm deprecate foo@1.0.0 ""
```

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

The registry to publish to. Defaults to the registry configured for the package.

### --otp &lt;code\>

When the registry requires two-factor authentication, supply the one-time password via this flag or the `PNPM_CONFIG_OTP` environment variable.

## pnpm undeprecate

Remove a deprecation message from a package version. Equivalent to running `pnpm deprecate <pkg>[@<version-range>] ""`.

```sh
pnpm undeprecate <pkg>[@<version-range>]
```
