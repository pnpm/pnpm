---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

Make trusted publishing (OIDC) take precedence over a configured static `_authToken` in `pnpm publish`, mirroring the npm CLI's behavior. When OIDC succeeds, the OIDC-derived token overrides any pre-configured `_authToken`; when OIDC is not applicable (no CI environment, exchange fails, registry has no trusted publisher configured), the static token is used as a fallback. This applies on every package during recursive publish, so each workspace package independently attempts trusted publishing.

Additionally, the `NPM_ID_TOKEN` env var is now honored as a CI-agnostic injection point for an OIDC ID token. Previously OIDC was only attempted on GitHub Actions or GitLab; now any CI provider that exposes its own OIDC mechanism (e.g. CircleCI's `CIRCLE_OIDC_TOKEN_V2`, Buildkite, etc.) can forward its token via `NPM_ID_TOKEN` and trusted publishing will work without pnpm needing to recognize the provider explicitly.
