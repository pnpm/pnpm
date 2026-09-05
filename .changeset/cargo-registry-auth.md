---
"pacquet": patch
---

Cargo index and crate downloads now use URL-matched credentials from pnpm configuration. The crates.io index also supports `CARGO_REGISTRY_TOKEN` and the token in `$CARGO_HOME/credentials.toml`, using Cargo's bare `Authorization` header format.
