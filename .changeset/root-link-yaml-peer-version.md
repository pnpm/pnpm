---
"pnpm": patch
---

A peer dependency that the workspace root provides through a `link:` dependency now resolves to the linked package's version when that package's manifest is `package.yaml` or `package.json5`. pnpm resolved it to the newest matching version from the registry.
