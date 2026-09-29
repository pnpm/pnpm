---
id: access
title: pnpm access
---

Added in: v11.11.0

Manages package access and visibility on the registry.

```sh
pnpm access list packages [<user>|<scope>|<scope:team>]
pnpm access list collaborators <package> [<user>]
pnpm access get status <package>
pnpm access set status=public|private <package>
pnpm access set mfa=none|publish|automation <package>
pnpm access grant <read-only|read-write> <scope:team> <package>
pnpm access revoke <scope:team> <package>
```

## Subcommands

### list packages

List the packages a user, scope, or team can access. With no argument, your own packages are listed.

```sh
pnpm access list packages
pnpm access list packages alice
pnpm access list packages @myorg
pnpm access list packages @myorg:developers
```

The argument type is inferred from its shape: a value containing `:` is a team, a value starting with `@` is an organization, and anything else is a user.

`ls` is accepted as an alias, so `pnpm access ls` is equivalent to `pnpm access list packages`.

### list collaborators

List the collaborators on a package, optionally filtered to a single user.

```sh
pnpm access list collaborators @myorg/pkg
pnpm access list collaborators @myorg/pkg alice
```

### get status

Show whether a package is public or restricted.

```sh
pnpm access get status @myorg/pkg
```

### set status

Set the package visibility.

```sh
pnpm access set status=public @myorg/pkg
pnpm access set status=private @myorg/pkg
```

Only scoped packages can change visibility. Unscoped packages are always public, and attempting to change one fails with `ERR_PNPM_ACCESS_SET_STATUS_UNSCOPED`.

### set mfa

Set the two-factor authentication requirement for publishing a package.

```sh
pnpm access set mfa=none @myorg/pkg
pnpm access set mfa=publish @myorg/pkg
pnpm access set mfa=automation @myorg/pkg
```

`none` disables the requirement, while `publish` and `automation` both require two-factor authentication for publishing.

### grant

Grant a team read-only or read-write access to a package.

```sh
pnpm access grant read-only @myorg:developers @myorg/pkg
pnpm access grant read-write @myorg:developers @myorg/pkg
```

### revoke

Revoke a team's access to a package.

```sh
pnpm access revoke @myorg:developers @myorg/pkg
```

## Options

### --registry &lt;url\&gt;

The base URL of the npm registry to use for the operation. Per-scope and named registries (configured via [`registries`](../settings/dependency-resolution.md#registries) and [`namedRegistries`](../settings/dependency-resolution.md#namedregistries)) are respected for the package being modified.

### --json

Output results in JSON format. Applies to `list packages`, `list collaborators`, and `get status`.

### --otp &lt;code\&gt;

When the registry requires two-factor authentication, this option supplies a one-time password. It applies to the subcommands that modify state: `set status`, `set mfa`, `grant`, and `revoke`.
