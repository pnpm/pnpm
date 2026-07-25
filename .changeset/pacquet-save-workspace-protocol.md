---
"pacquet": minor
---

The Rust engine now supports the `saveWorkspaceProtocol` setting, so `pnpm add <pkg>@workspace:…` writes back the same specifier pnpm does. Under the default `rolling`, a request like `workspace:^1.2.3` is saved as `workspace:^` — a range with no version in it, so bumping the workspace package never has to touch its dependents' manifests. `saveWorkspaceProtocol: true` saves the workspace package's resolved version instead (`workspace:^2.5.0`), and `false` keeps the `workspace:` form only when it was asked for explicitly. Previously the specifier was written back exactly as typed.
