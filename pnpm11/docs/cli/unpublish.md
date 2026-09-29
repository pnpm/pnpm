---
id: unpublish
title: pnpm unpublish
---

Added in: v11.0.0

Remove a published package version from the registry.

```sh
pnpm unpublish [<pkg>[@<version>]] [--force]
```

:::warning

Unpublishing is generally discouraged. Most registries (including the public npm registry) restrict when and how packages can be unpublished. Prefer [`pnpm deprecate`](./deprecate.md) whenever possible.

:::

## Examples

Unpublish a specific version:

```sh
pnpm unpublish foo@1.0.0
```

Unpublish a range of versions using a semver specifier:

```sh
pnpm unpublish "foo@<2"
```

Unpublish an entire package (all versions). Requires `--force`:

```sh
pnpm unpublish foo --force
```

When run without arguments inside a package directory, pnpm unpublishes the current package version read from the local `package.json`.

## Options

### --force

Required when removing an entire package (all versions) rather than a specific version or range.

### --registry &lt;url\>

The registry to publish to. Defaults to the registry configured for the package.

### --otp &lt;code\>

When the registry requires two-factor authentication, supply the one-time password via this flag or the `PNPM_CONFIG_OTP` environment variable.
