---
"pacquet": patch
---

Colored output is no longer printed as raw escape sequences in the Windows Command Prompt [#14292](https://github.com/pnpm/pnpm/issues/14292). Commands such as `pnpm list` now style their output instead of writing `←[32m` and friends into the console.
