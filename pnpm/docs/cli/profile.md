---
id: profile
title: pnpm profile
---

Change settings on your registry profile.

```sh
pnpm profile get [<property>...]
pnpm profile set <property> <value>
pnpm profile set password
pnpm profile enable-2fa [auth-only|auth-and-writes]
pnpm profile disable-2fa
```

## Commands

### get

Display all properties of your profile, or the values of the given properties separated by tabs.

```sh
pnpm profile get
pnpm profile get email
```

### set

Set the value of a profile property. The writable properties are `email`, `fullname`, `homepage`, `freenode`, `twitter`, `github`, and `password`.

```sh
pnpm profile set fullname "Example User"
```

`pnpm profile set password` prompts for your current and new password. Do not pass passwords on the command line.

### enable-2fa

Enable two-factor authentication, or change its mode. The default mode is `auth-and-writes`. pnpm prompts for your password. On first setup, pnpm shows a QR code for your authenticator app, asks for a code from it, and prints your recovery codes.

This command is interactive, so it does not support `--json` or `--parseable`.

```sh
pnpm profile enable-2fa
pnpm profile enable-2fa auth-only
```

### disable-2fa

Disable two-factor authentication on your account. pnpm prompts for your password.

```sh
pnpm profile disable-2fa
```

## Options

### --registry &lt;url\>

The base URL of the npm registry.

### --json

Output results as JSON.

### -p, --parseable

Output results in tab-separated format.

### --otp &lt;otp\>

One-time password from a two-factor authenticator.
