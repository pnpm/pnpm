---
id: identity
title: Account and team identity
---

pnpr assigns each account an opaque, immutable ID. Usernames identify login
names and can change. `pnpr add-user` prints the account ID; the admin user
listing also returns it.

Use account IDs in `auth.admins`, access lists, team membership lists, and
OIDC `userId` bindings. Tokens, package ownership, staged uploads, and publication
idempotency belong to account IDs. Renaming an account preserves these
relationships. Deleting it revokes its credentials and retains a deleted record
under a generated name. Registering the original username creates a new ID and
inherits none of those relationships.

Package maintainer entries include an account `userId` and a display-name
snapshot. Updating an unchanged entry preserves that ID. To assign a reused
username to its new account, an authorized maintainer or admin must explicitly
supply the new `userId` with the current name. A mismatched ID and name are rejected.

Teams also have stable IDs and mutable names:

```yaml
registries:
  private:
    type: hosted
    teams:
      team-release-01:
        name: release
        members: [<account-id>]
    packages:
      '@example/*':
        publish: team:team-release-01
```

The `teams:` keys are IDs. Keep them stable when editing names. API-created
teams receive server-generated IDs. OIDC group grants use these IDs too;
deleting and recreating a team name does not restore an old grant.

The native file backend uses `auth.users.file`, a JSON account file containing
IDs and password hashes. Protect it like any other credentials file. Apache
and Verdaccio htpasswd files are import sources for `pnpr import-htpasswd`.
An overwrite changes credentials on the existing account without changing its ID.

This experimental release changes auth database, account file, team roster,
and configuration formats. Start with fresh experimental state. There is no
migration from older alpha formats.
