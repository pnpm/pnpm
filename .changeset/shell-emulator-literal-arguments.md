---
"@pnpm/exec.lifecycle": patch
"pnpm": patch
"pacquet": patch
---

Fixed argument forwarding on Windows when `shellEmulator` is enabled. Paths ending in a backslash, line breaks, and literal shell expressions are preserved [pnpm/pnpm#14548](https://github.com/pnpm/pnpm/issues/14548).
