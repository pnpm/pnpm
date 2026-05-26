---
"@pnpm/config.reader": patch
"pnpm": patch
---

Fix a credential disclosure issue where an unscoped `_authToken` (or `_auth`, or `username` + `_password`) defined in the user-level `~/.npmrc` or `~/.config/pnpm/auth.ini` would be sent as an `Authorization` header to a registry chosen by a workspace-local `.npmrc` (or `pnpm-workspace.yaml`). When the workspace overrides `registry=` to a different value than the user-level config would have set, pnpm now drops the binding between the user-level default credential and the workspace-selected registry and emits a warning instead. To send a credential to a specific registry across configs, scope it to the registry URL (e.g. `//registry.example.com/:_authToken=...`).

pnpm now also emits a deprecation warning whenever it reads any unscoped authentication credential (`_authToken`, `_auth`, `username`, `_password`, `tokenHelper`) from an `.npmrc` or `auth.ini` file. URL-scoped tokens have been the npm-recommended pattern since `npm@9`, and unscoped credentials will be removed in a future major release.
