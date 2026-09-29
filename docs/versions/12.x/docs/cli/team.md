---
id: team
title: pnpm team
---

Added in: v11.13.0

Manages organization teams and team memberships on the registry.

```sh
pnpm team create <scope:team> [--otp <code>]
pnpm team destroy <scope:team> [--otp <code>]
pnpm team add <scope:team> <user> [--otp <code>]
pnpm team rm <scope:team> <user> [--otp <code>]
pnpm team ls <scope|scope:team>
```

Team references are always written with a leading `@`: `@myorg` for an organization and `@myorg:developers` for a team within it.

## Subcommands

### create

Create a new team in an organization.

```sh
pnpm team create @myorg:developers
```

### destroy

Destroy an existing team.

```sh
pnpm team destroy @myorg:developers
```

### add

Add a user to an existing team.

```sh
pnpm team add @myorg:developers alice
```

### rm

Remove a user from an existing team.

```sh
pnpm team rm @myorg:developers alice
```

### ls

List the teams in an organization, or the members of a team.

```sh
pnpm team ls @myorg
pnpm team ls @myorg:developers
```

Aliases: `list`. If no subcommand is given and the first argument looks like a scope or team, `ls` is assumed, so `pnpm team @myorg` is equivalent to `pnpm team ls @myorg`.

## Options

### --registry &lt;url\&gt;

The base URL of the npm registry to use for the operation. A registry configured for the organization's scope (via [`registries`](../settings/dependency-resolution.md#registries)) is respected.

### --otp &lt;code\&gt;

When the registry requires two-factor authentication, this option supplies a one-time password. It applies to `create`, `destroy`, `add`, and `rm`.

### --parseable

Print the results of `ls` as bare names, one per line, without headers or indentation.

### --json

Print the results of `ls` as a JSON array of names. Takes precedence over `--parseable`.
