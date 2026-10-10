---
id: token
title: pnpm token
---

Added in: v12.12.0

Manage authentication tokens on the configured registry.

## Commands

### list

```sh
pnpm token list
```

Aliases: `ls`

List all authentication tokens issued to the authenticated account. This is the default subcommand when none is provided.

### revoke

```sh
pnpm token revoke <id|token...>
```

Aliases: `rm`, `delete`, `remove`

Revoke one or more authentication tokens by their key, ID prefix, or token value.

### create

```sh
pnpm token create [--read-only] [--cidr <cidr>]
```

Create a new authentication token.

## Options

### --registry &lt;url\>

The base URL of the npm registry.

### --json

Output results in JSON format.

### -p, --parseable

Output results in tab-separated table format.

### --read-only

Mark a created token as read-only.

### --cidr &lt;cidr\>

Restrict a created token to specific CIDR address ranges.

### --otp &lt;otp\>

One-time password for registries that require two-factor authentication.
