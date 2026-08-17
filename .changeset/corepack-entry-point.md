---
"pacquet": patch
---

Corepack can run pnpm 12 again [#13018](https://github.com/pnpm/pnpm/issues/13018). Corepack installs no dependencies and runs no lifecycle scripts, so the native binary that the `pnpm` package normally receives from its platform-specific optional dependency was never there, and `corepack use pnpm@next-12` failed with `MODULE_NOT_FOUND`. The package now ships the `bin/pnpm.mjs` and `bin/pnpx.mjs` entry points Corepack looks for; they fetch the pinned native binary on first use — verified against npm's signature and checksum, honouring `COREPACK_NPM_REGISTRY` and the rest of Corepack's registry environment — and hand over to it. Installing pnpm with a package manager is unaffected and still runs the binary directly, with no Node.js startup in between.
