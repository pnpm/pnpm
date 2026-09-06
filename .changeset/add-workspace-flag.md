---
"pacquet": patch
---

`pnpm add --workspace <pkg>` works again. It saves the package under the `workspace:` protocol and links it from the workspace. It fails when no workspace project provides the package. pnpm v12 had rejected the flag as an unknown argument [#14602](https://github.com/pnpm/pnpm/issues/14602).
