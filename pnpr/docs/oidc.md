---
id: oidc
title: OpenID Connect
---

Added in: v0.1.0-alpha.11

pnpr supports OIDC browser sign-in and keyless npm publishing from workloads
such as GitHub Actions. Both use discovery and signed ID tokens from explicitly
configured issuers. Local password authentication keeps working alongside them.

## Browser sign-in

Register a web application with your identity provider. Set its callback to
`https://registry.example/-/oidc/company/callback`, replacing the hostname and
`company` with your pnpr provider name, and enable the authorization code flow.
Start pnpr with `--public-url https://registry.example`, its HTTPS origin
without a path prefix.

```yaml
auth:
  oidc:
    - name: company
      issuer: https://accounts.google.com
      audience: your-client-id
      login:
        clientSecret: ${OIDC_CLIENT_SECRET}
        users:
          - subject: 'the-users-stable-sub-claim'
            username: alice
            claims:
              hd: example.com
```

`audience` is the application's client ID. `clientSecret` can be omitted for
providers that support public clients with PKCE. `subject` is the exact `sub`
claim issued to this application, not an email address. Each binding maps that
subject to a pnpr username, and the registry's existing access rules and teams
then apply. Extra `claims` are exact string matches and all of them must match.
There is no automatic account creation, and no linking by email.

Use the issuer your provider publishes:

| Provider | Issuer |
| --- | --- |
| Google Workspace | `https://accounts.google.com` |
| Microsoft Entra ID | `https://login.microsoftonline.com/<tenant-id>/v2.0` |
| Okta org authorization server | `https://<your-org>.okta.com` |
| Okta custom authorization server | `https://<your-org>.okta.com/oauth2/<authorization-server-id>` |

Use a tenant-specific Entra issuer. For Google Workspace, require the `hd` claim
for your organization's domain, as shown above. See the provider documentation
for [Google](https://developers.google.com/identity/openid-connect/reference),
[Entra](https://learn.microsoft.com/en-us/entra/identity-platform/v2-protocols-oidc),
and [Okta](https://developer.okta.com/docs/concepts/auth-servers/).

Open `https://registry.example/-/oidc/company/login` in a browser. After
sign-in, pnpr displays a token to put in the registry's `_authToken` setting,
for example in your private user `.npmrc`:

```ini
//registry.example/:_authToken=THE_DISPLAYED_TOKEN
```

The browser session expires at the earlier of one hour or the ID token's expiry,
and `npm logout` revokes it. Sessions live in memory and disappear on restart,
so a multi-replica deployment needs affinity for both the login flow and the
authenticated requests that follow. They do not appear in `npm token list`. This
sign-in URL is separate from the npm CLI's `npm login` web protocol. An OIDC
session cannot yet be exchanged for an [OCI scoped bearer token](container-images.md#authentication).

## GitHub Actions keyless publishing

Configure a dedicated workload username, a named hosted npm registry, and exact
package names. The package's normal `publish` rule must grant that username
access too.

```yaml
registries:
  private:
    type: hosted
    packages:
      '@example/widget':
        publish: [release-ci]
auth:
  oidc:
    - name: github
      issuer: https://token.actions.githubusercontent.com
      audience: https://registry.example
      workloads:
        - identity:
            subject: repo:example/widgets:ref:refs/heads/main
            username: release-ci
            claims:
              repository_id: '123456789'
              repository_owner_id: '987654321'
              workflow_ref: example/widgets/.github/workflows/release.yml@refs/heads/main
          registry: private
          packages: ['@example/widget']
```

Use immutable repository and owner IDs so trust does not transfer when names are
reused. For reusable workflows, also require `job_workflow_ref` to pin the
workflow that performs the publication. A job using a GitHub environment has an
environment-based subject, such as
`repo:example/widgets:environment:production`. Configure the exact subject and
the additional claims you intend to trust. See
[GitHub's OIDC reference](https://docs.github.com/en/actions/reference/security/oidc).

The publishing job needs `id-token: write`. Request an ID token with pnpr's
configured audience and prefix it with `pnpr_workload_` as the registry token.
No pnpr password, persistent API key, or token exchange is involved:

```yaml
permissions:
  contents: read
  id-token: write
steps:
  # Check out, install dependencies, and build before requesting the token.
  - name: Publish to pnpr
    shell: bash
    run: |
      response=$(curl --fail --silent --show-error \
        -H "Authorization: bearer $ACTIONS_ID_TOKEN_REQUEST_TOKEN" \
        "${ACTIONS_ID_TOKEN_REQUEST_URL}&audience=https%3A%2F%2Fregistry.example")
      token=$(jq -er '.value' <<< "$response")
      echo "::add-mask::$token"
      export NODE_AUTH_TOKEN="pnpr_workload_${token}"
      npm publish --registry=https://registry.example/~private/
```

The project's `.npmrc` holds only the environment reference:

```ini
//registry.example/~private/:_authToken=${NODE_AUTH_TOKEN}
```

On a server with more than one ecosystem, use `/npm/~private/` in both places.

The credential permits ordinary `PUT` publications to the configured packages
through that named registry and nothing else. It cannot read packages, unpublish,
change dist-tags separately, manage accounts, publish batches, or reach other
pnpr services. Request the token after building so it stays valid through the
publish request; its issuer's expiry applies to every request.

## Validation and operations

pnpr accepts RS256 and ES256 signatures and verifies the issuer, audience,
authorized party where present, expiry, issue time, and not-before claims.
Browser login also verifies a browser-bound state, a nonce, and S256 PKCE.
Unmapped and ambiguous identities are rejected, and user subjects must be unique
within a provider, its workload bindings included.

The `pnpr_oidc_` and `pnpr_workload_` credential prefixes are reserved for OIDC,
and workload credentials bypass the persistent token backend.

Discovery and JWKS use HTTPS with redirects disabled and bounded response sizes
and deadlines. Literal and DNS-resolved destinations must be public IP addresses,
and OIDC requests ignore environment-configured HTTP proxies. Metadata is cached
for five minutes, and a verification failure can trigger a refresh at most once
every 30 seconds per provider, so an unavailable provider can never turn an
invalid credential into anonymous access.

Login state lives in a signed HttpOnly cookie for five minutes. Anonymous login
starts reserve no server-side entries. Successful login replays and browser
sessions are each capped at 1,024 per process, and a separate 1,024-entry
callback-attempt history rejects recent failed and concurrent replays before
token exchange, evicting the oldest attempt when full. At most 16 browser
callbacks perform network operations concurrently.

pnpr omits OIDC callback query strings from its request logs. Configure reverse
proxies to omit them too.
