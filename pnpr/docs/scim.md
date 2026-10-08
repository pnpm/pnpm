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
`https://registry.example/-/pnpr/v0/scim/v2`, and map its `userName` to the
pnpr username: the `username` of the user's
[OIDC binding](oidc.md#browser-sign-in), or the account name of a password
user.

## What deprovisioning does

When the identity provider sets a user's `active` to `false`, or deletes the
user, pnpr:

- revokes every token the user holds;
- removes the user's password account, if it has one;
- ends the user's browser sessions on the replica that took the request.

pnpr also refuses every credential of that username and any new sign-in with
it. Other replicas apply this within 10 seconds, the same delay as
[admin API changes](configuration.md#changing-rules-through-the-api).

pnpr keeps refusing a deleted user. Provisioning the same `userName` again
with `POST`, or setting `active` back to `true`, lets the user sign in again.

Provisioning a user does not create a password account. Users sign in through
OIDC, or an admin creates the account.

## Endpoints

All paths are under `/-/pnpr/v0/scim/v2`.

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/ServiceProviderConfig`, `/ResourceTypes`, `/Schemas` | What pnpr supports. |
| `GET` | `/Users` | Provisioned users, as a `ListResponse`. `filter` supports only `userName eq "<name>"`. `startIndex` and `count` page the list. |
| `POST` | `/Users` | Provision a user. Returns `201`, or `409` with `scimType: uniqueness` when the `userName` exists. |
| `GET` | `/Users/{id}` | One user. Its `id` is its `userName`. |
| `PUT` | `/Users/{id}` | Replace the user's attributes and `active`. `userName` cannot change. |
| `PATCH` | `/Users/{id}` | A `PatchOp` with `add`, `replace`, or `remove` operations. Operations on a filtered path such as `emails[type eq "work"].value` are accepted and ignored. |
| `DELETE` | `/Users/{id}` | Deprovision the user and stop listing it. Returns `204`. |

pnpr reads only `userName` and `active`. It stores the other attributes it
receives and returns them unchanged. Groups, bulk operations, sorting, and
ETags are not supported. Assign teams with OIDC
[groups](oidc.md#teams-from-groups) or the
[team endpoints](endpoints.md#team-endpoints).
