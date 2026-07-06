# @pnpm/napi

Node.js bindings for pnpm v12's Rust engine (pacquet), exposing pnpm's
programmatic API — install, rebuild, dependency resolution, and pack — to a
JavaScript host. The reference consumer is [Bit](https://bit.dev), which drives
pnpm entirely through its programmatic API.

This package binds only pnpm's **engine**. Pure data utilities that operate on
in-memory objects or the (byte-stable) on-disk lockfile/store formats stay as
regular `@pnpm/*` JS packages — both stacks share the same lockfile v9 shape,
`.modules.yaml` format, and store layout.

## API

See [`index.d.ts`](./index.d.ts) for the full typed contract.

| Export | Purpose |
| --- | --- |
| `install(options, onLog?, readPackageHook?)` | Install in-memory importers (single or workspace); `readPackageHook` transforms each resolved dependency manifest (must be synchronous). Returns `{ stats, depsRequiringBuild?, storeDir }`. |
| `rebuild(options, onLog?, selectedNames?)` | Re-run dependency build scripts against a materialized install (frozen path). |
| `resolveDependency(wanted, options)` | Resolve an npm-registry specifier to `{ id, manifest, resolvedVia, … }`. |
| `pack(options, onLog?)` | Build a publishable `.tgz` from a project directory. |
| `parseBareSpecifier(spec, alias?)` | Split/validate a dependency specifier; `null` when unparsable. |
| `engineVersion()` | Version string of the underlying Rust engine (pacquet). |
| `getPeerDependencyIssues(options)` | **Not yet implemented** — throws `ERR_PNPM_NAPI_UNIMPLEMENTED`. Peer-issue reporting is not ported in pacquet's CLI either; consumers should degrade gracefully. |

Errors are plain `Error` objects carrying pnpm's `code` (`ERR_PNPM_*`) and,
where applicable, `hint` — lifted onto the error by the loader from the engine's
structured envelope.

Auth: pass `authHeaderByUri` — a map of nerf-darted registry URI → `Authorization`
header value (with `""` for the default registry). The host resolves these from
its `.npmrc` credentials; the engine applies them as-is.

## Distribution

The addon ships as prebuilt per-platform packages, the same model as the
`@pnpm/exe.*` CLI packages:

- `index.js` resolves the addon at load time in this order: a
  `PNPM_NAPI_BINARY` env override, the matching
  `@pnpm/napi.<platform>` optional dependency, then a local build.
- CI cross-compiles the addon per target (`napi build --release --target
  <rust-triple>`), uploads each as `pnpm-napi.<codeTarget>.node` at the repo
  root, then runs `scripts/generate-packages.mjs` to produce the eight
  `@pnpm/napi.<platform>` packages and wire them as this wrapper's
  `optionalDependencies`.

Supported targets: `win32-x64`, `win32-arm64`, `darwin-x64`, `darwin-arm64`,
`linux-x64`, `linux-arm64`, `linux-x64-musl`, `linux-arm64-musl`.

## Local development

Build the Rust crate and point the loader at the artifact:

```sh
cargo build -p pacquet-napi --profile napi-release
cp ../../../target/napi-release/libpacquet_napi.dylib \
   ./pnpm-napi.darwin-arm64.node   # .so on Linux, .dll on Windows
node -e "console.log(require('.').engineVersion())"
```

Or set `PNPM_NAPI_BINARY=/path/to/addon.node`.
