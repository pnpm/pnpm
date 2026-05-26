---
"@pnpm/config.reader": patch
"pnpm": patch
---

Fix a credential disclosure issue where an unscoped `_authToken` (or `_auth`, or `username` + `_password`) defined in the user-level `~/.npmrc` or `~/.config/pnpm/auth.ini` would be sent as an `Authorization` header to a registry chosen by a workspace-local `.npmrc`. When a project `.npmrc` overrides `registry=` to a different value, pnpm now drops the binding between the user-level default credential and the workspace-selected registry and emits a warning instead. To send a credential to a specific registry across configs, scope it to the registry URL (e.g. `//registry.example.com/:_authToken=...`). Reported by JUNYI LIU.
