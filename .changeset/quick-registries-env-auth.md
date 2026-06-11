---
"@pnpm/config.reader": minor
"pnpm": minor
---

Added support for configuring URL-scoped registry settings through `npm_config_//…` and `pnpm_config_//…` environment variables, for example:

```
npm_config_//registry.npmjs.org/:_authToken=<token>
pnpm_config_//registry.npmjs.org/:_authToken=<token>
```

This provides a file-free way to supply registry authentication. Because the registry a value applies to is encoded in the (trusted) environment variable name, it is host-scoped by construction and cannot be redirected to another registry by repository-controlled config. The environment value is treated as trusted config: it takes precedence over a project/workspace `.npmrc` but is still overridden by command-line options. When the same key is provided through both prefixes, `pnpm_config_` wins.
