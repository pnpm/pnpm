---
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

Fixed `pnpm update --interactive` table breaking with long version strings (e.g., prerelease versions like `7.0.0-dev.20251209.1`) by dynamically calculating column widths instead of using hardcoded values [#10316](https://github.com/pnpm/pnpm/issues/10316).
