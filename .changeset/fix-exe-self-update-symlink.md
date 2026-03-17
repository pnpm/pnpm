---
"@pnpm/tools.plugin-commands-self-updater": patch
"pnpm": patch
---

Fixed `pnpm self-update` breaking when running `@pnpm/exe`. The platform binary (e.g., `@pnpm/macos-arm64`) was not found in pnpm's symlinked `node_modules` layout because it was looked up at the top level instead of as a sibling of `@pnpm/exe` in the virtual store.
