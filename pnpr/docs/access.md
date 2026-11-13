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

The serving registry determines the package policy. Its most specific matching
`packages` rule selects `access`, `publish`, or `unpublish`; omitted fields use
the registry defaults. The rules are the ones
[stored in the database](configuration.md#registries-in-the-database).

An access list admits a caller when any listed entry matches. Entries can be
account IDs, built-in groups, or `team:<teamId>` references to that registry's
team directory. The teams are stored with the registry in the metadata
database.

`$owners` is available for ownership-aware writes. For an existing package it
matches the immutable maintainer IDs in the stored package document. On first
publication it admits an authenticated caller. Accounts listed in `auth.admins`
also satisfy `$owners`; admin status does not bypass an arbitrary package rule
that omits `$owners`.

A named upstream's `access` policy is an additional check. It does not replace
package rules. Upstream credentials stay on the server and do not grant the
caller access by themselves.

## Teams from OIDC groups and personal tokens

An OIDC browser session carries the team grants earned from its verified groups
claim. Those grants last for that session. They apply only to the configured
registry and team, including artifact rules that reference that same team.

Personal access tokens do not inherit session group grants. They use their
account's configured or API-managed team memberships. To give a session and a
personal token the same durable access, put the account ID in the team's roster.
See [Teams from groups](oidc.md#teams-from-groups).

## Resolver, artifacts, and pipelines

The resolver requires an authenticated caller. Package reads during resolution
still apply package and upstream access rules.

Artifacts, compiler caches, and pipeline records require an authenticated caller
and a declared `artifacts.owners` entry. Its `access` list permits reads;
publication requires both `access` and `publish`. A caller without read access
gets `404`, as does an undeclared owner.

Artifact lists can reference a hosted team with `team:<registry>:<teamId>`.
They use the same live team roster and session grants as package rules.
`$owners` is not valid here. `auth.admins` does not grant implicit artifact access;
include the admin account ID or one of its teams in the owner's lists.

## Administration

Admin endpoints require an account in `auth.admins`. SCIM endpoints instead
require the dedicated `auth.scim.token`; that secret grants no user, package,
or admin access.
