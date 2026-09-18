---
"pacquet": patch
---

`pnpm install` now installs wheels whose `RECORD` hashes disagree with their contents. The wheel archive's locked SHA-256 hash remains verified, and pnpm writes correct hashes to the installed `RECORD` [pnpm/pnpm#15061](https://github.com/pnpm/pnpm/issues/15061).
