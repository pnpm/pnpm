---
"@pnpm/installing.commands": patch
"pnpm": patch
---

The options type of the `fetch` command now declares `allowBuilds`, a setting its handler already forwarded to the installer. Type-level only — what `pnpm fetch` does is unchanged.
