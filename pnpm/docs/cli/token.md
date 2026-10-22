---
id: token
title: pnpm token
---

Added in: v12.13.0

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

An ID prefix that matches several tokens fails with `ERR_PNPM_TOKEN_AMBIGUOUS`. One that matches no token fails with `ERR_PNPM_TOKEN_NOT_FOUND`. Without credentials, the command fails with `ERR_PNPM_TOKEN_UNAUTHORIZED`.

### create

```sh
pnpm token create [--read-only] [--cidr <cidr>]
```

Create a new authentication token. pnpm asks for your account password. If standard input is not a terminal, pnpm reads the password from its first line.

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

Restrict a created token to specific CIDR address ranges. Separate several ranges with commas, or repeat the option:

```sh
pnpm token create --cidr 192.0.2.0/24,198.51.100.0/24
```

### --otp &lt;otp\>

One-time password for registries that require two-factor authentication.
