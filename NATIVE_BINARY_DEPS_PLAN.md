# Native binary dependencies (variants synthesis)

Working plan for the `native-binary-deps` branch. Delete before opening the real PR.

## Goal

For a configurable list of packages that ship a JS launcher shim plus per-platform
native binaries as `optionalDependencies` (`pacquet`, `@pnpm/pacquet`, `@pnpm/exe`),
install **only the one matching platform's native binary** and link its bin directly —
no Node launcher shim, no lifecycle scripts.

## Approach: extend the npm resolver to emit one more resolution type

pnpm and pacquet already have the full `variations`/`binary` resolution pipeline,
used today only by the `runtime:` engine resolvers (node/bun/deno). The runtime
resolvers are *separate* resolvers only because they fetch from non-npm sources.
Native-deps packages are plain registry packages, so the synthesis belongs **inside
the npm resolver** — it already fetches the wrapper packument and picks the version;
it just returns a `VariationsResolution` instead of a `TarballResolution` for matching
packages. No new resolver package, no resolver-chain change. Everything downstream
(lockfile, `selectPlatformVariant`, `BinaryFetcher`, bin-linking, no-scripts) is reused
unchanged.

### Synthesis shape (settled)

For wrapper package `P@V` whose `optionalDependencies` are the platform packages:

```
resolve P@V via npm  ->  version V, manifest.bin command names, optionalDependencies
for each platform dep D (e.g. @pacquet/darwin-arm64):
  fetch D's packument @ matching version
  read: os, cpu, libc (from os/cpu/libc or publishConfig), dist.tarball, dist.integrity
  variant = {
    resolution: { type: 'binary', archive: 'tarball',
                  url: D.dist.tarball, integrity: D.dist.integrity,
                  bin: { <cmd>: '<binaryName>[.exe]' } },   // binary at package root after `package/` strip
    targets: [{ os, cpu, libc? }],
  }
return { id: P@V, manifest: { name: P, version: V, bin }, resolution: { type: 'variations', variants } }
```

Why `archive: 'tarball'` works for npm packages: the binary-fetcher's tarball branch
delegates to the normal remote-tarball fetcher (`fetching/binary-fetcher/src/index.ts:62-69`),
which strips the `package/` prefix and applies `appendManifest`, so our `bin` mapping
wins over the platform package's own (absent) bin.

Binary-path-at-root heuristic (matches Bun's `resolve_bin_target` and the real layouts):
the binary inside each platform package is the **command name** at the package root
(`pacquet` -> `pacquet`, `pnpm` -> `pnpm`), with `.exe` on Windows.

## Default list + setting

- Setting: `nativeDependencies` (array of package names) in `pnpm-workspace.yaml`
  (mirrors Bun's `nativeDependencies` field name).
- Hardcoded default: `['pacquet', '@pnpm/pacquet', '@pnpm/exe']`, user list extends it.
- A package only takes the native path when it actually resolves a platform-matched
  optional dep (otherwise fall through to normal npm resolution) — same guard Bun uses.

## pnpm (TypeScript) change map

1. Branch inside `resolving/npm-resolver/src/index.ts` at the resolution-construction
   site (~`:588-607`, where `pickedPackage` + the `TarballResolution` are built):
   when `pickedPackage.name ∈ nativeDependencies` and `pickedPackage.optionalDependencies`
   has platform packages, fetch each platform packument via the existing `ctx.pickPackage`
   machinery, build the `variants`, and return `resolution: { type: 'variations', variants }`
   plus a `manifest.bin` mapping. Otherwise return the normal tarball resolution.
   - Widen `NpmResolveResult.resolution` to `TarballResolution | VariationsResolution`.
   - Thread `nativeDependencies` into `createNpmResolver` ctx (it's created in
     `default-resolver` from `pnpmOpts`).
   - Guard: only take the native path when ≥1 optional dep matches a real platform
     target; else fall through to the tarball (same guard Bun uses).
2. Config setting:
   - `config/reader/src/types.ts` (pnpmTypes) — add `native-dependencies: Array`.
   - `config/reader/src/Config.ts` — `nativeDependencies?: string[]`.
   - `core/types/src/package.ts` (PnpmSettings) — `nativeDependencies?: string[]`.
   - `config/reader/src/getOptionsFromRootManifest.ts` — pass through.
4. Tests:
   - `resolving/native-binary-resolver/test/` — unit test the synthesis with a mocked
     registry (wrapper + 2 platform packuments) -> assert variants/targets/bin.
   - `pnpm/test/install/` e2e — install `pacquet`, assert only host binary fetched,
     `.bin/pacquet` is the native binary, no postinstall ran.
5. Changeset (`pnpm` minor + the new package).

## pacquet (Rust) change map — port after pnpm lands

Pipeline already complete (`lockfile/src/resolution.rs`, `select_platform_variant`,
`fetch_binary_resolution_to_cas`, bin-linking). Needs:

1. New resolver producing `LockfileResolution::Variations` for listed packages,
   added to the resolver dispatch chain (template: `engine-runtime-node-resolver`).
2. `Config::native_dependencies: Vec<String>` + default list, read from
   `pnpm-workspace.yaml` (`crates/config`).
3. Integration tests under `crates/cli/tests` using the in-process registry.

## Open decisions for the user

- Names: setting `nativeDependencies`, package `@pnpm/resolving.native-binary-resolver`.
- Should `@pnpm/exe` be in the default list given it currently relies on its own
  `preinstall`/`prepare` scripts (`pnpm/artifacts/exe`)? Switching it to the native
  path changes how pnpm-installs-pnpm behaves — worth a closer look / its own stage.

## Relation to the postinstall PR (pnpm/pnpm#12507)

That PR speeds up `pacquet` for **npm/yarn** users (and pnpm/Bun after trust). This
feature supersedes it for **pnpm/pacquet** installs of the listed packages (no script
needed at all). They are complementary; keep both.
