---
"@pnpm/pnpr": minor
---

pnpr now emits structured audit records under tracing target `pnpr::audit` at `info` level [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131). Audit events cover user management, rule overrides, team membership and package access, SCIM operations, and package deletion. When structured JSON logging is enabled, each audit record inherits the active request metadata including the request ID and client IP.
