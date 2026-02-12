---
"@pnpm/plugin-commands-publishing": patch
"pnpm": patch
---

Disable git checks for `pnpm publish` with arguments. It doesn't make sense to check the cleanliness of the current directory when the publishing is targeting another package elsewhere.
