---
"pacquet": minor
"@pnpm/pnpr": minor
---

pnpm installs the platform variants of packages from a registry declared with `serverType: pnpr`. The lockfile records every variant of a package, and pnpm links the one that fits the host, or the package's own archive when none does.

pnpr accepts platform variants with a published version and serves each one at `<name>@<version>/variants/<tag>`. Only pnpr's own package document lists them, so npm clients install the version's own archive.
