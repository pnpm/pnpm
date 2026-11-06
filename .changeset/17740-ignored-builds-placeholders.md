---
"@pnpm/installing.commands": patch
"pnpm": patch
---

When `pnpm install` adds placeholder entries to `allowBuilds` in `pnpm-workspace.yaml`, it no longer copies `allowBuilds` entries from other config sources into the file.
