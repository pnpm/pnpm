---
id: profile
title: pnpm profile
---

Change settings on your registry profile.

```sh
pnpm profile get [<property>]
pnpm profile set <property> <value>
pnpm profile enable-2fa [auth-only|auth-and-writes]
pnpm profile disable-2fa
```

## Commands

### get

Display all properties of your profile, or a specific property.

```sh
pnpm profile get
pnpm profile get email
```

### set

Set the value of a profile property.

```sh
pnpm profile set fullname "Example User"
```

### enable-2fa

Enable or update two-factor authentication mode.

```sh
pnpm profile enable-2fa
pnpm profile enable-2fa auth-only
```

### disable-2fa

Disable two-factor authentication on your account.

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
