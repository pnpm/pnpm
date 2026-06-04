---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
---

Fixed `pnpm sbom --filter` being silently ignored in workspaces. The sbom command was missing `recursiveByDefault`, so the `--filter` flag never populated the workspace project graph. The handler always used the workspace root's metadata for the SBOM root component regardless of the filter.
