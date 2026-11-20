---
id: access
title: Access evaluation
---

pnpr authenticates a request once, then evaluates the rules for the requested
resource. Passing one check does not skip the checks that follow it.

## Identity and credentials

1. pnpr resolves the credential to an immutable account ID. A personal access
   token must belong to an existing account. An OIDC browser session or workload
   credential must also name an existing account.
2. Token expiry, read-only restrictions, and allowed IP ranges apply before
   the requested operation. Workload credentials additionally restrict the
   registry, packages, and operations they permit.
3. SCIM deprovisioning rejects the account's credentials. A directory read
   failure refuses user credentials until a reload succeeds. See
   [SCIM provisioning](scim.md) for replica timing and credential revocation.
4. The resource checks below use that same resolved identity.

Missing or unknown ordinary tokens resolve to anonymous access. Invalid OIDC
credentials are rejected even for public resources.

## Package access

[Grants](configuration.md#grants) decide what a caller may do to a package. A
caller may do an action when any grant that covers the serving registry and
the package gives the action to a subject the caller matches. Subjects are
account IDs, built-in groups, or `team:<teamId>` references to the
[teams](configuration.md#teams) of the pnpr instance.

`$owners` is available for `publish`, `annotate` and `delete`. For an existing
package it matches the immutable maintainer IDs in the stored package document.
On first publication it admits an authenticated caller. Accounts listed in
`auth.admins` also satisfy `$owners`; admin status does not give an action that
no grant gives `$owners`.

Upstream credentials stay on the server and do not grant the caller access by
themselves. Reading through an upstream takes `read`, and having it fetched
takes `cache`.

## Teams from OIDC groups and personal tokens

An OIDC browser session carries the team memberships earned from its verified
groups claim. They last for that session. They apply wherever a grant names the
team, and in artifact rules.

Personal access tokens do not inherit those session memberships. They use their
account's team memberships. To give a session and a personal token the same
durable access, add the account to the team.
See [Teams from groups](oidc.md#teams-from-groups).

## Resolver, artifacts, and pipelines

The resolver requires an authenticated caller. Package reads during resolution
still apply the grants.

Artifacts, compiler caches, and pipeline records require an authenticated caller
and a declared `artifacts.owners` entry. Its `access` list permits reads;
publication requires both `access` and `publish`. A caller without read access
gets `404`, as does an undeclared owner.

Artifact lists can name a team with `team:<teamId>`. They use the same team
members and session memberships as grants.
`$owners` is not valid here. `auth.admins` does not grant implicit artifact access;
include the admin account ID or one of its teams in the owner's lists.

## Administration

Admin endpoints require an account in `auth.admins`. SCIM endpoints instead
require the dedicated `auth.scim.token`; that secret grants no user, package,
or admin access.
