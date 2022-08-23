---
"@pnpm/audit": patch
"@pnpm/plugin-commands-audit": patch
---

- Add new Error type: AuditEndpointNotExistsError
- On AuditUrl returns 404, AuditEndpointNotExistsError will throw
- When audit handler catches AuditEndpointNotExistsError, the command will return to avoid execute further codes
