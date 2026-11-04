---
"pacquet": minor
---

pnpm installs a version that a registry declared `serverType: pnpr` publishes with platform variants and no archive for every other host. It links the variant for the host. On a host that none of the variants fits, an optional dependency is skipped, and any other dependency fails with an error that lists the platforms the version is published for.
