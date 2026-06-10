---
"@pnpm/config": patch
"pnpm": patch
---

Environment variable expansion is now trust-aware for registry/auth config and request destinations. Repository-controlled config files (the project and workspace `.npmrc` and `pnpm-workspace.yaml`) can no longer expand `${...}` placeholders in registry/proxy request destinations, URL-scoped keys, or registry credential values, preventing repository-controlled configuration from exfiltrating environment secrets through request URLs. Trusted user/global/CLI/env config keeps full env expansion, so existing token and registry setup flows continue to work.
