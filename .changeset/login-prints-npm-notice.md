---
"@pnpm/auth.commands": patch
"pnpm": patch
---

`pnpm login` now prints the registry's `npm-notice` message before the web login URL. Some registries use it to show a code that you type in the browser.
