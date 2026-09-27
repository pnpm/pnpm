---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

Installing with `pnprServer` set now records the pnpmfile checksum in the lockfile, so a later `pnpm install --frozen-lockfile` accepts that lockfile. If the pnpmfile defines a `readPackage` or `afterAllResolved` hook or custom resolvers, pnpm resolves dependencies locally and warns that the pnpr server is not used [#14460](https://github.com/pnpm/pnpm/issues/14460).
