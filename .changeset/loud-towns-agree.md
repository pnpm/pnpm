---
"@pnpm/deps.inspection.commands": patch
---

Improve `pnpm view <pkg> versions` output by showing the resolved registry URL in interactive terminals, which helps diagnose stale or mismatched mirror data.

To preserve script compatibility, non-interactive output remains machine-readable JSON for the `versions` field.