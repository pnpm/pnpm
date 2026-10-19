# pnpr

A pnpm-compatible registry server, written in Rust. It speaks the npm, Cargo,
Python, and OCI distribution protocols, hosts packages, proxies upstream
registries, and resolves dependency graphs for pnpm.

This crate ships the `pnpr` binary and the library behind it. It lives in the
[pnpm monorepo](https://github.com/pnpm/pnpm) under
[`pnpr/`](https://github.com/pnpm/pnpm/tree/main/pnpr).

- User documentation: [pnpm.io/pnpr](https://pnpm.io/pnpr), built from
  [`pnpr/docs/`](../../docs/).
- Contributor guide: [`pnpr/AGENTS.md`](../../AGENTS.md) and
  [`pnpr/CONTRIBUTING.md`](../../CONTRIBUTING.md).
- License: [PolyForm Shield 1.0.0](../../LICENSE.md).

## Extensibility

pnpr v1 uses compile-time trait implementations (`UserBackend`, `TokenBackend`
in `pnpr-auth`) rather than a dynamic plugin runtime. Custom backends compile
directly into the server binary, dispatching through trait objects without
external scripting runtimes.
