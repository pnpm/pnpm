# @pnpm/wasm

pnpm compiled to WebAssembly for [StackBlitz WebContainers](https://webcontainers.io), where the native `pnpm` package cannot run. It requires Node.js 22.13 or newer.

```sh
npm install --global @pnpm/wasm
pnpm install
```

The package provides the `pnpm`, `pn`, `pnpx`, and `pnx` commands. See the [WebContainers guide](https://github.com/pnpm/pnpm/blob/main/pnpm/docs/webcontainers.md) for the differences from native pnpm.
