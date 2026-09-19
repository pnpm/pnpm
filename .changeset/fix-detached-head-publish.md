---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm publish` now allows a detached Git HEAD in CI, including checkouts of release tags. The working tree must still be clean [pnpm/pnpm#5894](https://github.com/pnpm/pnpm/issues/5894).
