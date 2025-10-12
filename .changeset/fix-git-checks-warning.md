---
"@pnpm/plugin-commands-publishing": patch
---

Remove pnpm-specific CLI options before passing to npm publish to prevent "Unknown cli config" warnings [#9646](https://github.com/pnpm/pnpm/issues/9646).
