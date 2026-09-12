---
"@pnpm/exec.pnpm-cli-runner": patch
"pnpm": patch
---

`pnpm runtime set` and `pnpm env use` now run the pnpm that started them. They fell back to whichever pnpm was on PATH whenever pnpm itself was started through an entry script other than `bin/pnpm.mjs`, such as the `bin/pnpm.cjs` shim that older Corepack versions use.
