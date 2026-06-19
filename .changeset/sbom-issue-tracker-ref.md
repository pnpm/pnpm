---
"@pnpm/deps.compliance.sbom": patch
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
---

`pnpm sbom` now emits a CycloneDX `issue-tracker` external reference for components (and the root) whose `package.json` declares a `bugs` URL. Email-only `bugs` entries are skipped, since the reference requires a URL.
