---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

Make trusted publishing (OIDC) take precedence over a configured static `_authToken` in `pnpm publish`, mirroring the npm CLI's behavior. When OIDC succeeds, the OIDC-derived token overrides any pre-configured `NPM_TOKEN`/`_authToken`; when OIDC is not applicable (no CI environment, exchange fails, registry has no trusted publisher configured), the static token is used as a fallback. This applies on every package during recursive publish, so each workspace package independently attempts trusted publishing.
