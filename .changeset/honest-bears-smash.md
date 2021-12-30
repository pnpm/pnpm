---
"pnpm": minor
---

Add support for token helper, a command line tool to obtain a token.

A token helper is an executable, set in the user's `.npmrc` which
outputs an auth token. This can be used in situations where the
authToken is not a constant value, but is something that refreshes
regularly, where a script or other tool can use an existing refresh
token to obtain a new access token.

The configuration for the path to the helper must be an absolute path,
with no arguments. In order to be secure, it is only permitted to set
this value in the user `.npmrc`, otherwise a project could place a value
in a project local `.npmrc` and run arbitrary executables.

Usage example:

```ini
; Setting a token helper for the default registry
tokenHelper=/home/ivan/token-generator

; Setting a token helper for the specified registry
//registry.corp.com:tokenHelper=/home/ivan/token-generator
```

Related PRs:

- [pnpm/credentials-by-uri#2](https://github.com/pnpm/credentials-by-uri/pull/2)
- [#4163](https://github.com/pnpm/pnpm/pull/4163)
