---
id: pack-app
title: pnpm pack-app
---

Added in: v11.0.0

:::warning Experimental

`pnpm pack-app` is experimental. Its flags, `pnpm.app` configuration schema, and output layout may change in future releases.

:::

Pack a CommonJS entry file into a standalone executable for one or more target platforms, using the [Node.js Single Executable Applications](https://nodejs.org/api/single-executable-applications.html) API under the hood.

```sh
pnpm pack-app --entry <path> --target <triplet> [--target <triplet> ...]
```

Each target produces an executable under `<output-dir>/<target>/` (default `dist-app/<target>/`). On Windows targets the output is suffixed with `.exe`; macOS outputs are ad-hoc signed automatically (via `codesign` on macOS hosts or `ldid` on Linux hosts) because SEA injection invalidates the existing code signature.

## Requirements

* The host must run Node.js v25.5+ to perform the SEA injection. If the running Node.js is older (or does not match the embedded runtime version — SEA blobs are not compatible across minor releases), pnpm downloads a matching builder automatically.
* Cross-compiling macOS targets from Linux requires [`ldid`](https://github.com/ProcursusTeam/ldid) on `$PATH`. Windows hosts cannot ad-hoc sign macOS outputs; build macOS targets on macOS or Linux.

## Supported targets

Targets use the format `<os>-<arch>[-<libc>]`:

* `linux-x64`, `linux-x64-musl`, `linux-arm64`, `linux-arm64-musl`
* `darwin-x64`, `darwin-arm64`
* `win32-x64`, `win32-arm64`

The `-musl` suffix is only valid for `linux` targets. The `<os>` segment matches `process.platform` values so the flag is consistent with pnpm's `--os` flag and with `supportedArchitectures.os` in `pnpm-workspace.yaml`.

## Known limitations

### `darwin-x64` binaries crash on Intel Macs

`darwin-x64` outputs segfault at startup on Intel Macs because of an upstream Node.js bug in the `--build-sea` injection step. LIEF's Mach-O surgery for x64 leaves `LC_DYLD_CHAINED_FIXUPS` chain entries pointing at stale targets after the SEA segment is inserted; dyld then dereferences a raw chain-encoded value as a pointer and the binary crashes in `__cxx_global_var_init` before any user code runs. This is reproducible with the canonical `node --build-sea` + `codesign --sign -` flow with no pnpm involvement.

The Node.js team has opted not to fix this on the grounds that x64 macOS is being phased out. Signature-related workarounds do not help — the corruption happens in the injection step, *before* signing, so swapping `ldid` for `codesign` (or vice versa) makes no difference. Re-signing produces a valid signature over already-broken bytes.

Tracking:

* [nodejs/node#62893](https://github.com/nodejs/node/issues/62893) — minimal `node --build-sea` repro
* [nodejs/node#59553](https://github.com/nodejs/node/issues/59553) — long-running SEA test failures on macOS x64 with the same root cause
* [nodejs/node#60250](https://github.com/nodejs/node/pull/60250) — Node.js skipping the SEA tests on x64 macOS rather than fixing them

If you need to ship a CLI that runs on Intel Macs, build the `darwin-x64` artifact with a non-SEA tool such as [`@yao-pkg/pkg`](https://github.com/yao-pkg/pkg) (which appends to the binary tail rather than mutating Mach-O sections). Note that Rosetta is *not* an escape hatch — it only translates x64 → arm64 (for Apple Silicon Macs running Intel binaries), not the other direction, so Intel Macs cannot run a `darwin-arm64` build.

## Examples

Build for Linux and Windows at once:

```sh
pnpm pack-app --entry dist/index.cjs --target linux-x64 --target win32-x64
```

Embed a specific Node.js version:

```sh
pnpm pack-app --entry dist/index.cjs --target linux-x64-musl --runtime node@25.5.0
```

## Options

### --entry &lt;path\>

Path to the CJS entry file to embed in the executable. Required unless [`pnpm.app.entry`](#configuration) is set in `package.json`. A bare positional argument is also accepted (e.g. `pnpm pack-app dist/index.cjs`).

### --target, -t &lt;triplet\>

Target to build for. May be specified multiple times. See [Supported targets](#supported-targets) for the accepted values. Required unless [`pnpm.app.targets`](#configuration) is set. When passed on the CLI, `--target` entirely replaces the configured list so you can narrow the build at invocation time.

### --runtime &lt;spec\>

Runtime to embed in the output executables, as a `<name>@<version>` spec (e.g. `node@25`, `node@25.5.0`). Only `node` is supported today; the `<name>@` prefix leaves room for future runtimes (`bun`, `deno`). The version must be >= v25.5 (the minimum that supports `--build-sea`). Defaults to the running Node.js version.

### --output-dir, -o &lt;dir\>

Output directory for the built executables. Defaults to `dist-app`.

### --output-name &lt;name\>

Name for the output executable (without extension). Defaults to the unscoped `name` from `package.json` (e.g. `my-cli` for `@acme/my-cli`).

## Configuration

Defaults for every flag can be set in `package.json` under `pnpm.app`. CLI flags override the config:

```json title="package.json"
{
  "name": "my-cli",
  "pnpm": {
    "app": {
      "entry": "dist/index.cjs",
      "targets": [
        "linux-x64",
        "linux-arm64",
        "darwin-x64",
        "darwin-arm64",
        "win32-x64"
      ],
      "runtime": "node@25.5.0",
      "outputDir": "dist-app",
      "outputName": "my-cli"
    }
  }
}
```

With this config in place, `pnpm pack-app` can be run with no arguments. `--target` on the CLI replaces the configured `targets` list, which is useful for narrowing a build (e.g. `pnpm pack-app --target linux-x64`).
