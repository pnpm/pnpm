---
id: scim
title: SCIM deprovisioning
---

pnpr serves the SCIM 2.0 `Users` endpoints, so an identity provider such as
Microsoft Entra ID or Okta can deprovision an account when someone leaves.

## Configuration

```yaml title="pnpr.yaml"
auth:
  scim:
    token: ${PNPR_SCIM_TOKEN}
```

- `token` is the secret the identity provider sends as `Authorization: Bearer
  <token>`. It must be at least 32 characters long. Generate a random one and
  keep it out of the committed config with
  [environment variable substitution](configuration.md#environment-variable-substitution).
- The token authenticates only the SCIM endpoints. It is not a user token,
  and it cannot call the npm API or the admin API.
- Without `auth.scim`, pnpr does not serve the SCIM endpoints.

In the identity provider, set the tenant URL to
`https://registry.example/-/pnpr/v0/scim/v2`. `userName` is the account's
login name. The server assigns the immutable account `id`; use it as the
`userId` in an [OIDC binding](oidc.md#browser-sign-in).

## What deprovisioning does

Provisioning creates a passwordless account. An existing login name returns
`409`; provisioning does not take over a password account. A login request
cannot establish a password for a provisioned account. Use OIDC to sign in,
or have an administrator set its password.

Setting `active` to `false` revokes tokens, clears password credentials, and
ends browser sessions. The account retains its ID and login name. Setting
`active` to `true` restores that identity after credential cleanup. Credentials
issued before deactivation remain revoked.

Deleting a SCIM resource also deletes the account and frees its login name.
A later `POST` using that name creates a different account ID with no inherited
grants. A removed resource cannot be reactivated. `PUT` and `PATCH` can change
`userName` while retaining the same ID.

Other replicas follow the [replica reload timing](configuration.md#replica-reloads).
Applying a deactivation ends their browser sessions. If a replica cannot read
the directory, it refuses user credentials until a read succeeds.

## Endpoints

All paths are under `/-/pnpr/v0/scim/v2`.

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/ServiceProviderConfig`, `/ResourceTypes`, `/Schemas` | What pnpr supports. |
| `GET` | `/Users` | Provisioned users, as a `ListResponse`. `filter` supports only `userName eq "<name>"`. `startIndex` and `count` page the list. |
| `POST` | `/Users` | Provision a user. Returns `201`, or `409` with `scimType: uniqueness` when the `userName` exists. |
| `GET` | `/Users/{id}` | One user. Its `id` is its immutable account ID. |
| `PUT` | `/Users/{id}` | Replace the user's attributes and `active`. `userName` may change. |
| `PATCH` | `/Users/{id}` | A `PatchOp` with `add`, `replace`, or `remove` operations. Operations on a filtered path such as `emails[type eq "work"].value` are accepted and ignored. |
| `DELETE` | `/Users/{id}` | Deprovision the user and stop listing it. Returns `204`. |

pnpr reads only `userName` and `active`. It stores the other attributes it
receives, extension attributes under their schema URN, and returns them
unchanged. Groups, bulk operations, sorting, and
ETags are not supported. Assign teams with OIDC
[groups](oidc.md#teams-from-groups) or the
[team endpoints](endpoints.md#team-endpoints).

Account names come from the account store. If a name update returns a storage
error, fetch the resource by ID before retrying. Its name may have changed while
its other attributes retain their previous values.
