---
"@pnpm/resolving.resolver-base": minor
"@pnpm/resolving.npm-resolver": minor
"@pnpm/resolving.default-resolver": minor
"@pnpm/installing.client": minor
"@pnpm/store.connection-manager": minor
"@pnpm/testing.temp-store": minor
"@pnpm/installing.deps-installer": minor
"pnpm": patch
---

Restructured the `minimumReleaseAge` lockfile revalidation gate around a generic `ResolutionVerifier` interface. Each resolver may now export a sibling verifier factory (today: `createNpmResolutionVerifier`) that re-checks an already-resolved lockfile entry against its policies; `createResolver`'s companion `createResolutionVerifier` combines them and the `Client` exposes the combined `verifyResolution` for the install layer to consume. The npm verifier reuses the same on-disk metadata mirror the resolver writes to, so steady-state installs pay only a headers-only conditional GET per locked package [#11675](https://github.com/pnpm/pnpm/issues/11675).
