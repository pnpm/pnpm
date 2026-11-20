---
"@pnpm/pnpr": patch
---

`pnpr config check` now prints the S3 key prefix as plain text, such as `prefix: pnpr/`, and leaves it out when none is set. It printed `prefix: Some("pnpr")` or `prefix: None`.
