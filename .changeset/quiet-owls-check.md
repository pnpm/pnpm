---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm install` now checks the project's `engines` field when the project is already up to date. A repeat install of a project outside a workspace warns about an incompatible Node.js version, or fails with `engineStrict` enabled [pnpm/pnpm#17751](https://github.com/pnpm/pnpm/issues/17751).
