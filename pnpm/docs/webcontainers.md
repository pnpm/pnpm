---
id: webcontainers
title: StackBlitz WebContainers
---

The experimental `@pnpm/wasm` package runs pnpm inside a StackBlitz
WebContainer. It requires Node.js 22.13 or newer and shared WebAssembly memory.
The regular `pnpm` package runs a native executable, which WebContainers cannot
execute.

## Install pnpm

Install `@pnpm/wasm` with npm. It provides the `pnpm`, `pn`, `pnpx`, and `pnx`
commands:

```sh
npm install --global @pnpm/wasm
pnpm --version
pnpm install
```

Global installation needs a writable npm prefix. You can also add `@pnpm/wasm`
to a project and run it with `npx pnpm`.

Corepack and the `packageManager` field install the regular `pnpm` package,
so they cannot select this distribution. Projects that pin a different
package-manager version receive `ERR_PNPM_UNSUPPORTED_RUNTIME` when switching
would require installing a native distribution.
To keep using the installed WebAssembly distribution for such a project, disable
automatic version switching for the command:

```sh
npm_config_manage_package_manager_versions=false pnpm install
```

## Build the distribution

Check out the pnpm repository and follow the toolchain setup in
[the WASM runtime guide](https://github.com/pnpm/pnpm/tree/main/pnpm/wasm).
From the repository root, run:

```sh
pnpm build:pnpm:wasm
pnpm pack:pnpm:wasm
```

The result is `target/pnpm-wasm.tgz`, the `@pnpm/wasm` package. The `pnpm
WebAssembly` CI workflow also uploads this tarball after its browser tests pass.
Copy it into the WebContainer and install it with
`npm install --global ./pnpm-wasm.tgz`.

## Runtime differences

Filesystem access, HTTP requests, and child processes use the WebContainer's
Node.js APIs. Lifecycle scripts run inside the same container and keep pnpm's
build-approval rules. Packages that require native executables or native Node.js
addons remain subject to WebContainer limitations.

WebContainer's bundled npm applies its own package compatibility replacements.
This runtime does not use those private integrations. Select
WebAssembly alternatives explicitly when a dependency normally downloads native
code. For example, a Vite project can use these entries:

```yaml title="pnpm-workspace.yaml"
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

To update pnpm in a WebContainer, install a newer version of `@pnpm/wasm` with
npm.
