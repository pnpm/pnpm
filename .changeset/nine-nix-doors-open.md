---
"@pnpm/config.reader": minor
"@pnpm/deps.graph-builder": minor
"@pnpm/fs.symlink-dependency": minor
"@pnpm/installing.deps-resolver": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/installing.deps-restorer": minor
"@pnpm/installing.linking.direct-dep-linker": minor
"@pnpm/installing.linking.hoist": minor
"@pnpm/installing.commands": minor
"pnpm": minor
---

Added a new setting `package-provider`: the path to an external executable that materializes packages as read-only directories (for example Nix store paths) instead of pnpm's virtual store. pnpm sends the executable the resolved dependency graph on stdin and symlinks `node_modules` directly to the returned directories (with absolute symlinks, since the provider's directories outlive the project location); lifecycle scripts run inside the provider's build. A reference provider for the Nix store is developed separately in the `pnpm-nix` project.
