---
"pacquet": patch
---

`pnpm sbom` now normalizes an npm shorthand `repository` field (e.g. `vercel/ms`) to a full GitHub URL, and omits the `vcs` external reference entirely when the field can't be turned into a valid URL, instead of writing an invalid reference into the generated SBOM [pnpm/pnpm#14773](https://github.com/pnpm/pnpm/issues/14773).
