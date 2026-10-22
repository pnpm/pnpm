---
"@pnpm/pnpr": minor
---

The web UI can now sign in with a password or with an OIDC provider that offers browser sign-in. pnpr lists those providers at `GET /-/pnpr/v0/sign-in`. A browser sign-in started with `/-/oidc/<provider>/login?return=ui` ends in the web UI.
