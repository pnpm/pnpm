---
"pnpm": patch
---

Add Nix flake support with prebuilt binary outputs for Linux and macOS. The flake exposes `#default` (prebuilt standalone binary), `#prebuilt`, and `#nixpkgs` (nixpkgs-packaged version with shell completions). A scheduled workflow auto-bumps the version and hashes on each new release.
