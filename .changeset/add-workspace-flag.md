---
"pacquet": patch
---

`pnpm add --workspace` is supported again. It saves the named packages under the `workspace:` protocol and links them from the workspace, and it fails when no workspace project provides one of them [#14602](https://github.com/pnpm/pnpm/issues/14602).
