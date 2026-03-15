---
"@pnpm/resolving.aqua-resolver": minor
"@pnpm/default-resolver": minor
"pnpm": minor
---

Added a new `aqua:` protocol for installing binary tools from GitHub Releases using the [aqua-registry](https://github.com/aquaproj/aqua-registry). This provides cross-platform binary installation from a single data source covering 3,000+ tools.

Usage: `pnpm add --global aqua:BurntSushi/ripgrep` or `pnpm add --global aqua:BurntSushi/ripgrep@14.1.1`
