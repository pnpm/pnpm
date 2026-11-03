---
"pacquet": minor
"@pnpm/pnpr": minor
---

pnpm installs the platform variants of packages from a registry declared with `serverType: pnpr`. The lockfile records every variant of a package, and pnpm links the one that fits the host, or the package's own archive when none does. A lockfile that records platform variants has `lockfileVersion: '13.0'`. Older pnpm versions refuse to read it, so they cannot rewrite the lockfile without its variants. pnpr returns such a lockfile only to a pnpm that accepts that format.

pnpr accepts platform variants with a published version and serves each one at `<name>@<version>/variants/<tag>`. Only pnpr's own package document lists them, so npm clients install the version's own archive.
