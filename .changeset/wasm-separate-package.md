---
"pacquet": patch
---

The WebAssembly build for StackBlitz WebContainers now ships as a separate `@pnpm/wasm` package. The `pnpm` and `@pnpm/exe` packages no longer include it, which brings their unpacked size back from about 55 MB to about 4 MB. In a WebContainer, install `@pnpm/wasm` with npm to get the `pnpm` command.
