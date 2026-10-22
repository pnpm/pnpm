---
id: dist-tag
title: pnpm dist-tag
---

Manage distribution tags for a package. Dist-tags provide human-readable aliases (like `latest`, `next`, `beta`) that point to specific package versions on the registry.

Aliases: `dist-tags`

```sh
pnpm dist-tag add <pkg>@<version> [<tag>]
pnpm dist-tag rm <pkg> <tag>
pnpm dist-tag ls [<pkg>]
```

## Subcommands

### add &lt;pkg\>@&lt;version\> [&lt;tag\>]

Tag the specified version of a package with a dist-tag. If `<tag>` is omitted, `latest` is used. The version must be an exact semver version. A range fails with `ERR_PNPM_DIST_TAG_ADD_INVALID_VERSION`.

```sh
pnpm dist-tag add foo@1.2.0 next
```

### rm &lt;pkg\> &lt;tag\>

Remove a dist-tag from a package.

```sh
pnpm dist-tag rm foo next
```

### ls [&lt;pkg\>]

List all dist-tags for a package. `list` is accepted as an alias of `ls`. If no package name is given, the dist-tags for the current package (read from the local `package.json`) are shown.

```sh
pnpm dist-tag ls foo
```

## Trusted publishing

`pnpm dist-tag add`, `rm`, and `ls` can manage tags without a stored npm token when the registry trusts the CI identity. Enable **Allow npm dist-tag** in the package's trusted publishing settings.

- In GitHub Actions, grant the job `id-token: write`. pnpm reads `GITHUB_ACTIONS`, `ACTIONS_ID_TOKEN_REQUEST_TOKEN`, and `ACTIONS_ID_TOKEN_REQUEST_URL` to request an identity token.
- A supported trusted CI provider can supply its identity token through `NPM_ID_TOKEN`. Its `aud` claim must match the selected registry, such as `npm:registry.npmjs.org`. This takes priority over GitHub's request variables. Setting `GITLAB_CI` alone does not supply a token.

pnpm exchanges the CI identity token for a temporary token scoped to the package and its configured registry. The token stays in memory and is not saved to `.npmrc`. Dist-tag commands do not generate provenance.

When OIDC is unavailable or the registry rejects the exchange, pnpm falls back to configured credentials. Missing GitHub request variables and rejected identity requests also use this fallback. Network errors and invalid identity-request URLs are reported.

OIDC requests use the configured fetch timeout and retry settings.

## Options

### --registry &lt;url\>

The registry to operate on. Defaults to the registry configured for the package.

### --otp &lt;code\>

When the registry requires two-factor authentication, supply the one-time password via this flag. When the registry uses web-based authentication, pnpm prints a URL to open instead. Rerun the command with `--otp` once the login finishes.
