---
"@pnpm/deps.compliance.sbom": patch
"@pnpm/deps.compliance.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm sbom` now validates and normalizes the `repository` field before publishing it. A shorthand such as `vercel/ms`, `github:vercel/ms` or `gitlab:group/subgroup/project`, and an scp-style remote such as `git@github.com:vercel/ms.git`, are expanded to the `git+https` URL npm derives for them. Any other URL is published in its normalized form, without embedded credentials. A value that names no repository, such as an email address, is left out of the CycloneDX `externalReferences[].url` and the SPDX `homepage`. pnpm used to publish the raw value, so a shorthand produced a URL that fails CycloneDX schema validation in consumers such as Dependency-Track [pnpm/pnpm#14773](https://github.com/pnpm/pnpm/issues/14773).
