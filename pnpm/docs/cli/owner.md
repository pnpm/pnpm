---
id: owner
title: pnpm owner
---

Added in: v11.1.0

Aliases: `owners`

Manages package owners on the registry.

## Commands

### ls

```sh
pnpm owner ls <package>
```

Aliases: `list`

List all owners of a package. This is the default subcommand if no other subcommand is given.

### add

```sh
pnpm owner add <package> <user>
```

Add a user as an owner of a package. Requires authentication.

### rm

```sh
pnpm owner rm <package> <user>
```

Remove a user from the list of owners of a package. Requires authentication.

## Options

### --registry &lt;url\>

The base URL of the npm registry to use for the operation. Per-scope and named registries (configured via [`registries`](../settings/dependency-resolution.md#registries) and [`namedRegistries`](../settings/dependency-resolution.md#namedregistries)) are respected for the package being modified.

### --otp &lt;otp\>

When the registry requires two-factor authentication, this option supplies a one-time password.
