---
"@pnpm/config.reader": minor
"pnpm": minor
---

Added an `_auth` setting for configuring registry authentication as a single structured (URL-keyed) value. It can be set in the **global** pnpm config (`config.yaml`) or, for CI, via the `pnpm_config__auth` environment variable. The env form sidesteps the GitHub Actions / bash / zsh limitation that broke the existing `pnpm_config_//host/:_authToken=…` form (env var names containing `/`, `:`, or `.` are silently dropped). Closes pnpm/pnpm#12314.

The value is keyed by registry URL so each secret is explicitly bound to the host that may receive it. Registry URL keys must use `http` or `https` and must not include credentials, query strings, or fragments:

```sh
export pnpm_config__auth='{"https://registry.npmjs.org":{"@":{"authToken":"npm-token"},"@org":{"authToken":"org-token"}}}'
```

The equivalent in the global `config.yaml`:

```yaml
_auth:
  https://registry.npmjs.org:
    '@':
      authToken: npm-token
    '@org':
      authToken: org-token
```

Within each registry URL, `@` means registry-wide/default credentials and package scopes like `@org` bind credentials to that scope on the same host. The only supported credential field is `authToken` (maps to `_authToken` / bearer auth); the deprecated `basicAuth` / `username` + `password` forms are intentionally not accepted here and are dropped with a warning.

Each entry also infers a trusted registry route: `@` routes the default registry (and `pnpm add <pkg>` resolves there), and `@org` routes that scope. Because the credential and destination host arrive in one trusted value, repo-controlled `pnpm-workspace.yaml` or project `.npmrc` cannot redirect the token to a different host. `_auth` is honored **only** from the env var and the global config — it is ignored in a project `pnpm-workspace.yaml` / `.npmrc`, so repo-controlled config can never supply registry auth. Precedence: CLI flags (`--registry`, `--@scope:registry`) > `pnpm_config__auth` > global `config.yaml` `_auth` > `pnpm-workspace.yaml`.

Both `pnpm_config__auth` (lowercase, documented form) and `PNPM_CONFIG__AUTH` (all-caps, the shell convention some CI runners apply) are honored. If both are set, lowercase wins unless it is empty, in which case uppercase is used. The env var wins over the global `config.yaml` `_auth` on a conflicting key. `tokenHelper` is not supported in `_auth`. Malformed values warn and the install continues.

**Pacquet parity note:** the pacquet (Rust) port supports the same single credential field as the TS CLI: `authToken`.
