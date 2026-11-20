---
"@pnpm/deps.compliance.sbom": patch
"@pnpm/deps.compliance.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm sbom` now publishes a valid URL in the CycloneDX `externalReferences[].url` and the SPDX `homepage`. An npm shorthand such as `vercel/ms` or `gitlab:group/subgroup/project` is expanded to the `git+https` URL npm derives for it. An scp-style remote such as `git@github.com:vercel/ms.git` is expanded the same way. Any other URL is published in its normalized form, without embedded credentials. A value that names no repository, an email address for example, is left out. pnpm used to publish the raw value, so a shorthand produced a URL that consumers such as Dependency-Track reject [pnpm/pnpm#14773](https://github.com/pnpm/pnpm/issues/14773).
