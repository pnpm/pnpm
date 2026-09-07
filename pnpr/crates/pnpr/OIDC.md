# OpenID Connect authentication

pnpr supports OIDC browser sign-in and keyless npm publishing from workloads
such as GitHub Actions. Both use discovery and signed ID tokens from explicitly
configured issuers. Local password authentication remains available.

## Browser sign-in

Register a web application with your identity provider. Set its callback to
`https://registry.example/-/oidc/company/callback`, replacing the hostname and
`company` with your pnpr provider name. Enable the authorization code flow.
Start pnpr with `--public-url https://registry.example`, using its HTTPS
origin without a path prefix.

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
claim issued to this application. It is not an email address. Each binding
maps that subject to a pnpr username; existing registry access rules and teams
then apply. Extra `claims` are exact string matches, and all must match.
There is no automatic account creation or linking by email.

Use the issuer published by your provider:

| Provider | Issuer |
| --- | --- |
| Google Workspace | `https://accounts.google.com` |
| Microsoft Entra ID | `https://login.microsoftonline.com/<tenant-id>/v2.0` |
| Okta org authorization server | `https://<your-org>.okta.com` |
| Okta custom authorization server | `https://<your-org>.okta.com/oauth2/<authorization-server-id>` |

Use a tenant-specific Entra issuer. For Google Workspace, require the `hd`
claim for the organization's domain as shown above. See the provider's
[Google](https://developers.google.com/identity/openid-connect/reference),
[Entra](https://learn.microsoft.com/en-us/entra/identity-platform/v2-protocols-oidc),
and [Okta](https://developer.okta.com/docs/concepts/auth-servers/) documentation.

Open `https://registry.example/-/oidc/company/login` in a browser. After sign-in,
pnpr displays a token to place in the registry's `_authToken` setting, for example
in your private user `.npmrc`:

```ini
//registry.example/:_authToken=THE_DISPLAYED_TOKEN
```

The browser session expires at the earlier of one hour or the ID token's
expiry. `npm logout` revokes it. Sessions are kept in memory and disappear on
restart; multi-replica deployments need affinity for both the login flow and
subsequent authenticated requests. They do not appear in `npm token list`.
This sign-in URL is separate from the npm CLI's `npm login` web protocol.
OIDC sessions do not currently exchange for OCI scoped bearer tokens.

## GitHub Actions keyless publishing

Configure a dedicated workload username, a named hosted npm registry, and
exact package names. The package's normal `publish` ACL must also grant this
username access.

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

Use immutable repository and owner IDs to keep trust from transferring when
names are reused. For reusable workflows, also require `job_workflow_ref` to pin the reusable
workflow that performs the publication.
A job using a GitHub environment has an environment-based subject, for example
`repo:example/widgets:environment:production`. Configure the exact subject and
additional claims you intend to trust. See
[GitHub's OIDC reference](https://docs.github.com/en/actions/reference/security/oidc).

The publishing job needs `id-token: write`. Request an ID token with pnpr's
configured audience and prefix it with `pnpr_workload_` as the registry token. No pnpr
password, persistent API key, or token exchange is needed:

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

The project's `.npmrc` contains only the environment reference:

```ini
//registry.example/~private/:_authToken=${NODE_AUTH_TOKEN}
```

On a server with multiple ecosystems, use `/npm/~private/` in both places.
The credential permits only ordinary `PUT` publications to the configured
packages through that named registry. It cannot read packages, unpublish,
change dist-tags separately, manage accounts, publish batches, or use other
pnpr services. Request the token after building so it remains valid throughout
the publish request. Its issuer's expiry applies to every request.

## Validation and operations

pnpr accepts RS256 and ES256 signatures and verifies issuer, audience,
authorized party when present, expiry, issue time, and not-before claims.
Browser login also verifies a browser-bound state, nonce, and S256 PKCE.
Unmapped or ambiguous identities are rejected. User subjects must be unique
within a provider, including its workload bindings.

The `pnpr_oidc_` and `pnpr_workload_` credential prefixes are reserved for OIDC.
Workload credentials bypass the persistent token backend.

Discovery and JWKS use HTTPS with redirects disabled and bounded response
sizes and deadlines. Literal and DNS-resolved destinations must be public IP
addresses. OIDC requests do not use environment-configured HTTP proxies.
Metadata is cached for five minutes. A verification
failure can trigger a refresh at most once every 30 seconds per provider.
An unavailable provider cannot turn an invalid credential into anonymous
access. Login state lives in a signed HttpOnly browser cookie for five minutes.
Anonymous login starts reserve no server-side entries. Successful login replay
records and browser sessions are each capped at 1,024 per process. A separate
1,024-entry callback-attempt history rejects recent failed and concurrent replays
before token exchange, evicting the oldest attempt when full. At most 16 browser
callbacks can perform network operations concurrently. pnpr omits OIDC callback
query strings from its request logs. Configure reverse proxies to omit those queries too.
