---
"@pnpm/pnpr.client": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/worker": patch
"pnpm": minor
---

The pnpr install accelerator is now used only to create the lockfile. Previously `POST /v1/install` returned the resolved lockfile **and** all missing file contents inline over a single connection, which was bandwidth-bound on cold/WAN installs (one TCP stream can't compete with a registry's parallel CDN fetches). The accelerator is now a two-phase flow: the pnpr server resolves and verifies the lockfile server-side (collapsing resolution's round-trip depth), then the client fetches every tarball directly from the registries in parallel, exactly like a normal install. This makes the accelerated path never slower than a plain install, and turns pnpr into a stateless resolver that stores no tarballs and serves no file content [#12230](https://github.com/pnpm/pnpm/issues/12230).
