---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

Installing with `pnprServer` set now records the pnpmfile checksum in the lockfile, so a later `pnpm install --frozen-lockfile` accepts that lockfile. A frozen install through the pnpr server now fails if the pnpmfile changed. If the pnpmfile defines a `readPackage`, `afterAllResolved` or `preResolution` hook or custom resolvers, pnpm resolves dependencies locally. pnpm then prints a warning that the pnpr server was not used [#14460](https://github.com/pnpm/pnpm/issues/14460).
