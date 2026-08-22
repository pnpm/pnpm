# @pnpm/napi

Node.js bindings for pnpm v12's Rust engine (pacquet), exposing pnpm's
programmatic API — install, rebuild, dependency resolution, and pack — to a
JavaScript host. The reference consumer is [Bit](https://bit.dev), which drives
pnpm entirely through its programmatic API.

This package binds pnpm's **engine** plus the two things a host cannot
reasonably reimplement over it: pnpm's terminal output (`options.reporter`)
and its reverse dependency tree (`getDependents` / `renderDependents`, what
`pnpm why` is built on). Pure data utilities that operate on in-memory
objects or the (byte-stable) on-disk lockfile/store formats stay as regular
`@pnpm/*` JS packages — both stacks share the same lockfile v9 shape,
`.modules.yaml` format, and store layout.

## API

See [`index.d.ts`](./index.d.ts) for the full typed contract.

| Export | Purpose |
| --- | --- |
| `install(options, onLog?, readPackageHook?, onOutput?)` | Install in-memory importers (single or workspace); `readPackageHook` transforms each resolved dependency manifest (must be synchronous). Returns `{ stats, depsRequiringBuild?, storeDir }`. |
| `rebuild(options, onLog?, selectedNames?, onOutput?)` | Re-run dependency build scripts against a materialized install (frozen path). |
| `resolveDependency(wanted, options)` | Resolve an npm-registry specifier to `{ id, manifest, resolvedVia, … }`. |
| `pack(options, onLog?)` | Build a publishable `.tgz` from a project directory. |
| `parseBareSpecifier(spec, alias?)` | Split/validate a dependency specifier; `null` when unparsable. |
| `getDependents(options)` | Every package matching `packages`, each with the reverse tree of what depends on it — the engine side of `pnpm why`. |
| `renderDependents(trees, options?)` | Render those trees as `pnpm why` does: `tree`, `parseable`, or `json`. |
| `engineVersion()` | Version string of the underlying Rust engine (pacquet). |
| `getPeerDependencyIssues(options)` | **Not yet implemented** — throws `ERR_PNPM_NAPI_UNIMPLEMENTED`. Peer-issue reporting is not ported in pacquet's CLI either; consumers should degrade gracefully. |

### Output

Set `options.reporter` and the engine renders pnpm's own terminal output —
progress line, packages-diff summary, lifecycle output, the `Done in …`
footer — with the reporter `pnpm install` itself uses. Without it the call
prints nothing and `onLog` hands the host the raw event stream to render
however it likes.

By default the rendered chunks go to stdout. Pass `onOutput` to receive them
instead, for a host that has redirected its own output at the JavaScript
level (a monkey-patched `process.stdout.write`, a stream forwarding to a
remote terminal) where a write from Rust would bypass the redirection. Pass
`reporter.width` alongside it: the engine cannot see where those chunks end
up.

### Dependents (`pnpm why`)

`getDependents` returns the reverse trees as plain data and
`renderDependents` prints them, mirroring the split between
`@pnpm/deps.inspection.tree-builder` and `@pnpm/deps.inspection.list`. The
split is also what replaces that API's `nameFormatter` callback: the tree
walk is synchronous Rust and cannot call back into JavaScript, so a host
that renames nodes after a manifest field asks for the field via
`manifestFields`, writes `displayName` onto the returned trees, and passes
them back to be rendered.

Errors are plain `Error` objects carrying pnpm's `code` (`ERR_PNPM_*`) and,
where applicable, `hint` — lifted onto the error by the loader from the engine's
structured envelope.

Auth: pass `authHeaderByUri` — a map of nerf-darted registry URI → `Authorization`
header value (with `""` for the default registry). The host resolves these from
its `.npmrc` credentials; the engine applies them as-is. The `""` entry is
pinned to the `registry` / `registries.default` passed alongside it (npmjs when
neither is given), so a `registry=` in the project's own `.npmrc` cannot
redirect that credential to another host.

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
cargo build -p pnpm-napi --profile napi-release
cp ../../../target/napi-release/libpnpm_napi.dylib \
   ./pnpm-napi.darwin-arm64.node   # .so on Linux, .dll on Windows
node -e "console.log(require('.').engineVersion())"
```

Or set `PNPM_NAPI_BINARY=/path/to/addon.node`.
