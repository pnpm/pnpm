---
"@pnpm/workspace.project-manifest-reader": patch
"pacquet": patch
"pnpm": patch
---

`pnpm add` and `pnpm install` keep an empty `peerDependencies`, `dependencies`, `devDependencies`, or `optionalDependencies` field that was already in `package.json`. pnpm still drops such a field when it removed the last entry itself, as `pnpm remove` does [#5096](https://github.com/pnpm/pnpm/issues/5096).
