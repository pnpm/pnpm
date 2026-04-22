---
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

Globally-installed bins no longer fail with `ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND` when pnpm was installed via the standalone `@pnpm/exe` binary (e.g. `curl -fsSL https://get.pnpm.io/install.sh | sh -`) on a system without a separate Node.js installation. Previously, when `which('node')` failed during `pnpm add --global`, pnpm fell back to `process.execPath`, which in `@pnpm/exe` is the pnpm binary itself — and that path was baked into the generated bin shim, causing the shim to invoke pnpm instead of Node [#11291](https://github.com/pnpm/pnpm/issues/11291), [#4645](https://github.com/pnpm/pnpm/issues/4645).
