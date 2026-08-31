---
"pacquet": patch
---

Fixed `pnpm repo <package>` and `pnpm docs <package>` resolving bare package names through the `latest` tag, and prevented malformed package ranges from crashing registry selection.
