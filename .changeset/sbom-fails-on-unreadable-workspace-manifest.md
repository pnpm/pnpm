---
"pacquet": patch
---

`pnpm sbom` now fails with an error that names the file when the `package.json` of a linked workspace project cannot be parsed or read. It used to leave that project and its dependencies out of the SBOM without any message.
