---
"pacquet": minor
---

`pnpm install` and `pnpm add crate:<package>` now manage crates.io dependencies when `cargo.enabled` is set in `pnpm-workspace.yaml`. Mixed-ecosystem workspaces install their dependency graphs in one command. A single `pnpm add` command can include both npm packages and crates. Cargo workspace discovery excludes configured stores and caches.

Cargo lockfiles and source configuration are published after all enabled ecosystems finish successfully. Failed publication restores the previous Cargo metadata. Failed `pnpm add` operations that include crates restore participating manifests and lockfiles.

Cargo index and crate downloads use URL-matched credentials from pnpm configuration. The crates.io index also supports `CARGO_REGISTRY_TOKEN` and the token in `$CARGO_HOME/credentials.toml`, using Cargo's bare `Authorization` header format. Cargo registry requests respect the configured fetch retry budget.
