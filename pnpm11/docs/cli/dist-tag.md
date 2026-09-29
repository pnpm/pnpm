---
id: dist-tag
title: pnpm dist-tag
---

Added in: v11.0.0

Manage distribution tags for a package. Dist-tags provide human-readable aliases (like `latest`, `next`, `beta`) that point to specific package versions on the registry.

```sh
pnpm dist-tag add <pkg>@<version> [<tag>]
pnpm dist-tag rm <pkg> <tag>
pnpm dist-tag ls [<pkg>]
```

## Subcommands

### add &lt;pkg\>@&lt;version\> [&lt;tag\>]

Tag the specified version of a package with a dist-tag. If `<tag>` is omitted, the value of the [`tag`](../settings/other.md#tag) setting is used (defaults to `latest`).

```sh
pnpm dist-tag add foo@1.2.0 next
```

### rm &lt;pkg\> &lt;tag\>

Remove a dist-tag from a package.

```sh
pnpm dist-tag rm foo next
```

### ls [&lt;pkg\>]

List all dist-tags for a package. If no package name is given, the dist-tags for the current package (read from the local `package.json`) are shown.

```sh
pnpm dist-tag ls foo
```

## Options

### --registry &lt;url\>

The registry to operate on. Defaults to the registry configured for the package.

### --otp &lt;code\>

When the registry requires two-factor authentication, supply the one-time password via this flag or the `PNPM_CONFIG_OTP` environment variable.
