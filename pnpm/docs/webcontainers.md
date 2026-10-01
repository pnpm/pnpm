---
id: webcontainers
title: StackBlitz WebContainers
---

The experimental WebAssembly distribution runs pnpm's Rust CLI inside a
StackBlitz WebContainer. The npm package selects WebAssembly automatically in
WebContainers. It requires Node.js 22.13 or newer and shared WebAssembly memory.
Native pnpm installations continue to use the native executable.

## Install pnpm

Install a release that includes WebContainer support through npm:

```sh
npm install pnpm@12
npx pnpm --version
npx pnpm install
```

The package includes the WebAssembly runtime, so no separate WASM package or
runtime setting is needed. Its launchers also work when npm skips installation
scripts. Global installation works with a writable npm prefix.

The Corepack entry point selects the same runtime. A cold download with Corepack
0.36.0 failed in the tested WebContainer runtime's streaming hash implementation
before pnpm started. Use npm if Corepack reports a hash-related host error.
The npm package is larger because it includes that payload; native installations
still launch the native executable directly.

## Build the distribution

Check out the pnpm repository and follow the toolchain setup in
[the WASM runtime guide](https://github.com/pnpm/pnpm/tree/main/pnpm/wasm).
From the repository root, run:

```sh
pnpm build:pnpm:wasm
pnpm pack:pnpm:wasm
```

The result is `target/pnpm-wasm.tgz`. The `pnpm WebAssembly` CI workflow also
uploads this tarball after its browser tests pass.

Copy the tarball into the WebContainer, then install it with npm:

```sh
npm install --global ./pnpm-wasm.tgz
pnpm --version
pnpm install
```

The standalone tarball is useful for testing a build before release. Native
executable archives cannot run inside a WebContainer. Projects that pin a different
package-manager version receive `ERR_PNPM_UNSUPPORTED_RUNTIME` when switching
would require installing a native distribution.
To keep using the installed WebAssembly distribution for such a project, disable
automatic version switching for the command:

```sh
npm_config_manage_package_manager_versions=false pnpm install
```

## Runtime differences

Filesystem access, HTTP requests, and child processes use the WebContainer's
Node.js APIs. Lifecycle scripts run inside the same container and keep pnpm's
build-approval rules. Packages that require native executables or native Node.js
addons remain subject to WebContainer limitations.

WebContainer's bundled npm applies its own package compatibility replacements.
This runtime does not use those private integrations. Select
WebAssembly alternatives explicitly when a dependency normally downloads native
code. For example, a Vite project can use these entries in `pnpm-workspace.yaml`:

```yaml
overrides:
  esbuild: npm:esbuild-wasm@^0.27.0
  rollup: npm:@rollup/wasm-node@^4.43.1
```

Choose replacement versions compatible with your project's dependency versions,
then run `pnpm install` and commit the updated lockfile. These overrides apply
wherever that workspace configuration is used, including outside WebContainers.

When pnpm redirects a subprocess's stdout or stderr to a file, it waits for the
redirected output to finish writing. A background descendant that inherits that
stream can keep pnpm waiting after the original subprocess exits. Close or replace
inherited redirected streams in background children.

The browser manages HTTP connections. Custom certificate authorities, client
certificates, explicit proxy settings, local network addresses, and DNS pinning
are unavailable. Unsupported network settings produce an error.

The package store uses a separate `v11-wasm` directory with the same package
file and index formats. It cannot share a live store with native pnpm because
their file locks differ. The WASM store uses SQLite rollback journals and a
process lease. Concurrent
commands sharing a store wait for its owner. A nested pnpm command cannot acquire
the same store while its parent holds it; use a separate store for that command.

Automatic restoration of changed installation metadata after an error is
unavailable because WebContainers cannot securely pin the parent directory.
pnpm reports this limitation instead of restoring through a replaced directory.

Automatic provenance signing, Cargo workspace integration, `pnpm setup`,
`pnpm self-update`, and `pnpm pack-app` are unavailable in this distribution.
These operations fail explicitly before performing unsupported work.

To update pnpm in a WebContainer, install the newer pnpm release with npm or
select it through Corepack. For a standalone test build, install the newer
`pnpm-wasm.tgz` package with npm.
