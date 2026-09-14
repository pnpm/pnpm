---
"@pnpm/deps.compliance.sbom": patch
"@pnpm/deps.compliance.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm sbom` now validates and normalizes the `repository` field before publishing it. An npm shorthand such as `vercel/ms` or `github:vercel/ms` is expanded to the `git+https` URL npm derives for it. Any other URL is published in its normalized form, without embedded credentials. A value that is neither, such as an scp-style git remote, is left out of the CycloneDX `externalReferences[].url` and the SPDX `homepage`. pnpm used to publish the raw value, so a shorthand produced a URL that fails CycloneDX schema validation in consumers such as Dependency-Track [pnpm/pnpm#14773](https://github.com/pnpm/pnpm/issues/14773).
