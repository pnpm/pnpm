---
"pacquet": patch
---

Read-only commands, `dedupe --check`, and rejected commands no longer create a missing `package.json`. Fetching or generating an SBOM from a lockfile leaves the manifest absent, and the SBOM does not include initialization-template metadata [pnpm/pnpm#16907](https://github.com/pnpm/pnpm/issues/16907).
