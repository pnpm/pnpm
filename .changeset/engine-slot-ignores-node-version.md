---
"pacquet": patch
---

pnpm no longer downloads the project's pinned pnpm version again on every command when `nodeVersion` in `pnpm-workspace.yaml` names a different Node.js major than the `node` on `PATH`. Before, each of those commands took about a second longer and failed without network access [#16497](https://github.com/pnpm/pnpm/issues/16497).
