---
"pacquet": patch
---

Sped up installs in large workspaces: the anchor for re-rendering workspace `link:` targets is now derived once per project instead of once per dependency edge, and project ordering hashes paths by their raw bytes [#14352](https://github.com/pnpm/pnpm/issues/14352).
