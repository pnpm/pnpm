---
"@pnpm/pnpr": patch
---

Running pnpr servers now refuse requests after another server upgrades their database beyond the version they support. Schema migration records now include their application time.

Database operation timeouts now return `503 Service Unavailable`.
