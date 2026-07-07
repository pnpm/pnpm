---
"@pnpm/config.reader": minor
"@pnpm/installing.deps-resolver": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/installing.commands": minor
"pnpm": minor
---

Added a new setting `package-provider`: the path to an external executable that materializes packages as read-only directories (for example Nix store paths) instead of pnpm's virtual store. pnpm sends the executable the resolved dependency graph on stdin and symlinks `node_modules` directly to the returned directories; lifecycle scripts run inside the provider's build. A reference provider for the Nix store is developed separately in the `pnpm-nix` project.
