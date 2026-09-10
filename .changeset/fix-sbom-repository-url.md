---
"@pnpm/deps.compliance.sbom": patch
"@pnpm/deps.compliance.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm sbom` now validates and normalizes the `repository` field before publishing it in the SBOM. The npm `owner/repo` shorthand is expanded to the `git+https` GitHub URL that npm derives for it. Absolute URLs are emitted in their normalized form with any embedded `user:password` removed. Values that are neither a valid URL nor the shorthand, such as git scp-style remotes, are no longer published as a CycloneDX `externalReferences[].url` or an SPDX `homepage`. The raw value used to be published as-is, so the shorthand produced URLs that fail CycloneDX schema validation in consumers such as Dependency-Track [pnpm/pnpm#14773](https://github.com/pnpm/pnpm/issues/14773).
